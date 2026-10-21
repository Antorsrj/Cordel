//! ADR-005 media admission and LiveKit webhook checks.
//!
//! These need a disposable PostgreSQL database, like the other ignored flows.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use jsonwebtoken::Algorithm;
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const API_KEY: &str = "devkey";
const API_SECRET: &str = "local-livekit-secret-with-32-bytes-minimum";

/// The LiveKit key and secret. The environment overrides the defaults so a
/// test can point at a running LiveKit with its own credentials.
fn livekit_credentials() -> (String, String) {
    let api_key =
        std::env::var("CORDEL_TEST_LIVEKIT_API_KEY").unwrap_or_else(|_| API_KEY.to_owned());
    let api_secret =
        std::env::var("CORDEL_TEST_LIVEKIT_API_SECRET").unwrap_or_else(|_| API_SECRET.to_owned());
    (api_key, api_secret)
}

fn media_config() -> MediaConfig {
    let (api_key, api_secret) = livekit_credentials();
    let url = std::env::var("CORDEL_TEST_LIVEKIT_URL")
        .unwrap_or_else(|_| "ws://127.0.0.1:7880".to_owned());
    MediaConfig::new(api_key, api_secret, url).unwrap()
}

async fn call(
    state: &AppState,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(value) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {value}"));
    }
    let response = router(state.clone())
        .oneshot(
            request
                .body(Body::from(body.unwrap_or_default().to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap();
    (status, value)
}

async fn call_raw(
    state: &AppState,
    path: &str,
    signature: Option<&str>,
    body: &[u8],
) -> StatusCode {
    let mut request = Request::builder().method("POST").uri(path);
    if let Some(value) = signature {
        request = request.header(header::AUTHORIZATION, value);
    }
    let response = router(state.clone())
        .oneshot(request.body(Body::from(body.to_vec())).unwrap())
        .await
        .unwrap();
    response.status()
}

async fn login(state: &AppState, login: &str) -> String {
    let body = format!(r#"{{"login":"{login}","password":"correct horse battery staple"}}"#);
    let (status, value) = call(state, "POST", "/v1/sessions", None, Some(&body)).await;
    assert_eq!(status, StatusCode::OK);
    value["access_token"].as_str().unwrap().to_owned()
}

/// Sign a webhook the way LiveKit does: a JWT with the API secret whose
/// `sha256` claim pins the raw body.
fn sign_webhook(body: &[u8]) -> String {
    #[derive(Serialize)]
    struct Claims {
        exp: i64,
        nbf: i64,
        iat: i64,
        iss: String,
        sha256: String,
    }
    let now = Utc::now().timestamp();
    let (api_key, api_secret) = livekit_credentials();
    let claims = Claims {
        exp: now + 300,
        nbf: now,
        iat: now,
        iss: api_key,
        sha256: BASE64.encode(sha2::Sha256::digest(body)),
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(api_secret.as_bytes()),
    )
    .unwrap()
}

#[derive(Debug, Deserialize)]
struct TokenClaims {
    sub: String,
    iss: String,
    video: VideoClaim,
}

#[derive(Debug, Deserialize)]
struct VideoClaim {
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
    #[serde(rename = "roomAdmin")]
    room_admin: bool,
}

async fn seed() -> (PgPool, Uuid, Uuid) {
    let url = std::env::var("CORDEL_TEST_DATABASE_URL").expect("disposable database URL required");
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        name.starts_with("cordel_identity_test_"),
        "refusing a non-disposable database"
    );
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("TRUNCATE messages,community_audit,channels,admin_audit,admin_grants,host_audit,refresh_credentials,sessions,invitations,memberships,accounts,communities CASCADE")
        .execute(&pool).await.unwrap();
    bootstrap_admin(
        &pool,
        "admin",
        "correct horse battery staple",
        "Test",
        "test-operator",
    )
    .await
    .unwrap();
    let community_id: Uuid = sqlx::query_scalar("SELECT id FROM communities")
        .fetch_one(&pool)
        .await
        .unwrap();
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO accounts (id,login,login_key,password_hash) VALUES ($1,'user','user',$2)",
    )
    .bind(user_id)
    .bind(hash_password("correct horse battery staple").unwrap())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO memberships (community_id,account_id,role) VALUES ($1,$2,'user')")
        .bind(community_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    (pool, community_id, user_id)
}

async fn create_channel(state: &AppState, community_id: Uuid, token: &str, kind: &str) -> Uuid {
    let (status, channel) = call(
        state,
        "POST",
        &format!("/v1/communities/{community_id}/channels"),
        Some(token),
        Some(&format!(r#"{{"kind":"{kind}","name":"room"}}"#)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    Uuid::parse_str(channel["channel_id"].as_str().unwrap()).unwrap()
}

mod admission;
mod gate;
