//! ADR-002 identity boundary. This crate deliberately exposes no public admin-recovery route.

#[cfg(test)]
mod adr003_tests;
#[cfg(test)]
mod adr004_client_smoke;
#[cfg(test)]
mod adr004_tests;
#[cfg(test)]
mod adr004_ws_tests;
mod chat;
mod client_ip;
mod routes;
pub use client_ip::ClientIp;
pub use routes::{gate_router, router};
mod browser;
#[cfg(test)]
mod browser_tests;
mod community;
mod diagnostic;
mod events;
mod host_admin;
mod host_identity;
mod identity_handlers;
mod media;
#[cfg(test)]
mod media_tests;
mod policy;
mod presence;

pub use host_admin::change_admin_role;
pub use host_identity::{bootstrap_admin, recover_admin};
use identity_handlers::*;
use media::webhook::RoomObservation;
use media::MediaState;
pub use media::{LiveKitApi, MediaConfig};

use argon2::password_hash::{
    rand_core::OsRng as SaltRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use axum::extract::{rejection::JsonRejection, DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use cordel_protocol::ChatEvent;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration as StdDuration, Instant};
use tokio::sync::broadcast;
use uuid::Uuid;

const ACCESS_SECONDS: i64 = 300;
const REFRESH_DAYS: i64 = 30;
const AUTH_WINDOW: StdDuration = StdDuration::from_secs(600);
/// How many login attempts one address may make in the window. It is high
/// enough for one office behind one address and low enough that one noisy
/// client does not fill the window for everybody else.
const LOGIN_ATTEMPTS_PER_ADDRESS: usize = 60;
/// How many invitation redemptions one address may make in the window.
const INVITE_ATTEMPTS_PER_ADDRESS: usize = 100;
/// How many refresh attempts one address may make in the window.
const REFRESH_ATTEMPTS_PER_ADDRESS: usize = 300;
const ADMIN_GRANT_SECONDS: i64 = 600;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    signing: Arc<Vec<u8>>,
    attempts: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    pub(crate) chat_events: broadcast::Sender<ChatEvent>,
    pub(crate) presence: Arc<presence::PresenceNode>,
    /// The LiveKit service, when the host configured one. Without it the
    /// admission and webhook routes are not registered, so a deployment that
    /// does not serve media keeps working unchanged.
    pub(crate) media: Option<MediaState>,
    browser_origin: Option<String>,
}

