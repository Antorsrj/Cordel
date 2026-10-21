use cordel_identity::{
    bootstrap_admin, change_admin_role, recover_admin, router, AppState, LiveKitApi, MediaConfig,
};
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
        let mut state = AppState::new(pool, secret)?;
        // The LiveKit service is optional. Without it the media routes are not
        // registered and the API serves identity, community, and chat only.
        match media_config() {
            Ok(Some(config)) => state = state.with_media(config),
            Ok(None) => {}
            Err(message) => return Err(message.into()),
        }
        // The host-local server API lets a moderation action drain a room. It
        // is optional, and without it the action says the drain did not happen.
        if let Ok(api_url) = env::var("CORDEL_LIVEKIT_API_URL") {
            if !api_url.trim().is_empty() {
                let api = LiveKitApi::new(
                    env::var("CORDEL_LIVEKIT_API_KEY").unwrap_or_default(),
                    env::var("CORDEL_LIVEKIT_API_SECRET").unwrap_or_default(),
                    api_url,
                )
                .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
                state = state.with_media_api(api);
            }
        }
        let address: SocketAddr = env::var("CORDEL_LISTEN_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
            .parse()?;
        if !address.ip().is_loopback() {
            return Err("Identity listener must bind to loopback behind the reverse proxy".into());
        }
        let listener = tokio::net::TcpListener::bind(address).await?;
        // The peer address is needed for the rate limit, and the proxy
        // connects from loopback so a forwarded address can be trusted.
        let service = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
        // The admission gate has its own loopback listener. The public path
        // never forwards to it, so it is not reachable from the Internet.
        let gate = if state.has_media() {
            let gate_address: SocketAddr = env::var("CORDEL_MEDIA_GATE_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:3001".to_owned())
                .parse()?;
            if !gate_address.ip().is_loopback() {
                return Err("The media gate must bind to loopback".into());
            }
            let gate_listener = tokio::net::TcpListener::bind(gate_address).await?;
            let gate_state = state.clone();
            tokio::spawn(async move {
                axum::serve(gate_listener, cordel_identity::gate_router(gate_state)).await
            });
            println!("Media admission gate listening on {gate_address}");
            Some(gate_address)
        } else {
            None
        };
        let _ = gate;
        axum::serve(listener, service).await?;
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

/// Read the LiveKit configuration from the environment. All three values must
/// be present together; a partial configuration is a host error, not a
/// silently disabled media service.
fn media_config() -> Result<Option<MediaConfig>, String> {
    let api_key = env::var("CORDEL_LIVEKIT_API_KEY").ok();
    let api_secret = env::var("CORDEL_LIVEKIT_API_SECRET").ok();
    let url = env::var("CORDEL_LIVEKIT_URL").ok();
    match (api_key, api_secret, url) {
        (None, None, None) => Ok(None),
        (Some(api_key), Some(api_secret), Some(url)) => MediaConfig::new(api_key, api_secret, url)
            .map(Some)
            .map_err(|message| message.to_owned()),
        _ => Err(
            "CORDEL_LIVEKIT_API_KEY, CORDEL_LIVEKIT_API_SECRET, and CORDEL_LIVEKIT_URL must all be set"
                .into(),
        ),
    }
}
