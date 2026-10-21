use std::cell::RefCell;
use std::collections::VecDeque;

use cordel_protocol::{ChatEvent, HistoryPage, MessageAck, WsFrame};

use super::*;
use crate::catchup::AppliedEvent;
use crate::credentials::MemoryRefreshStore;
use crate::retry::{AccessBlock, FIRST_RETRY};
use crate::session::{visible_bodies, ClientState, CursorCache};

const CHANNEL: &str = "7efc7a7b-b06f-4632-8780-bb611b8cd0a8";
const OTHER_CHANNEL: &str = "0700eaa6-8204-4f0b-a45e-57d929289818";
const FIXTURE_MESSAGE: &str =
    include_str!("../../../../docs/contracts/fixtures/native/ws_message.json");

struct FakeLive {
    incoming: VecDeque<Result<WsFrame, LiveTransportError>>,
    sent: Vec<WsFrame>,
    connects: Vec<(String, String)>,
    connected: bool,
    fail_connect: bool,
}

impl FakeLive {
    fn new(frames: impl IntoIterator<Item = WsFrame>) -> Self {
        Self {
            incoming: frames.into_iter().map(Ok).collect(),
            sent: Vec::new(),
            connects: Vec::new(),
            connected: false,
            fail_connect: false,
        }
    }

    fn push(&mut self, frame: WsFrame) {
        self.incoming.push_back(Ok(frame));
    }

    fn fail_next(&mut self, error: LiveTransportError) {
        self.incoming.push_back(Err(error));
    }
}

impl LiveTransport for FakeLive {
    fn connect(&mut self, endpoint: &str, token: &str) -> Result<(), LiveTransportError> {
        if self.fail_connect {
            return Err(LiveTransportError::Network);
        }
        self.connected = true;
        self.connects.push((endpoint.to_owned(), token.to_owned()));
        Ok(())
    }

    fn send(&mut self, frame: &WsFrame) -> Result<(), LiveTransportError> {
        if !self.connected {
            return Err(LiveTransportError::Closed);
        }
        self.sent.push(frame.clone());
        Ok(())
    }

    fn receive(&mut self) -> Result<WsFrame, LiveTransportError> {
        self.incoming
            .pop_front()
            .unwrap_or(Err(LiveTransportError::Closed))
    }

    fn close(&mut self) {
        self.connected = false;
    }
}

struct FakeChat {
    messages: Vec<ChatEvent>,
    failure: RefCell<Option<ChatTransportError>>,
    sends: RefCell<Vec<(String, String, String)>>,
    history_requests: RefCell<Vec<u64>>,
    page_limit: usize,
    next_seq: u64,
}

impl FakeChat {
    fn new(messages: Vec<ChatEvent>) -> Self {
        Self {
            messages,
            failure: RefCell::new(None),
            sends: RefCell::new(Vec::new()),
            history_requests: RefCell::new(Vec::new()),
            page_limit: HISTORY_PAGE_LIMIT as usize,
            next_seq: 7,
        }
    }

    fn fail(&self, error: ChatTransportError) {
        let _ = self.failure.replace(Some(error));
    }

    fn clear_failure(&self) {
        let _ = self.failure.replace(None);
    }
}

