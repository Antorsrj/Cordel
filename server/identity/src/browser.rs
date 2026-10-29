//! ADR-009 browser admission. Native bearer requests never enter this path.
//!
//! The session secret stays in an HttpOnly cookie. JavaScript receives only a
//! CSRF value, and a browser WebSocket proves that value in its first message.

use axum::extract::rejection::JsonRejection;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Duration;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

use super::*;

const PRE_COOKIE: &str = "cordel_pre";
const SESSION_COOKIE: &str = "cordel_session";
const PRE_SECONDS: i64 = 600;
const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");
const FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

pub(crate) enum SocketStart {
    Ready(Principal),
    Browser { principal: Principal, csrf: String },
}

pub(crate) struct CookieAuth {
    pub(crate) principal: Principal,
    csrf: String,
}

#[derive(Serialize)]
struct CsrfBody {
    schema_version: u8,
    csrf_token: String,
}

#[derive(Serialize)]
struct BrowserSessionBody {
    schema_version: u8,
    user_id: Uuid,
    community_id: Uuid,
    csrf_token: String,
    expires_in_secs: i64,
}

pub(crate) fn fetch_site_ok(value: Option<&str>) -> bool {
    matches!(value, None | Some("same-origin") | Some("none"))
}

pub(crate) fn csrf_matches(text: &str, expected: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    value.get("schema_version").and_then(Value::as_u64) == Some(1)
        && value.get("type").and_then(Value::as_str) == Some("browser.authenticate")
        && value.get("csrf_token").and_then(Value::as_str) == Some(expected)
}

