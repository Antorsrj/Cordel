use super::*;

pub async fn bootstrap_admin(
    pool: &PgPool,
    login: &str,
    password: &str,
    community_name: &str,
    operator: &str,
) -> Result<(), String> {
    let key =
        normalize_login(login).ok_or("Login must be 3-32 ASCII letters, digits, '_' or '-'")?;
    if !valid_password(password) {
        return Err("Password must be 12-128 bytes".to_owned());
    }
    if community_name.is_empty() || community_name.chars().count() > 100 {
        return Err("Community name must be 1-100 characters".to_owned());
    }
    let hash = hash_password(password).map_err(|_| "Password hashing failed")?;
    let mut tx = pool.begin().await.map_err(|_| "Database unavailable")?;
    sqlx::query("SELECT pg_advisory_xact_lock(721002)")
        .execute(&mut *tx)
        .await
        .map_err(|_| "Database unavailable")?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM communities")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "Database unavailable")?;
    if count != 0 {
        return Err("Community already exists; bootstrap refused".to_owned());
    }
    let community_id = Uuid::new_v4();
    let account_id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id,name) VALUES ($1,$2)")
        .bind(community_id)
        .bind(community_name)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Bootstrap failed")?;
    sqlx::query("INSERT INTO accounts (id,login,login_key,password_hash) VALUES ($1,$2,$3,$4)")
        .bind(account_id)
        .bind(login)
        .bind(key)
        .bind(hash)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Bootstrap failed")?;
    sqlx::query("INSERT INTO memberships (community_id,account_id,role) VALUES ($1,$2,'admin')")
        .bind(community_id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Bootstrap failed")?;
    sqlx::query("INSERT INTO host_audit (id,operator_name,action,account_id,community_id) VALUES ($1,$2,'bootstrap_admin',$3,$4)")
        .bind(Uuid::new_v4()).bind(operator).bind(account_id).bind(community_id).execute(&mut *tx).await.map_err(|_| "Bootstrap failed")?;
    tx.commit().await.map_err(|_| "Bootstrap failed")?;
    Ok(())
}

pub async fn recover_admin(
    pool: &PgPool,
    login: &str,
    password: &str,
    operator: &str,
) -> Result<(), String> {
    let key = normalize_login(login).ok_or("Invalid login")?;
    if !valid_password(password) {
        return Err("Password must be 12-128 bytes".to_owned());
    }
    let hash = hash_password(password).map_err(|_| "Password hashing failed")?;
    let mut tx = pool.begin().await.map_err(|_| "Database unavailable")?;
    let row = sqlx::query("SELECT a.id,m.community_id FROM accounts a JOIN memberships m ON m.account_id=a.id WHERE a.login_key=$1 AND m.role='admin' FOR UPDATE OF a,m")
        .bind(key).fetch_optional(&mut *tx).await.map_err(|_| "Database unavailable")?
        .ok_or("Target must be an existing admin")?;
    let account_id: Uuid = row.get("id");
    let community_id: Uuid = row.get("community_id");
    sqlx::query("UPDATE accounts SET password_hash=$1,credential_changed_at=now() WHERE id=$2")
        .bind(hash)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Recovery failed")?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE account_id=$1 AND revoked_at IS NULL")
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Recovery failed")?;
    sqlx::query("INSERT INTO host_audit (id,operator_name,action,account_id,community_id) VALUES ($1,$2,'recover_admin',$3,$4)")
        .bind(Uuid::new_v4()).bind(operator).bind(account_id).bind(community_id).execute(&mut *tx).await.map_err(|_| "Recovery failed")?;
    tx.commit().await.map_err(|_| "Recovery failed")?;
    Ok(())
}