impl ChatTransport for FakeChat {
    fn history(
        &self,
        _bearer: &str,
        channel_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<HistoryPage, ChatTransportError> {
        if let Some(error) = self.failure.borrow().clone() {
            return Err(error);
        }
        self.history_requests.borrow_mut().push(after_seq);
        let matching: Vec<ChatEvent> = self
            .messages
            .iter()
            .filter(|message| message.channel_id == channel_id && message.seq > after_seq)
            .cloned()
            .collect();
        let take = (limit as usize).min(self.page_limit);
        let messages: Vec<ChatEvent> = matching.iter().take(take).cloned().collect();
        Ok(HistoryPage {
            channel_id: channel_id.to_owned(),
            has_more: matching.len() > messages.len(),
            messages,
        })
    }

    fn send(
        &self,
        _bearer: &str,
        channel_id: &str,
        client_message_id: &str,
        body: &str,
    ) -> Result<MessageAck, ChatTransportError> {
        self.sends.borrow_mut().push((
            channel_id.to_owned(),
            client_message_id.to_owned(),
            body.to_owned(),
        ));
        if let Some(error) = self.failure.borrow().clone() {
            return Err(error);
        }
        Ok(MessageAck {
            channel_id: channel_id.to_owned(),
            client_message_id: client_message_id.to_owned(),
            seq: self.next_seq,
        })
    }
}

fn state() -> ClientState<MemoryRefreshStore> {
    ClientState::restore(MemoryRefreshStore::default(), None)
}

fn state_at(seq: u64) -> ClientState<MemoryRefreshStore> {
    let mut cache = CursorCache::default();
    cache.set_cursor(CHANNEL, seq);
    ClientState::restore(MemoryRefreshStore::default(), Some(cache))
}

fn session(live: FakeLive, chat: FakeChat) -> LiveSession<FakeLive, FakeChat> {
    LiveSession::new(
        live,
        chat,
        "wss://example.test/v1/events",
        vec![CHANNEL.to_owned()],
    )
}

fn message(seq: u64, body: &str) -> ChatEvent {
    ChatEvent {
        channel_id: CHANNEL.to_owned(),
        seq,
        kind: "message".to_owned(),
        client_message_id: Some(format!("client-{seq}")),
        body: Some(body.to_owned()),
        extra: Default::default(),
    }
}

fn fixture_frame(raw: &str) -> WsFrame {
    serde_json::from_str(raw).unwrap()
}

fn ready() -> WsFrame {
    fixture_frame(include_str!(
        "../../../../docs/contracts/fixtures/native/ws_ready.json"
    ))
}

fn subscribed(last_seq: u64) -> WsFrame {
    WsFrame::subscribed(CHANNEL, last_seq)
}

fn expect_applied(outcome: LiveOutcome) -> Vec<AppliedEvent> {
    match outcome {
        LiveOutcome::Applied { applied, .. } => applied,
        other => panic!("expected Applied, got {other:?}"),
    }
}

#[test]
fn start_subscribes_and_subscribed_history_applies_once() {
    let mut live = FakeLive::new([ready()]);
    live.push(fixture_frame(include_str!(
        "../../../../docs/contracts/fixtures/native/ws_subscribed.json"
    )));
    let chat = FakeChat::new(vec![message(41, "forty-one"), message(42, "forty-two")]);
    let mut session = session(live, chat);
    let mut state = state_at(40);

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(session.live.sent[0].kind, "subscribe");

    let outcome = session.pump(&mut state, "token", 0);
    match outcome {
        LiveOutcome::Subscribed {
            channel_id,
            last_seq,
            applied,
        } => {
            assert_eq!(channel_id, CHANNEL);
            assert_eq!(last_seq, 42);
            assert_eq!(visible_bodies(&applied), ["forty-one", "forty-two"]);
        }
        other => panic!("expected Subscribed, got {other:?}"),
    }
    assert_eq!(state.cursor(CHANNEL), 42);
}

#[test]
fn out_of_order_live_event_recovers_missing_history() {
    let mut live = FakeLive::new([ready()]);
    live.push(WsFrame::message(&message(42, "live-two")).unwrap());
    let chat = FakeChat::new(vec![message(41, "one"), message(42, "two")]);
    let mut session = session(live, chat);
    let mut state = state_at(40);

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    let applied = expect_applied(session.pump(&mut state, "token", 0));
    assert_eq!(visible_bodies(&applied), ["one", "two"]);
    assert_eq!(state.cursor(CHANNEL), 42);
}

#[test]
fn lagged_frame_recovers_after_the_client_cursor() {
    let mut live = FakeLive::new([ready()]);
    live.push(fixture_frame(include_str!(
        "../../../../docs/contracts/fixtures/native/ws_lagged.json"
    )));
    let chat = FakeChat::new(vec![message(43, "three"), message(44, "four")]);
    let mut session = session(live, chat);
    let mut state = state_at(42);

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    let applied = expect_applied(session.pump(&mut state, "token", 0));
    assert_eq!(visible_bodies(&applied), ["three", "four"]);
    assert_eq!(state.cursor(CHANNEL), 44);
}

#[test]
fn history_pagination_fetches_every_page() {
    let mut live = FakeLive::new([ready()]);
    live.push(subscribed(3));
    let mut chat = FakeChat::new(vec![
        message(1, "one"),
        message(2, "two"),
        message(3, "three"),
    ]);
    chat.page_limit = 1;
    let mut session = session(live, chat);
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    let outcome = session.pump(&mut state, "token", 0);
    match outcome {
        LiveOutcome::Subscribed { applied, .. } => {
            assert_eq!(visible_bodies(&applied), ["one", "two", "three"]);
        }
        other => panic!("expected Subscribed, got {other:?}"),
    }
    assert_eq!(state.cursor(CHANNEL), 3);
    assert_eq!(session.chat.history_requests.borrow().as_slice(), [0, 1, 2]);
}

#[test]
fn revocation_stops_and_clears_credentials() {
    let mut live = FakeLive::new([ready()]);
    live.push(fixture_frame(include_str!(
        "../../../../docs/contracts/fixtures/native/ws_access_revoked.json"
    )));
    let chat = FakeChat::new(Vec::new());
    let mut session = session(live, chat);
    let mut state = state();
    state
        .save_refresh(crate::credentials::RefreshCredential::new("refresh"))
        .unwrap();
    state.tokens_mut().set_access("access");

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Stopped {
            reason: AccessBlock::Revoked
        }
    );
    assert_eq!(state.blocked(), Some(AccessBlock::Revoked));
    assert_eq!(state.refresh(), None);
    assert_eq!(state.tokens().access(), None);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Stopped {
            reason: AccessBlock::Revoked
        }
    );
}

