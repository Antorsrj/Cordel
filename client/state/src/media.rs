//! ADR-005 media admission: one scoped room credential for one voice channel.
//!
//! The admission is a separate credential from the app session, so it lives
//! with the other server-facing calls rather than with the media adapter. The
//! caller passes the current access token; the server decides what that
//! account may join.

use cordel_protocol::MediaAdmission;
use serde_json::Value;

use crate::identity::{HttpReply, TransportFailure};

/// Why an admission was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    /// The server could not be reached.
    Network,
    /// The app session is missing, expired, or the account is moderated.
    Unauthorized,
    /// The account may not use this channel, or it is not a voice channel.
    Forbidden,
    /// The channel does not exist.
    NotFound,
    /// The server answered with something this client cannot use.
    InvalidResponse,
}

impl std::fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Network => "network",
            Self::Unauthorized => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::InvalidResponse => "invalid_response",
        };
        formatter.write_str(text)
    }
}

/// A client for the media admission route.
#[derive(Debug, Clone)]
pub struct MediaAdmissionClient<T> {
    transport: T,
}

impl<T> MediaAdmissionClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

/// The transport the admission client needs: a POST with a bearer token and
/// an empty body, answered with JSON.
pub trait AdmissionTransport {
    fn post(&self, path: &str, bearer: &str, body: &[u8]) -> Result<HttpReply, TransportFailure>;
}

/// A borrowed transport works the same as an owned one, so a caller can keep
/// one HTTP client and share it between the identity and admission calls.
impl<T: AdmissionTransport + ?Sized> AdmissionTransport for &T {
    fn post(&self, path: &str, bearer: &str, body: &[u8]) -> Result<HttpReply, TransportFailure> {
        (**self).post(path, bearer, body)
    }
}

impl<T: AdmissionTransport> MediaAdmissionClient<T> {
    /// Ask the server for one admission for a voice channel.
    ///
    /// The access token must be current: a stale token is refused, and the
    /// caller refreshes it through `IdentityClient` before retrying.
    pub fn admission(
        &self,
        access_token: &str,
        channel_id: &str,
    ) -> Result<MediaAdmission, AdmissionError> {
        let path = format!("/v1/voice-channels/{channel_id}/admission");
        let reply = self
            .transport
            .post(&path, access_token, b"")
            .map_err(|_| AdmissionError::Network)?;
        match reply.status {
            200 => {}
            401 => return Err(AdmissionError::Unauthorized),
            403 => return Err(AdmissionError::Forbidden),
            404 => return Err(AdmissionError::NotFound),
            _ => return Err(AdmissionError::InvalidResponse),
        }
        let value: Value =
            serde_json::from_slice(&reply.body).map_err(|_| AdmissionError::InvalidResponse)?;
        parse_admission(&value)
    }
}

fn parse_admission(value: &Value) -> Result<MediaAdmission, AdmissionError> {
    let text = |field: &str| -> Result<String, AdmissionError> {
        value
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(AdmissionError::InvalidResponse)
    };
    let number = |field: &str| -> Result<u64, AdmissionError> {
        value
            .get(field)
            .and_then(Value::as_u64)
            .ok_or(AdmissionError::InvalidResponse)
    };
    if value.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(AdmissionError::InvalidResponse);
    }
    Ok(MediaAdmission {
        schema_version: 1,
        voice_channel_id: text("voice_channel_id")?,
        room: text("room")?,
        media_generation: number("media_generation")?,
        url: text("url")?,
        token: text("token")?,
        expires_at: text("expires_at")?,
        expires_in_secs: number("expires_in_secs")?,
    })
}

/// The native HTTPS transport for the admission route.
///
/// It reuses the identity transport so a client keeps one HTTP client, one
/// origin, and one set of TLS rules.
#[cfg(test)]
mod tests;

impl AdmissionTransport for crate::identity::NativeHttpsTransport {
    fn post(&self, path: &str, bearer: &str, body: &[u8]) -> Result<HttpReply, TransportFailure> {
        self.post_path(path, bearer, body)
    }
}