impl AppState {
    pub fn new(pool: PgPool, signing_key: Vec<u8>) -> Result<Self, &'static str> {
        if signing_key.len() < 32 {
            return Err("CORDEL_JWT_SECRET must contain at least 32 random bytes");
        }
        let (chat_events, _) = broadcast::channel(512);
        Ok(Self {
            pool,
            signing: Arc::new(signing_key),
            attempts: Arc::new(Mutex::new(HashMap::new())),
            chat_events,
            presence: presence::PresenceNode::new(),
            media: None,
            browser_origin: None,
        })
    }

    /// The exact origin allowed to use the browser cookie session.
    pub fn with_browser_origin(mut self, origin: String) -> Self {
        self.browser_origin = Some(origin);
        self
    }

    /// Attach the LiveKit service. The media routes exist only when it is
    /// present.
    pub fn with_media(mut self, config: MediaConfig) -> Self {
        self.media = Some(MediaState::new(config));
        self
    }

    /// Attach the host-local LiveKit server API, used to drain a room.
    pub fn with_media_api(mut self, api: LiveKitApi) -> Self {
        if let Some(media) = self.media.as_mut() {
            media.config.api = Some(api);
        }
        self
    }

    /// Whether a LiveKit service is configured.
    pub fn has_media(&self) -> bool {
        self.media.is_some()
    }

    /// The observed media rooms, for the Server 010 snapshot and for tests.
    pub fn media_observation(&self) -> BTreeMap<String, RoomObservation> {
        match &self.media {
            Some(media) => media.observation.snapshot(),
            None => BTreeMap::new(),
        }
    }

    /// Count one attempt against a key.
    ///
    /// There is no shared bucket: a single unauthenticated client must not be
    /// able to deny the service to everybody else. Each caller limits its own
    /// key, and the caller also limits the client address.
    fn limit(&self, key: &str, max_per_key: usize) -> Result<(), ApiError> {
        self.count(key, max_per_key)
    }

    /// Count one attempt against the client address.
    ///
    /// The address is optional: a request without a known address, which only
    /// happens in an internal test, is limited by its key alone.
    fn limit_address(
        &self,
        address: Option<IpAddr>,
        key: &str,
        max_per_key: usize,
        max_per_address: usize,
    ) -> Result<(), ApiError> {
        if let Some(address) = address {
            self.count(&format!("ip:{address}"), max_per_address)?;
        }
        self.count(key, max_per_key)
    }

    fn count(&self, key: &str, max_per_key: usize) -> Result<(), ApiError> {
        let mut attempts = self.attempts.lock().map_err(|_| ApiError::internal())?;
        let now = Instant::now();
        // Keep bounded memory even for unique guessed logins.
        if attempts.len() > 2048 {
            attempts.retain(|_, times| {
                times.retain(|time| now.duration_since(*time) < AUTH_WINDOW);
                !times.is_empty()
            });
        }
        let per_key = attempts.entry(key.to_owned()).or_default();
        per_key.retain(|time| now.duration_since(*time) < AUTH_WINDOW);
        if per_key.len() >= max_per_key {
            return Err(ApiError::rate_limited());
        }
        per_key.push(now);
        Ok(())
    }

    fn issue_access(&self, account_id: Uuid, session_id: Uuid) -> Result<String, ApiError> {
        let now = Utc::now().timestamp();
        let claims = Claims {
            sub: account_id.to_string(),
            sid: session_id.to_string(),
            iat: now as usize,
            exp: (now + ACCESS_SECONDS) as usize,
            iss: "cordel".to_owned(),
            aud: "cordel-app".to_owned(),
        };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.signing),
        )
        .map_err(|_| ApiError::internal())
    }

    async fn authenticate(&self, headers: &HeaderMap) -> Result<Principal, ApiError> {
        if headers.get(header::AUTHORIZATION).is_some() {
            return self.authenticate_bearer(headers).await;
        }
        // No cookie means there is no browser session to consider. Entering the
        // browser path anyway would answer `browser_disabled` on a deployment
        // that serves no browser, and the contract promises `unauthenticated`
        // for a request that carries no credential at all.
        if !browser::has_session_cookie(headers) {
            return Err(ApiError::unauthenticated());
        }
        browser::authenticate_cookie(self, headers, true)
            .await
            .map(|auth| auth.principal)
    }

    async fn authenticate_bearer(&self, headers: &HeaderMap) -> Result<Principal, ApiError> {
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(ApiError::unauthenticated)?;
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&["cordel-app"]);
        validation.set_issuer(&["cordel"]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 5;
        let claims = decode::<Claims>(
            bearer,
            &DecodingKey::from_secret(&self.signing),
            &validation,
        )
        .map_err(|_| ApiError::unauthenticated())?
        .claims;
        let account_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::unauthenticated())?;
        let session_id = Uuid::parse_str(&claims.sid).map_err(|_| ApiError::unauthenticated())?;
        let row = sqlx::query(
            "SELECT s.expires_at, m.community_id, m.role, m.kicked_until, m.banned_at, a.login \
             FROM sessions s JOIN accounts a ON a.id=s.account_id \
             JOIN memberships m ON m.account_id=a.id \
             WHERE s.id=$1 AND s.account_id=$2 AND s.revoked_at IS NULL AND s.expires_at>now()",
        )
        .bind(session_id)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::unauthenticated)?;
        let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
        let banned_at: Option<DateTime<Utc>> = row.get("banned_at");
        if banned_at.is_some() || kicked_until.is_some_and(|until| until > Utc::now()) {
            return Err(ApiError::forbidden());
        }
        Ok(Principal {
            account_id,
            session_id,
            community_id: row.get("community_id"),
            role: row.get("role"),
            login: row.get("login"),
            session_expires_at: row.get("expires_at"),
        })
    }

    async fn authenticate_admin_grant(&self, headers: &HeaderMap) -> Result<Principal, ApiError> {
        let token = bearer_token(headers).ok_or_else(ApiError::unauthenticated)?;
        if !valid_opaque_token(token) {
            return Err(ApiError::unauthenticated());
        }
        let row = sqlx::query(
            "SELECT g.account_id,g.session_id,g.community_id,s.expires_at AS session_expires_at, \
             m.role,m.kicked_until,m.banned_at,a.login \
             FROM admin_grants g JOIN sessions s ON s.id=g.session_id AND s.account_id=g.account_id \
             JOIN memberships m ON m.account_id=g.account_id AND m.community_id=g.community_id \
             JOIN accounts a ON a.id=g.account_id \
             WHERE g.token_hash=$1 AND g.revoked_at IS NULL AND g.expires_at>now() \
             AND s.revoked_at IS NULL AND s.expires_at>now()",
        )
        .bind(code_hash(token))
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::unauthenticated)?;
        let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
        let banned_at: Option<DateTime<Utc>> = row.get("banned_at");
        if row.get::<String, _>("role") != "admin"
            || banned_at.is_some()
            || kicked_until.is_some_and(|until| until > Utc::now())
        {
            return Err(ApiError::forbidden());
        }
        Ok(Principal {
            account_id: row.get("account_id"),
            session_id: row.get("session_id"),
            community_id: row.get("community_id"),
            role: "admin".to_owned(),
            login: row.get("login"),
            session_expires_at: row.get("session_expires_at"),
        })
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

