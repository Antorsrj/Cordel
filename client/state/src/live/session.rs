use cordel_protocol::{validate_history, FrameError, WsFrame};

use crate::catchup::AppliedEvent;
use crate::credentials::RefreshStore;
use crate::outbox::SendState;
use crate::retry::{AccessBlock, Interrupt, NextAction};
use crate::session::ClientState;

use super::{
    ChatTransport, ChatTransportError, LiveError, LiveOutcome, LiveSession, LiveTransport,
    SendOutcome, HISTORY_PAGE_LIMIT,
};

impl<L: LiveTransport, C: ChatTransport> LiveSession<L, C> {
    pub fn new(live: L, chat: C, endpoint: impl Into<String>, channels: Vec<String>) -> Self {
        let mut channels = channels;
        channels.sort();
        channels.dedup();
        Self {
            live,
            chat,
            endpoint: endpoint.into(),
            channels,
            connected: false,
            attempt: 0,
        }
    }

    pub fn channels(&self) -> &[String] {
        &self.channels
    }

    /// Connect, wait for `ready`, and subscribe to every open channel.
    /// A failure schedules the first reconnect and starts buffering.
    pub fn start<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        jitter_ms: u64,
    ) -> LiveOutcome {
        if let Some(reason) = state.blocked() {
            self.live.close();
            self.connected = false;
            return LiveOutcome::Stopped { reason };
        }
        if self.live.connect(&self.endpoint, token).is_err() {
            return self.interrupt(state, jitter_ms);
        }
        let ready = match self.live.receive() {
            Ok(frame) => frame,
            Err(_) => return self.interrupt(state, jitter_ms),
        };
        if let Err(error) = ready.check_version() {
            self.live.close();
            self.connected = false;
            return LiveOutcome::Fatal(version_error(error));
        }
        if ready.kind != "ready" {
            self.live.close();
            self.connected = false;
            return LiveOutcome::Fatal(LiveError::InvalidFrame { field: "ready" });
        }
        for channel in &self.channels {
            if self
                .live
                .send(&WsFrame::subscribe(channel.clone()))
                .is_err()
            {
                return self.interrupt(state, jitter_ms);
            }
        }
        self.connected = true;
        self.attempt = 0;
        LiveOutcome::Ready
    }

    pub fn add_channel(&mut self, channel_id: &str) -> Result<(), LiveError> {
        if !self.channels.iter().any(|channel| channel == channel_id) {
            self.channels.push(channel_id.to_owned());
        }
        if self.connected {
            self.live
                .send(&WsFrame::subscribe(channel_id))
                .map_err(LiveError::Transport)?;
        }
        Ok(())
    }

    pub fn remove_channel(&mut self, channel_id: &str) -> Result<(), LiveError> {
        self.channels.retain(|channel| channel != channel_id);
        if self.connected {
            self.live
                .send(&WsFrame::unsubscribe(channel_id))
                .map_err(LiveError::Transport)?;
        }
        Ok(())
    }

    /// Process one server frame. The caller decides when to sleep and how to
    /// refresh credentials after `Reconnect`.
    pub fn pump<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        jitter_ms: u64,
    ) -> LiveOutcome {
        if let Some(reason) = state.blocked() {
            self.live.close();
            self.connected = false;
            return LiveOutcome::Stopped { reason };
        }
        if !self.connected {
            return LiveOutcome::Error(LiveError::NotConnected);
        }
        let frame = match self.live.receive() {
            Ok(frame) => frame,
            Err(_) => return self.interrupt(state, jitter_ms),
        };
        if let Err(error) = frame.check_version() {
            self.live.close();
            self.connected = false;
            return LiveOutcome::Fatal(version_error(error));
        }
        match frame.kind.as_str() {
            "ready" => self.on_ready(state, jitter_ms),
            "subscribed" => self.on_subscribed(state, token, &frame, jitter_ms),
            "unsubscribed" => {
                let channel_id = match string_field(&frame, "channel_id") {
                    Ok(channel_id) => channel_id,
                    Err(error) => return LiveOutcome::Error(error),
                };
                self.channels.retain(|channel| channel != &channel_id);
                LiveOutcome::Unsubscribed { channel_id }
            }
            "lagged" => self.on_lagged(state, token, &frame, jitter_ms),
            "ack" => self.on_ack(state, &frame),
            "access_revoked" => self.revoke(state),
            "error" => self.on_error(&frame),
            _ => self.on_event(state, token, frame, jitter_ms),
        }
    }

    /// Periodic history reconciliation covers a commit whose live publish was
    /// lost. It is safe to call while live events are still arriving.
    pub fn reconcile<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        jitter_ms: u64,
    ) -> Vec<LiveOutcome> {
        if let Some(reason) = state.blocked() {
            self.live.close();
            self.connected = false;
            return vec![LiveOutcome::Stopped { reason }];
        }
        let channels = self.channels.clone();
        let mut outcomes = Vec::with_capacity(channels.len());
        for channel in channels {
            let outcome = match self.recover(state, token, &channel) {
                Ok(applied) => LiveOutcome::Applied {
                    channel_id: channel,
                    applied,
                },
                Err(error) => self.handle_live_error(state, error, jitter_ms),
            };
            let stop = matches!(
                outcome,
                LiveOutcome::Reconnect { .. } | LiveOutcome::Stopped { .. } | LiveOutcome::Fatal(_)
            );
            outcomes.push(outcome);
            if stop {
                break;
            }
        }
        outcomes
    }

    /// Send pending messages for one channel with their stable
    /// `client_message_id`. A failure is retried by the next call.
    pub fn send_pending<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        channel_id: &str,
    ) -> Vec<SendOutcome> {
        let pending: Vec<(String, String)> = state
            .pending_sends()
            .filter(|(_, send)| {
                send.channel_id == channel_id
                    && matches!(send.state, SendState::Sending | SendState::Failed)
            })
            .map(|(id, send)| (id.to_owned(), send.body.clone()))
            .collect();
        let mut outcomes = Vec::with_capacity(pending.len());
        for (id, body) in pending {
            match self.chat.send(token, channel_id, &id, &body) {
                Ok(ack) if ack.channel_id == channel_id && ack.client_message_id == id => {
                    state.acknowledge(&id, ack.seq);
                    outcomes.push(SendOutcome::Acknowledged {
                        client_message_id: id,
                        seq: ack.seq,
                    });
                }
                Ok(_) => {
                    state.mark_send_failed(&id);
                    outcomes.push(SendOutcome::Failed {
                        client_message_id: id,
                        error: ChatTransportError::InvalidResponse,
                    });
                    break;
                }
                Err(error) => {
                    state.mark_send_failed(&id);
                    outcomes.push(SendOutcome::Failed {
                        client_message_id: id,
                        error,
                    });
                    break;
                }
            }
        }
        outcomes
    }

    fn on_ready<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        jitter_ms: u64,
    ) -> LiveOutcome {
        for channel in &self.channels {
            if self
                .live
                .send(&WsFrame::subscribe(channel.clone()))
                .is_err()
            {
                return self.interrupt(state, jitter_ms);
            }
        }
        LiveOutcome::Ready
    }

    fn on_subscribed<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        frame: &WsFrame,
        jitter_ms: u64,
    ) -> LiveOutcome {
        let channel_id = match string_field(frame, "channel_id") {
            Ok(channel_id) => channel_id,
            Err(error) => return LiveOutcome::Error(error),
        };
        let last_seq = match u64_field(frame, "last_seq") {
            Ok(last_seq) => last_seq,
            Err(error) => return LiveOutcome::Error(error),
        };
        if !self.channels.iter().any(|channel| channel == &channel_id) {
            self.channels.push(channel_id.clone());
        }
        match self.recover(state, token, &channel_id) {
            Ok(applied) => LiveOutcome::Subscribed {
                channel_id,
                last_seq,
                applied,
            },
            Err(error) => self.handle_live_error(state, error, jitter_ms),
        }
    }

    fn on_lagged<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        frame: &WsFrame,
        jitter_ms: u64,
    ) -> LiveOutcome {
        let channel_id = match string_field(frame, "channel_id") {
            Ok(channel_id) => channel_id,
            Err(error) => return LiveOutcome::Error(error),
        };
        match self.recover(state, token, &channel_id) {
            Ok(applied) => LiveOutcome::Applied {
                channel_id,
                applied,
            },
            Err(error) => self.handle_live_error(state, error, jitter_ms),
        }
    }

    fn on_ack<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        frame: &WsFrame,
    ) -> LiveOutcome {
        let client_message_id = match string_field(frame, "client_message_id") {
            Ok(id) => id,
            Err(error) => return LiveOutcome::Error(error),
        };
        let seq = match u64_field(frame, "seq") {
            Ok(seq) => seq,
            Err(error) => return LiveOutcome::Error(error),
        };
        state.acknowledge(&client_message_id, seq);
        LiveOutcome::Ack {
            client_message_id,
            seq,
        }
    }

    fn on_error(&mut self, frame: &WsFrame) -> LiveOutcome {
        let code = match string_field(frame, "code") {
            Ok(code) => code,
            Err(error) => return LiveOutcome::Error(error),
        };
        let message = match string_field(frame, "message") {
            Ok(message) => message,
            Err(error) => return LiveOutcome::Error(error),
        };
        if code == "unsupported_version" {
            self.live.close();
            self.connected = false;
            return LiveOutcome::Fatal(LiveError::Protocol { code, message });
        }
        LiveOutcome::Error(LiveError::Protocol { code, message })
    }

    fn on_event<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        frame: WsFrame,
        jitter_ms: u64,
    ) -> LiveOutcome {
        let event = match frame.as_chat_event() {
            Ok(Some(event)) => event,
            Ok(None) => return LiveOutcome::Ignored,
            Err(error) => return LiveOutcome::Error(version_error(error)),
        };
        let channel_id = event.channel_id.clone();
        let Some(caught) = state.observe_live(event) else {
            return LiveOutcome::Ignored;
        };
        if caught.missing_seq.is_some() {
            return match self.recover(state, token, &channel_id) {
                Ok(applied) => LiveOutcome::Applied {
                    channel_id,
                    applied,
                },
                Err(error) => self.handle_live_error(state, error, jitter_ms),
            };
        }
        LiveOutcome::Applied {
            channel_id,
            applied: caught.applied,
        }
    }

    fn recover<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        token: &str,
        channel_id: &str,
    ) -> Result<Vec<AppliedEvent>, LiveError> {
        let mut applied = Vec::new();
        loop {
            let cursor = state.cursor(channel_id);
            let page = self
                .chat
                .history(token, channel_id, cursor, HISTORY_PAGE_LIMIT)
                .map_err(LiveError::History)?;
            if page.channel_id != channel_id {
                return Err(LiveError::History(ChatTransportError::InvalidResponse));
            }
            validate_history(cursor, &page)
                .map_err(|_| LiveError::History(ChatTransportError::InvalidResponse))?;
            let caught = state.apply_history(channel_id, &page.messages);
            applied.extend(caught.applied);
            if caught.missing_seq.is_none() && !page.has_more {
                return Ok(applied);
            }
            if page.messages.is_empty() {
                return Err(match caught.missing_seq {
                    Some(missing_seq) => LiveError::Gap {
                        channel_id: channel_id.to_owned(),
                        missing_seq,
                    },
                    None => LiveError::History(ChatTransportError::InvalidResponse),
                });
            }
            if state.cursor(channel_id) <= cursor {
                return Err(LiveError::Gap {
                    channel_id: channel_id.to_owned(),
                    missing_seq: caught.missing_seq.unwrap_or(cursor.saturating_add(1)),
                });
            }
        }
    }

    fn handle_live_error<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        error: LiveError,
        jitter_ms: u64,
    ) -> LiveOutcome {
        match &error {
            LiveError::History(ChatTransportError::Network)
            | LiveError::History(ChatTransportError::Unauthorized)
            | LiveError::Gap { .. } => self.interrupt(state, jitter_ms),
            LiveError::History(ChatTransportError::Forbidden) => self.revoke(state),
            LiveError::History(ChatTransportError::Api { status, .. })
                if *status >= 500 || *status == 408 || *status == 429 =>
            {
                self.interrupt(state, jitter_ms)
            }
            _ => {
                self.live.close();
                self.connected = false;
                LiveOutcome::Fatal(error)
            }
        }
    }

    fn revoke<S: RefreshStore>(&mut self, state: &mut ClientState<S>) -> LiveOutcome {
        self.live.close();
        self.connected = false;
        match state.begin_catch_up(Interrupt::Blocked(AccessBlock::Revoked), 0, 0) {
            NextAction::Stop { reason } => LiveOutcome::Stopped { reason },
            _ => LiveOutcome::Stopped {
                reason: AccessBlock::Revoked,
            },
        }
    }

    fn interrupt<S: RefreshStore>(
        &mut self,
        state: &mut ClientState<S>,
        jitter_ms: u64,
    ) -> LiveOutcome {
        self.live.close();
        self.connected = false;
        match state.begin_catch_up(Interrupt::Transport, self.attempt, jitter_ms) {
            NextAction::RetryChat { delay } => {
                let attempt = self.attempt;
                self.attempt = self.attempt.saturating_add(1);
                LiveOutcome::Reconnect { delay, attempt }
            }
            NextAction::Stop { reason } => LiveOutcome::Stopped { reason },
            NextAction::RetryMedia { delay, .. } => LiveOutcome::Reconnect {
                delay,
                attempt: self.attempt,
            },
        }
    }
}

fn string_field(frame: &WsFrame, field: &'static str) -> Result<String, LiveError> {
    frame
        .fields
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or(LiveError::InvalidFrame { field })
}

fn u64_field(frame: &WsFrame, field: &'static str) -> Result<u64, LiveError> {
    frame
        .fields
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or(LiveError::InvalidFrame { field })
}

fn version_error(error: FrameError) -> LiveError {
    match error {
        FrameError::UnsupportedVersion { v } => LiveError::UnsupportedVersion { v },
        FrameError::MissingField { field } => LiveError::InvalidFrame { field },
    }
}
