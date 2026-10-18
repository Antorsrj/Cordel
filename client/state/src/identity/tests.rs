use std::cell::RefCell;
use std::collections::VecDeque;

use super::*;
use crate::credentials::MemoryRefreshStore;

type RecordedRequest = (HttpRequest, Option<String>, Option<Value>);

struct FakeTransport {
    reply: RefCell<Result<HttpReply, TransportFailure>>,
    requests: RefCell<Vec<RecordedRequest>>,
}

impl FakeTransport {
    fn new(reply: Result<HttpReply, TransportFailure>) -> Self {
        Self {
            reply: RefCell::new(reply),
            requests: RefCell::new(Vec::new()),
        }
    }
}

impl IdentityTransport for FakeTransport {
    fn exchange(
        &self,
        request: HttpRequest,
        bearer: Option<&str>,
        body: Option<&Value>,
    ) -> Result<HttpReply, TransportFailure> {
        self.requests
            .borrow_mut()
            .push((request, bearer.map(str::to_owned), body.cloned()));
        self.reply.borrow().clone()
    }
}

fn session_reply() -> HttpReply {
    HttpReply {
        status: 200,
        body: include_bytes!(
            "../../../../docs/contracts/fixtures/v1/identity_session_response.json"
        )
        .to_vec(),
    }
}

#[test]
fn reply_debug_never_displays_response_tokens() {
    let reply = session_reply();
    let output = format!("{reply:?}");
    assert!(!output.contains("unusable.fixture.access-token"));
    assert!(!output.contains("unusable-fixture-refresh-token"));
}

