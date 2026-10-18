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
    let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap();
    (status, value)
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_community_flow() {
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
    sqlx::query("TRUNCATE community_audit,channels,admin_audit,admin_grants,host_audit,refresh_credentials,sessions,invitations,memberships,accounts,communities CASCADE")
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
    let (status, admin_login) = call(
        &AppState::new(pool.clone(), vec![7; 32]).unwrap(),
        "POST",
        "/v1/sessions",
        None,
        Some(r#"{"login":"admin","password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let state = AppState::new(pool.clone(), vec![7; 32]).unwrap();
    let admin_token = admin_login["access_token"].as_str().unwrap();
    let (status, user_login) = call(
        &state,
        "POST",
        "/v1/sessions",
        None,
        Some(r#"{"login":"user","password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let user_token = user_login["access_token"].as_str().unwrap();
    let root = format!("/v1/communities/{community_id}");
    assert_eq!(
        call(&state, "GET", &root, Some(user_token), None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &state,
            "PATCH",
            &root,
            Some(admin_token),
            Some(r#"{"name":"Renamed"}"#)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &state,
            "PATCH",
            &root,
            Some(user_token),
            Some(r#"{"name":"Denied"}"#)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, grant) = call(
        &state,
        "POST",
        "/v1/sessions/admin-grant",
        Some(admin_token),
        Some(r#"{"password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let grant_token = grant["admin_token"].as_str().unwrap();
    assert_eq!(
        call(
            &state,
            "PATCH",
            &root,
            Some(grant_token),
            Some(r#"{"name":"Renamed","description":"A test"}"#)
        )
        .await
        .0,
        StatusCode::OK
    );
    let channels = format!("{root}/channels");
    let (status, channel) = call(
        &state,
        "POST",
        &channels,
        Some(user_token),
        Some(r#"{"kind":"text","name":"General"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(channel["kind"], "text");
    assert_eq!(
        call(
            &state,
            "POST",
            &channels,
            Some(user_token),
            Some(r#"{"kind":"text","name":"general"}"#)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&state, "GET", &channels, Some(user_token), None)
            .await
            .1["channels"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE login_key='admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let admin_kick = format!("{root}/members/{admin_id}/kick");
    assert_eq!(
        call(&state, "POST", &admin_kick, Some(grant_token), Some("{}"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let kick = format!("{root}/members/{user_id}/kick");
    let (status, kicked) = call(
        &state,
        "POST",
        &kick,
        Some(grant_token),
        Some(r#"{"duration_minutes":30}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(kicked["state"], "kicked");
    let first_revision = kicked["authorization_revision"].as_i64().unwrap();
    assert_eq!(
        call(&state, "GET", &root, Some(user_token), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let until: DateTime<Utc> =
        sqlx::query_scalar("SELECT kicked_until FROM memberships WHERE account_id=$1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(until >= Utc::now() + Duration::minutes(29));
    sqlx::query(
        "UPDATE memberships SET kicked_until=now()-interval '1 minute' WHERE account_id=$1",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    let (status, after_kick) = call(
        &state,
        "POST",
        "/v1/sessions",
        None,
        Some(r#"{"login":"user","password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let fresh_user_token = after_kick["access_token"].as_str().unwrap();
    let ban = format!("{root}/members/{user_id}/ban");
    let (status, banned) = call(&state, "POST", &ban, Some(grant_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(banned["state"], "banned");
    assert!(banned["authorization_revision"].as_i64().unwrap() > first_revision);
    assert_eq!(
        call(&state, "GET", &root, Some(fresh_user_token), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, unbanned) = call(&state, "DELETE", &ban, Some(grant_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unbanned["state"], "active");
    change_admin_role(&pool, "user", true, "test-operator")
        .await
        .unwrap();
    change_admin_role(&pool, "admin", false, "test-operator")
        .await
        .unwrap();
    assert!(change_admin_role(&pool, "user", false, "test-operator")
        .await
        .is_err());
    assert_eq!(
        call(
            &state,
            "PATCH",
            &root,
            Some(grant_token),
            Some(r#"{"name":"Denied"}"#)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let actions: i64 = sqlx::query_scalar("SELECT count(*) FROM community_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actions, 4);
}
