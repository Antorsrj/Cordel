use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use futures_util::{SinkExt, StreamExt};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, COOKIE, ORIGIN};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

const ORIGIN_VALUE: &str = "https://cordel.test";
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Reply {
    status: StatusCode,
    body: serde_json::Value,
    headers: HeaderMap,
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_browser_session_flow() {
    let url = std::env::var("CORDEL_TEST_DATABASE_URL").expect("disposable database URL required");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(name.starts_with("cordel_identity_test_"));
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("TRUNCATE browser_pre_sessions,messages,community_audit,channels,admin_audit,admin_grants,host_audit,refresh_credentials,sessions,invitations,memberships,accounts,communities CASCADE")
        .execute(&pool).await.unwrap();
    bootstrap_admin(
        &pool,
        "Ada",
        "correct horse battery staple",
        "Browser",
        "test-operator",
    )
    .await
    .unwrap();
    let state = AppState::new(pool, vec![7; 32])
        .unwrap()
        .with_browser_origin(ORIGIN_VALUE.to_owned());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = state.clone();
    tokio::spawn(async move {
        axum::serve(listener, router(served)).await.unwrap();
    });

    // Neither header can be set by a page on another origin, so a request
    // without any evidence of a same-origin browser is refused.
    let bare = call(&state, "GET", "/v1/browser/csrf", &[], None).await;
    assert_eq!(bare.status, StatusCode::FORBIDDEN);
    assert_eq!(bare.body["error"]["code"], "origin_rejected");

    let crossed = call(
        &state,
        "GET",
        "/v1/browser/csrf",
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (
                header::HeaderName::from_static("sec-fetch-site"),
                "cross-site",
            ),
        ],
        None,
    )
    .await;
    assert_eq!(crossed.status, StatusCode::FORBIDDEN);

    // This is the request a browser really makes: a same-origin `GET` carries
    // no `Origin`, only `Sec-Fetch-Site`. A test that invents the `Origin`
    // header here would hide the request the client cannot produce.
    let pre = call(
        &state,
        "GET",
        "/v1/browser/csrf",
        &[(
            header::HeaderName::from_static("sec-fetch-site"),
            "same-origin",
        )],
        None,
    )
    .await;
    assert_eq!(pre.status, StatusCode::OK, "{}", pre.body);
    let pre_cookie = cookie(&pre, "cordel_pre");
    let pre_csrf = pre.body["csrf_token"].as_str().unwrap();

    let logged = call(
        &state,
        "POST",
        "/v1/sessions",
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (header::COOKIE, &format!("cordel_pre={pre_cookie}")),
            (header::HeaderName::from_static("x-csrf-token"), pre_csrf),
        ],
        Some(r#"{"login":"Ada","password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(logged.status, StatusCode::OK, "{}", logged.body);
    assert!(logged.body.get("access_token").is_none());
    assert!(logged.body.get("refresh_token").is_none());
    let session_cookie = cookie(&logged, "cordel_session");
    let csrf = logged.body["csrf_token"].as_str().unwrap().to_owned();

    let reused = call(
        &state,
        "POST",
        "/v1/sessions",
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (header::COOKIE, &format!("cordel_pre={pre_cookie}")),
            (header::HeaderName::from_static("x-csrf-token"), pre_csrf),
        ],
        Some(r#"{"login":"Ada","password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(reused.status, StatusCode::FORBIDDEN);
    assert_eq!(reused.body["error"]["code"], "csrf_rejected");

    let hostile = me(&state, "https://evil.test", &session_cookie, &csrf).await;
    assert_eq!(hostile.status, StatusCode::FORBIDDEN);
    let mine = me(&state, ORIGIN_VALUE, &session_cookie, &csrf).await;
    assert_eq!(mine.status, StatusCode::OK);
    assert_eq!(mine.body["login"], "Ada");

    // After login the client reads through a same-origin `GET`, which carries
    // no `Origin` either. The cookie proves the session and the CSRF header is
    // still required, so a page on another origin cannot read the answer.
    let browser_mine = call(
        &state,
        "GET",
        "/v1/me",
        &[
            (header::COOKIE, &format!("cordel_session={session_cookie}")),
            (
                header::HeaderName::from_static("x-csrf-token"),
                csrf.as_str(),
            ),
            (
                header::HeaderName::from_static("sec-fetch-site"),
                "same-origin",
            ),
        ],
        None,
    )
    .await;
    assert_eq!(browser_mine.status, StatusCode::OK, "{}", browser_mine.body);

    let grant = call(
        &state,
        "POST",
        "/v1/sessions/admin-grant",
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (header::COOKIE, &format!("cordel_session={session_cookie}")),
            (
                header::HeaderName::from_static("x-csrf-token"),
                csrf.as_str(),
            ),
        ],
        Some(r#"{"password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(grant.body["error"]["code"], "browser_management_disabled");

    let page = text(&state, "/").await;
    assert_eq!(page.0, StatusCode::OK);
    assert!(page.1.contains("/diagnostic.js"));
    assert!(!page.1.contains("localStorage"));
    let plain = AppState::new(state.pool.clone(), vec![7; 32]).unwrap();
    assert_eq!(text(&plain, "/").await.0, StatusCode::NOT_FOUND);

    let community = mine.body["community_id"].as_str().unwrap();
    let created = call(
        &state,
        "POST",
        &format!("/v1/communities/{community}/channels"),
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (header::COOKIE, &format!("cordel_session={session_cookie}")),
            (
                header::HeaderName::from_static("x-csrf-token"),
                csrf.as_str(),
            ),
        ],
        Some(r#"{"kind":"text","name":"geral"}"#),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let channel_id = created.body["channel_id"].as_str().unwrap();

    let mut browser = connect_cookie(port, &session_cookie, ORIGIN_VALUE)
        .await
        .unwrap();
    browser
        .send(Message::Text(
            format!(
                r#"{{"schema_version":1,"type":"browser.authenticate","csrf_token":"{csrf}"}}"#
            )
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_text(&mut browser).await["kind"], "ready");
    browser
        .send(Message::Text(
            format!(r#"{{"v":1,"kind":"subscribe","channel_id":"{channel_id}"}}"#).into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_text(&mut browser).await["kind"], "subscribed");
    let sent = call(
        &state,
        "POST",
        &format!("/v1/channels/{channel_id}/messages"),
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (header::COOKIE, &format!("cordel_session={session_cookie}")),
            (
                header::HeaderName::from_static("x-csrf-token"),
                csrf.as_str(),
            ),
        ],
        Some(r#"{"client_message_id":"browser-1","body":"from the browser"}"#),
    )
    .await;
    assert_eq!(sent.status, StatusCode::CREATED, "{}", sent.body);
    let mut live = next_text(&mut browser).await;
    while live["kind"] != "message" {
        live = next_text(&mut browser).await;
    }
    assert_eq!(live["body"], "from the browser");

    let mut rejected = connect_cookie(port, &session_cookie, ORIGIN_VALUE)
        .await
        .unwrap();
    rejected
        .send(Message::Text(
            r#"{"schema_version":1,"type":"browser.authenticate","csrf_token":"no"}"#.into(),
        ))
        .await
        .unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(2), rejected.next()).await;
    assert!(!matches!(closed, Ok(Some(Ok(Message::Text(_))))));

    assert!(connect_cookie(port, &session_cookie, "https://evil.test")
        .await
        .is_err());

    let refreshed = call(
        &state,
        "POST",
        "/v1/sessions/refresh",
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (header::COOKIE, &format!("cordel_session={session_cookie}")),
            (
                header::HeaderName::from_static("x-csrf-token"),
                csrf.as_str(),
            ),
        ],
        None,
    )
    .await;
    assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.body);
    let next_cookie = cookie(&refreshed, "cordel_session");
    let next_csrf = refreshed.body["csrf_token"].as_str().unwrap().to_owned();
    assert_ne!(next_cookie, session_cookie);
    assert_eq!(
        me(&state, ORIGIN_VALUE, &session_cookie, &csrf)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        me(&state, ORIGIN_VALUE, &next_cookie, &next_csrf)
            .await
            .status,
        StatusCode::OK
    );

    let logged_out = call(
        &state,
        "DELETE",
        "/v1/sessions/current",
        &[
            (header::ORIGIN, ORIGIN_VALUE),
            (header::COOKIE, &format!("cordel_session={next_cookie}")),
            (
                header::HeaderName::from_static("x-csrf-token"),
                next_csrf.as_str(),
            ),
        ],
        None,
    )
    .await;
    assert_eq!(logged_out.status, StatusCode::OK);
    assert_eq!(
        me(&state, ORIGIN_VALUE, &next_cookie, &next_csrf)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    let native = call(
        &state,
        "POST",
        "/v1/sessions",
        &[],
        Some(r#"{"login":"Ada","password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(native.status, StatusCode::OK);
    let token = native.body["access_token"].as_str().unwrap();
    let mut socket = connect_bearer(port, token).await;
    assert_eq!(next_text(&mut socket).await["kind"], "ready");
}

async fn me(state: &AppState, origin: &str, session: &str, csrf: &str) -> Reply {
    let cookie = format!("cordel_session={session}");
    call(
        state,
        "GET",
        "/v1/me",
        &[
            (header::ORIGIN, origin),
            (header::COOKIE, cookie.as_str()),
            (header::HeaderName::from_static("x-csrf-token"), csrf),
        ],
        None,
    )
    .await
}

async fn text(state: &AppState, path: &str) -> (StatusCode, String) {
    let response = router(state.clone())
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn call(
    state: &AppState,
    method: &str,
    path: &str,
    extra: &[(header::HeaderName, &str)],
    body: Option<&str>,
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    for (name, value) in extra {
        builder = builder.header(name, *value);
    }
    let response = router(state.clone())
        .oneshot(
            builder
                .body(Body::from(body.unwrap_or_default().to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    Reply {
        status,
        body,
        headers,
    }
}

fn cookie(reply: &Reply, name: &str) -> String {
    reply
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let text = value.to_str().ok()?;
            let (pair, _) = text.split_once(';')?;
            let (key, cookie) = pair.split_once('=')?;
            (key == name).then_some(cookie.to_owned())
        })
        .unwrap_or_else(|| panic!("missing {name}"))
}

async fn connect_cookie(port: u16, session: &str, origin: &str) -> Result<Socket, String> {
    let mut request = format!("ws://127.0.0.1:{port}/v1/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(ORIGIN, origin.parse().unwrap());
    request
        .headers_mut()
        .insert(COOKIE, format!("cordel_session={session}").parse().unwrap());
    match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => Ok(socket),
        Err(error) => Err(error.to_string()),
    }
}

async fn connect_bearer(port: u16, token: &str) -> Socket {
    let mut request = format!("ws://127.0.0.1:{port}/v1/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

async fn next_text(socket: &mut Socket) -> serde_json::Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out")
            .expect("closed")
            .expect("socket error");
        match message {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected frame {other:?}"),
        }
    }
}
