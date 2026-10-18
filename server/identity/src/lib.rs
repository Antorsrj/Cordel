//! ADR-002 identity boundary. This crate deliberately exposes no public admin-recovery route.

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
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration as StdDuration, Instant};
use uuid::Uuid;

const ACCESS_SECONDS: i64 = 300;
const REFRESH_DAYS: i64 = 30;
const AUTH_WINDOW: StdDuration = StdDuration::from_secs(600);

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    signing: Arc<Vec<u8>>,
    attempts: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl AppState {
    pub fn new(pool: PgPool, signing_key: Vec<u8>) -> Result<Self, &'static str> {
        if signing_key.len() < 32 {
            return Err("CORDEL_JWT_SECRET must contain at least 32 random bytes");
        }
        Ok(Self {
            pool,
            signing: Arc::new(signing_key),
            attempts: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn limit(&self, key: &str, max_per_key: usize) -> Result<(), ApiError> {
        let mut attempts = self.attempts.lock().map_err(|_| ApiError::internal())?;
        let now = Instant::now();
        // Keep bounded memory even for unique guessed logins.
        if attempts.len() > 2048 {
            attempts.retain(|_, times| {
                times.retain(|time| now.duration_since(*time) < AUTH_WINDOW);
                !times.is_empty()
            });
        }
        let global = attempts.entry("global".to_owned()).or_default();
        global.retain(|time| now.duration_since(*time) < AUTH_WINDOW);
        if global.len() >= 600 {
            return Err(ApiError::rate_limited());
        }
        global.push(now);
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
struct Principal {
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

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/accounts", post(registration_disabled))
        .route("/v1/accounts/from-invite", post(redeem))
        .route("/v1/sessions", post(login))
        .route("/v1/sessions/refresh", post(refresh))
        .route("/v1/sessions/current", delete(logout))
        .route("/v1/me", get(me))
        .route(
            "/v1/communities/{community_id}/invitations",
            post(create_invite),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found", "Not found") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed",
            )
        })
        .with_state(state)
}

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

async fn create_session(
    tx: &mut Transaction<'_, Postgres>,
    state: &AppState,
    account_id: Uuid,
    community_id: Uuid,
) -> Result<SessionResponse, ApiError> {
    let session_id = Uuid::new_v4();
    let refresh_token = random_code();
    let refresh_expires = Utc::now() + Duration::days(REFRESH_DAYS);
    sqlx::query("INSERT INTO sessions (id,account_id,expires_at) VALUES ($1,$2,$3)")
        .bind(session_id)
        .bind(account_id)
        .bind(refresh_expires)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal())?;
    sqlx::query(
        "INSERT INTO refresh_credentials (token_hash,session_id,expires_at) VALUES ($1,$2,$3)",
    )
    .bind(code_hash(&refresh_token))
    .bind(session_id)
    .bind(refresh_expires)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApiError::internal())?;
    let access_token = state.issue_access(account_id, session_id)?;
    Ok(SessionResponse {
        schema_version: 1,
        user_id: account_id,
        community_id,
        access_token,
        refresh_token,
        expires_in_secs: ACCESS_SECONDS,
    })
}