fn valid_opaque_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    status: StatusCode,
    schema_version: u8,
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            schema_version: 1,
            error: ErrorBody { code, message },
        }
    }
    fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "Authentication failed",
        )
    }
    fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", "Access denied")
    }
    fn invalid() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            "Unable to complete the request",
        )
    }
    fn rate_limited() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Try again later",
        )
    }
    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "temporarily_unavailable",
            "Service temporarily unavailable",
        )
    }
    fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not found")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

fn json_error(rejection: JsonRejection) -> ApiError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "Request body too large",
        )
    } else {
        ApiError::invalid()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    sid: String,
    iat: usize,
    exp: usize,
    iss: String,
    aud: String,
}

#[derive(Debug)]
pub(crate) struct Principal {
    account_id: Uuid,
    session_id: Uuid,
    community_id: Uuid,
    role: String,
    login: String,
    session_expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct RedeemRequest {
    invite_code: String,
    login: String,
    password: String,
}
#[derive(Deserialize)]
pub struct LoginRequest {
    login: String,
    password: String,
}
#[derive(Deserialize)]
pub struct RefreshRequest {
    refresh_token: String,
}
#[derive(Deserialize)]
pub struct InviteRequest {
    expires_in_hours: Option<i64>,
    max_uses: Option<i32>,
}
#[derive(Deserialize)]
pub struct AdminGrantRequest {
    password: String,
}

#[derive(Serialize)]
struct AdminGrantResponse {
    schema_version: u8,
    admin_token: String,
    expires_in_secs: i64,
}

impl std::fmt::Debug for AdminGrantResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdminGrantResponse")
            .field("schema_version", &self.schema_version)
            .field("admin_token", &"redacted")
            .field("expires_in_secs", &self.expires_in_secs)
            .finish()
    }
}

#[derive(Serialize)]
struct SessionResponse {
    schema_version: u8,
    user_id: Uuid,
    community_id: Uuid,
    access_token: String,
    refresh_token: String,
    expires_in_secs: i64,
}
impl std::fmt::Debug for SessionResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionResponse")
            .field("schema_version", &self.schema_version)
            .field("user_id", &self.user_id)
            .field("community_id", &self.community_id)
            .field("access_token", &"redacted")
            .field("refresh_token", &"redacted")
            .field("expires_in_secs", &self.expires_in_secs)
            .finish()
    }
}

#[derive(Serialize)]
struct InviteResponse {
    schema_version: u8,
    invitation_id: Uuid,
    invite_code: String,
    expires_at: DateTime<Utc>,
    max_uses: i32,
}
impl std::fmt::Debug for InviteResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InviteResponse")
            .field("schema_version", &self.schema_version)
            .field("invitation_id", &self.invitation_id)
            .field("invite_code", &"redacted")
            .field("expires_at", &self.expires_at)
            .field("max_uses", &self.max_uses)
            .finish()
    }
}
#[derive(Debug, Serialize)]
struct MeResponse {
    schema_version: u8,
    user_id: Uuid,
    login: String,
    community_id: Uuid,
    role: String,
    session_expires_at: DateTime<Utc>,
}
#[derive(Debug, Serialize)]
struct EmptyResponse {
    schema_version: u8,
}

/// The admission gate router.
///
/// It is bound to its own loopback listener and is never forwarded to by the
/// public path. The reverse proxy calls it before it proxies the LiveKit
/// signaling upgrade, and a non-2xx answer keeps the client out of the room.
async fn registration_disabled() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "public_registration_disabled",
        "An invitation is required",
    )
}

pub fn normalize_login(login: &str) -> Option<String> {
    if !(3..=32).contains(&login.len())
        || !login
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return None;
    }
    Some(login.to_ascii_lowercase())
}

pub fn valid_password(password: &str) -> bool {
    (12..=128).contains(&password.len()) && !password.as_bytes().contains(&0)
}

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut SaltRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| ApiError::internal())
}

async fn hash_password_async(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|_| ApiError::internal())?
}

async fn verify_password_async(password: String, hash: String) -> Result<bool, ApiError> {
    tokio::task::spawn_blocking(move || verify_password(&password, &hash))
        .await
        .map_err(|_| ApiError::internal())
}

fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).ok().is_some_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

fn random_code() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn code_hash(code: &str) -> Vec<u8> {
    Sha256::digest(code.as_bytes()).to_vec()
}

fn is_unique(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}

#[cfg(test)]
mod tests;
