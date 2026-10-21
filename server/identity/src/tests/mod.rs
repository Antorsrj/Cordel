use super::*;
use axum::body::{to_bytes, Body};
use axum::extract::FromRequestParts;
use axum::http::Request;
use sqlx::postgres::PgPoolOptions;
use std::net::{IpAddr, SocketAddr};
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

/// One noisy address must not be able to deny the service to everybody else.
#[tokio::test]
async fn a_noisy_address_does_not_rate_limit_the_others() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@localhost/unused")
        .unwrap();
    let state = AppState::new(pool, vec![7; 32]).unwrap();
    let noisy: IpAddr = "203.0.113.7".parse().unwrap();
    let quiet: IpAddr = "203.0.113.8".parse().unwrap();
    // The attack is many different guessed logins from one address. The
    // address limit stops it, and each login name stays under its own limit.
    let mut limited_at = None;
    for attempt in 0..200 {
        if let Err(error) =
            state.limit_address(Some(noisy), &format!("login:guess-{attempt}"), 10, 20)
        {
            limited_at = Some((attempt, error.status));
            break;
        }
    }
    let (attempt, status) = limited_at.expect("the noisy address must be limited");
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        attempt < 200,
        "the address limit must stop the flood before the window ends"
    );
    // That address is now limited.
    assert_eq!(
        state
            .limit_address(Some(noisy), "login:another", 10, 20)
            .unwrap_err()
            .status,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Another address is not.
    state
        .limit_address(Some(quiet), "login:guess-0", 10, 20)
        .unwrap();
}

/// A forwarded address is only believed when the connection is the local
/// proxy. A client cannot name the address it wants to be counted as.
#[tokio::test]
async fn a_forwarded_address_is_only_trusted_from_the_local_proxy() {
    let proxy: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    let direct: SocketAddr = "198.51.100.9:40000".parse().unwrap();
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("x-forwarded-for", "203.0.113.9")
        .body(())
        .unwrap();
    let (mut parts, _) = request.into_parts();
    parts.extensions.insert(axum::extract::ConnectInfo(proxy));
    let trusted = ClientIp::from_request_parts(&mut parts, &()).await.unwrap();
    assert_eq!(trusted.0, Some("203.0.113.9".parse().unwrap()));

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("x-forwarded-for", "203.0.113.9")
        .body(())
        .unwrap();
    let (mut parts, _) = request.into_parts();
    parts.extensions.insert(axum::extract::ConnectInfo(direct));
    let untrusted = ClientIp::from_request_parts(&mut parts, &()).await.unwrap();
    assert_eq!(
        untrusted.0,
        Some("198.51.100.9".parse().unwrap()),
        "a client outside the proxy is counted by its own address"
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
