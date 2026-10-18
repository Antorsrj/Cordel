//! ADR-003 community, channel, and moderation REST surface.
use super::*;
use crate::policy::{self, Action};

#[derive(Serialize)]
pub(super) struct CommunityResponse {
    schema_version: u8,
    community_id: Uuid,
    name: String,
    description: String,
}

#[derive(Deserialize)]
pub(super) struct UpdateCommunity {
    name: Option<String>,
    description: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ChannelResponse {
    schema_version: u8,
    channel_id: Uuid,
    community_id: Uuid,
    kind: String,
    name: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub(super) struct ChannelList {
    schema_version: u8,
    channels: Vec<ChannelResponse>,
}

#[derive(Deserialize)]
pub(super) struct CreateChannel {
    kind: String,
    name: String,
}

#[derive(Deserialize)]
pub(super) struct KickRequest {
    duration_minutes: Option<i64>,
}

#[derive(Serialize)]
pub(super) struct ModerationResponse {
    schema_version: u8,
    member_id: Uuid,
    state: &'static str,
    kicked_until: Option<DateTime<Utc>>,
    authorization_revision: i64,
}

fn community_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::invalid())
}

fn valid_name(name: &str, limit: usize) -> bool {
    !name.trim().is_empty() && name.chars().count() <= limit && !name.chars().any(char::is_control)
}

fn valid_description(description: &str) -> bool {
    description.chars().count() <= 1000
        && !description
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
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

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    principal: &Principal,
    target_id: Option<Uuid>,
    action: &str,
    revision: Option<i64>,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO community_audit (id,community_id,actor_id,session_id,target_id,action,authorization_revision) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(Uuid::new_v4())
        .bind(principal.community_id)
        .bind(principal.account_id)
        .bind(principal.session_id)
        .bind(target_id)
        .bind(action)
        .bind(revision)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok(())
}

