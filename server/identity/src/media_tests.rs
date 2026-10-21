//! ADR-005 media admission and LiveKit webhook checks.
//!
//! These need a disposable PostgreSQL database, like the other ignored flows.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const API_KEY: &str = "devkey";
const API_SECRET: &str = "local-livekit-secret-with-32-bytes-minimum";

fn media_config() -> MediaConfig {
    MediaConfig::new(
        API_KEY.to_owned(),
        API_SECRET.to_owned(),
        "ws://127.0.0.1:7880".to_owned(),
    )
    .unwrap()
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
    let claims = Claims {
        exp: now + 300,
        nbf: now,
        iat: now,
        iss: API_KEY.to_owned(),
        sha256: BASE64.encode(sha2::Sha256::digest(body)),
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(API_SECRET.as_bytes()),
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

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_media_admission_flow() {
    let (pool, community_id, user_id) = seed().await;
    let state = AppState::new(pool.clone(), vec![7; 32])
        .unwrap()
        .with_media(media_config());
    let user_token = login(&state, "user").await;

    let voice = create_channel(&state, community_id, &user_token, "voice").await;
    let text = create_channel(&state, community_id, &user_token, "text").await;

    // A member of a voice channel gets one scoped admission.
    let (status, admission) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(admission["schema_version"], 1);
    assert_eq!(admission["voice_channel_id"], voice.to_string());
    assert_eq!(admission["media_generation"], 0);
    assert_eq!(admission["url"], "ws://127.0.0.1:7880");
    assert_eq!(
        admission["room"],
        format!("voice:{voice}:0"),
        "the room name carries the current generation"
    );
    assert_eq!(admission["expires_in_secs"], 300);
    let token = admission["token"].as_str().unwrap().to_owned();
    assert!(!token.is_empty());

    // The token grants one account one room and no data track.
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[API_KEY]);
    let claims = decode::<TokenClaims>(
        &token,
        &DecodingKey::from_secret(API_SECRET.as_bytes()),
        &validation,
    )
    .unwrap()
    .claims;
    assert_eq!(claims.sub, user_id.to_string());
    assert_eq!(claims.iss, API_KEY);
    assert_eq!(claims.video.room, format!("voice:{voice}:0"));
    assert!(claims.video.room_join);
    assert!(claims.video.can_publish);
    assert!(claims.video.can_subscribe);
    assert!(
        !claims.video.can_publish_data,
        "chat never rides a data track"
    );
    assert!(!claims.video.room_create);
    assert!(!claims.video.room_admin);

    // A text channel has no media admission.
    let (status, error) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{text}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "forbidden");

    // An unknown channel is not found.
    let (status, error) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{}/admission", Uuid::new_v4()),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"]["code"], "not_found");

    // A malformed channel id is invalid input.
    let (status, error) = call(
        &state,
        "POST",
        "/v1/voice-channels/not-a-uuid/admission",
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "invalid_input");

    // No session means no admission.
    let (status, error) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"]["code"], "unauthenticated");

    // A moderation action advances the generation, so a later admission names
    // a different room.
    sqlx::query(
        "UPDATE communities SET authorization_revision=authorization_revision+1 WHERE id=$1",
    )
    .bind(community_id)
    .execute(&pool)
    .await
    .unwrap();
    let (status, admission) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(admission["media_generation"], 1);
    assert_eq!(admission["room"], format!("voice:{voice}:1"));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_media_admission_refuses_moderated_members() {
    let (pool, community_id, user_id) = seed().await;
    let state = AppState::new(pool.clone(), vec![7; 32])
        .unwrap()
        .with_media(media_config());
    let user_token = login(&state, "user").await;
    let voice = create_channel(&state, community_id, &user_token, "voice").await;

    // A kicked member cannot get an admission.
    sqlx::query(
        "UPDATE memberships SET kicked_until=now()+interval '30 minutes' WHERE community_id=$1 AND account_id=$2",
    )
    .bind(community_id)
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    let (status, error) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "forbidden");

    // A banned member cannot get an admission either.
    sqlx::query(
        "UPDATE memberships SET kicked_until=NULL, banned_at=now() WHERE community_id=$1 AND account_id=$2",
    )
    .bind(community_id)
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    let (status, error) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "forbidden");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_livekit_webhook_flow() {
    let (pool, _community_id, _user_id) = seed().await;
    let state = AppState::new(pool.clone(), vec![7; 32])
        .unwrap()
        .with_media(media_config());

    let body = br#"{"event":"participant_joined","room":{"name":"voice:room:0"},"participant":{"identity":"user-1","state":"ACTIVE"},"id":"EV_one"}"#;
    let signature = sign_webhook(body);
    assert_eq!(
        call_raw(&state, "/v1/livekit/webhook", Some(&signature), body).await,
        StatusCode::OK
    );
    let observed = state.media_observation();
    let room = observed.get("voice:room:0").expect("the room was observed");
    assert!(room.participants.contains_key("user-1"));

    // The same event id does not apply twice.
    assert_eq!(
        call_raw(&state, "/v1/livekit/webhook", Some(&signature), body).await,
        StatusCode::OK
    );
    assert_eq!(
        state
            .media_observation()
            .get("voice:room:0")
            .unwrap()
            .participants
            .len(),
        1
    );

    // A track publication is observed on the participant.
    let track_body = br#"{"event":"track_published","room":{"name":"voice:room:0"},"participant":{"identity":"user-1"},"track":{"sid":"TR_1","name":"screen","source":"SCREEN_SHARE"},"id":"EV_two"}"#;
    let track_signature = sign_webhook(track_body);
    assert_eq!(
        call_raw(
            &state,
            "/v1/livekit/webhook",
            Some(&track_signature),
            track_body
        )
        .await,
        StatusCode::OK
    );
    let room = state.media_observation();
    let room = room.get("voice:room:0").unwrap();
    let participant = room.participants.get("user-1").unwrap();
    assert_eq!(participant.tracks.len(), 1);
    assert_eq!(participant.tracks["TR_1"].source, "SCREEN_SHARE");

    // A body that does not match the signature is refused.
    let other = br#"{"event":"participant_joined","room":{"name":"voice:evil:0"},"participant":{"identity":"attacker"},"id":"EV_three"}"#;
    assert_eq!(
        call_raw(&state, "/v1/livekit/webhook", Some(&signature), other).await,
        StatusCode::UNAUTHORIZED
    );
    assert!(!state.media_observation().contains_key("voice:evil:0"));

    // A missing signature is refused.
    assert_eq!(
        call_raw(&state, "/v1/livekit/webhook", None, body).await,
        StatusCode::UNAUTHORIZED
    );

    // A signature made with the wrong secret is refused.
    #[derive(Serialize)]
    struct Claims {
        exp: i64,
        iss: String,
        sha256: String,
    }
    let now = Utc::now().timestamp();
    let forged = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &Claims {
            exp: now + 300,
            iss: API_KEY.to_owned(),
            sha256: BASE64.encode(sha2::Sha256::digest(body)),
        },
        &jsonwebtoken::EncodingKey::from_secret(b"a-different-secret-of-32-bytes-long"),
    )
    .unwrap();
    assert_eq!(
        call_raw(&state, "/v1/livekit/webhook", Some(&forged), body).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_media_routes_need_a_configured_service() {
    let (pool, community_id, _user_id) = seed().await;
    let state = AppState::new(pool.clone(), vec![7; 32]).unwrap();
    let user_token = login(&state, "user").await;
    let voice = create_channel(&state, community_id, &user_token, "voice").await;

    // Without a LiveKit service the media routes do not exist, so a
    // deployment that does not serve media is unchanged.
    let (status, _) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let status = call_raw(&state, "/v1/livekit/webhook", None, b"{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
