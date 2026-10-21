//! ADR-004 durable chat: authenticated sends and ordered history.
use std::collections::BTreeMap;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use cordel_protocol::ChatEvent;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgRow;
use sqlx::Row;
use uuid::Uuid;

use super::*;
use crate::policy::{self, Action};

const MAX_BODY_BYTES: usize = 4000;
const DEFAULT_PAGE: u32 = 50;
const MAX_PAGE: u32 = 200;

#[derive(Deserialize)]
pub(super) struct SendRequest {
    client_message_id: String,
    body: String,
}

#[derive(Serialize)]
pub(super) struct AckResponse {
    schema_version: u8,
    channel_id: String,
    client_message_id: String,
    seq: u64,
}

#[derive(Deserialize)]
pub(super) struct HistoryParams {
    after_seq: Option<u64>,
    before_seq: Option<u64>,
    limit: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct HistoryResponse {
    schema_version: u8,
    channel_id: String,
    messages: Vec<ChatEvent>,
    has_more: bool,
}

fn parse_channel_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::invalid())
}

fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "Not found")
}

fn conflict() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "conflict",
        "Request conflicts with current state",
    )
}

fn valid_client_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

fn valid_body(body: &str) -> bool {
    !body.trim().is_empty()
        && body.len() <= MAX_BODY_BYTES
        && !body
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
}

fn page_limit(limit: Option<u32>) -> Result<u32, ApiError> {
    match limit {
        None => Ok(DEFAULT_PAGE),
        Some(0) => Err(ApiError::invalid()),
        Some(value) => Ok(value.min(MAX_PAGE)),
    }
}

fn sequence(value: u64) -> Result<i64, ApiError> {
    i64::try_from(value).map_err(|_| ApiError::invalid())
}

fn event_from_row(channel_id: Uuid, row: &PgRow) -> ChatEvent {
    let created_at: DateTime<Utc> = row.get("created_at");
    let mut extra = BTreeMap::new();
    extra.insert(
        "message_id".to_owned(),
        json!(row.get::<Uuid, _>("id").to_string()),
    );
    extra.insert(
        "author_id".to_owned(),
        json!(row.get::<Uuid, _>("author_id").to_string()),
    );
    extra.insert(
        "created_at".to_owned(),
        json!(created_at.to_rfc3339_opts(SecondsFormat::Millis, true)),
    );
    ChatEvent {
        channel_id: channel_id.to_string(),
        seq: row.get::<i64, _>("seq") as u64,
        kind: "message".to_owned(),
        client_message_id: Some(row.get::<String, _>("client_message_id")),
        body: Some(row.get("body")),
        extra,
    }
}

