//! Blocking chat worker for Phase 0.
//!
//! The identity worker owns the refresh credential. This worker receives only
//! the current access token and account identity, and keeps its own disposable
//! cursor/outbox state. That avoids sharing a mutable `ClientState` across
//! threads while still exercising the real `LiveSession` path.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use cordel_state::{
    AccessBlock, AppliedEvent, ChatTransportError, ClientState, LiveError, LiveOutcome,
    LiveSession, MemoryRefreshStore, NativeChatTransport, NativeLiveTransport, SendOutcome,
};

use crate::account::{AccessSource, AccountEvent};
use crate::identity::Command as IdentityCommand;

use super::{author_label, events_url};

const START_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_TIMEOUT: Duration = Duration::from_millis(100);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 4000;

#[derive(Debug)]
pub enum Command {
    Send(String),
    Reconnect,
    SignOut,
}

#[derive(Debug)]
pub enum UiEvent {
    Reset,
    Message { author: String, body: String },
    Connected(bool),
    Status(String),
}

struct Credentials {
    token: String,
    user_id: String,
    login: String,
}

/// Endpoints the worker needs. The shell reads them from the environment; a
/// test supplies them directly.
#[derive(Debug, Clone)]
pub struct Config {
    pub origin: String,
    pub channel_id: String,
}

pub fn run(
    commands: Receiver<Command>,
    accounts: Receiver<AccountEvent>,
    ui: Sender<UiEvent>,
    identity: Sender<IdentityCommand>,
) {
    let Some(channel_id) = env_value("CORDEL_CHANNEL_ID") else {
        status(
            &ui,
            "Set CORDEL_CHANNEL_ID to a text channel UUID to enable live chat.",
        );
        return;
    };
    let Some(origin) = env_value("CORDEL_API_ORIGIN") else {
        status(&ui, "Set CORDEL_API_ORIGIN to enable live chat.");
        return;
    };
    run_with(
        Config { origin, channel_id },
        commands,
        accounts,
        ui,
        identity,
    );
}

