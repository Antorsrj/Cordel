//! ADR-005 media admission: one scoped LiveKit token for one account and room.
//!
//! Axum stays the authorization authority. The LiveKit API secret never
//! leaves this module, and a client never receives it. The admission names
//! the community's current authorization revision, so a moderation action
//! makes an admission minted before it point at a room that is no longer
//! current.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use cordel_protocol::MediaAdmission;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::*;

/// How long an admission stays valid. It matches the app access token, and it
/// bounds the window in which a held token can enter a room again. LiveKit
/// checks the token when a participant joins, not during a call.
const ADMISSION_SECONDS: i64 = ACCESS_SECONDS;

/// The LiveKit service configuration, read from the host environment.
#[derive(Clone)]
pub struct MediaConfig {
    api_key: String,
    api_secret: String,
    /// The public signaling URL clients connect to.
    pub(crate) url: String,
}

impl MediaConfig {
    /// LiveKit refuses a secret shorter than 32 characters, so refuse it here
    /// too instead of failing later inside the media service.
    pub fn new(api_key: String, api_secret: String, url: String) -> Result<Self, &'static str> {
        if api_key.trim().is_empty() {
            return Err("CORDEL_LIVEKIT_API_KEY must not be empty");
        }
        if api_secret.len() < 32 {
            return Err("CORDEL_LIVEKIT_API_SECRET must contain at least 32 bytes");
        }
        if url.trim().is_empty() {
            return Err("CORDEL_LIVEKIT_URL must not be empty");
        }
        Ok(Self {
            api_key,
            api_secret,
            url,
        })
    }
}

/// The minimum LiveKit grant for one participant in one room.
///
/// `can_publish_data` is false because chat stays on Axum and PostgreSQL under
/// ADR-004 and never travels on a LiveKit data track. The administrative
/// grants stay false so a client can neither create nor administer a room.
#[derive(Serialize)]
struct VideoGrant {
    room: String,
    #[serde(rename = "roomJoin")]
    room_join: bool,
    #[serde(rename = "canPublish")]
    can_publish: bool,
    #[serde(rename = "canSubscribe")]
    can_subscribe: bool,
    #[serde(rename = "canPublishData")]
    can_publish_data: bool,
    #[serde(rename = "roomCreate")]
    room_create: bool,
    #[serde(rename = "roomList")]
    room_list: bool,
    #[serde(rename = "roomAdmin")]
    room_admin: bool,
    #[serde(rename = "roomRecord")]
    room_record: bool,
    hidden: bool,
    recording: bool,
}

#[derive(Serialize)]
struct AdmissionClaims {
    exp: i64,
    nbf: i64,
    iat: i64,
    iss: String,
    sub: String,
    name: String,
    video: VideoGrant,
}

/// The physical room name for a voice channel at one generation.
pub(crate) fn room_name(channel_id: Uuid, generation: u64) -> String {
    format!("voice:{channel_id}:{generation}")
}

impl MediaConfig {
    fn mint(&self, principal: &Principal, room: &str) -> Result<String, ApiError> {
        let now = Utc::now().timestamp();
        let claims = AdmissionClaims {
            exp: now + ADMISSION_SECONDS,
            nbf: now,
            iat: now,
            iss: self.api_key.clone(),
            sub: principal.account_id.to_string(),
            name: principal.login.clone(),
            video: VideoGrant {
                room: room.to_owned(),
                room_join: true,
                can_publish: true,
                can_subscribe: true,
                can_publish_data: false,
                room_create: false,
                room_list: false,
                room_admin: false,
                room_record: false,
                hidden: false,
                recording: false,
            },
        };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.api_secret.as_bytes()),
        )
        .map_err(|_| ApiError::internal())
    }
}

/// `POST /v1/voice-channels/{channel_id}/admission` — one scoped admission
/// for a voice channel, as the v1 contract names it.
///
/// The channel decides the community and the kind. The caller must hold an
/// active app session, be a member of that community, and not be kicked or
/// banned; the session check reads the current moderation state from
/// PostgreSQL rather than trusting the access token.
pub(crate) async fn admission(
    State(state): State<AppState>,
    Path(raw_channel_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<MediaAdmission>, ApiError> {
    let media = state.media.as_ref().ok_or_else(ApiError::not_found)?;
    let channel_id = Uuid::parse_str(&raw_channel_id).map_err(|_| ApiError::invalid())?;
    let row = sqlx::query(
        "SELECT c.community_id, c.kind, k.authorization_revision \
         FROM channels c JOIN communities k ON k.id=c.community_id WHERE c.id=$1",
    )
    .bind(channel_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(ApiError::not_found)?;
    if row.get::<String, _>("kind") != "voice" {
        return Err(ApiError::forbidden());
    }
    let community_id: Uuid = row.get("community_id");
    let generation = row.get::<i64, _>("authorization_revision").max(0) as u64;

    let principal =
        policy::authorize(&state, &headers, community_id, policy::Action::Member).await?;
    let room = room_name(channel_id, generation);
    let token = media.config.mint(&principal, &room)?;
    let expires_at = Utc::now() + chrono::Duration::seconds(ADMISSION_SECONDS);
    Ok(Json(MediaAdmission {
        schema_version: 1,
        voice_channel_id: channel_id.to_string(),
        room,
        media_generation: generation,
        url: media.config.url.clone(),
        token,
        expires_at: expires_at.to_rfc3339(),
        expires_in_secs: ADMISSION_SECONDS as u64,
    }))
}

/// The base64 form of the SHA-256 digest of a webhook body, which is what the
/// LiveKit webhook signature carries.
pub fn body_digest(raw: &[u8]) -> String {
    BASE64.encode(Sha256::digest(raw))
}

/// The media configuration and the ephemeral observation it collects.
#[derive(Clone)]
pub(crate) struct MediaState {
    pub(crate) config: Arc<MediaConfig>,
    pub(crate) observation: Arc<webhook::Observations>,
}

impl MediaState {
    pub(crate) fn new(config: MediaConfig) -> Self {
        Self {
            config: Arc::new(config),
            observation: Arc::new(webhook::Observations::default()),
        }
    }
}

pub(crate) mod webhook;
