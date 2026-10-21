use super::*;

async fn create_session(
    tx: &mut Transaction<'_, Postgres>,
    state: &AppState,
    account_id: Uuid,
    community_id: Uuid,
) -> Result<SessionResponse, ApiError> {
    let session_id = Uuid::new_v4();
    let refresh_token = random_code();
    let refresh_expires = Utc::now() + Duration::days(REFRESH_DAYS);
    sqlx::query("INSERT INTO sessions (id,account_id,expires_at) VALUES ($1,$2,$3)")
        .bind(session_id)
        .bind(account_id)
        .bind(refresh_expires)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal())?;
    sqlx::query(
        "INSERT INTO refresh_credentials (token_hash,session_id,expires_at) VALUES ($1,$2,$3)",
    )
    .bind(code_hash(&refresh_token))
    .bind(session_id)
    .bind(refresh_expires)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApiError::internal())?;
    let access_token = state.issue_access(account_id, session_id)?;
    Ok(SessionResponse {
        schema_version: 1,
        user_id: account_id,
        community_id,
        access_token,
        refresh_token,
        expires_in_secs: ACCESS_SECONDS,
    })
}

pub(super) async fn redeem(
    State(state): State<AppState>,
    ClientIp(address): ClientIp,
    payload: Result<Json<RedeemRequest>, JsonRejection>,
) -> Result<Json<SessionResponse>, ApiError> {
    let body = payload.map_err(json_error)?.0;
    let login_key = normalize_login(&body.login).ok_or_else(ApiError::invalid)?;
    if !valid_password(&body.password)
        || body.invite_code.len() != 64
        || !body
            .invite_code
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError::invalid());
    }
    state.limit_address(
        address,
        "invite-redemption",
        100,
        INVITE_ATTEMPTS_PER_ADDRESS,
    )?;
    let password_hash = hash_password_async(body.password).await?;
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    let invite = sqlx::query("SELECT id,community_id,uses_remaining,expires_at FROM invitations WHERE code_hash=$1 FOR UPDATE")
        .bind(code_hash(&body.invite_code)).fetch_optional(&mut *tx).await.map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::invalid)?;
    let uses: i32 = invite.get("uses_remaining");
    let expires: DateTime<Utc> = invite.get("expires_at");
    if uses < 1 || expires <= Utc::now() {
        return Err(ApiError::invalid());
    }
    let account_id = Uuid::new_v4();
    let community_id: Uuid = invite.get("community_id");
    let inserted =
        sqlx::query("INSERT INTO accounts (id,login,login_key,password_hash) VALUES ($1,$2,$3,$4)")
            .bind(account_id)
            .bind(&body.login)
            .bind(login_key)
            .bind(password_hash)
            .execute(&mut *tx)
            .await;
    if let Err(error) = inserted {
        if is_unique(&error) {
            return Err(ApiError::invalid());
        }
        return Err(ApiError::internal());
    }
    sqlx::query("INSERT INTO memberships (community_id,account_id,role) VALUES ($1,$2,'user')")
        .bind(community_id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    sqlx::query("UPDATE invitations SET uses_remaining=uses_remaining-1 WHERE id=$1")
        .bind(invite.get::<Uuid, _>("id"))
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    let response = create_session(&mut tx, &state, account_id, community_id).await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(response))
}

