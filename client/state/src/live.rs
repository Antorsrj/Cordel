//! ADR-004 live chat: subscribe, catch up, reconcile, and recover.
//!
//! Socket and HTTP calls are blocking and belong on a worker thread, never the
//! UI thread. The session owns no credentials: the caller passes the current
//! access token and refreshes it through `IdentityClient` after a reconnect.

use std::time::Duration;

use cordel_protocol::{HistoryPage, MessageAck, WsFrame};

use crate::catchup::AppliedEvent;
use crate::retry::AccessBlock;

pub const HISTORY_PAGE_LIMIT: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveTransportError {
    Network,
    Closed,
    MalformedFrame,
    /// No frame arrived before the read deadline. The connection remains usable.
    Timeout,
}

pub trait LiveTransport {
    fn connect(&mut self, endpoint: &str, token: &str) -> Result<(), LiveTransportError>;
    fn send(&mut self, frame: &WsFrame) -> Result<(), LiveTransportError>;
    fn receive(&mut self) -> Result<WsFrame, LiveTransportError>;
    fn close(&mut self);
    /// Interactive clients shorten this between frames so the UI thread can
    /// process commands. The default keeps the production transport's deadline.
    fn set_read_timeout(&mut self, _timeout: Duration) -> Result<(), LiveTransportError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatTransportError {
    Network,
    Unauthorized,
    Forbidden,
    InvalidResponse,
    Api { status: u16, code: String },
}

pub trait ChatTransport {
    fn history(
        &self,
        bearer: &str,
        channel_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<HistoryPage, ChatTransportError>;
    fn send(
        &self,
        bearer: &str,
        channel_id: &str,
        client_message_id: &str,
        body: &str,
    ) -> Result<MessageAck, ChatTransportError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveError {
    NotConnected,
    MissingCredential,
    InvalidEndpoint,
    Transport(LiveTransportError),
    UnsupportedVersion {
        v: u32,
    },
    Protocol {
        code: String,
        message: String,
    },
    InvalidFrame {
        field: &'static str,
    },
    History(ChatTransportError),
    Gap {
        channel_id: String,
        missing_seq: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveOutcome {
    Ready,
    Subscribed {
        channel_id: String,
        last_seq: u64,
        applied: Vec<AppliedEvent>,
    },
    Applied {
        channel_id: String,
        applied: Vec<AppliedEvent>,
    },
    Ack {
        client_message_id: String,
        seq: u64,
    },
    Unsubscribed {
        channel_id: String,
    },
    Reconnect {
        delay: Duration,
        attempt: u32,
    },
    Stopped {
        reason: AccessBlock,
    },
    Fatal(LiveError),
    Error(LiveError),
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Acknowledged {
        client_message_id: String,
        seq: u64,
    },
    Failed {
        client_message_id: String,
        error: ChatTransportError,
    },
}

pub struct LiveSession<L, C> {
    live: L,
    chat: C,
    endpoint: String,
    channels: Vec<String>,
    connected: bool,
    attempt: u32,
}

mod native;
mod session;

pub use native::{NativeChatTransport, NativeLiveTransport};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod transport_tests;
