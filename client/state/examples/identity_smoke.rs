//! Isolated ADR-002 ↔ Client-102 network proof. Use disposable accounts only.
use cordel_state::{ClientState, IdentityClient, MemoryRefreshStore, NativeHttpsTransport};

fn main() -> Result<(), String> {
    let mut args = std::env::args();
    let _program = args.next();
    let base = args
        .next()
        .ok_or("Usage: identity_smoke <https-origin> <disposable-login>")?;
    let login = args
        .next()
        .ok_or("Usage: identity_smoke <https-origin> <disposable-login>")?;
    if args.next().is_some() {
        return Err("Usage: identity_smoke <https-origin> <disposable-login>".to_owned());
    }
    let password = rpassword::prompt_password("Disposable account password: ")
        .map_err(|_| "Could not read password from terminal".to_owned())?;
    let transport = NativeHttpsTransport::new(&base).map_err(|error| format!("{error:?}"))?;
    let state = ClientState::restore(MemoryRefreshStore::default(), None);
    let mut client = IdentityClient::new(state, transport);

    client
        .login(&login, &password)
        .map_err(|error| format!("login: {error:?}"))?;
    let me = client.me().map_err(|error| format!("me: {error:?}"))?;
    if me.login != login {
        return Err("/v1/me returned a different login".to_owned());
    }
    client
        .refresh()
        .map_err(|error| format!("refresh: {error:?}"))?;
    client
        .logout()
        .map_err(|error| format!("logout: {error:?}"))?;
    println!("Identity smoke passed: login, me, refresh rotation, logout.");
    Ok(())
}