#[test]
fn transport_failure_backs_off_then_resume_resets_attempt() {
    let mut live = FakeLive::new([ready()]);
    live.fail_next(LiveTransportError::Network);
    let chat = FakeChat::new(Vec::new());
    let mut session = session(live, chat);
    let mut state = state();
    state.tokens_mut().set_access("access");

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Reconnect {
            delay: FIRST_RETRY,
            attempt: 0
        }
    );
    assert_eq!(state.tokens().access(), None);

    session.live.push(ready());
    assert_eq!(
        session.start(&mut state, "fresh-token", 0),
        LiveOutcome::Ready
    );
    session.live.fail_next(LiveTransportError::Network);
    assert_eq!(
        session.pump(&mut state, "fresh-token", 0),
        LiveOutcome::Reconnect {
            delay: FIRST_RETRY,
            attempt: 0
        }
    );
}

#[test]
fn unsupported_frame_version_is_fatal() {
    let mut unsupported = ready();
    unsupported.v = 99;
    let mut live = FakeLive::new([ready()]);
    live.push(unsupported);
    let mut session = session(live, FakeChat::new(Vec::new()));
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Fatal(LiveError::UnsupportedVersion { v: 99 })
    );
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Error(LiveError::NotConnected)
    );
}

#[test]
fn unauthorized_history_requests_reauthorization() {
    let live = FakeLive::new([ready(), subscribed(0)]);
    let chat = FakeChat::new(Vec::new());
    chat.fail(ChatTransportError::Unauthorized);
    let mut session = session(live, chat);
    let mut state = state();
    state.tokens_mut().set_access("access");

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Reconnect {
            delay: FIRST_RETRY,
            attempt: 0
        }
    );
    assert_eq!(state.tokens().access(), None);
}

#[test]
fn forbidden_history_stops_the_session() {
    let live = FakeLive::new([ready(), subscribed(0)]);
    let chat = FakeChat::new(Vec::new());
    chat.fail(ChatTransportError::Forbidden);
    let mut session = session(live, chat);
    let mut state = state();
    state.tokens_mut().set_access("access");

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Stopped {
            reason: AccessBlock::Revoked
        }
    );
    assert_eq!(state.blocked(), Some(AccessBlock::Revoked));
}

#[test]
fn server_error_history_reconnects() {
    let live = FakeLive::new([ready(), subscribed(0)]);
    let chat = FakeChat::new(Vec::new());
    chat.fail(ChatTransportError::Api {
        status: 503,
        code: "temporarily_unavailable".to_owned(),
    });
    let mut session = session(live, chat);
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert!(matches!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Reconnect { attempt: 0, .. }
    ));
}

#[test]
fn invalid_history_is_fatal_instead_of_leaving_the_client_buffering() {
    let live = FakeLive::new([ready(), subscribed(0)]);
    let chat = FakeChat::new(Vec::new());
    chat.fail(ChatTransportError::InvalidResponse);
    let mut session = session(live, chat);
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Fatal(LiveError::History(ChatTransportError::InvalidResponse))
    );
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Error(LiveError::NotConnected)
    );
}

