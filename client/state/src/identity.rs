//! Native ADR-002 identity transport and ADR-102 credential rotation.
//!
//! Blocking HTTPS calls belong on a worker thread, never the Slint UI thread.

use std::io::Read;
use std::time::Duration;

use cordel_protocol::{
    ApiError, EmptyResponse, IdentitySession, LoginRequest, MeResponse, RefreshRequest,
};
use serde_json::Value;

use crate::credentials::{RefreshCredential, RefreshStore, StoreError};
use crate::session::ClientState;

const RESPONSE_LIMIT: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRequest {
    Login,
    Refresh,
    Me,
    Logout,
}

impl HttpRequest {
    fn path(self) -> &'static str {
        match self {
            Self::Login => "v1/sessions",
            Self::Refresh => "v1/sessions/refresh",
            Self::Me => "v1/me",
            Self::Logout => "v1/sessions/current",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for HttpReply {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpReply")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// Transport errors contain no URL, header, credential, or response body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFailure {
    Network,
    ResponseTooLarge,
}

pub trait IdentityTransport {
    fn exchange(
        &self,
        request: HttpRequest,
        bearer: Option<&str>,
        body: Option<&Value>,
    ) -> Result<HttpReply, TransportFailure>;
}

/// HTTPS-only native transport. Redirects are disabled so credentials never
/// follow a response to another origin. Normal certificate validation remains on.
pub struct NativeHttpsTransport {
    client: reqwest::blocking::Client,
    base: reqwest::Url,
}

impl NativeHttpsTransport {
    pub fn new(base: &str) -> Result<Self, IdentityError> {
        let base = reqwest::Url::parse(base).map_err(|_| IdentityError::InvalidEndpoint)?;
        if base.scheme() != "https"
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(IdentityError::InvalidEndpoint);
        }
        let client = reqwest::blocking::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| IdentityError::InvalidEndpoint)?;
        Ok(Self { client, base })
    }
}