pub(super) async fn login(
    State(state): State<AppState>,
    ClientIp(address): ClientIp,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Json<SessionResponse>, ApiError> {
    let body = payload.map_err(json_error)?.0;
    let key = normalize_login(&body.login).unwrap_or_default();
    state.limit_address(
        address,
        &format!("login:{key}"),
        10,
        LOGIN_ATTEMPTS_PER_ADDRESS,
    )?;
    if key.is_empty() || body.password.len() > 128 {
        return Err(ApiError::unauthenticated());
    }
    let row = sqlx::query("SELECT a.id,a.password_hash,m.community_id,m.kicked_until,m.banned_at FROM accounts a JOIN memberships m ON m.account_id=a.id WHERE a.login_key=$1")
        .bind(key).fetch_optional(&state.pool).await.map_err(|_| ApiError::internal())?;
    let Some(row) = row else {
        // Perform an Argon2id operation for unknown logins too. The public
        // response is identical and a cheap lookup must not reveal existence.
        let _ = hash_password_async(body.password).await;
        return Err(ApiError::unauthenticated());
    };
    let hash: String = row.get("password_hash");
    if !verify_password_async(body.password, hash).await? {
        return Err(ApiError::unauthenticated());
    }
    let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
    let banned_at: Option<DateTime<Utc>> = row.get("banned_at");
    if banned_at.is_some() || kicked_until.is_some_and(|until| until > Utc::now()) {
        return Err(ApiError::forbidden());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    let response = create_session(&mut tx, &state, row.get("id"), row.get("community_id")).await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(response))
}

pub(super) async fn refresh(
    State(state): State<AppState>,
    ClientIp(address): ClientIp,
    payload: Result<Json<RefreshRequest>, JsonRejection>,
) -> Result<Json<SessionResponse>, ApiError> {
    let body = payload.map_err(json_error)?.0;
    state.limit_address(address, "refresh", 300, REFRESH_ATTEMPTS_PER_ADDRESS)?;
    if body.refresh_token.len() != 64
        || !body
            .refresh_token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError::unauthenticated());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    let row = sqlx::query("SELECT r.session_id,r.expires_at,r.consumed_at,s.account_id,s.revoked_at,s.expires_at AS session_expires,m.community_id,m.kicked_until,m.banned_at \
        FROM refresh_credentials r JOIN sessions s ON s.id=r.session_id JOIN memberships m ON m.account_id=s.account_id \
        WHERE r.token_hash=$1 FOR UPDATE OF r,s")
        .bind(code_hash(&body.refresh_token)).fetch_optional(&mut *tx).await.map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::unauthenticated)?;
    let session_id: Uuid = row.get("session_id");
    let consumed_at: Option<DateTime<Utc>> = row.get("consumed_at");
    if consumed_at.is_some() {
        sqlx::query("UPDATE sessions SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL")
            .bind(session_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ApiError::internal())?;
        tx.commit().await.map_err(|_| ApiError::internal())?;
        return Err(ApiError::unauthenticated());
    }
    let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
    let expires_at: DateTime<Utc> = row.get("expires_at");
    let session_expires: DateTime<Utc> = row.get("session_expires");
    let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
    let banned_at: Option<DateTime<Utc>> = row.get("banned_at");
    if revoked_at.is_some() || expires_at <= Utc::now() || session_expires <= Utc::now() {
        return Err(ApiError::unauthenticated());
    }
    if banned_at.is_some() || kicked_until.is_some_and(|until| until > Utc::now()) {
        return Err(ApiError::forbidden());
    }
    let next = random_code();
    sqlx::query("UPDATE refresh_credentials SET consumed_at=now() WHERE token_hash=$1")
        .bind(code_hash(&body.refresh_token))
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    sqlx::query(
        "INSERT INTO refresh_credentials (token_hash,session_id,expires_at) VALUES ($1,$2,$3)",
    )
    .bind(code_hash(&next))
    .bind(session_id)
    .bind(session_expires)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?;
    let account_id: Uuid = row.get("account_id");
    let community_id: Uuid = row.get("community_id");
    let access_token = state.issue_access(account_id, session_id)?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(SessionResponse {
        schema_version: 1,
        user_id: account_id,
        community_id,
        access_token,
        refresh_token: next,
        expires_in_secs: ACCESS_SECONDS,
    }))
}

pub(super) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<EmptyResponse>, ApiError> {
    let principal = state.authenticate(&headers).await?;
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    sqlx::query(
        "UPDATE admin_grants SET revoked_at=now() WHERE session_id=$1 AND revoked_at IS NULL",
    )
    .bind(principal.session_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL")
        .bind(principal.session_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(EmptyResponse { schema_version: 1 }))
}

async fn audit_admin(
    tx: &mut Transaction<'_, Postgres>,
    principal: &Principal,
    action: &str,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO admin_audit (id,account_id,session_id,community_id,action) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(principal.account_id)
        .bind(principal.session_id)
        .bind(principal.community_id)
        .bind(action)
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::internal())?;
    Ok(())
}

