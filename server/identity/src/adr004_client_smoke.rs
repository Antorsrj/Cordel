use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use cordel_state::{
    ClientState, LiveOutcome, LiveSession, MemoryRefreshStore, NativeChatTransport,
    NativeLiveTransport, SendOutcome,
};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tower::ServiceExt;

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
    let bytes = to_bytes(response.into_body(), 32 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap();
    (status, value)
}

async fn login(state: &AppState, login: &str) -> String {
    let body = format!(r#"{{"login":"{login}","password":"correct horse battery staple"}}"#);
    let (status, value) = call(state, "POST", "/v1/sessions", None, Some(&body)).await;
    assert_eq!(status, StatusCode::OK);
    value["access_token"].as_str().unwrap().to_owned()
}

async fn wait_for_no_receivers(state: &AppState) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.chat_events.receiver_count() > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native client did not release the broadcast receiver");
}

/// Drives the real blocking client transports against the real loopback Axum
/// router. This is the cross-component seam that the client unit fakes and the
/// server's raw WebSocket test do not cover.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_native_client_flow() {
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

    let app = AppState::new(pool.clone(), vec![7; 32]).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = {
        let app = app.clone();
        tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        })
    };

    let user_token = login(&app, "user").await;
    let root = format!("/v1/communities/{community_id}");
    let (status, channel) = call(
        &app,
        "POST",
        &format!("{root}/channels"),
        Some(&user_token),
        Some(r#"{"kind":"text","name":"general"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let channel_id = channel["channel_id"].as_str().unwrap().to_owned();
    let messages = format!("/v1/channels/{channel_id}/messages");
    let endpoint = format!("ws://127.0.0.1:{port}/v1/events");
    let base = format!("http://127.0.0.1:{port}/");

    let client_state = tokio::task::spawn_blocking({
        let endpoint = endpoint.clone();
        let base = base.clone();
        let channel_id = channel_id.clone();
        let token = user_token.clone();
        move || {
            let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
            state.tokens_mut().set_access(&token);
            let live = NativeLiveTransport::new_plain(&endpoint).unwrap();
            let chat = NativeChatTransport::new_plain(&base).unwrap();
            let mut session = LiveSession::new(live, chat, endpoint, vec![channel_id.clone()]);

            assert_eq!(session.start(&mut state, &token, 0), LiveOutcome::Ready);
            match session.pump(&mut state, &token, 0) {
                LiveOutcome::Subscribed {
                    last_seq, applied, ..
                } => {
                    assert_eq!(last_seq, 0);
                    assert!(applied.is_empty());
                }
                other => panic!("expected Subscribed, got {other:?}"),
            }

            let id = state.enqueue(channel_id.clone(), "hello from native client");
            let outcomes = session.send_pending(&mut state, &token, &channel_id);
            assert_eq!(outcomes.len(), 1);
            match &outcomes[0] {
                SendOutcome::Acknowledged {
                    client_message_id,
                    seq,
                } => {
                    assert_eq!(client_message_id, id.as_str());
                    assert_eq!(*seq, 1);
                }
                other => panic!("expected Acknowledged, got {other:?}"),
            }

            match session.pump(&mut state, &token, 0) {
                LiveOutcome::Applied { applied, .. } => {
                    assert_eq!(applied.len(), 1);
                    assert_eq!(applied[0].seq, 1);
                    assert_eq!(applied[0].body.as_deref(), Some("hello from native client"));
                }
                other => panic!("expected Applied, got {other:?}"),
            }
            assert_eq!(state.cursor(&channel_id), 1);
            state
        }
    })
    .await
    .unwrap();

    wait_for_no_receivers(&app).await;
    assert_eq!(app.chat_events.receiver_count(), 0);

    let (status, ack) = call(
        &app,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{}","body":"committed while offline"}}"#,
            Uuid::new_v4()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 2);
    assert_eq!(app.chat_events.receiver_count(), 0);

    let state_after = tokio::task::spawn_blocking({
        let endpoint = endpoint.clone();
        let base = base.clone();
        let channel_id = channel_id.clone();
        let token = user_token.clone();
        move || {
            let mut state = client_state;
            let live = NativeLiveTransport::new_plain(&endpoint).unwrap();
            let chat = NativeChatTransport::new_plain(&base).unwrap();
            let mut session = LiveSession::new(live, chat, endpoint, vec![channel_id.clone()]);

            assert_eq!(session.start(&mut state, &token, 0), LiveOutcome::Ready);
            match session.pump(&mut state, &token, 0) {
                LiveOutcome::Subscribed {
                    last_seq, applied, ..
                } => {
                    assert_eq!(last_seq, 2);
                    assert_eq!(applied.len(), 1);
                    assert_eq!(applied[0].seq, 2);
                    assert_eq!(applied[0].body.as_deref(), Some("committed while offline"));
                }
                other => panic!("expected Subscribed catch-up, got {other:?}"),
            }
            assert_eq!(state.cursor(&channel_id), 2);
            state
        }
    })
    .await
    .unwrap();
    assert_eq!(state_after.cursor(&channel_id), 2);

    server.abort();
}