#[test]
fn management_grant_is_volatile_and_uses_separate_bearer() {
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    state.tokens_mut().set_access("ordinary-access");
    let grant = "a".repeat(64);
    let transport = FakeTransport::new(Ok(HttpReply {
        status: 200,
        body: format!(r#"{{"schema_version":1,"admin_token":"{grant}","expires_in_secs":600}}"#)
            .into_bytes(),
    }));
    let mut client = IdentityClient::new(state, transport);
    client.unlock_management("a-private-password").unwrap();
    assert_eq!(client.admin_token(), Some(grant.as_str()));
    assert!((1..=600).contains(&client.management_seconds_remaining()));
    assert_eq!(
        client.transport.requests.borrow()[0].0,
        HttpRequest::AdminUnlock
    );
    assert_eq!(
        client.transport.requests.borrow()[0].1.as_deref(),
        Some("ordinary-access")
    );
    assert_eq!(
        client.transport.requests.borrow()[0].2.as_ref().unwrap()["password"],
        "a-private-password"
    );
    let _ = client.transport.reply.replace(Ok(HttpReply {
        status: 200,
        body: br#"{"schema_version":1}"#.to_vec(),
    }));
    client.lock_management().unwrap();
    assert!(client.admin_token().is_none());
    assert_eq!(client.management_seconds_remaining(), 0);
    assert_eq!(
        client.transport.requests.borrow()[1].0,
        HttpRequest::AdminLock
    );
    assert_eq!(
        client.transport.requests.borrow()[1].1.as_deref(),
        Some(grant.as_str())
    );
    let restored = IdentityClient::new(
        ClientState::restore(MemoryRefreshStore::default(), None),
        FakeTransport::new(Err(TransportFailure::Network)),
    );
    assert!(restored.admin_token().is_none());
}

#[test]
fn management_lock_clears_token_when_network_fails() {
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    state.tokens_mut().set_access("ordinary-access");
    let grant = "b".repeat(64);
    let transport = FakeTransport::new(Ok(HttpReply {
        status: 200,
        body: format!(r#"{{"schema_version":1,"admin_token":"{grant}","expires_in_secs":600}}"#)
            .into_bytes(),
    }));
    let mut client = IdentityClient::new(state, transport);
    client.unlock_management("password").unwrap();
    let _ = client
        .transport
        .reply
        .replace(Err(TransportFailure::Network));
    assert_eq!(
        client.lock_management(),
        Err(IdentityError::Transport(TransportFailure::Network))
    );
    assert!(client.admin_token().is_none());
}

struct SequenceTransport {
    replies: RefCell<VecDeque<HttpReply>>,
    requests: RefCell<Vec<HttpRequest>>,
}

impl IdentityTransport for SequenceTransport {
    fn exchange(
        &self,
        request: HttpRequest,
        _bearer: Option<&str>,
        _body: Option<&Value>,
    ) -> Result<HttpReply, TransportFailure> {
        self.requests.borrow_mut().push(request);
        self.replies
            .borrow_mut()
            .pop_front()
            .ok_or(TransportFailure::Network)
    }
}

#[test]
fn login_stores_only_refresh_and_me_uses_bearer() {
    let state = ClientState::restore(MemoryRefreshStore::default(), None);
    let transport = FakeTransport::new(Ok(session_reply()));
    let mut client = IdentityClient::new(state, transport);
    let session = client.login("TestUser", "sample-password").unwrap();
    assert_eq!(
        client.state().refresh().unwrap().expose(),
        session.refresh_token
    );
    assert_eq!(
        client.state().tokens().access(),
        Some(session.access_token.as_str())
    );
    assert_eq!(client.transport.requests.borrow()[0].0, HttpRequest::Login);
    assert_eq!(
        client.transport.requests.borrow()[0].2.as_ref().unwrap()["login"],
        "TestUser"
    );
    assert!(!format!("{session:?}").contains("sample-password"));
    let _ = client.transport.reply.replace(Ok(HttpReply {
        status: 200,
        body: include_bytes!("../../../../docs/contracts/fixtures/v1/identity_me_response.json")
            .to_vec(),
    }));
    assert_eq!(client.me().unwrap().role, "user");
    assert_eq!(
        client.transport.requests.borrow()[1].1.as_deref(),
        Some(session.access_token.as_str())
    );
}

#[test]
fn refresh_clears_old_before_request_and_never_replays_after_uncertainty() {
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    state
        .save_refresh(RefreshCredential::new("old-token"))
        .unwrap();
    state.tokens_mut().set_access("old-access");
    let transport = FakeTransport::new(Err(TransportFailure::Network));
    let mut client = IdentityClient::new(state, transport);
    assert_eq!(
        client.refresh(),
        Err(IdentityError::Transport(TransportFailure::Network))
    );
    assert!(client.state().refresh().is_none());
    assert!(client.state().tokens().access().is_none());
    assert_eq!(
        client.transport.requests.borrow()[0].2.as_ref().unwrap()["refresh_token"],
        "old-token"
    );
    assert_eq!(client.refresh(), Err(IdentityError::MissingCredential));
    assert_eq!(client.transport.requests.borrow().len(), 1);
}

#[test]
fn successful_refresh_replaces_the_stored_credential() {
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    state
        .save_refresh(RefreshCredential::new("old-token"))
        .unwrap();
    let transport = FakeTransport::new(Ok(session_reply()));
    let mut client = IdentityClient::new(state, transport);
    let result = client.refresh().unwrap();
    assert_eq!(
        client.state().refresh().unwrap().expose(),
        result.refresh_token
    );
    assert_eq!(
        client.transport.requests.borrow()[0].0,
        HttpRequest::Refresh
    );
}

#[test]
fn expired_access_on_me_rotates_once_then_retries_with_fresh_access() {
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    state
        .save_refresh(RefreshCredential::new("old-token"))
        .unwrap();
    state.tokens_mut().set_access("expired-access");
    let unauthorized = HttpReply {
        status: 401,
        body: include_bytes!("../../../../docs/contracts/fixtures/v1/identity_error.json").to_vec(),
    };
    let transport = SequenceTransport {
        replies: RefCell::new(VecDeque::from([
            unauthorized,
            session_reply(),
            HttpReply {
                status: 200,
                body: include_bytes!(
                    "../../../../docs/contracts/fixtures/v1/identity_me_response.json"
                )
                .to_vec(),
            },
        ])),
        requests: RefCell::new(Vec::new()),
    };
    let mut client = IdentityClient::new(state, transport);
    assert_eq!(client.me().unwrap().role, "user");
    assert_eq!(
        client.transport.requests.borrow().as_slice(),
        &[HttpRequest::Me, HttpRequest::Refresh, HttpRequest::Me]
    );
    assert!(client.state().refresh().is_some());
}

#[test]
fn logout_clears_local_session_even_if_response_is_lost() {
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    state
        .save_refresh(RefreshCredential::new("old-token"))
        .unwrap();
    state.tokens_mut().set_access("access");
    let mut client = IdentityClient::new(state, FakeTransport::new(Err(TransportFailure::Network)));
    assert_eq!(
        client.logout(),
        Err(IdentityError::Transport(TransportFailure::Network))
    );
    assert!(client.state().refresh().is_none());
    assert!(client.state().tokens().access().is_none());
    assert_eq!(
        client.transport.requests.borrow()[0].1.as_deref(),
        Some("access")
    );
}

#[test]
fn logout_after_restart_refreshes_before_remote_revocation() {
    let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
    state
        .save_refresh(RefreshCredential::new("old-token"))
        .unwrap();
    let transport = SequenceTransport {
        replies: RefCell::new(VecDeque::from([
            session_reply(),
            HttpReply {
                status: 200,
                body: br#"{"schema_version":1}"#.to_vec(),
            },
        ])),
        requests: RefCell::new(Vec::new()),
    };
    let mut client = IdentityClient::new(state, transport);
    assert_eq!(client.logout(), Ok(()));
    assert!(client.state().refresh().is_none());
    assert_eq!(
        client.transport.requests.borrow().as_slice(),
        &[HttpRequest::Refresh, HttpRequest::Logout]
    );
}

#[test]
fn rejects_non_https_and_redirect_friendly_endpoints() {
    assert!(matches!(
        NativeHttpsTransport::new("http://example.com/"),
        Err(IdentityError::InvalidEndpoint)
    ));
    assert!(matches!(
        NativeHttpsTransport::new("https://example.com/api"),
        Err(IdentityError::InvalidEndpoint)
    ));
    assert!(matches!(
        NativeHttpsTransport::new("https://user:pass@example.com/"),
        Err(IdentityError::InvalidEndpoint)
    ));
}