pub(super) async fn unlock_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<AdminGrantRequest>, JsonRejection>,
) -> Result<Json<AdminGrantResponse>, ApiError> {
    let principal = state.authenticate(&headers).await?;
    let body = payload.map_err(json_error)?.0;
    if principal.role != "admin" {
        return Err(ApiError::forbidden());
    }
    state.limit(&format!("admin-unlock:{}", principal.account_id), 5)?;
    if body.password.len() > 128 {
        return Err(ApiError::unauthenticated());
    }
    let password_hash: String =
        sqlx::query_scalar("SELECT password_hash FROM accounts WHERE id=$1")
            .bind(principal.account_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|_| ApiError::internal())?;
    if !verify_password_async(body.password, password_hash.clone()).await? {
        let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
        audit_admin(&mut tx, &principal, "grant_denied").await?;
        tx.commit().await.map_err(|_| ApiError::internal())?;
        return Err(ApiError::unauthenticated());
    }
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    // Lock account, session, and membership through issuance so a concurrent
    // password recovery, logout, or demotion cannot issue a stale grant.
    let row = sqlx::query(
        "SELECT a.password_hash,s.revoked_at,s.expires_at,m.role,m.kicked_until,m.banned_at \
        FROM accounts a JOIN sessions s ON s.account_id=a.id \
        JOIN memberships m ON m.account_id=a.id \
        WHERE a.id=$1 AND s.id=$2 AND m.community_id=$3 FOR SHARE OF a,s,m",
    )
    .bind(principal.account_id)
    .bind(principal.session_id)
    .bind(principal.community_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(ApiError::unauthenticated)?;
    let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
    if row.get::<String, _>("password_hash") != password_hash
        || row.get::<Option<DateTime<Utc>>, _>("revoked_at").is_some()
        || row.get::<DateTime<Utc>, _>("expires_at") <= Utc::now()
        || row.get::<String, _>("role") != "admin"
        || row.get::<Option<DateTime<Utc>>, _>("banned_at").is_some()
        || kicked_until.is_some_and(|until| until > Utc::now())
    {
        return Err(ApiError::forbidden());
    }
    let token = random_code();
    sqlx::query("INSERT INTO admin_grants (token_hash,session_id,account_id,community_id,expires_at) VALUES ($1,$2,$3,$4,now()+interval '10 minutes')")
        .bind(code_hash(&token))
        .bind(principal.session_id)
        .bind(principal.account_id)
        .bind(principal.community_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal())?;
    audit_admin(&mut tx, &principal, "grant_issued").await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(AdminGrantResponse {
        schema_version: 1,
        admin_token: token,
        expires_in_secs: ADMIN_GRANT_SECONDS,
    }))
}

pub(super) async fn lock_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<EmptyResponse>, ApiError> {
    let principal = state.authenticate_admin_grant(&headers).await?;
    let token = bearer_token(&headers).ok_or_else(ApiError::unauthenticated)?;
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    sqlx::query(
        "UPDATE admin_grants SET revoked_at=now() WHERE token_hash=$1 AND revoked_at IS NULL",
    )
    .bind(code_hash(token))
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal())?;
    audit_admin(&mut tx, &principal, "grant_revoked").await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(EmptyResponse { schema_version: 1 }))
}

pub(super) async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let principal = state.authenticate(&headers).await?;
    Ok(Json(MeResponse {
        schema_version: 1,
        user_id: principal.account_id,
        login: principal.login,
        community_id: principal.community_id,
        role: principal.role,
        session_expires_at: principal.session_expires_at,
    }))
}

pub(super) async fn create_invite(
    State(state): State<AppState>,
    Path(raw_community_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<InviteRequest>, JsonRejection>,
) -> Result<Json<InviteResponse>, ApiError> {
    let community_id = Uuid::parse_str(&raw_community_id).map_err(|_| ApiError::invalid())?;
    let body = payload.map_err(json_error)?.0;
    let principal =
        policy::authorize(&state, &headers, community_id, policy::Action::Manage).await?;
    let hours = body.expires_in_hours.unwrap_or(24);
    let uses = body.max_uses.unwrap_or(1);
    if !(1..=168).contains(&hours) || !(1..=25).contains(&uses) {
        return Err(ApiError::invalid());
    }
    let code = random_code();
    let expires_at = Utc::now() + Duration::hours(hours);
    let invitation_id = Uuid::new_v4();
    let mut tx = state.pool.begin().await.map_err(|_| ApiError::internal())?;
    policy::lock_write(&mut tx, &principal, &headers, policy::Action::Manage).await?;
    sqlx::query("INSERT INTO invitations (id,community_id,issued_by,code_hash,uses_remaining,expires_at) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(invitation_id).bind(community_id).bind(principal.account_id).bind(code_hash(&code))
        .bind(uses).bind(expires_at).execute(&mut *tx).await.map_err(|_| ApiError::internal())?;
    audit_admin(&mut tx, &principal, "invitation_created").await?;
    tx.commit().await.map_err(|_| ApiError::internal())?;
    Ok(Json(InviteResponse {
        schema_version: 1,
        invitation_id,
        invite_code: code,
        expires_at,
        max_uses: uses,
    }))
}
