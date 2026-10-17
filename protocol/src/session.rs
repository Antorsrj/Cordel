use serde::{Deserialize, Serialize};
use std::fmt;

pub const ACCESS_TOKEN_HEADER: &str = "authorization";

/// App session issued by Axum. This is not a LiveKit room token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in_secs: u64,
}

impl fmt::Debug for SessionTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionTokens")
            .field("access_token", &"redacted")
            .field("refresh_token", &"redacted")
            .field("expires_in_secs", &self.expires_in_secs)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

impl fmt::Debug for LoginRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoginRequest")
            .field("login", &self.login)
            .field("password", &"redacted")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedeemInviteRequest {
    pub invite_code: String,
    pub login: String,
    pub password: String,
}

impl fmt::Debug for RedeemInviteRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RedeemInviteRequest")
            .field("invite_code", &"redacted")
            .field("login", &self.login)
            .field("password", &"redacted")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

impl fmt::Debug for RefreshRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RefreshRequest")
            .field("refresh_token", &"redacted")
            .finish()
    }
}

/// Media admission is a separate credential from the app session.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveKitAdmission {
    pub url: String,
    pub token: String,
    pub room: String,
}

impl fmt::Debug for LiveKitAdmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveKitAdmission")
            .field("url", &self.url)
            .field("token", &"redacted")
            .field("room", &self.room)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCode {
    PublicRegistrationDisabled,
    InviteUnknown,
    InviteExpired,
    InviteExhausted,
    UsernameTaken,
    Unauthorized,
    RefreshReused,
    Forbidden,
    Other(String),
}

impl ErrorCode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::PublicRegistrationDisabled => "public_registration_disabled",
            Self::InviteUnknown => "invite_unknown",
            Self::InviteExpired => "invite_expired",
            Self::InviteExhausted => "invite_exhausted",
            Self::UsernameTaken => "username_taken",
            Self::Unauthorized => "unauthorized",
            Self::RefreshReused => "refresh_reused",
            Self::Forbidden => "forbidden",
            Self::Other(code) => code,
        }
    }

    pub fn parse(code: &str) -> Self {
        match code {
            "public_registration_disabled" => Self::PublicRegistrationDisabled,
            "invite_unknown" => Self::InviteUnknown,
            "invite_expired" => Self::InviteExpired,
            "invite_exhausted" => Self::InviteExhausted,
            "username_taken" => Self::UsernameTaken,
            "unauthorized" => Self::Unauthorized,
            "refresh_reused" => Self::RefreshReused,
            "forbidden" => Self::Forbidden,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Ok(Self::parse(&code))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: ErrorCode,
}

pub fn authorization_header(access_token: &str) -> String {
    format!("Bearer {access_token}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_tokens_are_distinct_from_livekit_admission() {
        let session = SessionTokens {
            access_token: "access-secret".to_owned(),
            refresh_token: "refresh-secret".to_owned(),
            expires_in_secs: 300,
        };
        let json = serde_json::to_value(&session).unwrap();
        assert!(json.get("token").is_none());
        assert!(json.get("room").is_none());
        let parsed: SessionTokens = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, session);

        let admission = LiveKitAdmission {
            url: "wss://media.example".to_owned(),
            token: "room-token".to_owned(),
            room: "voice-1".to_owned(),
        };
        let parsed: LiveKitAdmission =
            serde_json::from_str(&serde_json::to_string(&admission).unwrap()).unwrap();
        assert_eq!(parsed.room, "voice-1");
        assert!(!format!("{admission:?}").contains("room-token"));
        assert!(!format!("{session:?}").contains("access-secret"));
        assert!(!format!("{session:?}").contains("refresh-secret"));
    }

    #[test]
    fn public_registration_and_moderation_have_stable_codes() {
        let disabled = ApiError {
            error: ErrorCode::PublicRegistrationDisabled,
        };
        let json = serde_json::to_string(&disabled).unwrap();
        assert!(json.contains("public_registration_disabled"));
        let parsed: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.error, ErrorCode::PublicRegistrationDisabled);

        let future = ApiError {
            error: ErrorCode::parse("future_limit"),
        };
        let parsed: ApiError =
            serde_json::from_str(&serde_json::to_string(&future).unwrap()).unwrap();
        assert_eq!(parsed.error, ErrorCode::Other("future_limit".to_owned()));
        assert_eq!(ErrorCode::Forbidden.as_str(), "forbidden");
        assert_ne!(ErrorCode::Forbidden, ErrorCode::Unauthorized);
    }

    #[test]
    fn native_websocket_header_uses_the_access_token() {
        assert_eq!(authorization_header("abc"), "Bearer abc");
        assert_eq!(ACCESS_TOKEN_HEADER, "authorization");
    }
}
