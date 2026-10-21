//! The admission and webhook checks.

use super::*;
use jsonwebtoken::{decode, DecodingKey, Validation};

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
    let (api_key, api_secret) = livekit_credentials();
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[api_key.as_str()]);
    let claims = decode::<TokenClaims>(
        &token,
        &DecodingKey::from_secret(api_secret.as_bytes()),
        &validation,
    )
    .unwrap()
    .claims;
    assert_eq!(claims.sub, user_id.to_string());
    assert_eq!(claims.iss, api_key);
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