#[test]
fn reconcile_applies_a_commit_without_a_live_publish() {
    let live = FakeLive::new([ready(), subscribed(0)]);
    let chat = FakeChat::new(Vec::new());
    let mut session = session(live, chat);
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert!(matches!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Subscribed { .. }
    ));
    session
        .chat
        .messages
        .push(message(1, "committed without publish"));

    let outcomes = session.reconcile(&mut state, "token", 0);
    assert_eq!(outcomes.len(), 1);
    let applied = expect_applied(outcomes.into_iter().next().unwrap());
    assert_eq!(visible_bodies(&applied), ["committed without publish"]);
    assert_eq!(state.cursor(CHANNEL), 1);
}

#[test]
fn duplicate_live_and_history_apply_once() {
    let live = FakeLive::new([ready(), subscribed(1)]);
    let chat = FakeChat::new(vec![message(1, "once")]);
    let mut session = session(live, chat);
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    let applied = match session.pump(&mut state, "token", 0) {
        LiveOutcome::Subscribed { applied, .. } => applied,
        other => panic!("expected Subscribed, got {other:?}"),
    };
    assert_eq!(visible_bodies(&applied), ["once"]);

    let outcomes = session.reconcile(&mut state, "token", 0);
    assert_eq!(outcomes.len(), 1);
    assert!(expect_applied(outcomes.into_iter().next().unwrap()).is_empty());
    assert_eq!(state.cursor(CHANNEL), 1);
}

#[test]
fn send_pending_uses_a_stable_id_and_retries() {
    let mut state = state();
    let id = state.enqueue(CHANNEL, "hello");
    let chat = FakeChat::new(Vec::new());
    let mut session = session(FakeLive::new(Vec::new()), chat);

    let outcomes = session.send_pending(&mut state, "token", CHANNEL);
    assert_eq!(
        outcomes,
        vec![SendOutcome::Acknowledged {
            client_message_id: id.as_str().to_owned(),
            seq: 7
        }]
    );
    assert!(session
        .send_pending(&mut state, "token", CHANNEL)
        .is_empty());

    let retry_id = state.enqueue(CHANNEL, "retry");
    session.chat.fail(ChatTransportError::Network);
    let outcomes = session.send_pending(&mut state, "token", CHANNEL);
    assert!(matches!(outcomes.as_slice(), [SendOutcome::Failed { .. }]));
    session.chat.clear_failure();
    let outcomes = session.send_pending(&mut state, "token", CHANNEL);
    assert!(matches!(
        outcomes.as_slice(),
        [SendOutcome::Acknowledged { .. }]
    ));
    let sends = session.chat.sends.borrow();
    assert_eq!(sends.len(), 3);
    assert_eq!(sends[1].1, retry_id.as_str());
    assert_eq!(sends[2].1, retry_id.as_str());
}

#[test]
fn native_fixture_drives_a_live_message() {
    let mut live = FakeLive::new([ready()]);
    live.push(fixture_frame(FIXTURE_MESSAGE));
    let mut session = session(live, FakeChat::new(Vec::new()));
    let mut state = state_at(41);

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    let applied = expect_applied(session.pump(&mut state, "token", 0));
    assert_eq!(visible_bodies(&applied), ["Hello"]);
    assert_eq!(state.cursor(CHANNEL), 42);
    assert_eq!(applied[0].seq, 42);
}

#[test]
fn add_and_remove_channel_sends_subscriptions() {
    let live = FakeLive::new([ready()]);
    let mut session = session(live, FakeChat::new(Vec::new()));
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    session.add_channel(OTHER_CHANNEL).unwrap();
    assert_eq!(session.live.sent.last().unwrap().kind, "subscribe");
    session.remove_channel(OTHER_CHANNEL).unwrap();
    assert_eq!(session.live.sent.last().unwrap().kind, "unsubscribe");
    assert_eq!(session.channels(), [CHANNEL.to_owned()]);
}

#[test]
fn a_closed_fake_socket_is_a_transport_interruption() {
    let live = FakeLive::new([ready()]);
    let mut session = session(live, FakeChat::new(Vec::new()));
    let mut state = state();

    assert_eq!(session.start(&mut state, "token", 0), LiveOutcome::Ready);
    assert_eq!(
        session.pump(&mut state, "token", 0),
        LiveOutcome::Reconnect {
            delay: FIRST_RETRY,
            attempt: 0
        }
    );
}
