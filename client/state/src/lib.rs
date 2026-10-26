//! Client recovery for ADR-102.
//!
//! PostgreSQL and Axum stay authoritative. This crate tracks the disposable
//! per-channel cursor, catch-up order, pending sends, and which credentials
//! may survive a reconnect. Screen capture and audio devices belong to ADR-103.

mod catchup;
mod credentials;
mod identity;
mod live;
mod media;
mod outbox;
mod presence;
mod protocol;
mod retry;
mod session;

pub use catchup::{catch_up, AppliedEvent, CatchUp};
pub use credentials::{
    MemoryRefreshStore, RefreshCredential, RefreshStore, StoreError, VolatileTokens,
};
pub use identity::{
    HttpReply, HttpRequest, IdentityClient, IdentityError, IdentityTransport, NativeHttpsTransport,
    TransportFailure,
};
pub use live::{
    ChatTransport, ChatTransportError, LiveError, LiveOutcome, LiveSession, LiveTransport,
    LiveTransportError, NativeChatTransport, NativeLiveTransport, SendOutcome, HISTORY_PAGE_LIMIT,
};
pub use media::{AdmissionError, AdmissionTransport, MediaAdmissionClient};
pub use outbox::{AckResult, ClientMessageId, Outbox, PendingSend, SendState};
pub use presence::{PresenceStatus, PresenceSync};
pub use protocol::ChatEvent;
pub use retry::{plan_interrupt, retry_delay, AccessBlock, Interrupt, NextAction, FIRST_RETRY};
pub use session::{visible_bodies, ClientState, CursorCache};

#[cfg(feature = "secret-service")]
pub use credentials::{OsRefreshStore, SecretServiceRefreshStore};
