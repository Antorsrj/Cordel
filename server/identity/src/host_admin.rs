//! Host-only admin role changes. No HTTP route calls this module.
use super::*;

pub async fn change_admin_role(
    pool: &PgPool,
    login: &str,
    make_admin: bool,
    operator: &str,
) -> Result<(), String> {
    let key = normalize_login(login).ok_or("Invalid login")?;
    if operator.trim().is_empty() {
        return Err("Host operator name is required".to_owned());
    }
    let mut tx = pool.begin().await.map_err(|_| "Database unavailable")?;
    // Serializes all host role changes, including two simultaneous demotions.
    sqlx::query("SELECT pg_advisory_xact_lock(721003)")
        .execute(&mut *tx)
        .await
        .map_err(|_| "Database unavailable")?;
    let row = sqlx::query(
        "SELECT a.id,m.community_id,m.role,m.kicked_until,m.banned_at \
         FROM accounts a JOIN memberships m ON m.account_id=a.id \
         WHERE a.login_key=$1 FOR UPDATE OF m",
    )
    .bind(key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| "Database unavailable")?
    .ok_or("Account is not a member")?;
    let account_id: Uuid = row.get("id");
    let community_id: Uuid = row.get("community_id");
    let role: String = row.get("role");
    let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
    let banned_at: Option<DateTime<Utc>> = row.get("banned_at");
    if make_admin {
        if role == "admin" {
            return Err("Account is already an admin".to_owned());
        }
        if banned_at.is_some() || kicked_until.is_some_and(|until| until > Utc::now()) {
            return Err("Moderated account cannot become admin".to_owned());
        }
    } else {
        if role != "admin" {
            return Err("Account is not an admin".to_owned());
        }
        let active_admins: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memberships WHERE community_id=$1 AND role='admin' \
             AND banned_at IS NULL AND (kicked_until IS NULL OR kicked_until<=now())",
        )
        .bind(community_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "Database unavailable")?;
        if active_admins <= 1 {
            return Err("Cannot remove the last active admin".to_owned());
        }
    }
    let new_role = if make_admin { "admin" } else { "user" };
    sqlx::query("UPDATE memberships SET role=$3 WHERE community_id=$1 AND account_id=$2")
        .bind(community_id)
        .bind(account_id)
        .bind(new_role)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Role change failed")?;
    let revision: i64 = sqlx::query_scalar(
        "UPDATE communities SET authorization_revision=authorization_revision+1 WHERE id=$1 \
         RETURNING authorization_revision",
    )
    .bind(community_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| "Role change failed")?;
    let action = if make_admin {
        "grant_admin"
    } else {
        "revoke_admin"
    };
    sqlx::query("INSERT INTO host_audit (id,operator_name,action,account_id,community_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(operator)
        .bind(action)
        .bind(account_id)
        .bind(community_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Audit write failed")?;
    sqlx::query("SELECT pg_notify('cordel_authorization', $1)")
        .bind(format!("{community_id}:{account_id}:{revision}"))
        .execute(&mut *tx)
        .await
        .map_err(|_| "Role change failed")?;
    tx.commit().await.map_err(|_| "Role change failed")?;
    Ok(())
}
