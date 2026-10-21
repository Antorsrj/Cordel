//! The account facts the workers need.
//!
//! The identity worker owns the refresh credential and the app session. The
//! chat and media workers only ever see the current access token and the
//! account identity, and each keeps its own disposable state.

/// Where an access token came from. A fresh login may lift a revoked state;
/// a refresh may not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessSource {
    Login,
    Refresh,
}

#[derive(Debug, Clone)]
pub enum AccountEvent {
    Access {
        token: String,
        user_id: String,
        login: String,
        source: AccessSource,
    },
    SignedOut,
}
