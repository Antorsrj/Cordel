//! The admission gate: may this LiveKit token still enter that room?
//!
//! Self-hosted LiveKit cannot deny an identity that holds a valid token. The
//! host does have a single entry path, the `/rtc` signaling upgrade, so the
//! gate sits in front of it and asks Axum, which is the authorization
//! authority. A denied answer keeps the client out of the room.
//!
//! The gate never grants anything. It answers a question, and the question is
//! about the current state of the account, the session, and the generation.

use axum::http::HeaderMap;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use super::*;

/// The LiveKit SDK sends the access token in the `Authorization` header, not
/// in the query string. The header keeps the token out of every access log and
/// out of the URL. The query parameter is still accepted, because a client may
/// put it there.
pub(crate) const TOKEN_HEADER: &str = "x-cordel-livekit-token";

/// The claims of a LiveKit access token.
#[derive(Debug, Deserialize)]
struct GateClaims {
    #[serde(default)]
    iss: String,
    sub: String,
    #[serde(rename = "sid")]
    session_id: String,
    video: GateVideo,
}

#[derive(Debug, Deserialize)]
struct GateVideo {
    room: String,
}

/// The room name a media admission carries: `voice:<channel-uuid>:<generation>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoomName {
    pub channel_id: Uuid,
    pub generation: u64,
}

/// Parse a room name. Anything that is not a voice room of this shape is
/// refused, so a token minted for another purpose cannot slip through.
pub(crate) fn parse_room(room: &str) -> Option<RoomName> {
    let rest = room.strip_prefix("voice:")?;
    let (channel, generation) = rest.rsplit_once(':')?;
    let channel_id = Uuid::parse_str(channel).ok()?;
    let generation: u64 = generation.parse().ok()?;
    Some(RoomName {
        channel_id,
        generation,
    })
}

impl MediaConfig {
    /// Decode a LiveKit token and read the account, the session, and the room.
    /// The signature and the issuer must match, and the token must not have
    /// expired.
    fn decode(&self, token: &str) -> Result<GateClaims, ApiError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[self.api_key.as_str()]);
        validation.set_required_spec_claims(&["exp", "iss"]);
        let claims = decode::<GateClaims>(
            token,
            &DecodingKey::from_secret(self.api_secret.as_bytes()),
            &validation,
        )
        .map_err(|_| ApiError::unauthenticated())?
        .claims;
        if claims.iss != self.api_key {
            return Err(ApiError::unauthenticated());
        }
        Ok(claims)
    }

    /// The answer to the gate's question.
    ///
    /// The token is allowed only when the room is a known voice channel of a
    /// community, the generation is the community's current one, the account is
    /// an active member, and the app session is still live. Every check reads
    /// the current state from PostgreSQL; none of them trusts the token.
    pub(crate) async fn authorize_room(&self, pool: &PgPool, token: &str) -> Result<(), ApiError> {
        let claims = self.decode(token)?;
        let account_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::unauthenticated())?;
        let session_id =
            Uuid::parse_str(&claims.session_id).map_err(|_| ApiError::unauthenticated())?;
        let room = parse_room(&claims.video.room).ok_or_else(ApiError::forbidden)?;

        let row = sqlx::query(
            "SELECT c.community_id, c.kind, k.authorization_revision \
             FROM channels c JOIN communities k ON k.id=c.community_id WHERE c.id=$1",
        )
        .bind(room.channel_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::forbidden)?;
        if row.get::<String, _>("kind") != "voice" {
            return Err(ApiError::forbidden());
        }
        let community_id: Uuid = row.get("community_id");
        let current = row.get::<i64, _>("authorization_revision").max(0) as u64;
        if room.generation != current {
            // The token was minted before a moderation action. The room it
            // names is no longer the current one.
            return Err(ApiError::forbidden());
        }

        let member = sqlx::query(
            "SELECT m.kicked_until, m.banned_at, s.revoked_at, s.expires_at \
             FROM memberships m JOIN sessions s ON s.account_id=m.account_id \
             WHERE m.community_id=$1 AND m.account_id=$2 AND s.id=$3",
        )
        .bind(community_id)
        .bind(account_id)
        .bind(session_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::forbidden)?;
        if member
            .get::<Option<DateTime<Utc>>, _>("banned_at")
            .is_some()
            || member
                .get::<Option<DateTime<Utc>>, _>("kicked_until")
                .is_some_and(|until| until > Utc::now())
            || member
                .get::<Option<DateTime<Utc>>, _>("revoked_at")
                .is_some()
            || member
                .get::<Option<DateTime<Utc>>, _>("expires_at")
                .is_some_and(|at| at <= Utc::now())
        {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }
}

/// Read the LiveKit token from the request.
///
/// The SDK sends it as `Authorization: Bearer <token>`. The reverse proxy
/// copies the original headers to the gate, so the token arrives untouched.
/// A token in the query string is accepted as well, for a client that puts it
/// there.
fn request_token<'a>(headers: &'a HeaderMap, query: Option<&'a String>) -> Option<&'a str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get(TOKEN_HEADER)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            query
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        })
}

/// `GET /v1/livekit/authorize` — the gate the reverse proxy calls before it
/// proxies the LiveKit signaling upgrade.
pub(crate) async fn authorize(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let media = state.media.as_ref().ok_or_else(ApiError::not_found)?;
    let token =
        request_token(&headers, query.get("access_token")).ok_or_else(ApiError::unauthenticated)?;
    media.config.authorize_room(&state.pool, token).await?;
    Ok(StatusCode::OK)
}

/// The digest helper, shared with the webhook verification.
pub(crate) fn digest(raw: &[u8]) -> String {
    BASE64.encode(Sha256::digest(raw))
}
