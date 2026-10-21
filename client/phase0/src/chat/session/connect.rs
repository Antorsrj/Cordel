//! The connection attempt and the outcome handling.
//!
//! The worker owns the backoff: the live session says when the next
//! attempt may happen, and the worker waits for that moment instead of
//! retrying the instant a token arrives.

use super::*;

pub(crate) struct Attempt {
    pub(crate) connected: bool,
    /// When the next attempt may happen. `None` means the caller decides.
    pub(crate) retry_after: Option<Duration>,
}

pub(crate) fn connect(
    session: &mut LiveSession<NativeLiveTransport, NativeChatTransport>,
    state: &mut ClientState<MemoryRefreshStore>,
    credentials: &Credentials,
    channel_id: &str,
    blocked: &mut bool,
    ui: &Sender<UiEvent>,
    identity: &Sender<IdentityCommand>,
) -> Attempt {
    if session.set_read_timeout(START_TIMEOUT).is_err() {
        status(ui, "Could not configure the chat socket.");
        return Attempt {
            connected: false,
            retry_after: None,
        };
    }
    match session.start(state, &credentials.token, 0) {
        LiveOutcome::Ready => {
            let _ = session.set_read_timeout(POLL_TIMEOUT);
            let _ = ui.send(UiEvent::Connected(true));
            status(
                ui,
                &format!("Connected to channel {}.", short_id(channel_id)),
            );
            Attempt {
                connected: true,
                retry_after: None,
            }
        }
        LiveOutcome::Reconnect { delay, attempt } => {
            status(
                ui,
                &format!("Chat reconnect {attempt} in {} ms.", delay.as_millis()),
            );
            // The access token is still valid, so the next attempt reuses it.
            // A new token is only needed when the server refuses the session.
            let _ = identity.send(IdentityCommand::RefreshAccess);
            Attempt {
                connected: false,
                retry_after: Some(delay),
            }
        }
        LiveOutcome::Stopped { reason } => {
            *blocked = true;
            status(ui, &blocked_message(reason));
            let _ = identity.send(IdentityCommand::CheckProfile);
            Attempt {
                connected: false,
                retry_after: None,
            }
        }
        LiveOutcome::Fatal(error) => {
            status(ui, &format!("Chat failed: {}", describe_live_error(&error)));
            Attempt {
                connected: false,
                retry_after: None,
            }
        }
        other => {
            status(ui, &format!("Chat did not become ready: {other:?}"));
            Attempt {
                connected: false,
                retry_after: None,
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_outcome(
    outcome: LiveOutcome,
    connected: &mut bool,
    blocked: &mut bool,
    needs_reset: &mut bool,
    credentials: Option<&Credentials>,
    ui: &Sender<UiEvent>,
    identity: &Sender<IdentityCommand>,
) -> Option<Duration> {
    let mut retry = None;
    match outcome {
        LiveOutcome::Ready | LiveOutcome::Ignored => {}
        LiveOutcome::Subscribed {
            channel_id,
            last_seq,
            applied,
        } => {
            if *needs_reset {
                *needs_reset = false;
                let _ = ui.send(UiEvent::Reset);
            }
            if let Some(current) = credentials {
                emit_applied(&applied, current, ui);
            }
            status(
                ui,
                &format!(
                    "Live on channel {} at sequence {last_seq}.",
                    short_id(&channel_id)
                ),
            );
        }
        LiveOutcome::Applied { applied, .. } => {
            if let Some(current) = credentials {
                emit_applied(&applied, current, ui);
            }
        }
        LiveOutcome::Ack { seq, .. } => {
            status(ui, &format!("Message acknowledged at sequence {seq}."));
        }
        LiveOutcome::Unsubscribed { channel_id } => {
            status(
                ui,
                &format!("Stopped watching channel {}.", short_id(&channel_id)),
            );
        }
        LiveOutcome::Reconnect { delay, attempt } => {
            *connected = false;
            let _ = ui.send(UiEvent::Connected(false));
            status(
                ui,
                &format!(
                    "Chat lost the socket; reconnect {attempt} in {} ms.",
                    delay.as_millis()
                ),
            );
            // A new token is asked for once, so the next attempt has a current
            // access token even if the old one expired while the network was
            // down.
            let _ = identity.send(IdentityCommand::RefreshAccess);
            retry = Some(delay);
        }
        LiveOutcome::Stopped { reason } => {
            *connected = false;
            *blocked = true;
            let _ = ui.send(UiEvent::Connected(false));
            status(ui, &blocked_message(reason));
            let _ = identity.send(IdentityCommand::CheckProfile);
        }
        LiveOutcome::Fatal(error) => {
            *connected = false;
            let _ = ui.send(UiEvent::Connected(false));
            status(
                ui,
                &format!("Chat stopped: {}", describe_live_error(&error)),
            );
        }
        LiveOutcome::Error(error) => {
            status(ui, &format!("Chat error: {}", describe_live_error(&error)));
        }
    }
    retry
}

fn emit_applied(applied: &[AppliedEvent], credentials: &Credentials, ui: &Sender<UiEvent>) {
    for event in applied {
        if !event.known_message {
            continue;
        }
        let Some(body) = &event.body else {
            continue;
        };
        let author = author_label(
            event.author_id.as_deref(),
            &credentials.user_id,
            &credentials.login,
        );
        let _ = ui.send(UiEvent::Message {
            author,
            body: body.clone(),
        });
    }
}

fn blocked_message(reason: AccessBlock) -> String {
    match reason {
        AccessBlock::Kick => "Access revoked: kicked.".to_owned(),
        AccessBlock::Ban => "Access revoked: banned.".to_owned(),
        AccessBlock::Revoked => "Access revoked. Sign in again.".to_owned(),
    }
}

fn describe_live_error(error: &LiveError) -> String {
    match error {
        LiveError::NotConnected => "not connected".to_owned(),
        LiveError::MissingCredential => "missing access token".to_owned(),
        LiveError::InvalidEndpoint => "invalid endpoint".to_owned(),
        LiveError::Transport(_) => "network transport failed".to_owned(),
        LiveError::UnsupportedVersion { v } => format!("unsupported protocol version {v}"),
        LiveError::Protocol { code, message } => format!("{code}: {message}"),
        LiveError::InvalidFrame { field } => format!("invalid frame field {field}"),
        LiveError::History(_) => "history request failed".to_owned(),
        LiveError::Gap { .. } => "history gap".to_owned(),
    }
}
