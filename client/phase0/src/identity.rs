//! Prototype login and Settings bridge. HTTPS calls stay off the Slint UI thread.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use cordel_state::{
    ClientState, IdentityClient, IdentityError, MemoryRefreshStore, NativeHttpsTransport,
};
use slint::Weak;

use crate::chat::{AccessSource, AccountEvent};
use crate::AppWindow;

pub enum Command {
    Login { login: String, password: String },
    CheckProfile,
    RefreshAccess,
    Unlock { password: String },
    Lock,
    Logout,
}

pub fn run(commands: Receiver<Command>, ui: Weak<AppWindow>, accounts: Sender<AccountEvent>) {
    let origin = match std::env::var("CORDEL_API_ORIGIN") {
        Ok(value) => value,
        Err(_) => {
            update(&ui, |ui| {
                ui.set_identity_status("Set CORDEL_API_ORIGIN to the HTTPS API origin.".into());
            });
            return;
        }
    };
    let transport = match build_transport(&origin) {
        Ok(transport) => transport,
        Err(_) => {
            update(&ui, |ui| {
                ui.set_identity_status(
                    "CORDEL_API_ORIGIN must be a bare HTTPS origin (HTTP requires local-smoke)."
                        .into(),
                );
            });
            return;
        }
    };
    let state = ClientState::restore(MemoryRefreshStore::default(), None);
    let mut client = IdentityClient::new(state, transport);
    let mut last_login = String::new();
    let mut grant_visible = false;
    update(&ui, |ui| {
        ui.set_identity_ready(true);
        ui.set_identity_status("Sign in to the test community.".into());
    });

    loop {
        let command = match commands.recv_timeout(Duration::from_secs(1)) {
            Ok(command) => command,
            Err(RecvTimeoutError::Timeout) => {
                let seconds = client.management_seconds_remaining();
                if grant_visible {
                    if seconds == 0 {
                        grant_visible = false;
                        update(&ui, |ui| {
                            ui.set_management_unlocked(false);
                            ui.set_management_remaining(0);
                            ui.set_identity_status("Management Mode expired.".into());
                        });
                    } else {
                        update(&ui, move |ui| ui.set_management_remaining(seconds as i32));
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        update(&ui, |ui| ui.set_identity_busy(true));
        match command {
            Command::Login { login, password } => {
                let result = client
                    .login(&login, &password)
                    .and_then(|session| client.me().map(|profile| (session, profile)));
                match result {
                    Ok((session, profile)) => {
                        grant_visible = false;
                        last_login = profile.login.clone();
                        send_access(
                            &accounts,
                            &session.access_token,
                            &profile.user_id,
                            &profile.login,
                            AccessSource::Login,
                        );
                        update(&ui, move |ui| {
                            ui.set_logged_in(true);
                            ui.set_settings_open(false);
                            ui.set_account_login(profile.login.into());
                            ui.set_account_role(profile.role.into());
                            ui.set_management_unlocked(false);
                            ui.set_management_remaining(0);
                            ui.set_identity_status("Signed in. Ordinary view is active.".into());
                        });
                    }
                    Err(error) => {
                        let _ = accounts.send(AccountEvent::SignedOut);
                        let _ = client.state_mut().clear_session();
                        clear_ui(&ui, &format!("Sign in failed: {}", describe(&error)));
                    }
                }
            }
            Command::CheckProfile => match client.me() {
                Ok(profile) => {
                    if client.admin_token().is_none() {
                        grant_visible = false;
                    }
                    if profile.role != "admin" {
                        grant_visible = false;
                        if client.admin_token().is_some() {
                            let _ = client.lock_management();
                        }
                    }
                    last_login = profile.login.clone();
                    if let Some(token) = client.state().tokens().access() {
                        send_access(
                            &accounts,
                            token,
                            &profile.user_id,
                            &profile.login,
                            AccessSource::Refresh,
                        );
                    }
                    update(&ui, move |ui| {
                        ui.set_account_role(profile.role.into());
                        if !grant_visible {
                            ui.set_management_unlocked(false);
                            ui.set_management_remaining(0);
                        }
                        ui.set_identity_status("Settings refreshed.".into());
                    });
                }
                Err(error) => {
                    grant_visible = false;
                    if client.admin_token().is_some() {
                        let _ = client.lock_management();
                    }
                    if matches!(error, IdentityError::Transport(_)) {
                        update(&ui, |ui| {
                            ui.set_management_unlocked(false);
                            ui.set_management_remaining(0);
                            ui.set_identity_status(
                                "Could not verify the account. Try Settings again after reconnecting."
                                    .into(),
                            );
                        });
                    } else {
                        let _ = accounts.send(AccountEvent::SignedOut);
                        let _ = client.state_mut().clear_session();
                        clear_ui(&ui, &format!("Session check failed: {}", describe(&error)));
                    }
                }
            },
            Command::RefreshAccess => match client.refresh() {
                Ok(session) => {
                    send_access(
                        &accounts,
                        &session.access_token,
                        &session.user_id,
                        &last_login,
                        AccessSource::Refresh,
                    );
                    update(&ui, |ui| {
                        ui.set_identity_status("Chat session refreshed.".into());
                    });
                }
                Err(error) => {
                    let _ = accounts.send(AccountEvent::SignedOut);
                    clear_ui(
                        &ui,
                        &format!("Session refresh failed: {}", describe(&error)),
                    );
                }
            },
            Command::Unlock { password } => {
                let result = client.me().and_then(|profile| {
                    if profile.role == "admin" {
                        client.unlock_management(&password)
                    } else {
                        Err(IdentityError::MissingCredential)
                    }
                });
                match result {
                    Ok(()) => {
                        grant_visible = true;
                        let seconds = client.management_seconds_remaining();
                        update(&ui, move |ui| {
                            ui.set_management_unlocked(true);
                            ui.set_management_remaining(seconds as i32);
                            ui.set_identity_status(
                                "Management Mode unlocked for this session.".into(),
                            );
                        });
                    }
                    Err(error) => {
                        grant_visible = false;
                        update(&ui, move |ui| {
                            ui.set_management_unlocked(false);
                            ui.set_management_remaining(0);
                            ui.set_identity_status(
                                format!("Unlock failed: {}", describe(&error)).into(),
                            );
                        });
                    }
                }
            }
            Command::Lock => {
                grant_visible = false;
                let result = client.lock_management();
                let message = if result.is_ok() {
                    "Management Mode locked.".to_owned()
                } else {
                    "Management Mode locked locally; server revocation could not be confirmed."
                        .to_owned()
                };
                update(&ui, move |ui| {
                    ui.set_management_unlocked(false);
                    ui.set_management_remaining(0);
                    ui.set_identity_status(message.into());
                });
            }
            Command::Logout => {
                grant_visible = false;
                let _ = accounts.send(AccountEvent::SignedOut);
                let result = client.logout();
                let message = if result.is_ok() {
                    "Signed out.".to_owned()
                } else {
                    "Signed out locally; server revocation could not be confirmed.".to_owned()
                };
                clear_ui(&ui, &message);
            }
        }
        update(&ui, |ui| ui.set_identity_busy(false));
    }
}

fn build_transport(origin: &str) -> Result<NativeHttpsTransport, IdentityError> {
    #[cfg(feature = "local-smoke")]
    {
        if origin.starts_with("http://") {
            return NativeHttpsTransport::new_plain(origin);
        }
    }
    NativeHttpsTransport::new(origin)
}

fn send_access(
    accounts: &Sender<AccountEvent>,
    token: &str,
    user_id: &str,
    login: &str,
    source: AccessSource,
) {
    let _ = accounts.send(AccountEvent::Access {
        token: token.to_owned(),
        user_id: user_id.to_owned(),
        login: login.to_owned(),
        source,
    });
}

fn clear_ui(ui: &Weak<AppWindow>, message: &str) {
    let message = message.to_owned();
    update(ui, move |ui| {
        ui.set_logged_in(false);
        ui.set_settings_open(false);
        ui.set_account_login("".into());
        ui.set_account_role("".into());
        ui.set_management_unlocked(false);
        ui.set_management_remaining(0);
        ui.set_identity_status(message.into());
    });
}

fn describe(error: &IdentityError) -> String {
    match error {
        IdentityError::Api { status, code } => format!("{code} (HTTP {status})"),
        IdentityError::InvalidEndpoint => "invalid HTTPS origin".to_owned(),
        IdentityError::MissingCredential => "no eligible signed-in admin session".to_owned(),
        IdentityError::Store(_) => "credential store unavailable".to_owned(),
        IdentityError::Transport(_) => "network request failed".to_owned(),
        IdentityError::InvalidResponse => "unexpected server response".to_owned(),
    }
}

fn update(ui: &Weak<AppWindow>, callback: impl FnOnce(AppWindow) + Send + 'static) {
    let _ = ui.upgrade_in_event_loop(callback);
}
