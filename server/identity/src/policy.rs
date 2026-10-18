//! One authorization entry point for community REST handlers and future chat/media work.
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Member,
    Manage,
}

pub(crate) async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    community_id: Uuid,
    action: Action,
) -> Result<Principal, ApiError> {
    let principal = match action {
        Action::Member => state.authenticate(headers).await?,
        Action::Manage => state.authenticate_admin_grant(headers).await?,
    };
    if principal.community_id != community_id {
        return Err(ApiError::forbidden());
    }
    Ok(principal)
}

// Hold these locks through commit. A concurrent logout, demotion, kick, ban,
// or grant revocation must serialize before a protected write can succeed.
pub(crate) async fn lock_write(
    tx: &mut Transaction<'_, Postgres>,
    principal: &Principal,
    headers: &HeaderMap,
    action: Action,
) -> Result<(), ApiError> {
    let row = sqlx::query(
        "SELECT m.role,m.kicked_until,m.banned_at \
         FROM memberships m JOIN sessions s ON s.account_id=m.account_id \
         WHERE m.community_id=$1 AND m.account_id=$2 AND s.id=$3 \
         AND s.revoked_at IS NULL AND s.expires_at>now() FOR SHARE OF m,s",
    )
    .bind(principal.community_id)
    .bind(principal.account_id)
    .bind(principal.session_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(ApiError::unauthenticated)?;
    let kicked_until: Option<DateTime<Utc>> = row.get("kicked_until");
    if row.get::<Option<DateTime<Utc>>, _>("banned_at").is_some()
        || kicked_until.is_some_and(|until| until > Utc::now())
    {
        return Err(ApiError::forbidden());
    }
    if let Action::Manage = action {
        if row.get::<String, _>("role") != "admin" {
            return Err(ApiError::forbidden());
        }
        let token = bearer_token(headers).ok_or_else(ApiError::unauthenticated)?;
        let grant = sqlx::query(
            "SELECT 1 FROM admin_grants WHERE token_hash=$1 AND account_id=$2 \
             AND session_id=$3 AND community_id=$4 AND revoked_at IS NULL \
             AND expires_at>now() FOR SHARE",
        )
        .bind(code_hash(token))
        .bind(principal.account_id)
        .bind(principal.session_id)
        .bind(principal.community_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ApiError::internal())?;
        if grant.is_none() {
            return Err(ApiError::unauthenticated());
        }
    }
    Ok(())
}
