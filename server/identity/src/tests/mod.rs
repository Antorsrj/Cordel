use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

mod database;

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
