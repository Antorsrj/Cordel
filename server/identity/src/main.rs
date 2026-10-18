use cordel_identity::{bootstrap_admin, change_admin_role, recover_admin, router, AppState};
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("");
    if !matches!(
        command,
        "migrate" | "serve" | "bootstrap-admin" | "recover-admin" | "grant-admin" | "revoke-admin"
    ) {
        return Err("Usage: cordel-identity migrate | serve | bootstrap-admin <login> <community-name> | recover-admin <login> | grant-admin <login> | revoke-admin <login>".into());
    }
    let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;
    if command == "migrate" {
        if args.len() != 2 {
            return Err("Usage: cordel-identity migrate".into());
        }
        sqlx::migrate!("./migrations").run(&pool).await?;
        println!("Identity migrations applied.");
        return Ok(());
    }

    if command == "serve" {
        if args.len() != 2 {
            return Err("Usage: cordel-identity serve".into());
        }
        let secret_hex =
            env::var("CORDEL_JWT_SECRET_HEX").map_err(|_| "CORDEL_JWT_SECRET_HEX is required")?;
        let secret =
            hex::decode(secret_hex).map_err(|_| "CORDEL_JWT_SECRET_HEX must be hexadecimal")?;
        let state = AppState::new(pool, secret)?;
        let address: SocketAddr = env::var("CORDEL_LISTEN_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
            .parse()?;
        if !address.ip().is_loopback() {
            return Err("Identity listener must bind to loopback behind the reverse proxy".into());
        }
        let listener = tokio::net::TcpListener::bind(address).await?;
        axum::serve(listener, router(state)).await?;
        return Ok(());
    }

    let login = args.get(2).ok_or("Login argument is required")?;
    let expected_args = if command == "bootstrap-admin" { 4 } else { 3 };
    if args.len() != expected_args {
        return Err("Incorrect command arguments".into());
    }
    let operator = env::var("SUDO_USER")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "host-operator".to_owned());
    if command == "grant-admin" || command == "revoke-admin" {
        change_admin_role(&pool, login, command == "grant-admin", &operator).await?;
        println!("Host admin role changed; audit record written.");
        return Ok(());
    }
    let password = rpassword::prompt_password("New admin password: ")?;
    let confirmation = rpassword::prompt_password("Confirm password: ")?;
    if password != confirmation {
        return Err("Passwords do not match".into());
    }
    if command == "bootstrap-admin" {
        bootstrap_admin(&pool, login, &password, &args[3], &operator).await?;
        println!("First admin created; host audit record written.");
    } else {
        recover_admin(&pool, login, &password, &operator).await?;
        println!("Admin credential reset; existing sessions revoked; host audit record written.");
    }
    Ok(())
}
