//! Native ADR-002 identity transport and ADR-102 credential rotation.
//!
//! Blocking HTTPS calls belong on a worker thread, never the Slint UI thread.

use std::io::Read;
use std::time::{Duration, Instant};

use cordel_protocol::{
    AdminGrantRequest, AdminGrantResponse, ApiError, EmptyResponse, IdentitySession, LoginRequest,
    MeResponse, RefreshRequest,
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
    AdminUnlock,
    AdminLock,
}

impl HttpRequest {
    fn path(self) -> &'static str {
        match self {
            Self::Login => "v1/sessions",
            Self::Refresh => "v1/sessions/refresh",
            Self::Me => "v1/me",
            Self::Logout => "v1/sessions/current",
            Self::AdminUnlock | Self::AdminLock => "v1/sessions/admin-grant",
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
        Self::with_scheme(base, "https")
    }

    /// Plain `http` origin for a local Phase 0 run. Enable the `local-smoke`
    /// feature; production builds use `new`.
    #[cfg(any(test, feature = "local-smoke"))]
    pub fn new_plain(base: &str) -> Result<Self, IdentityError> {
        Self::with_scheme(base, "http")
    }

    /// A POST to an absolute path with a bearer token and a raw body.
    ///
    /// The identity routes have a fixed set of shapes, but the media
    /// admission route carries a channel id in its path.
    pub fn post_path(
        &self,
        path: &str,
        bearer: &str,
        body: &[u8],
    ) -> Result<HttpReply, TransportFailure> {
        let url = self
            .base
            .join(path)
            .map_err(|_| TransportFailure::Network)?;
        let response = self
            .client
            .post(url)
            .bearer_auth(bearer)
            .body(body.to_vec())
            .send()
            .map_err(|_| TransportFailure::Network)?;
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

    fn with_scheme(base: &str, scheme: &str) -> Result<Self, IdentityError> {
        let base = reqwest::Url::parse(base).map_err(|_| IdentityError::InvalidEndpoint)?;
        if base.scheme() != scheme
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(IdentityError::InvalidEndpoint);
        }
        let mut builder = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10));
        if scheme == "https" {
            builder = builder.https_only(true);
        }
        let client = builder
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
            HttpRequest::Login | HttpRequest::Refresh | HttpRequest::AdminUnlock => {
                self.client.post(url)
            }
            HttpRequest::Me => self.client.get(url),
            HttpRequest::Logout | HttpRequest::AdminLock => self.client.delete(url),
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
    admin_grant: Option<(String, Instant)>,
}

impl<S: RefreshStore, T: IdentityTransport> IdentityClient<S, T> {
    pub fn new(state: ClientState<S>, transport: T) -> Self {
        Self {
            state,
            transport,
            admin_grant: None,
        }
    }

    pub fn state(&self) -> &ClientState<S> {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut ClientState<S> {
        self.admin_grant = None;
        &mut self.state
    }

    /// Returns a volatile token only while the local grant remains live.
    /// Callers must send it only to a management endpoint on the same origin.
    pub fn admin_token(&self) -> Option<&str> {
        self.admin_grant
            .as_ref()
            .and_then(|(token, expires)| (Instant::now() < *expires).then_some(token.as_str()))
    }

    /// Seconds left on the local grant, rounded up for a user-facing timer.
    pub fn management_seconds_remaining(&self) -> u64 {
        self.admin_grant
            .as_ref()
            .map(|(_, expires)| {
                let remaining = expires.saturating_duration_since(Instant::now());
                remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
            })
            .unwrap_or(0)
    }

    pub fn unlock_management(&mut self, password: &str) -> Result<(), IdentityError> {
        self.admin_grant = None;
        let access = self
            .state
            .tokens()
            .access()
            .ok_or(IdentityError::MissingCredential)?;
        let body = serde_json::to_value(AdminGrantRequest {
            password: password.to_owned(),
        })
        .map_err(|_| IdentityError::InvalidResponse)?;
        let response = self
            .transport
            .exchange(HttpRequest::AdminUnlock, Some(access), Some(&body))
            .map_err(IdentityError::Transport)?;
        let grant: AdminGrantResponse = decode(response)?;
        if grant.schema_version != 1
            || grant.expires_in_secs == 0
            || grant.expires_in_secs > 600
            || grant.admin_token.len() != 64
            || !grant
                .admin_token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(IdentityError::InvalidResponse);
        }
        let expires = Instant::now() + Duration::from_secs(grant.expires_in_secs);
        self.admin_grant = Some((grant.admin_token, expires));
        Ok(())
    }

    /// Lock locally first, including when the revocation response is lost.
    pub fn lock_management(&mut self) -> Result<(), IdentityError> {
        let grant = self
            .admin_grant
            .take()
            .ok_or(IdentityError::MissingCredential)?
            .0;
        let response = self
            .transport
            .exchange(HttpRequest::AdminLock, Some(&grant), None)
            .map_err(IdentityError::Transport)?;
        let empty: EmptyResponse = decode(response)?;
        if empty.schema_version != 1 {
            return Err(IdentityError::InvalidResponse);
        }
        Ok(())
    }

    pub fn login(&mut self, login: &str, password: &str) -> Result<IdentitySession, IdentityError> {
        self.admin_grant = None;
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
        self.admin_grant = None;
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
            self.admin_grant = None;
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
        self.admin_grant = None;
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
mod tests;