async fn redeem(
    State(state): State<AppState>,
    payload: Result<Json<RedeemRequest>, JsonRejection>,
) -> Result<Json<SessionResponse>, ApiError> {
    let body = payload.map_err(json_error)?.0;
    let login_key = normalize_login(&body.login).ok_or_else(ApiError::invalid)?;
    if !valid_password(&body.password)
        || body.invite_code.len() != 64
        || !body
            .invite_code
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError::invalid());
    }
    state.limit("invite-redemption", 100)?;
    let password_hash = hash_password_async(body.password).await?;
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    let invite = sqlx::query("SELECT id,community_id,uses_remaining,expires_at FROM invitations WHERE code_hash=$1 FOR UPDATE")
        .bind(code_hash(&body.invite_code)).fetch_optional(&mut *tx).await.map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::invalid)?;
    let uses: i32 = invite.get("uses_remaining");
    let expires: DateTime<Utc> = invite.get("expires_at");
    if uses < 1 || expires <= Utc::now() {
        return Err(ApiError::invalid());
    }
    let account_id = Uuid::new_v4();
    let community_id: Uuid = invite.get("community_id");
    let inserted =
        sqlx::query("INSERT INTO accounts (id,login,login_key,password_hash) VALUES ($1,$2,$3,$4)")
            .bind(account_id)
            .bind(&body.login)
            .bind(login_key)
            .bind(password_hash)
            .execute(&mut *tx)
            .await;
    if let Err(error) = inserted {
        if is_unique(&error) {
            return Err(ApiError::invalid());
        }
        return Err(ApiError::internal());
    }
    sqlx::query("INSERT INTO memberships (community_id,account_id,role) VALUES ($1,$2,'user')")
        .bind(community_id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    sqlx::query("UPDATE invitations SET uses_remaining=uses_remaining-1 WHERE id=$1")
        .bind(invite.get::<Uuid, _>("id"))
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    let response = create_session(&mut tx, &state, account_id, community_id).await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(response))
}

async fn login(
    State(state): State<AppState>,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Json<SessionResponse>, ApiError> {
    let body = payload.map_err(json_error)?.0;
    let key = normalize_login(&body.login).unwrap_or_default();
    state.limit(&format!("login:{key}"), 10)?;
    if key.is_empty() || body.password.len() > 128 {
        return Err(ApiError::unauthenticated());
    }
    let row = sqlx::query("SELECT a.id,a.password_hash,m.community_id,m.kicked_until,m.banned_at FROM accounts a JOIN memberships m ON m.account_id=a.id WHERE a.login_key=$1")
        .bind(key).fetch_optional(&state.pool).await.map_err(|_| ApiError::internal())?;
    let Some(row) = row else {
        // Perform an Argon2id operation for unknown logins too. The public
        // response is identical and a cheap lookup must not reveal existence.
        let _ = hash_password_async(body.password).await;
        return Err(ApiError::unauthenticated());
    };
    let hash: String = row.get("password_hash");
    if !verify_password_async(body.password, hash).await? {
        return Err(ApiError::unauthenticated());
    }
    let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
    let banned_at: Option<DateTime<Utc>> = row.get("banned_at");
    if banned_at.is_some() || kicked_until.is_some_and(|until| until > Utc::now()) {
        return Err(ApiError::forbidden());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    let response = create_session(&mut tx, &state, row.get("id"), row.get("community_id")).await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(response))
}

async fn refresh(
    State(state): State<AppState>,
    payload: Result<Json<RefreshRequest>, JsonRejection>,
) -> Result<Json<SessionResponse>, ApiError> {
    let body = payload.map_err(json_error)?.0;
    state.limit("refresh", 300)?;
    if body.refresh_token.len() != 64
        || !body
            .refresh_token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError::unauthenticated());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    let row = sqlx::query("SELECT r.session_id,r.expires_at,r.consumed_at,s.account_id,s.revoked_at,s.expires_at AS session_expires,m.community_id,m.kicked_until,m.banned_at \
        FROM refresh_credentials r JOIN sessions s ON s.id=r.session_id JOIN memberships m ON m.account_id=s.account_id \
        WHERE r.token_hash=$1 FOR UPDATE OF r,s")
        .bind(code_hash(&body.refresh_token)).fetch_optional(&mut *tx).await.map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::unauthenticated)?;
    let session_id: Uuid = row.get("session_id");
    let consumed_at: Option<DateTime<Utc>> = row.get("consumed_at");
    if consumed_at.is_some() {
        sqlx::query("UPDATE sessions SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL")
            .bind(session_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ApiError::internal())?;
        tx.commit().await.map_err(|_| ApiError::internal())?;
        return Err(ApiError::unauthenticated());
    }
    let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
    let expires_at: DateTime<Utc> = row.get("expires_at");
    let session_expires: DateTime<Utc> = row.get("session_expires");
    let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
    let banned_at: Option<DateTime<Utc>> = row.get("banned_at");
    if revoked_at.is_some() || expires_at <= Utc::now() || session_expires <= Utc::now() {
        return Err(ApiError::unauthenticated());
    }
    if banned_at.is_some() || kicked_until.is_some_and(|until| until > Utc::now()) {
        return Err(ApiError::forbidden());
    }
    let next = random_code();
    sqlx::query("UPDATE refresh_credentials SET consumed_at=now() WHERE token_hash=$1")
        .bind(code_hash(&body.refresh_token))
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    sqlx::query(
        "INSERT INTO refresh_credentials (token_hash,session_id,expires_at) VALUES ($1,$2,$3)",
    )
    .bind(code_hash(&next))
    .bind(session_id)
    .bind(session_expires)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?;
    let account_id: Uuid = row.get("account_id");
    let community_id: Uuid = row.get("community_id");
    let access_token = state.issue_access(account_id, session_id)?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(SessionResponse {
        schema_version: 1,
        user_id: account_id,
        community_id,
        access_token,
        refresh_token: next,
        expires_in_secs: ACCESS_SECONDS,
    }))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<EmptyResponse>, ApiError> {
    let principal = state.authenticate(&headers).await?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL")
        .bind(principal.session_id)
        .execute(&state.pool)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok(Json(EmptyResponse { schema_version: 1 }))
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let principal = state.authenticate(&headers).await?;
    Ok(Json(MeResponse {
        schema_version: 1,
        user_id: principal.account_id,
        login: principal.login,
        community_id: principal.community_id,
        role: principal.role,
        session_expires_at: principal.session_expires_at,
    }))
}

