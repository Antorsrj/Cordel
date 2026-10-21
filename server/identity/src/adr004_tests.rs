use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use sqlx::postgres::PgPoolOptions;
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

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_chat_flow() {
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
    let messages = format!("/v1/channels/{text_channel}/messages");

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

    let first_id = Uuid::new_v4().to_string();
    let first_body = format!(r#"{{"client_message_id":"{first_id}","body":"hello"}}"#);
    assert_eq!(
        call(&state, "POST", &messages, None, Some(&first_body))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &state,
            "POST",
            &messages,
            Some(&user_token),
            Some(&format!(
                r#"{{"client_message_id":"{first_id}","body":""}}"#
            ))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &state,
            "POST",
            &messages,
            Some(&user_token),
            Some(&format!(
                r#"{{"client_message_id":"{first_id}","body":"{}"}}"#,
                "a".repeat(4001)
            ))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let (status, ack) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&first_body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 1);
    assert_eq!(ack["client_message_id"], first_id);

    let (status, retry) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&first_body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry["seq"], 1);

    let (status, _) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{first_id}","body":"different"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let second_id = "client-2".to_owned();
    let (status, ack) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{second_id}","body":"second"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 2);

    let (status, ack) = call(
        &state,
        "POST",
        &messages,
        Some(&admin_token),
        Some(&format!(
            r#"{{"client_message_id":"{first_id}","body":"same id, other author"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ack["seq"], 3);

    assert_eq!(
        call(
            &state,
            "POST",
            &format!("/v1/channels/{voice_channel}/messages"),
            Some(&user_token),
            Some(&format!(
                r#"{{"client_message_id":"{}","body":"no"}}"#,
                Uuid::new_v4()
            ))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let (status, page) = call(
        &state,
        "GET",
        &format!("{messages}?after_seq=0&limit=1"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(page["messages"][0]["seq"], 1);
    assert_eq!(page["has_more"], true);
    assert_eq!(page["messages"][0]["author_id"], user_id.to_string());
    assert!(page["messages"][0]["message_id"].is_string());

    let (status, page) = call(
        &state,
        "GET",
        &format!("{messages}?after_seq=1"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["messages"].as_array().unwrap().len(), 2);
    assert_eq!(page["messages"][0]["seq"], 2);
    assert_eq!(page["messages"][1]["seq"], 3);
    assert_eq!(page["has_more"], false);

    let (status, page) = call(
        &state,
        "GET",
        &format!("{messages}?before_seq=3&limit=2"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["messages"].as_array().unwrap().len(), 2);
    assert_eq!(page["messages"][0]["seq"], 1);
    assert_eq!(page["messages"][1]["seq"], 2);
    assert_eq!(page["has_more"], false);

    let (status, page) = call(
        &state,
        "GET",
        &format!("{messages}?limit=2"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["messages"][0]["seq"], 2);
    assert_eq!(page["messages"][1]["seq"], 3);
    assert_eq!(page["has_more"], true);

    for query in ["after_seq=1&before_seq=3", "limit=0", "after_seq=nope"] {
        assert_eq!(
            call(
                &state,
                "GET",
                &format!("{messages}?{query}"),
                Some(&user_token),
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }

    let mut sends = Vec::new();
    for index in 0..8 {
        let state = state.clone();
        let messages = messages.clone();
        let token = user_token.clone();
        sends.push(tokio::spawn(async move {
            let body = format!(
                r#"{{"client_message_id":"concurrent-{index}","body":"concurrent {index}"}}"#
            );
            call(&state, "POST", &messages, Some(&token), Some(&body)).await
        }));
    }
    let mut concurrent_seqs = Vec::new();
    for send in sends {
        let (status, ack) = send.await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        concurrent_seqs.push(ack["seq"].as_u64().unwrap());
    }
    concurrent_seqs.sort_unstable();
    assert_eq!(concurrent_seqs, (4..=11).collect::<Vec<_>>());

    let (status, page) = call(
        &state,
        "GET",
        &format!("{messages}?after_seq=3"),
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let seqs: Vec<u64> = page["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, (4..=11).collect::<Vec<_>>());

    assert_eq!(
        call(&state, "GET", &messages, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let (status, _) = call(
        &state,
        "DELETE",
        "/v1/sessions/current",
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &state,
        "POST",
        &messages,
        Some(&user_token),
        Some(&format!(
            r#"{{"client_message_id":"{}","body":"after logout"}}"#,
            Uuid::new_v4()
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
