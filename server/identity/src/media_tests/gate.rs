//! The admission gate and the drain checks.

use super::*;

/// Ask the gate about a token, the way the reverse proxy does.
async fn gate(state: &AppState, token: &str) -> StatusCode {
    let response = gate_router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/livekit/authorize")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.status()
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_media_gate_flow() {
    let (pool, community_id, user_id) = seed().await;
    let state = AppState::new(pool.clone(), vec![7; 32])
        .unwrap()
        .with_media(media_config());
    let user_token = login(&state, "user").await;
    let voice = create_channel(&state, community_id, &user_token, "voice").await;

    let (status, admission) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let room_token = admission["token"].as_str().unwrap().to_owned();

    // A fresh admission passes the gate.
    assert_eq!(gate(&state, &room_token).await, StatusCode::OK);

    // The gate is not on the public path.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/livekit/authorize")
                .header("authorization", format!("Bearer {room_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // A moderation action advances the generation, so the held token names a
    // room that is no longer current.
    sqlx::query(
        "UPDATE communities SET authorization_revision=authorization_revision+1 WHERE id=$1",
    )
    .bind(community_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(gate(&state, &room_token).await, StatusCode::FORBIDDEN);

    // A fresh admission for the new generation passes again.
    let (status, admission) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let current_token = admission["token"].as_str().unwrap().to_owned();
    assert_eq!(gate(&state, &current_token).await, StatusCode::OK);

    // A revoked session does not admit the room either.
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE account_id=$1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(gate(&state, &current_token).await, StatusCode::FORBIDDEN);

    // A token signed with another secret is refused.
    let forged = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &serde_json::json!({
            "exp": Utc::now().timestamp() + 300,
            "iss": API_KEY,
            "sub": user_id.to_string(),
            "sid": Uuid::new_v4().to_string(),
            "name": "user",
            "video": {"room": format!("voice:{voice}:2"), "roomJoin": true},
        }),
        &jsonwebtoken::EncodingKey::from_secret(b"another-secret-with-32-bytes-long!!"),
    )
    .unwrap();
    assert_eq!(gate(&state, &forged).await, StatusCode::UNAUTHORIZED);

    // A token without a token header is refused.
    let response = gate_router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/livekit/authorize")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn only_a_voice_room_of_this_shape_is_a_room_name() {
    use crate::media::gate::parse_room;
    let channel = Uuid::new_v4();
    let room = format!("voice:{channel}:7");
    let parsed = parse_room(&room).expect("a voice room");
    assert_eq!(parsed.channel_id, channel);
    assert_eq!(parsed.generation, 7);
    assert!(parse_room("camera").is_none());
    assert!(parse_room("voice:not-a-uuid:1").is_none());
    assert!(parse_room("voice:00000000-0000-0000-0000-000000000000").is_none());
    assert!(parse_room(&format!("voice:{channel}:x")).is_none());
    assert!(parse_room(&format!("text:{channel}:1")).is_none());
}

/// The moderation action must drain the room the moderated account was in.
///
/// It needs a running LiveKit: `CORDEL_TEST_LIVEKIT_URL` is the signaling
/// URL and `CORDEL_TEST_LIVEKIT_API_URL` is the host-local server API.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database, a running LiveKit, and the CORDEL_TEST_LIVEKIT_* variables"]
async fn disposable_database_media_drain_flow() {
    let api_url = match std::env::var("CORDEL_TEST_LIVEKIT_API_URL") {
        Ok(value) => value,
        Err(_) => {
            println!(
                "SKIP the drain flow needs a running LiveKit: set CORDEL_TEST_LIVEKIT_URL and \
                 CORDEL_TEST_LIVEKIT_API_URL"
            );
            return;
        }
    };
    let (pool, community_id, user_id) = seed().await;
    let (api_key, api_secret) = livekit_credentials();
    let api = LiveKitApi::new(api_key, api_secret, api_url).unwrap();
    let state = AppState::new(pool.clone(), vec![7; 32])
        .unwrap()
        .with_media(media_config())
        .with_media_api(api.clone());
    let user_token = login(&state, "user").await;
    let voice = create_channel(&state, community_id, &user_token, "voice").await;

    let (status, admission) = call(
        &state,
        "POST",
        &format!("/v1/voice-channels/{voice}/admission"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let room = admission["room"].as_str().unwrap().to_owned();

    // The room exists, the way it does when somebody is in it.
    api.create_room(&room).await.expect("creating the room");
    assert!(
        api.list_rooms()
            .await
            .expect("listing rooms")
            .iter()
            .any(|name| name == &room),
        "the room must exist before the moderation action"
    );

    // The admin kicks the member.
    let admin_token = login(&state, "admin").await;
    let grant = call(
        &state,
        "POST",
        "/v1/sessions/admin-grant",
        Some(&admin_token),
        Some(r#"{"password":"correct horse battery staple"}"#),
    )
    .await;
    let grant_token = grant.1["admin_token"].as_str().unwrap().to_owned();
    let (status, moderation) = call(
        &state,
        "POST",
        &format!("/v1/communities/{community_id}/members/{user_id}/kick"),
        Some(&grant_token),
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moderation["state"], "kicked");
    assert_eq!(
        moderation["media_drained"], true,
        "the room of the previous generation must be drained"
    );

    // The room of the previous generation is gone.
    assert!(
        !api.list_rooms()
            .await
            .expect("listing rooms")
            .iter()
            .any(|name| name == &room),
        "the room of the previous generation must be drained"
    );
}