pub(super) async fn csrf(
    State(state): State<AppState>,
    ClientIp(address): ClientIp,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    guard(&state, &headers)?;
    state.limit_address(address, "browser-csrf", 30, 60)?;
    if cookie_value(&headers, SESSION_COOKIE).is_some() {
        let auth = authenticate_cookie(&state, &headers, false).await?;
        return Ok(Json(CsrfBody {
            schema_version: 1,
            csrf_token: auth.csrf,
        })
        .into_response());
    }
    let nonce = random_code();
    let csrf_token = random_code();
    let expires = Utc::now() + Duration::seconds(PRE_SECONDS);
    sqlx::query(
        "INSERT INTO browser_pre_sessions (id,nonce_hash,csrf_token,expires_at) VALUES ($1,$2,$3,$4)",
    )
    .bind(Uuid::new_v4())
    .bind(code_hash(&nonce))
    .bind(&csrf_token)
    .bind(expires)
    .execute(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?;
    Ok(with_cookies(
        Json(CsrfBody {
            schema_version: 1,
            csrf_token,
        })
        .into_response(),
        &[set_cookie(PRE_COOKIE, &nonce, PRE_SECONDS)],
    ))
}

pub(super) async fn login_route(
    State(state): State<AppState>,
    ClientIp(address): ClientIp,
    headers: HeaderMap,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Response {
    if !browser_attempt(&headers) {
        return login(State(state), ClientIp(address), payload)
            .await
            .into_response();
    }
    browser_login(state, address, headers, payload)
        .await
        .into_response()
}

pub(super) async fn redeem_route(
    State(state): State<AppState>,
    ClientIp(address): ClientIp,
    headers: HeaderMap,
    payload: Result<Json<RedeemRequest>, JsonRejection>,
) -> Response {
    if !browser_attempt(&headers) {
        return redeem(State(state), ClientIp(address), payload)
            .await
            .into_response();
    }
    browser_redeem(state, address, headers, payload)
        .await
        .into_response()
}

pub(super) async fn refresh_route(
    State(state): State<AppState>,
    ClientIp(address): ClientIp,
    headers: HeaderMap,
    payload: Result<Json<RefreshRequest>, JsonRejection>,
) -> Response {
    if cookie_value(&headers, SESSION_COOKIE).is_none() {
        return refresh(State(state), ClientIp(address), payload)
            .await
            .into_response();
    }
    browser_refresh(state, address, headers)
        .await
        .into_response()
}

pub(super) async fn logout_route(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let browser = cookie_value(&headers, SESSION_COOKIE).is_some();
    match logout(State(state), headers).await {
        Ok(body) if browser => with_cookies(
            body.into_response(),
            &[clear_cookie(SESSION_COOKIE), clear_cookie(PRE_COOKIE)],
        ),
        Ok(body) => body.into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn authenticate_cookie(
    state: &AppState,
    headers: &HeaderMap,
    csrf_required: bool,
) -> Result<CookieAuth, ApiError> {
    guard(state, headers)?;
    let token = cookie_value(headers, SESSION_COOKIE)
        .filter(|value| valid_opaque_token(value))
        .ok_or_else(ApiError::unauthenticated)?;
    let row = sqlx::query(
        "SELECT s.id,s.expires_at,s.browser_csrf,m.community_id,m.role,m.kicked_until,m.banned_at,a.login,a.id AS account_id \
         FROM refresh_credentials r \
         JOIN sessions s ON s.id=r.session_id \
         JOIN accounts a ON a.id=s.account_id \
         JOIN memberships m ON m.account_id=s.account_id \
         WHERE r.token_hash=$1 AND r.consumed_at IS NULL AND r.expires_at>now() \
         AND s.revoked_at IS NULL AND s.expires_at>now()",
    )
    .bind(code_hash(token))
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(ApiError::unauthenticated)?;
    let kicked_until: Option<chrono::DateTime<Utc>> = row.get("kicked_until");
    let banned_at: Option<chrono::DateTime<Utc>> = row.get("banned_at");
    if banned_at.is_some() || kicked_until.is_some_and(|until| until > Utc::now()) {
        return Err(ApiError::forbidden());
    }
    let csrf: Option<String> = row.get("browser_csrf");
    let csrf = csrf.ok_or_else(ApiError::unauthenticated)?;
    if csrf_required {
        let presented = headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(csrf_rejected)?;
        if presented != csrf {
            return Err(csrf_rejected());
        }
    }
    Ok(CookieAuth {
        principal: Principal {
            account_id: row.get("account_id"),
            session_id: row.get("id"),
            community_id: row.get("community_id"),
            role: row.get("role"),
            login: row.get("login"),
            session_expires_at: row.get("expires_at"),
        },
        csrf,
    })
}

pub(crate) async fn open_socket(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<SocketStart, ApiError> {
    if headers.get(header::AUTHORIZATION).is_some() {
        return Ok(SocketStart::Ready(
            state.authenticate_bearer(headers).await?,
        ));
    }
    let auth = authenticate_cookie(state, headers, false).await?;
    Ok(SocketStart::Browser {
        csrf: auth.csrf,
        principal: auth.principal,
    })
}

async fn browser_login(
    state: AppState,
    address: Option<std::net::IpAddr>,
    headers: HeaderMap,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    guard(&state, &headers)?;
    confirm_pre(&state, &headers).await?;
    let created = login(State(state.clone()), ClientIp(address), payload).await?;
    finish_browser(&state, &headers, &created.0).await
}

async fn browser_redeem(
    state: AppState,
    address: Option<std::net::IpAddr>,
    headers: HeaderMap,
    payload: Result<Json<RedeemRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    guard(&state, &headers)?;
    confirm_pre(&state, &headers).await?;
    let created = redeem(State(state.clone()), ClientIp(address), payload).await?;
    finish_browser(&state, &headers, &created.0).await
}

async fn browser_refresh(
    state: AppState,
    address: Option<std::net::IpAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let auth = authenticate_cookie(&state, &headers, true).await?;
    let token = cookie_value(&headers, SESSION_COOKIE)
        .ok_or_else(ApiError::unauthenticated)?
        .to_owned();
    let rotated = refresh(
        State(state.clone()),
        ClientIp(address),
        Ok(Json(RefreshRequest {
            refresh_token: token,
        })),
    )
    .await?;
    let csrf_token = random_code();
    sqlx::query("UPDATE sessions SET browser_csrf=$1 WHERE id=$2 AND revoked_at IS NULL")
        .bind(&csrf_token)
        .bind(auth.principal.session_id)
        .execute(&state.pool)
        .await
        .map_err(|_| ApiError::internal())?;
    let max_age = REFRESH_DAYS * 24 * 60 * 60;
    Ok(with_cookies(
        Json(BrowserSessionBody {
            schema_version: 1,
            user_id: rotated.0.user_id,
            community_id: rotated.0.community_id,
            csrf_token,
            expires_in_secs: max_age,
        })
        .into_response(),
        &[set_cookie(
            SESSION_COOKIE,
            &rotated.0.refresh_token,
            max_age,
        )],
    ))
}

async fn confirm_pre(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let nonce = cookie_value(headers, PRE_COOKIE)
        .filter(|value| valid_opaque_token(value))
        .ok_or_else(csrf_rejected)?;
    let presented = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(csrf_rejected)?;
    let found = sqlx::query(
        "SELECT 1 FROM browser_pre_sessions \
         WHERE nonce_hash=$1 AND csrf_token=$2 AND consumed_at IS NULL AND expires_at>now()",
    )
    .bind(code_hash(nonce))
    .bind(presented)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?;
    if found.is_none() {
        return Err(csrf_rejected());
    }
    Ok(())
}

async fn finish_browser(
    state: &AppState,
    headers: &HeaderMap,
    created: &SessionResponse,
) -> Result<Response, ApiError> {
    let nonce = cookie_value(headers, PRE_COOKIE).ok_or_else(csrf_rejected)?;
    let presented = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(csrf_rejected)?;
    let csrf_token = random_code();
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    let consumed = sqlx::query(
        "UPDATE browser_pre_sessions SET consumed_at=now() \
         WHERE nonce_hash=$1 AND csrf_token=$2 AND consumed_at IS NULL AND expires_at>now()",
    )
    .bind(code_hash(nonce))
    .bind(presented)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?;
    if consumed.rows_affected() != 1 {
        return Err(csrf_rejected());
    }
    let session_id: Uuid = sqlx::query(
        "SELECT session_id FROM refresh_credentials WHERE token_hash=$1 AND consumed_at IS NULL",
    )
    .bind(code_hash(&created.refresh_token))
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?
    .get("session_id");
    sqlx::query("UPDATE sessions SET browser_csrf=$1 WHERE id=$2")
        .bind(&csrf_token)
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    let max_age = REFRESH_DAYS * 24 * 60 * 60;
    Ok(with_cookies(
        Json(BrowserSessionBody {
            schema_version: 1,
            user_id: created.user_id,
            community_id: created.community_id,
            csrf_token,
            expires_in_secs: max_age,
        })
        .into_response(),
        &[
            set_cookie(SESSION_COOKIE, &created.refresh_token, max_age),
            clear_cookie(PRE_COOKIE),
        ],
    ))
}

fn browser_attempt(headers: &HeaderMap) -> bool {
    headers.get(CSRF_HEADER).is_some() || cookie_value(headers, PRE_COOKIE).is_some()
}

fn guard(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = state
        .browser_origin
        .as_deref()
        .ok_or_else(browser_disabled)?;
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin != Some(expected) {
        return Err(origin_rejected());
    }
    let site = headers
        .get(FETCH_SITE)
        .and_then(|value| value.to_str().ok());
    if !fetch_site_ok(site) {
        return Err(origin_rejected());
    }
    Ok(())
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

fn set_cookie(name: &str, value: &str, max_age: i64) -> String {
    format!("{name}={value}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={max_age}")
}

fn clear_cookie(name: &str) -> String {
    format!("{name}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0")
}

fn with_cookies(mut response: Response, cookies: &[String]) -> Response {
    for cookie in cookies {
        let Ok(value) = HeaderValue::from_str(cookie) else {
            continue;
        };
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

fn browser_disabled() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "browser_disabled",
        "Browser admission is not configured",
    )
}

fn origin_rejected() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "origin_rejected",
        "Origin is not allowed",
    )
}

fn csrf_rejected() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "csrf_rejected",
        "CSRF validation failed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_site_rejects_another_site() {
        assert!(fetch_site_ok(None));
        assert!(fetch_site_ok(Some("same-origin")));
        assert!(fetch_site_ok(Some("none")));
        assert!(!fetch_site_ok(Some("cross-site")));
        assert!(!fetch_site_ok(Some("same-site")));
    }

    #[test]
    fn first_socket_message_must_carry_the_session_csrf() {
        let csrf = "ab".repeat(32);
        let ok = format!(
            r#"{{"schema_version":1,"type":"browser.authenticate","csrf_token":"{csrf}"}}"#
        );
        assert!(csrf_matches(&ok, &csrf));
        assert!(!csrf_matches(&ok, &"cd".repeat(32)));
        assert!(!csrf_matches(
            r#"{"schema_version":1,"type":"ready"}"#,
            &csrf
        ));
    }
}
