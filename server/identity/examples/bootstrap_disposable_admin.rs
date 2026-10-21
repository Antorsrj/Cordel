//! Create a disposable first admin for a local automated run.
//!
//! The shipped `bootstrap-admin` command reads the password from the terminal
//! on purpose, so it cannot run unattended. This example exists for a
//! throwaway environment only: the password arrives in an environment
//! variable and the account is a disposable test account.
//!
//! Environment:
//!   DATABASE_URL                        required, PostgreSQL connection URL
//!   CORDEL_DISPOSABLE_ADMIN_PASSWORD    required, 12-128 UTF-8 bytes
//!   CORDEL_DISPOSABLE_ADMIN_LOGIN       optional, default "e2e-admin"
//!   CORDEL_DISPOSABLE_COMMUNITY_NAME    optional, default "E2E Community"
//!
//! The server refuses to bootstrap a second community, so this only works
//! once per database.

use cordel_identity::bootstrap_admin;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let password = std::env::var("CORDEL_DISPOSABLE_ADMIN_PASSWORD")
        .map_err(|_| "CORDEL_DISPOSABLE_ADMIN_PASSWORD is required")?;
    let login =
        std::env::var("CORDEL_DISPOSABLE_ADMIN_LOGIN").unwrap_or_else(|_| "e2e-admin".to_owned());
    let community = std::env::var("CORDEL_DISPOSABLE_COMMUNITY_NAME")
        .unwrap_or_else(|_| "E2E Community".to_owned());
    let operator = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "host-operator".to_owned());

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;
    bootstrap_admin(&pool, &login, &password, &community, &operator).await?;
    println!("Disposable first admin created; host audit record written.");
    Ok(())
}
