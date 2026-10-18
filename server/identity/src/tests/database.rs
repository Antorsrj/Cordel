use super::*;

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database named cordel_identity_test_* and CORDEL_TEST_DATABASE_URL"]
async fn disposable_database_identity_flow() {
    let url = std::env::var("CORDEL_TEST_DATABASE_URL")
        .expect("set CORDEL_TEST_DATABASE_URL to a disposable database");
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    let db_name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db_name.starts_with("cordel_identity_test_"),
        "refusing to clear a non-disposable database"
    );
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("TRUNCATE admin_audit,admin_grants,host_audit,refresh_credentials,sessions,invitations,memberships,accounts,communities CASCADE")
            .execute(&pool).await.unwrap();
    let state = AppState::new(pool.clone(), vec![9; 32]).unwrap();
    bootstrap_admin(
        &pool,
        "Admin",
        "correct horse battery staple",
        "Test community",
        "test-operator",
    )
    .await
    .unwrap();
    assert!(bootstrap_admin(
        &pool,
        "Other",
        "correct horse battery staple",
        "Another",
        "test-operator"
    )
    .await
    .is_err());

    let app = router(state.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"login":"Admin","password":"correct horse battery staple"}"#,
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let session: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(session["schema_version"], 1);
    assert_eq!(session["expires_in_secs"], ACCESS_SECONDS);
    let access_token = session["access_token"].as_str().unwrap();
    let request = Request::builder()
        .method("GET")
        .uri("/v1/me")
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let me_body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(me_body["schema_version"], 1);
    assert_eq!(me_body["role"], "admin");

    let admin = login(
        State(state.clone()),
        Ok(Json(LoginRequest {
            login: "Admin".into(),
            password: "correct horse battery staple".into(),
        })),
    )
    .await
    .unwrap()
    .0;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", admin.access_token).parse().unwrap(),
    );
    let ordinary_denied = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        headers.clone(),
        Ok(Json(InviteRequest {
            expires_in_hours: None,
            max_uses: None,
        })),
    )
    .await;
    assert_eq!(
        ordinary_denied.unwrap_err().status,
        StatusCode::UNAUTHORIZED
    );
    let wrong = unlock_admin(
        State(state.clone()),
        headers.clone(),
        Ok(Json(AdminGrantRequest {
            password: "incorrect password".into(),
        })),
    )
    .await;
    assert_eq!(wrong.unwrap_err().status, StatusCode::UNAUTHORIZED);
    let grant = unlock_admin(
        State(state.clone()),
        headers.clone(),
        Ok(Json(AdminGrantRequest {
            password: "correct horse battery staple".into(),
        })),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(grant.expires_in_secs, ADMIN_GRANT_SECONDS);
    let mut grant_headers = HeaderMap::new();
    grant_headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", grant.admin_token).parse().unwrap(),
    );
    let invite = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        grant_headers.clone(),
        Ok(Json(InviteRequest {
            expires_in_hours: Some(24),
            max_uses: Some(1),
        })),
    )
    .await
    .unwrap()
    .0;
    let code = invite.invite_code.clone();
    let first = redeem(
        State(state.clone()),
        Ok(Json(RedeemRequest {
            invite_code: code.clone(),
            login: "Alice".into(),
            password: "correct horse battery staple".into(),
        })),
    );
    let second = redeem(
        State(state.clone()),
        Ok(Json(RedeemRequest {
            invite_code: code.clone(),
            login: "Bob".into(),
            password: "correct horse battery staple".into(),
        })),
    );
    let (a, b) = tokio::join!(first, second);
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "single-use invite admitted more than one account"
    );
    let winner = a.ok().or_else(|| b.ok()).unwrap().0;
    let replay = redeem(
        State(state.clone()),
        Ok(Json(RedeemRequest {
            invite_code: code,
            login: "Third".into(),
            password: "correct horse battery staple".into(),
        })),
    )
    .await;
    assert_eq!(replay.unwrap_err().error.code, "invalid_input");
    let user_headers = {
        let mut value = HeaderMap::new();
        value.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", winner.access_token).parse().unwrap(),
        );
        value
    };
    let regular_unlock = unlock_admin(
        State(state.clone()),
        user_headers.clone(),
        Ok(Json(AdminGrantRequest {
            password: "correct horse battery staple".into(),
        })),
    )
    .await;
    assert_eq!(regular_unlock.unwrap_err().status, StatusCode::FORBIDDEN);
    let denied = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        user_headers,
        Ok(Json(InviteRequest {
            expires_in_hours: None,
            max_uses: None,
        })),
    )
    .await;
    assert_eq!(denied.unwrap_err().status, StatusCode::UNAUTHORIZED);

    let collision_invite = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        grant_headers.clone(),
        Ok(Json(InviteRequest {
            expires_in_hours: None,
            max_uses: None,
        })),
    )
    .await
    .unwrap()
    .0;
    let collision = redeem(
        State(state.clone()),
        Ok(Json(RedeemRequest {
            invite_code: collision_invite.invite_code.clone(),
            login: "Admin".into(),
            password: "correct horse battery staple".into(),
        })),
    )
    .await;
    assert_eq!(collision.unwrap_err().error.code, "invalid_input");
    let remaining: i32 = sqlx::query_scalar("SELECT uses_remaining FROM invitations WHERE id=$1")
        .bind(collision_invite.invitation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 1, "username collision consumed the invite");
    sqlx::query("UPDATE invitations SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(collision_invite.invitation_id)
        .execute(&pool)
        .await
        .unwrap();
    let expired = redeem(
        State(state.clone()),
        Ok(Json(RedeemRequest {
            invite_code: collision_invite.invite_code,
            login: "Unused".into(),
            password: "correct horse battery staple".into(),
        })),
    )
    .await;
    assert_eq!(expired.unwrap_err().error.code, "invalid_input");

    sqlx::query("UPDATE memberships SET role='user' WHERE community_id=$1 AND account_id=(SELECT id FROM accounts WHERE login_key='admin')")
            .bind(admin.community_id).execute(&pool).await.unwrap();
    let demoted = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        grant_headers.clone(),
        Ok(Json(InviteRequest {
            expires_in_hours: None,
            max_uses: None,
        })),
    )
    .await;
    assert_eq!(demoted.unwrap_err().status, StatusCode::UNAUTHORIZED);
    sqlx::query("UPDATE memberships SET role='admin' WHERE community_id=$1 AND account_id=(SELECT id FROM accounts WHERE login_key='admin')")
            .bind(admin.community_id).execute(&pool).await.unwrap();
    let old_grant = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        grant_headers,
        Ok(Json(InviteRequest {
            expires_in_hours: None,
            max_uses: None,
        })),
    )
    .await;
    assert_eq!(old_grant.unwrap_err().status, StatusCode::UNAUTHORIZED);
    let lockable = unlock_admin(
        State(state.clone()),
        headers.clone(),
        Ok(Json(AdminGrantRequest {
            password: "correct horse battery staple".into(),
        })),
    )
    .await
    .unwrap()
    .0;
    let mut lock_headers = HeaderMap::new();
    lock_headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", lockable.admin_token).parse().unwrap(),
    );
    let _ = lock_admin(State(state.clone()), lock_headers.clone())
        .await
        .unwrap();
    let locked = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        lock_headers,
        Ok(Json(InviteRequest {
            expires_in_hours: None,
            max_uses: None,
        })),
    )
    .await;
    assert_eq!(locked.unwrap_err().status, StatusCode::UNAUTHORIZED);
    let expiring = unlock_admin(
        State(state.clone()),
        headers.clone(),
        Ok(Json(AdminGrantRequest {
            password: "correct horse battery staple".into(),
        })),
    )
    .await
    .unwrap()
    .0;
    sqlx::query("UPDATE admin_grants SET issued_at=now()-interval '20 minutes', expires_at=now()-interval '10 minutes' WHERE token_hash=$1")
            .bind(code_hash(&expiring.admin_token)).execute(&pool).await.unwrap();
    let mut expired_headers = HeaderMap::new();
    expired_headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", expiring.admin_token).parse().unwrap(),
    );
    let expired_grant = create_invite(
        State(state.clone()),
        Path(admin.community_id.to_string()),
        expired_headers,
        Ok(Json(InviteRequest {
            expires_in_hours: None,
            max_uses: None,
        })),
    )
    .await;
    assert_eq!(expired_grant.unwrap_err().status, StatusCode::UNAUTHORIZED);

    let rotated = refresh(
        State(state.clone()),
        Ok(Json(RefreshRequest {
            refresh_token: winner.refresh_token.clone(),
        })),
    )
    .await
    .unwrap()
    .0;
    let reuse = refresh(
        State(state.clone()),
        Ok(Json(RefreshRequest {
            refresh_token: winner.refresh_token,
        })),
    )
    .await;
    assert_eq!(reuse.unwrap_err().status, StatusCode::UNAUTHORIZED);
    let revoked = refresh(
        State(state.clone()),
        Ok(Json(RefreshRequest {
            refresh_token: rotated.refresh_token,
        })),
    )
    .await;
    assert_eq!(revoked.unwrap_err().status, StatusCode::UNAUTHORIZED);

    let again = login(
        State(state.clone()),
        Ok(Json(LoginRequest {
            login: "Alice".into(),
            password: "correct horse battery staple".into(),
        })),
    )
    .await;
    let active_user = if again.is_ok() { "Alice" } else { "Bob" };
    let active = login(
        State(state.clone()),
        Ok(Json(LoginRequest {
            login: active_user.into(),
            password: "correct horse battery staple".into(),
        })),
    )
    .await
    .unwrap()
    .0;
    let mut active_headers = HeaderMap::new();
    active_headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", active.access_token).parse().unwrap(),
    );
    let _ = logout(State(state.clone()), active_headers).await.unwrap();
    assert_eq!(
        refresh(
            State(state.clone()),
            Ok(Json(RefreshRequest {
                refresh_token: active.refresh_token
            }))
        )
        .await
        .unwrap_err()
        .status,
        StatusCode::UNAUTHORIZED
    );

    recover_admin(
        &pool,
        "Admin",
        "new correct horse battery staple",
        "test-operator",
    )
    .await
    .unwrap();
    assert_eq!(
        me(State(state.clone()), headers).await.unwrap_err().status,
        StatusCode::UNAUTHORIZED
    );
    assert!(login(
        State(state.clone()),
        Ok(Json(LoginRequest {
            login: "Admin".into(),
            password: "correct horse battery staple".into()
        }))
    )
    .await
    .is_err());
    assert!(login(
        State(state.clone()),
        Ok(Json(LoginRequest {
            login: "Admin".into(),
            password: "new correct horse battery staple".into()
        }))
    )
    .await
    .is_ok());
    assert_eq!(
        registration_disabled().await.error.code,
        "public_registration_disabled"
    );
    let stored_hash: String =
        sqlx::query_scalar("SELECT password_hash FROM accounts WHERE login_key='admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(stored_hash.starts_with("$argon2id$") && !stored_hash.contains("new correct horse"));
    let invitation_hash: Vec<u8> = sqlx::query_scalar("SELECT code_hash FROM invitations LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(invitation_hash.len(), 32);
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM host_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit_count, 2);
    let grant_audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE account_id=(SELECT id FROM accounts WHERE login_key='admin')")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(grant_audit_count, 7);
}