impl IdentityTransport for NativeHttpsTransport {
    fn exchange(
        &self,
        request: HttpRequest,
        bearer: Option<&str>,
        body: Option<&Value>,
    ) -> Result<HttpReply, TransportFailure> {
        let url = self
            .base
            .join(request.path())
            .map_err(|_| TransportFailure::Network)?;
        let builder = match request {
            HttpRequest::Login | HttpRequest::Refresh => self.client.post(url),
            HttpRequest::Me => self.client.get(url),
            HttpRequest::Logout => self.client.delete(url),
        };
        let builder = if let Some(token) = bearer {
            builder.bearer_auth(token)
        } else {
            builder
        };
        let builder = if let Some(body) = body {
            builder.json(body)
        } else {
            builder
        };
        let response = builder.send().map_err(|_| TransportFailure::Network)?;
        let status = response.status().as_u16();
        let mut bounded = Vec::new();
        response
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut bounded)
            .map_err(|_| TransportFailure::Network)?;
        if bounded.len() as u64 > RESPONSE_LIMIT {
            return Err(TransportFailure::ResponseTooLarge);
        }
        Ok(HttpReply {
            status,
            body: bounded,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    InvalidEndpoint,
    MissingCredential,
    Store(StoreError),
    Transport(TransportFailure),
    InvalidResponse,
    Api { status: u16, code: String },
}

pub struct IdentityClient<S, T> {
    state: ClientState<S>,
    transport: T,
}

impl<S: RefreshStore, T: IdentityTransport> IdentityClient<S, T> {
    pub fn new(state: ClientState<S>, transport: T) -> Self {
        Self { state, transport }
    }

    pub fn state(&self) -> &ClientState<S> {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut ClientState<S> {
        &mut self.state
    }

    pub fn login(&mut self, login: &str, password: &str) -> Result<IdentitySession, IdentityError> {
        self.state.clear_session().map_err(IdentityError::Store)?;
        let body = serde_json::to_value(LoginRequest {
            login: login.to_owned(),
            password: password.to_owned(),
        })
        .map_err(|_| IdentityError::InvalidResponse)?;
        let response = self
            .transport
            .exchange(HttpRequest::Login, None, Some(&body))
            .map_err(IdentityError::Transport)?;
        let session: IdentitySession = decode(response)?;
        self.accept(session)
    }

    /// The old credential is removed from durable storage *before* transmission.
    /// Any uncertain result therefore requires login; automatic replay is unsafe.
    pub fn refresh(&mut self) -> Result<IdentitySession, IdentityError> {
        let old = self
            .state
            .refresh()
            .ok_or(IdentityError::MissingCredential)?;
        self.state.clear_session().map_err(IdentityError::Store)?;
        let body = serde_json::to_value(RefreshRequest {
            refresh_token: old.expose().to_owned(),
        })
        .map_err(|_| IdentityError::InvalidResponse)?;
        let response = self
            .transport
            .exchange(HttpRequest::Refresh, None, Some(&body))
            .map_err(IdentityError::Transport)?;
        let session: IdentitySession = decode(response)?;
        self.accept(session)
    }

    pub fn me(&mut self) -> Result<MeResponse, IdentityError> {
        let token = match self.state.tokens().access() {
            Some(token) => token.to_owned(),
            None => self.refresh()?.access_token,
        };
        let mut response = self
            .transport
            .exchange(HttpRequest::Me, Some(&token), None)
            .map_err(IdentityError::Transport)?;
        if response.status == 401 && self.state.refresh().is_some() {
            let session = self.refresh()?;
            response = self
                .transport
                .exchange(HttpRequest::Me, Some(&session.access_token), None)
                .map_err(IdentityError::Transport)?;
        }
        let denied = matches!(response.status, 401 | 403);
        let parsed: Result<MeResponse, IdentityError> = decode(response);
        if denied {
            self.state.clear_session().map_err(IdentityError::Store)?;
        }
        let me = parsed?;
        if me.schema_version != 1 {
            return Err(IdentityError::InvalidResponse);
        }
        Ok(me)
    }

    /// Clear the local session even when the server response is lost. Callers
    /// must report that remote revocation is uncertain on transport failure.
    pub fn logout(&mut self) -> Result<(), IdentityError> {
        if self.state.tokens().access().is_none() && self.state.refresh().is_some() {
            self.refresh()?;
        }
        let token = self.state.tokens().access().map(str::to_owned);
        self.state.clear_session().map_err(IdentityError::Store)?;
        let token = token.ok_or(IdentityError::MissingCredential)?;
        let response = self
            .transport
            .exchange(HttpRequest::Logout, Some(&token), None)
            .map_err(IdentityError::Transport)?;
        let empty: EmptyResponse = decode(response)?;
        if empty.schema_version != 1 {
            return Err(IdentityError::InvalidResponse);
        }
        Ok(())
    }

    fn accept(&mut self, session: IdentitySession) -> Result<IdentitySession, IdentityError> {
        if session.schema_version != 1
            || session.user_id.is_empty()
            || session.community_id.is_empty()
            || session.access_token.is_empty()
            || session.refresh_token.is_empty()
            || session.expires_in_secs == 0
        {
            return Err(IdentityError::InvalidResponse);
        }
        if let Err(error) = self
            .state
            .save_refresh(RefreshCredential::new(&session.refresh_token))
        {
            let _ = self.state.clear_session();
            return Err(IdentityError::Store(error));
        }
        self.state.tokens_mut().set_access(&session.access_token);
        Ok(session)
    }
}

fn decode<T: serde::de::DeserializeOwned>(response: HttpReply) -> Result<T, IdentityError> {
    if (200..300).contains(&response.status) {
        return serde_json::from_slice(&response.body).map_err(|_| IdentityError::InvalidResponse);
    }
    let envelope: ApiError =
        serde_json::from_slice(&response.body).map_err(|_| IdentityError::InvalidResponse)?;
    if envelope.schema_version != 1 {
        return Err(IdentityError::InvalidResponse);
    }
    Err(IdentityError::Api {
        status: response.status,
        code: envelope.error.code.as_str().to_owned(),
    })
}

#[cfg(test)]
mod tests {
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
                "../../../docs/contracts/fixtures/v1/identity_session_response.json"
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
            body: include_bytes!("../../../docs/contracts/fixtures/v1/identity_me_response.json")
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
            body: include_bytes!("../../../docs/contracts/fixtures/v1/identity_error.json")
                .to_vec(),
        };
        let transport = SequenceTransport {
            replies: RefCell::new(VecDeque::from([
                unauthorized,
                session_reply(),
                HttpReply {
                    status: 200,
                    body: include_bytes!(
                        "../../../docs/contracts/fixtures/v1/identity_me_response.json"
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
        let mut client =
            IdentityClient::new(state, FakeTransport::new(Err(TransportFailure::Network)));
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
}