pub(super) async fn send_message(
    State(state): State<AppState>,
    Path(raw_channel_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<SendRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<AckResponse>), ApiError> {
    let channel_id = parse_channel_id(&raw_channel_id)?;
    let payload = payload.map_err(json_error)?.0;
    if !valid_client_id(&payload.client_message_id) {
        return Err(ApiError::invalid());
    }
    let client_message_id = payload.client_message_id.as_str();
    if !valid_body(&payload.body) {
        return Err(ApiError::invalid());
    }

    let principal = state.authenticate(&headers).await?;
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    policy::lock_write(&mut tx, &principal, &headers, Action::Member).await?;

    let channel =
        sqlx::query("SELECT community_id,kind,last_seq FROM channels WHERE id=$1 FOR UPDATE")
            .bind(channel_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ApiError::internal())?
            .ok_or_else(not_found)?;
    if channel.get::<Uuid, _>("community_id") != principal.community_id {
        return Err(not_found());
    }
    if channel.get::<String, _>("kind") != "text" {
        return Err(ApiError::invalid());
    }

    let existing = sqlx::query(
        "SELECT seq,body FROM messages \
         WHERE channel_id=$1 AND author_id=$2 AND client_message_id=$3",
    )
    .bind(channel_id)
    .bind(principal.account_id)
    .bind(client_message_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?;
    if let Some(row) = existing {
        if row.get::<String, _>("body") != payload.body {
            return Err(conflict());
        }
        return Ok((
            StatusCode::OK,
            Json(AckResponse {
                schema_version: 1,
                channel_id: channel_id.to_string(),
                client_message_id: client_message_id.to_owned(),
                seq: row.get::<i64, _>("seq") as u64,
            }),
        ));
    }

    let last_seq = channel.get::<i64, _>("last_seq");
    let next_seq = last_seq.checked_add(1).ok_or_else(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO messages (id,channel_id,author_id,client_message_id,seq,body) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(channel_id)
    .bind(principal.account_id)
    .bind(client_message_id)
    .bind(next_seq)
    .bind(&payload.body)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if is_unique(&error) {
            conflict()
        } else {
            ApiError::internal()
        }
    })?;
    let updated = sqlx::query("UPDATE channels SET last_seq=$2 WHERE id=$1 AND last_seq=$3")
        .bind(channel_id)
        .bind(next_seq)
        .bind(last_seq)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    if updated.rows_affected() != 1 {
        return Err(conflict());
    }
    tx.commit().await.map_err(|_| ApiError::internal())?;

    Ok((
        StatusCode::CREATED,
        Json(AckResponse {
            schema_version: 1,
            channel_id: channel_id.to_string(),
            client_message_id: client_message_id.to_owned(),
            seq: next_seq as u64,
        }),
    ))
}

pub(super) async fn history(
    State(state): State<AppState>,
    Path(raw_channel_id): Path<String>,
    query: Result<Query<HistoryParams>, QueryRejection>,
    headers: HeaderMap,
) -> Result<Json<HistoryResponse>, ApiError> {
    let params = query.map_err(|_| ApiError::invalid())?.0;
    if params.after_seq.is_some() && params.before_seq.is_some() {
        return Err(ApiError::invalid());
    }
    let limit = page_limit(params.limit)?;
    let after = params.after_seq.map(sequence).transpose()?;
    let before = params.before_seq.map(sequence).transpose()?;
    let channel_id = parse_channel_id(&raw_channel_id)?;
    let principal = state.authenticate(&headers).await?;

    let channel = sqlx::query(
        "SELECT c.kind,m.account_id AS member_id,m.banned_at,m.kicked_until, \
         s.id AS active_session \
         FROM channels c \
         LEFT JOIN memberships m ON m.community_id=c.community_id AND m.account_id=$2 \
         LEFT JOIN sessions s ON s.id=$3 AND s.account_id=$2 \
         AND s.revoked_at IS NULL AND s.expires_at>now() \
         WHERE c.id=$1 AND c.community_id=$4",
    )
    .bind(channel_id)
    .bind(principal.account_id)
    .bind(principal.session_id)
    .bind(principal.community_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(not_found)?;
    if channel.get::<Option<Uuid>, _>("member_id").is_none()
        || channel.get::<Option<Uuid>, _>("active_session").is_none()
    {
        return Err(ApiError::unauthenticated());
    }
    let kicked_until: Option<DateTime<Utc>> = channel.get("kicked_until");
    if channel
        .get::<Option<DateTime<Utc>>, _>("banned_at")
        .is_some()
        || kicked_until.is_some_and(|until| until > Utc::now())
    {
        return Err(ApiError::forbidden());
    }
    if channel.get::<String, _>("kind") != "text" {
        return Err(ApiError::invalid());
    }

    let row_limit = i64::from(limit) + 1;
    let columns = "id,author_id,client_message_id,seq,body,created_at";
    let rows = if let Some(after) = after {
        sqlx::query(&format!(
            "SELECT {columns} FROM messages WHERE channel_id=$1 AND seq>$2 \
             ORDER BY seq ASC LIMIT $3"
        ))
        .bind(channel_id)
        .bind(after)
        .bind(row_limit)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| ApiError::internal())?
    } else if let Some(before) = before {
        sqlx::query(&format!(
            "SELECT {columns} FROM messages WHERE channel_id=$1 AND seq<$2 \
             ORDER BY seq DESC LIMIT $3"
        ))
        .bind(channel_id)
        .bind(before)
        .bind(row_limit)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| ApiError::internal())?
    } else {
        sqlx::query(&format!(
            "SELECT {columns} FROM messages WHERE channel_id=$1 \
             ORDER BY seq DESC LIMIT $2"
        ))
        .bind(channel_id)
        .bind(row_limit)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| ApiError::internal())?
    };

    let mut messages: Vec<ChatEvent> = rows
        .iter()
        .map(|row| event_from_row(channel_id, row))
        .collect();
    let has_more = messages.len() > limit as usize;
    messages.truncate(limit as usize);
    if after.is_none() {
        messages.reverse();
    }

    Ok(Json(HistoryResponse {
        schema_version: 1,
        channel_id: channel_id.to_string(),
        messages,
        has_more,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_ids_are_bounded_stable_strings() {
        assert!(valid_client_id("client-1"));
        assert!(valid_client_id(&Uuid::new_v4().to_string()));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("   "));
        assert!(!valid_client_id("bad\0id"));
        assert!(!valid_client_id(&"a".repeat(129)));
    }

    #[test]
    fn message_bodies_are_bounded_and_control_free() {
        assert!(valid_body("hello"));
        assert!(valid_body("line one\nline two\ttabbed"));
        assert!(!valid_body(""));
        assert!(!valid_body(" \n\t "));
        assert!(!valid_body("null\0byte"));
        assert!(valid_body(&"a".repeat(MAX_BODY_BYTES)));
        assert!(!valid_body(&"a".repeat(MAX_BODY_BYTES + 1)));
    }

    #[test]
    fn history_limits_default_and_cap() {
        assert_eq!(page_limit(None).unwrap(), DEFAULT_PAGE);
        assert_eq!(page_limit(Some(25)).unwrap(), 25);
        assert_eq!(page_limit(Some(10_000)).unwrap(), MAX_PAGE);
        assert!(page_limit(Some(0)).is_err());
    }

    #[test]
    fn cursors_must_fit_the_database_sequence() {
        assert_eq!(sequence(1).unwrap(), 1);
        assert!(sequence(u64::MAX).is_err());
    }

    #[test]
    fn channel_ids_must_be_uuids() {
        assert!(parse_channel_id("not-a-uuid").is_err());
        assert!(parse_channel_id(&Uuid::new_v4().to_string()).is_ok());
    }

    #[test]
    fn responses_parse_as_the_shared_protocol_types() {
        let event = ChatEvent {
            channel_id: "c".to_owned(),
            seq: 1,
            kind: "message".to_owned(),
            client_message_id: Some("client-1".to_owned()),
            body: Some("hello".to_owned()),
            extra: BTreeMap::new(),
        };
        let page = HistoryResponse {
            schema_version: 1,
            channel_id: "c".to_owned(),
            messages: vec![event],
            has_more: false,
        };
        let parsed: cordel_protocol::HistoryPage =
            serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].body.as_deref(), Some("hello"));

        let ack = AckResponse {
            schema_version: 1,
            channel_id: "c".to_owned(),
            client_message_id: "client-1".to_owned(),
            seq: 4,
        };
        let parsed: cordel_protocol::MessageAck =
            serde_json::from_str(&serde_json::to_string(&ack).unwrap()).unwrap();
        assert_eq!(parsed.seq, 4);
        assert_eq!(parsed.client_message_id, "client-1");
    }
}
