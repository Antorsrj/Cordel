use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use futures_util::{SinkExt, StreamExt};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

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

async fn connect(port: u16, token: &str) -> Client {
    let mut request = format!("ws://127.0.0.1:{port}/v1/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    let (socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    socket
}

async fn send(socket: &mut Client, frame: &cordel_protocol::WsFrame) {
    let text = serde_json::to_string(frame).unwrap();
    socket.send(Message::Text(text.into())).await.unwrap();
}

async fn next_text(socket: &mut Client) -> cordel_protocol::WsFrame {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out waiting for a WebSocket frame")
            .expect("WebSocket closed")
            .expect("WebSocket error");
        match message {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected WebSocket message: {other:?}"),
        }
    }
}

async fn no_text_within(socket: &mut Client, wait: Duration) {
    let result = tokio::time::timeout(wait, next_text(socket)).await;
    assert!(result.is_err(), "unexpected WebSocket text frame");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_websocket_flow() {
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

    let state = AppState::new(pool.clone(), vec![7; 32]).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = {
        let state = state.clone();
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        })
    };

    let admin_token = login(&state, "admin").await;
    let user_token = login(&state, "user").await;
    let root = format!("/v1/communities/{community_id}");

    let (status, channel) = call(
        &state,
        "POST",
        &format!("{root}/channels"),
        Some(&user_token),
        Some(r#"{"kind":"text","name":"general"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let text_channel = channel["channel_id"].as_str().unwrap().to_owned();

    let (status, voice) = call(
        &state,
        "POST",
        &format!("{root}/channels"),
        Some(&user_token),
        Some(r#"{"kind":"voice","name":"voice"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let voice_channel = voice["channel_id"].as_str().unwrap().to_owned();

    let unauthenticated = format!("ws://127.0.0.1:{port}/v1/events")
        .into_client_request()
        .unwrap();
    assert!(tokio_tungstenite::connect_async(unauthenticated)
        .await
        .is_err());

    let mut socket = connect(port, &user_token).await;
    let ready = next_text(&mut socket).await;
    assert_eq!(ready.kind, "ready");
    let expected_community = community_id.to_string();
    assert_eq!(
        ready
            .fields
            .get("community_id")
            .and_then(serde_json::Value::as_str),
        Some(expected_community.as_str())
    );

    send(
        &mut socket,
        &cordel_protocol::WsFrame::subscribe(&voice_channel),
    )
    .await;
    let error = next_text(&mut socket).await;
    assert_eq!(error.kind, "error");
    assert_eq!(
        error.fields.get("code").and_then(serde_json::Value::as_str),
        Some("invalid_channel")
    );

    send(
        &mut socket,
        &cordel_protocol::WsFrame::subscribe(&text_channel),
    )
    .await;
    let subscribed = next_text(&mut socket).await;
    assert_eq!(subscribed.kind, "subscribed");
    assert_eq!(
        subscribed
            .fields
            .get("last_seq")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );

    let first_id = Uuid::new_v4().to_string();
    let messages = format!("/v1/channels/{text_channel}/messages");
    let (status, ack) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{first_id}","body":"hello"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 1);

    let first = next_text(&mut socket).await;
    assert_eq!(first.kind, "message");
    assert_eq!(
        first.fields.get("seq").and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        first.fields.get("body").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    let expected_user = user_id.to_string();
    assert_eq!(
        first
            .fields
            .get("author_id")
            .and_then(serde_json::Value::as_str),
        Some(expected_user.as_str())
    );

    let (status, retry) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{first_id}","body":"hello"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry["seq"], 1);
    no_text_within(&mut socket, Duration::from_millis(300)).await;

    let (status, ack) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{}","body":"second"}}"#,
            Uuid::new_v4()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 2);
    let second = next_text(&mut socket).await;
    assert_eq!(second.kind, "message");
    assert_eq!(
        second.fields.get("seq").and_then(serde_json::Value::as_u64),
        Some(2)
    );

    let (status, out_of_order) = call(
        &state,
        "POST",
        &format!("{root}/channels"),
        Some(&user_token),
        Some(r#"{"kind":"text","name":"out-of-order"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let out_of_order_channel = out_of_order["channel_id"].as_str().unwrap().to_owned();
    send(
        &mut socket,
        &cordel_protocol::WsFrame::subscribe(&out_of_order_channel),
    )
    .await;
    let subscribed = next_text(&mut socket).await;
    assert_eq!(subscribed.kind, "subscribed");

    for seq in [2, 1] {
        let event = cordel_protocol::ChatEvent {
            channel_id: out_of_order_channel.clone(),
            seq,
            kind: "message".to_owned(),
            client_message_id: Some(format!("synthetic-{seq}")),
            body: Some(format!("synthetic {seq}")),
            extra: Default::default(),
        };
        state.chat_events.send(event).unwrap();
    }
    assert_eq!(
        next_text(&mut socket)
            .await
            .fields
            .get("seq")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    assert_eq!(
        next_text(&mut socket)
            .await
            .fields
            .get("seq")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    send(
        &mut socket,
        &cordel_protocol::WsFrame::unsubscribe(&out_of_order_channel),
    )
    .await;
    let unsubscribed = next_text(&mut socket).await;
    assert_eq!(unsubscribed.kind, "unsubscribed");

    send(
        &mut socket,
        &cordel_protocol::WsFrame::unsubscribe(&text_channel),
    )
    .await;
    let unsubscribed = next_text(&mut socket).await;
    assert_eq!(unsubscribed.kind, "unsubscribed");

    let (status, ack) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{}","body":"while away"}}"#,
            Uuid::new_v4()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 3);
    no_text_within(&mut socket, Duration::from_millis(300)).await;

    send(
        &mut socket,
        &cordel_protocol::WsFrame::subscribe(&text_channel),
    )
    .await;
    let subscribed = next_text(&mut socket).await;
    assert_eq!(subscribed.kind, "subscribed");
    assert_eq!(
        subscribed
            .fields
            .get("last_seq")
            .and_then(serde_json::Value::as_u64),
        Some(3)
    );

    let (status, grant) = call(
        &state,
        "POST",
        "/v1/sessions/admin-grant",
        Some(&admin_token),
        Some(r#"{"password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let grant_token = grant["admin_token"].as_str().unwrap().to_owned();

    let (status, ban) = call(
        &state,
        "POST",
        &format!("{root}/members/{user_id}/ban"),
        Some(&grant_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let revision = ban["authorization_revision"].as_u64().unwrap();

    let (status, ack) = call(
        &state,
        "POST",
        &messages,
        Some(&admin_token),
        Some(&format!(
            r#"{{"client_message_id":"{}","body":"after ban"}}"#,
            Uuid::new_v4()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 4);

    let revoked = next_text(&mut socket).await;
    assert_eq!(revoked.kind, "access_revoked");
    assert_eq!(
        revoked
            .fields
            .get("authorization_revision")
            .and_then(serde_json::Value::as_u64),
        Some(revision)
    );

    server.abort();
}