pub(super) async fn get_community(
    State(state): State<AppState>,
    Path(raw_community_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<CommunityResponse>, ApiError> {
    let id = community_id(&raw_community_id)?;
    policy::authorize(&state, &headers, id, Action::Member).await?;
    let row = sqlx::query("SELECT name,description FROM communities WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(not_found)?;
    Ok(Json(CommunityResponse {
        schema_version: 1,
        community_id: id,
        name: row.get("name"),
        description: row.get("description"),
    }))
}

pub(super) async fn update_community(
    State(state): State<AppState>,
    Path(raw_community_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<UpdateCommunity>, JsonRejection>,
) -> Result<Json<CommunityResponse>, ApiError> {
    let id = community_id(&raw_community_id)?;
    let principal = policy::authorize(&state, &headers, id, Action::Manage).await?;
    let body = payload.map_err(json_error)?.0;
    if (body.name.is_none() && body.description.is_none())
        || body
            .name
            .as_ref()
            .is_some_and(|name| !valid_name(name, 100))
        || body
            .description
            .as_ref()
            .is_some_and(|text| !valid_description(text))
    {
        return Err(ApiError::invalid());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    policy::lock_write(&mut tx, &principal, &headers, Action::Manage).await?;
    let row = sqlx::query("UPDATE communities SET name=COALESCE($2,name), description=COALESCE($3,description) WHERE id=$1 RETURNING name,description")
        .bind(id)
        .bind(body.name.map(|name| name.trim().to_owned()))
        .bind(body.description)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(not_found)?;
    audit(&mut tx, &principal, None, "community_updated", None).await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(CommunityResponse {
        schema_version: 1,
        community_id: id,
        name: row.get("name"),
        description: row.get("description"),
    }))
}

pub(super) async fn list_channels(
    State(state): State<AppState>,
    Path(raw_community_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ChannelList>, ApiError> {
    let id = community_id(&raw_community_id)?;
    policy::authorize(&state, &headers, id, Action::Member).await?;
    let rows = sqlx::query(
        "SELECT id,kind,name,created_at FROM channels WHERE community_id=$1 ORDER BY created_at,id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?;
    let channels = rows
        .into_iter()
        .map(|row| ChannelResponse {
            schema_version: 1,
            channel_id: row.get("id"),
            community_id: id,
            kind: row.get("kind"),
            name: row.get("name"),
            created_at: row.get("created_at"),
        })
        .collect();
    Ok(Json(ChannelList {
        schema_version: 1,
        channels,
    }))
}

pub(super) async fn create_channel(
    State(state): State<AppState>,
    Path(raw_community_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<CreateChannel>, JsonRejection>,
) -> Result<(StatusCode, Json<ChannelResponse>), ApiError> {
    let id = community_id(&raw_community_id)?;
    let principal = policy::authorize(&state, &headers, id, Action::Member).await?;
    let body = payload.map_err(json_error)?.0;
    if !matches!(body.kind.as_str(), "text" | "voice") || !valid_name(&body.name, 80) {
        return Err(ApiError::invalid());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    policy::lock_write(&mut tx, &principal, &headers, Action::Member).await?;
    let channel_id = Uuid::new_v4();
    let row = sqlx::query("INSERT INTO channels (id,community_id,kind,name,created_by) VALUES ($1,$2,$3,$4,$5) RETURNING created_at")
        .bind(channel_id)
        .bind(id)
        .bind(&body.kind)
        .bind(body.name.trim())
        .bind(principal.account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| if is_unique(&error) { conflict() } else { ApiError::internal() })?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok((
        StatusCode::CREATED,
        Json(ChannelResponse {
            schema_version: 1,
            channel_id,
            community_id: id,
            kind: body.kind,
            name: body.name.trim().to_owned(),
            created_at: row.get("created_at"),
        }),
    ))
}

#[derive(Clone, Copy)]
enum Moderation {
    Kick(i64),
    Ban,
    Unban,
}

async fn moderate(
    state: AppState,
    raw_community_id: String,
    raw_member_id: String,
    headers: HeaderMap,
    action: Moderation,
) -> Result<Json<ModerationResponse>, ApiError> {
    let id = community_id(&raw_community_id)?;
    let member_id = Uuid::parse_str(&raw_member_id).map_err(|_| ApiError::invalid())?;
    let principal = policy::authorize(&state, &headers, id, Action::Manage).await?;
    if member_id == principal.account_id {
        return Err(ApiError::forbidden());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    policy::lock_write(&mut tx, &principal, &headers, Action::Manage).await?;
    let target = sqlx::query(
        "SELECT role,banned_at FROM memberships WHERE community_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(member_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(not_found)?;
    if target.get::<String, _>("role") != "user" {
        return Err(ApiError::forbidden());
    }
    if matches!(action, Moderation::Kick(_))
        && target
            .get::<Option<DateTime<Utc>>, _>("banned_at")
            .is_some()
    {
        return Err(conflict());
    }
    let (state_name, kicked_until, audit_action) = match action {
        Moderation::Kick(minutes) => {
            let duration = Duration::try_minutes(minutes).ok_or_else(ApiError::invalid)?;
            let until = Utc::now()
                .checked_add_signed(duration)
                .ok_or_else(ApiError::invalid)?;
            let row = sqlx::query("UPDATE memberships SET kicked_until=GREATEST(COALESCE(kicked_until,$3),$3) WHERE community_id=$1 AND account_id=$2 RETURNING kicked_until")
                .bind(id).bind(member_id).bind(until).fetch_one(&mut *tx).await.map_err(|_| ApiError::internal())?;
            ("kicked", row.get("kicked_until"), "member_kicked")
        }
        Moderation::Ban => {
            sqlx::query("UPDATE memberships SET banned_at=now(),kicked_until=NULL WHERE community_id=$1 AND account_id=$2")
                .bind(id).bind(member_id).execute(&mut *tx).await.map_err(|_| ApiError::internal())?;
            ("banned", None, "member_banned")
        }
        Moderation::Unban => {
            if target
                .get::<Option<DateTime<Utc>>, _>("banned_at")
                .is_none()
            {
                return Err(conflict());
            }
            sqlx::query(
                "UPDATE memberships SET banned_at=NULL WHERE community_id=$1 AND account_id=$2",
            )
            .bind(id)
            .bind(member_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ApiError::internal())?;
            ("active", None, "member_unbanned")
        }
    };
    if !matches!(action, Moderation::Unban) {
        sqlx::query(
            "UPDATE sessions SET revoked_at=now() WHERE account_id=$1 AND revoked_at IS NULL",
        )
        .bind(member_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    }
    let revision: i64 = sqlx::query_scalar("UPDATE communities SET authorization_revision=authorization_revision+1 WHERE id=$1 RETURNING authorization_revision")
        .bind(id).fetch_one(&mut *tx).await.map_err(|_| ApiError::internal())?;
    audit(
        &mut tx,
        &principal,
        Some(member_id),
        audit_action,
        Some(revision),
    )
    .await?;
    sqlx::query("SELECT pg_notify('cordel_authorization', $1)")
        .bind(format!("{id}:{member_id}:{revision}"))
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(ModerationResponse {
        schema_version: 1,
        member_id,
        state: state_name,
        kicked_until,
        authorization_revision: revision,
    }))
}

pub(super) async fn kick(
    State(state): State<AppState>,
    Path((community_id, member_id)): Path<(String, String)>,
    headers: HeaderMap,
    payload: Result<Json<KickRequest>, JsonRejection>,
) -> Result<Json<ModerationResponse>, ApiError> {
    let minutes = payload
        .map_err(json_error)?
        .0
        .duration_minutes
        .unwrap_or(30);
    if minutes < 30 {
        return Err(ApiError::invalid());
    }
    moderate(
        state,
        community_id,
        member_id,
        headers,
        Moderation::Kick(minutes),
    )
    .await
}

pub(super) async fn ban(
    State(state): State<AppState>,
    Path((community_id, member_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ModerationResponse>, ApiError> {
    moderate(state, community_id, member_id, headers, Moderation::Ban).await
}

pub(super) async fn unban(
    State(state): State<AppState>,
    Path((community_id, member_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ModerationResponse>, ApiError> {
    moderate(state, community_id, member_id, headers, Moderation::Unban).await
}