async fn create_invite(
    State(state): State<AppState>,
    Path(raw_community_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<InviteRequest>, JsonRejection>,
) -> Result<Json<InviteResponse>, ApiError> {
    let community_id = Uuid::parse_str(&raw_community_id).map_err(|_| ApiError::invalid())?;
    let body = payload.map_err(json_error)?.0;
    let principal = state.authenticate(&headers).await?;
    if principal.community_id != community_id || principal.role != "admin" {
        return Err(ApiError::forbidden());
    }
    let hours = body.expires_in_hours.unwrap_or(24);
    let uses = body.max_uses.unwrap_or(1);
    if !(1..=168).contains(&hours) || !(1..=25).contains(&uses) {
        return Err(ApiError::invalid());
    }
    let code = random_code();
    let expires_at = Utc::now() + Duration::hours(hours);
    let invitation_id = Uuid::new_v4();
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    // Retain the membership lock until commit so concurrent demotion cannot authorize this issue.
    let role = sqlx::query("SELECT role FROM memberships WHERE community_id=$1 AND account_id=$2 AND banned_at IS NULL AND (kicked_until IS NULL OR kicked_until<=now()) FOR SHARE")
        .bind(community_id).bind(principal.account_id).fetch_optional(&mut *tx).await.map_err(|_| ApiError::internal())?;
    if role
        .as_ref()
        .map(|row| row.get::<String, _>("role").as_str() == "admin")
        != Some(true)
    {
        return Err(ApiError::forbidden());
    }
    sqlx::query("INSERT INTO invitations (id,community_id,issued_by,code_hash,uses_remaining,expires_at) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(invitation_id).bind(community_id).bind(principal.account_id).bind(code_hash(&code))
        .bind(uses).bind(expires_at).execute(&mut *tx).await.map_err(|_| ApiError::internal())?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(InviteResponse {
        schema_version: 1,
        invitation_id,
        invite_code: code,
        expires_at,
        max_uses: uses,
    }))
}

fn is_unique(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}

pub async fn bootstrap_admin(
    pool: &PgPool,
    login: &str,
    password: &str,
    community_name: &str,
    operator: &str,
) -> Result<(), String> {
    let key =
        normalize_login(login).ok_or("Login must be 3-32 ASCII letters, digits, '_' or '-'")?;
    if !valid_password(password) {
        return Err("Password must be 12-128 bytes".to_owned());
    }
    if community_name.is_empty() || community_name.chars().count() > 100 {
        return Err("Community name must be 1-100 characters".to_owned());
    }
    let hash = hash_password(password).map_err(|_| "Password hashing failed")?;
    let mut tx = pool.begin().await.map_err(|_| "Database unavailable")?;
    sqlx::query("SELECT pg_advisory_xact_lock(721002)")
        .execute(&mut *tx)
        .await
        .map_err(|_| "Database unavailable")?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM communities")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "Database unavailable")?;
    if count != 0 {
        return Err("Community already exists; bootstrap refused".to_owned());
    }
    let community_id = Uuid::new_v4();
    let account_id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id,name) VALUES ($1,$2)")
        .bind(community_id)
        .bind(community_name)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Bootstrap failed")?;
    sqlx::query("INSERT INTO accounts (id,login,login_key,password_hash) VALUES ($1,$2,$3,$4)")
        .bind(account_id)
        .bind(login)
        .bind(key)
        .bind(hash)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Bootstrap failed")?;
    sqlx::query("INSERT INTO memberships (community_id,account_id,role) VALUES ($1,$2,'admin')")
        .bind(community_id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Bootstrap failed")?;
    sqlx::query("INSERT INTO host_audit (id,operator_name,action,account_id,community_id) VALUES ($1,$2,'bootstrap_admin',$3,$4)")
        .bind(Uuid::new_v4()).bind(operator).bind(account_id).bind(community_id).execute(&mut *tx).await.map_err(|_| "Bootstrap failed")?;
    tx.commit().await.map_err(|_| "Bootstrap failed")?;
    Ok(())
}

pub async fn recover_admin(
    pool: &PgPool,
    login: &str,
    password: &str,
    operator: &str,
) -> Result<(), String> {
    let key = normalize_login(login).ok_or("Invalid login")?;
    if !valid_password(password) {
        return Err("Password must be 12-128 bytes".to_owned());
    }
    let hash = hash_password(password).map_err(|_| "Password hashing failed")?;
    let mut tx = pool.begin().await.map_err(|_| "Database unavailable")?;
    let row = sqlx::query("SELECT a.id,m.community_id FROM accounts a JOIN memberships m ON m.account_id=a.id WHERE a.login_key=$1 AND m.role='admin' FOR UPDATE OF a,m")
        .bind(key).fetch_optional(&mut *tx).await.map_err(|_| "Database unavailable")?
        .ok_or("Target must be an existing admin")?;
    let account_id: Uuid = row.get("id");
    let community_id: Uuid = row.get("community_id");
    sqlx::query("UPDATE accounts SET password_hash=$1,credential_changed_at=now() WHERE id=$2")
        .bind(hash)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Recovery failed")?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE account_id=$1 AND revoked_at IS NULL")
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Recovery failed")?;
    sqlx::query("INSERT INTO host_audit (id,operator_name,action,account_id,community_id) VALUES ($1,$2,'recover_admin',$3,$4)")
        .bind(Uuid::new_v4()).bind(operator).bind(account_id).bind(community_id).execute(&mut *tx).await.map_err(|_| "Recovery failed")?;
    tx.commit().await.map_err(|_| "Recovery failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    #[test]
    fn login_and_password_rules() {
        assert_eq!(normalize_login("Alice-1"), Some("alice-1".to_owned()));
        assert!(normalize_login("a!").is_none());
        assert!(valid_password("correct horse battery staple"));
        assert!(!valid_password("short"));
    }

    #[test]
    fn password_hash_is_argon2id_and_not_plaintext() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn refresh_and_invite_values_are_random_and_hashed() {
        let first = random_code();
        let second = random_code();
        assert_ne!(first, second);
        assert_eq!(first.len(), 64);
        assert_eq!(code_hash(&first).len(), 32);
        assert_ne!(code_hash(&first), code_hash(&second));
    }

    #[tokio::test]
    async fn limiter_rejects_excess_attempts() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@localhost/unused")
            .unwrap();
        let state = AppState::new(pool, vec![7; 32]).unwrap();
        for _ in 0..10 {
            state.limit("login:guess", 10).unwrap();
        }
        assert_eq!(
            state.limit("login:guess", 10).unwrap_err().status,
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn public_registration_and_bad_json_use_v1_error_envelope() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@localhost/unused")
            .unwrap();
        let app = router(AppState::new(pool, vec![7; 32]).unwrap());
        let request = Request::builder()
            .method("POST")
            .uri("/v1/accounts")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["error"]["code"], "public_registration_disabled");

        let request = Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{broken-json"))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["error"]["code"], "invalid_input");

        let request = Request::builder()
            .method("GET")
            .uri("/v1/accounts")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["error"]["code"], "method_not_allowed");

        let request = Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("x".repeat(17 * 1024)))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["error"]["code"], "payload_too_large");
    }

    #[tokio::test]
    #[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
    async fn disposable_database_identity_flow() {
        let url = std::env::var("CORDEL_TEST_DATABASE_URL")
            .expect("set CORDEL_TEST_DATABASE_URL to a disposable database");
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(&url)
            .await
            .unwrap();
        let db_name: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            db_name.starts_with("cordel_identity_test_"),
            "refusing to clear a non-disposable database"
        );
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("TRUNCATE host_audit,refresh_credentials,sessions,invitations,memberships,accounts,communities CASCADE")
            .execute(&pool).await.unwrap();
        let state = AppState::new(pool.clone(), vec![9; 32]).unwrap();
        bootstrap_admin(
            &pool,
            "Admin",
            "correct horse battery staple",
            "Test community",
            "test-operator",
        )
        .await
        .unwrap();
        assert!(bootstrap_admin(
            &pool,
            "Other",
            "correct horse battery staple",
            "Another",
            "test-operator"
        )
        .await
        .is_err());

        let app = router(state.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"login":"Admin","password":"correct horse battery staple"}"#,
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let session: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(session["schema_version"], 1);
        assert_eq!(session["expires_in_secs"], ACCESS_SECONDS);
        let access_token = session["access_token"].as_str().unwrap();
        let request = Request::builder()
            .method("GET")
            .uri("/v1/me")
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let me_body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(me_body["schema_version"], 1);
        assert_eq!(me_body["role"], "admin");

        let admin = login(
            State(state.clone()),
            Ok(Json(LoginRequest {
                login: "Admin".into(),
                password: "correct horse battery staple".into(),
            })),
        )
        .await
        .unwrap()
        .0;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", admin.access_token).parse().unwrap(),
        );
        let invite = create_invite(
            State(state.clone()),
            Path(admin.community_id.to_string()),
            headers.clone(),
            Ok(Json(InviteRequest {
                expires_in_hours: Some(24),
                max_uses: Some(1),
            })),
        )
        .await
        .unwrap()
        .0;
        let code = invite.invite_code.clone();
        let first = redeem(
            State(state.clone()),
            Ok(Json(RedeemRequest {
                invite_code: code.clone(),
                login: "Alice".into(),
                password: "correct horse battery staple".into(),
            })),
        );
        let second = redeem(
            State(state.clone()),
            Ok(Json(RedeemRequest {
                invite_code: code.clone(),
                login: "Bob".into(),
                password: "correct horse battery staple".into(),
            })),
        );
        let (a, b) = tokio::join!(first, second);
        assert_eq!(
            usize::from(a.is_ok()) + usize::from(b.is_ok()),
            1,
            "single-use invite admitted more than one account"
        );
        let winner = a.ok().or_else(|| b.ok()).unwrap().0;
        let replay = redeem(
            State(state.clone()),
            Ok(Json(RedeemRequest {
                invite_code: code,
                login: "Third".into(),
                password: "correct horse battery staple".into(),
            })),
        )
        .await;
        assert_eq!(replay.unwrap_err().error.code, "invalid_input");
        let user_headers = {
            let mut value = HeaderMap::new();
            value.insert(
                header::AUTHORIZATION,
                format!("Bearer {}", winner.access_token).parse().unwrap(),
            );
            value
        };
        let denied = create_invite(
            State(state.clone()),
            Path(admin.community_id.to_string()),
            user_headers,
            Ok(Json(InviteRequest {
                expires_in_hours: None,
                max_uses: None,
            })),
        )
        .await;
        assert_eq!(denied.unwrap_err().status, StatusCode::FORBIDDEN);

        let collision_invite = create_invite(
            State(state.clone()),
            Path(admin.community_id.to_string()),
            headers.clone(),
            Ok(Json(InviteRequest {
                expires_in_hours: None,
                max_uses: None,
            })),
        )
        .await
        .unwrap()
        .0;
        let collision = redeem(
            State(state.clone()),
            Ok(Json(RedeemRequest {
                invite_code: collision_invite.invite_code.clone(),
                login: "Admin".into(),
                password: "correct horse battery staple".into(),
            })),
        )
        .await;
        assert_eq!(collision.unwrap_err().error.code, "invalid_input");
        let remaining: i32 =
            sqlx::query_scalar("SELECT uses_remaining FROM invitations WHERE id=$1")
                .bind(collision_invite.invitation_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 1, "username collision consumed the invite");
        sqlx::query("UPDATE invitations SET expires_at=now()-interval '1 second' WHERE id=$1")
            .bind(collision_invite.invitation_id)
            .execute(&pool)
            .await
            .unwrap();
        let expired = redeem(
            State(state.clone()),
            Ok(Json(RedeemRequest {
                invite_code: collision_invite.invite_code,
                login: "Unused".into(),
                password: "correct horse battery staple".into(),
            })),
        )
        .await;
        assert_eq!(expired.unwrap_err().error.code, "invalid_input");

        let rotated = refresh(
            State(state.clone()),
            Ok(Json(RefreshRequest {
                refresh_token: winner.refresh_token.clone(),
            })),
        )
        .await
        .unwrap()
        .0;
        let reuse = refresh(
            State(state.clone()),
            Ok(Json(RefreshRequest {
                refresh_token: winner.refresh_token,
            })),
        )
        .await;
        assert_eq!(reuse.unwrap_err().status, StatusCode::UNAUTHORIZED);
        let revoked = refresh(
            State(state.clone()),
            Ok(Json(RefreshRequest {
                refresh_token: rotated.refresh_token,
            })),
        )
        .await;
        assert_eq!(revoked.unwrap_err().status, StatusCode::UNAUTHORIZED);

        let again = login(
            State(state.clone()),
            Ok(Json(LoginRequest {
                login: "Alice".into(),
                password: "correct horse battery staple".into(),
            })),
        )
        .await;
        let active_user = if again.is_ok() { "Alice" } else { "Bob" };
        let active = login(
            State(state.clone()),
            Ok(Json(LoginRequest {
                login: active_user.into(),
                password: "correct horse battery staple".into(),
            })),
        )
        .await
        .unwrap()
        .0;
        let mut active_headers = HeaderMap::new();
        active_headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", active.access_token).parse().unwrap(),
        );
        let _ = logout(State(state.clone()), active_headers).await.unwrap();
        assert_eq!(
            refresh(
                State(state.clone()),
                Ok(Json(RefreshRequest {
                    refresh_token: active.refresh_token
                }))
            )
            .await
            .unwrap_err()
            .status,
            StatusCode::UNAUTHORIZED
        );

        recover_admin(
            &pool,
            "Admin",
            "new correct horse battery staple",
            "test-operator",
        )
        .await
        .unwrap();
        assert_eq!(
            me(State(state.clone()), headers).await.unwrap_err().status,
            StatusCode::UNAUTHORIZED
        );
        assert!(login(
            State(state.clone()),
            Ok(Json(LoginRequest {
                login: "Admin".into(),
                password: "correct horse battery staple".into()
            }))
        )
        .await
        .is_err());
        assert!(login(
            State(state.clone()),
            Ok(Json(LoginRequest {
                login: "Admin".into(),
                password: "new correct horse battery staple".into()
            }))
        )
        .await
        .is_ok());
        assert_eq!(
            registration_disabled().await.error.code,
            "public_registration_disabled"
        );
        let stored_hash: String =
            sqlx::query_scalar("SELECT password_hash FROM accounts WHERE login_key='admin'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            stored_hash.starts_with("$argon2id$") && !stored_hash.contains("new correct horse")
        );
        let invitation_hash: Vec<u8> =
            sqlx::query_scalar("SELECT code_hash FROM invitations LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(invitation_hash.len(), 32);
        let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM host_audit")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(audit_count, 2);
    }
}
