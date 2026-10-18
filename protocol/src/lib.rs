//! Wire contract for Cordel session admission and chat catch-up.
//!
//! ADR-002 owns accounts, invites, and app sessions. ADR-004 owns ordered
//! chat history. This crate describes the messages those decisions share.
//! It does not store passwords, issue tokens, or open a database.

mod chat;
mod session;
mod ws;

pub use chat::{
    validate_history, ChatEvent, HistoryError, HistoryPage, HistoryQuery, MessageAck, SendMessage,
    PROTOCOL_VERSION,
};
pub use session::{
    authorization_header, ApiError, ErrorCode, ErrorDetail, LiveKitAdmission, LoginRequest,
    RedeemInviteRequest, RefreshRequest, SessionTokens, ACCESS_TOKEN_HEADER,
};
pub use ws::{FrameError, WsFrame};