pub fn run_with(
    config: Config,
    commands: Receiver<Command>,
    accounts: Receiver<AccountEvent>,
    ui: Sender<UiEvent>,
    identity: Sender<IdentityCommand>,
) {
    let Config { origin, channel_id } = config;
    if origin.starts_with("http://") && !cfg!(feature = "local-smoke") {
        status(
            &ui,
            "Local HTTP/WS requires a build with --features local-smoke.",
        );
        return;
    }
    let Some(endpoint) = events_url(&origin) else {
        status(
            &ui,
            "CORDEL_API_ORIGIN must be a bare http or https origin.",
        );
        return;
    };
    if endpoint.starts_with("ws://") && !cfg!(feature = "local-smoke") {
        status(
            &ui,
            "Local HTTP/WS requires a build with --features local-smoke.",
        );
        return;
    }

    let chat = match build_chat(&origin) {
        Ok(chat) => chat,
        Err(_) => {
            status(&ui, "The API origin is not a valid chat endpoint.");
            return;
        }
    };
    let live = match build_live(&endpoint) {
        Ok(live) => live,
        Err(_) => {
            status(&ui, "The API origin does not produce a valid events URL.");
            return;
        }
    };

    let mut session = LiveSession::new(live, chat, endpoint, vec![channel_id.clone()]);
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    let mut credentials: Option<Credentials> = None;
    let mut connected = false;
    let mut blocked = false;
    let mut needs_reset = true;
    let mut last_reconcile = Instant::now();
    // When the next connection attempt may happen. The client owns the backoff
    // instead of retrying the moment a token arrives.
    let mut retry_after: Option<Instant> = None;

    loop {
        while let Ok(event) = accounts.try_recv() {
            match event {
                AccountEvent::Access {
                    token,
                    user_id,
                    login,
                    source,
                } => {
                    state.tokens_mut().set_access(&token);
                    credentials = Some(Credentials {
                        token,
                        user_id,
                        login,
                    });
                    if source == AccessSource::Login {
                        blocked = false;
                    }
                    if !connected
                        && !blocked
                        && retry_after.map(|at| Instant::now() >= at).unwrap_or(true)
                    {
                        if let Some(current) = credentials.as_ref() {
                            let attempt = connect(
                                &mut session,
                                &mut state,
                                current,
                                &channel_id,
                                &mut blocked,
                                &ui,
                                &identity,
                            );
                            connected = attempt.connected;
                            retry_after = attempt.retry_after.map(|delay| Instant::now() + delay);
                            if connected {
                                last_reconcile = Instant::now();
                            }
                        }
                    }
                }
                AccountEvent::SignedOut => {
                    session.close();
                    connected = false;
                    blocked = false;
                    credentials = None;
                    state = ClientState::restore(MemoryRefreshStore::default(), None);
                    let _ = ui.send(UiEvent::Connected(false));
                    if !needs_reset {
                        let _ = ui.send(UiEvent::Reset);
                    }
                    needs_reset = true;
                    status(&ui, "Signed out of chat.");
                }
            }
        }

        let command = if connected {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => break,
            }
        } else {
            match commands.recv_timeout(POLL_TIMEOUT) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };

        if let Some(command) = command {
            handle_command(
                command,
                &mut session,
                &mut state,
                &mut credentials,
                &mut connected,
                &mut blocked,
                &mut needs_reset,
                &channel_id,
                &ui,
                &identity,
            );
            if !connected {
                last_reconcile = Instant::now();
            }
        }

        // A conexao perdida agenda a proxima tentativa, e a tentativa nao
        // depende de um evento novo: o token continua valido. Sem isso, um
        // token novo que chega antes do atraso terminaria descartado e o
        // cliente nunca voltaria.
        if !connected && !blocked && retry_after.map(|at| Instant::now() >= at).unwrap_or(false) {
            if let Some(current) = credentials.as_ref() {
                let attempt = connect(
                    &mut session,
                    &mut state,
                    current,
                    &channel_id,
                    &mut blocked,
                    &ui,
                    &identity,
                );
                connected = attempt.connected;
                retry_after = attempt.retry_after.map(|delay| Instant::now() + delay);
                if connected {
                    last_reconcile = Instant::now();
                }
            }
        }

        if !connected {
            continue;
        }
        let Some(current) = credentials.as_ref() else {
            connected = false;
            let _ = ui.send(UiEvent::Connected(false));
            status(&ui, "Chat has no access token. Sign in again.");
            continue;
        };

        let outcome = session.pump(&mut state, &current.token, 0);
        let retry = handle_outcome(
            outcome,
            &mut connected,
            &mut blocked,
            &mut needs_reset,
            Some(current),
            &ui,
            &identity,
        );
        if let Some(delay) = retry {
            retry_after = Some(Instant::now() + delay);
        }

        if connected && last_reconcile.elapsed() >= RECONCILE_INTERVAL {
            last_reconcile = Instant::now();
            let outcomes = session.reconcile(&mut state, &current.token, 0);
            for outcome in outcomes {
                let retry = handle_outcome(
                    outcome,
                    &mut connected,
                    &mut blocked,
                    &mut needs_reset,
                    Some(current),
                    &ui,
                    &identity,
                );
                if let Some(delay) = retry {
                    retry_after = Some(Instant::now() + delay);
                }
                if !connected {
                    break;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    command: Command,
    session: &mut LiveSession<NativeLiveTransport, NativeChatTransport>,
    state: &mut ClientState<MemoryRefreshStore>,
    credentials: &mut Option<Credentials>,
    connected: &mut bool,
    blocked: &mut bool,
    needs_reset: &mut bool,
    channel_id: &str,
    ui: &Sender<UiEvent>,
    identity: &Sender<IdentityCommand>,
) {
    match command {
        Command::Send(body) => {
            let body = body.trim().to_owned();
            if body.is_empty() {
                status(ui, "Type a message first.");
                return;
            }
            if body.len() > MAX_BODY_BYTES {
                status(ui, "A message is limited to 4000 bytes.");
                return;
            }
            let Some(current) = credentials.as_ref() else {
                status(ui, "Sign in before sending a message.");
                return;
            };
            state.enqueue(channel_id.to_owned(), body);
            let outcomes = session.send_pending(state, &current.token, channel_id);
            if outcomes.is_empty() {
                status(ui, "No pending message to send.");
            }
            for outcome in outcomes {
                match outcome {
                    SendOutcome::Acknowledged { seq, .. } => {
                        status(ui, &format!("Sent as sequence {seq}."));
                    }
                    SendOutcome::Failed { error, .. } => {
                        status(
                            ui,
                            &format!("Send failed: {}. Retry with Reconnect.", send_error(&error)),
                        );
                        if matches!(error, ChatTransportError::Unauthorized) {
                            let _ = identity.send(IdentityCommand::RefreshAccess);
                        }
                    }
                }
            }
        }
        Command::Reconnect => {
            session.close();
            *connected = false;
            let _ = ui.send(UiEvent::Connected(false));
            if let Some(current) = credentials.as_ref() {
                let attempt = connect(session, state, current, channel_id, blocked, ui, identity);
                *connected = attempt.connected;
            } else {
                status(ui, "Sign in before reconnecting chat.");
            }
        }
        Command::SignOut => {
            session.close();
            *connected = false;
            *blocked = false;
            *credentials = None;
            *state = ClientState::restore(MemoryRefreshStore::default(), None);
            let _ = ui.send(UiEvent::Connected(false));
            if !*needs_reset {
                let _ = ui.send(UiEvent::Reset);
            }
            *needs_reset = true;
            status(ui, "Signed out of chat.");
        }
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn status(ui: &Sender<UiEvent>, message: &str) {
    let _ = ui.send(UiEvent::Status(message.to_owned()));
}

fn short_id(id: &str) -> String {
    id.get(..8).unwrap_or(id).to_owned()
}

fn build_chat(origin: &str) -> Result<NativeChatTransport, LiveError> {
    let base = format!("{}/", origin.trim_end_matches('/'));
    #[cfg(feature = "local-smoke")]
    {
        if origin.starts_with("http://") {
            return NativeChatTransport::new_plain(&base);
        }
    }
    NativeChatTransport::new(&base)
}

fn build_live(endpoint: &str) -> Result<NativeLiveTransport, LiveError> {
    #[cfg(feature = "local-smoke")]
    {
        if endpoint.starts_with("ws://") {
            return NativeLiveTransport::new_plain(endpoint);
        }
    }
    NativeLiveTransport::new(endpoint)
}

fn send_error(error: &ChatTransportError) -> String {
    match error {
        ChatTransportError::Network => "network".to_owned(),
        ChatTransportError::Unauthorized => "session expired".to_owned(),
        ChatTransportError::Forbidden => "access denied".to_owned(),
        ChatTransportError::InvalidResponse => "invalid response".to_owned(),
        ChatTransportError::Api { status, code } => format!("{code} (HTTP {status})"),
    }
}

mod connect;
use connect::{connect, handle_outcome};
