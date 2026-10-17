//! Consent and lifecycle rules for publishing a screen or microphone.
//!
//! `AGENTS.md` requires that Cordel never captures or publishes a screen or
//! microphone without an explicit user action and an active OS permission, and
//! that the capture state stays visible to the user. Those are product rules, not
//! UI details, so they live here as testable state instead of being re-implemented
//! (and quietly forgotten) in each capture path.

use crate::capture::CaptureTarget;

/// OS permission state for the capture source the user wants to share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Not requested yet. Treated the same as denied for publishing purposes.
    Unknown,
    Granted,
    Denied,
}

/// Whether something is currently being published.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareState {
    Idle,
    Live(CaptureTarget),
}

/// Why a share stopped, so the UI can explain it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The user asked to stop.
    UserAction,
    /// The OS permission went away while sharing.
    PermissionLost,
}

/// Transitions a caller must react to (show/hide the "sharing" indicator).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareEvent {
    Started(CaptureTarget),
    Stopped(StopReason),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ShareError {
    /// No active permission: capture must not start.
    PermissionNotGranted,
    AlreadyLive,
    NotLive,
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShareError::PermissionNotGranted => {
                write!(f, "capture permission is not granted; refusing to start")
            }
            ShareError::AlreadyLive => write!(f, "a share is already live"),
            ShareError::NotLive => write!(f, "no share is live"),
        }
    }
}

impl std::error::Error for ShareError {}

/// Gate between "the user wants to share" and "frames leave the machine".
///
/// Order of operations is deliberate: permission first, then an explicit start,
/// then publishing. Anything that revokes permission stops the share immediately
/// and reports why.
#[derive(Debug)]
pub struct ShareController {
    permission: Permission,
    state: ShareState,
}

impl Default for ShareController {
    fn default() -> Self {
        Self::new()
    }
}

impl ShareController {
    /// Starts with [`Permission::Unknown`] — never assume a grant.
    pub fn new() -> Self {
        Self {
            permission: Permission::Unknown,
            state: ShareState::Idle,
        }
    }

    pub fn permission(&self) -> Permission {
        self.permission
    }

    pub fn state(&self) -> &ShareState {
        &self.state
    }

    pub fn is_live(&self) -> bool {
        matches!(self.state, ShareState::Live(_))
    }

    /// The UI must keep an indicator visible while this returns `true`.
    pub fn indicator_visible(&self) -> bool {
        self.is_live()
    }

    /// Records the OS permission answer. Losing permission while live stops the
    /// share and returns the event the UI must show.
    pub fn set_permission(&mut self, permission: Permission) -> Option<ShareEvent> {
        self.permission = permission;
        if permission != Permission::Granted {
            if let ShareState::Live(_) = self.state {
                self.state = ShareState::Idle;
                return Some(ShareEvent::Stopped(StopReason::PermissionLost));
            }
        }
        None
    }

    /// Starts publishing `target`. Requires an explicit call, like the user
    /// pressing "share screen".
    pub fn start(&mut self, target: CaptureTarget) -> Result<ShareEvent, ShareError> {
        if self.permission != Permission::Granted {
            return Err(ShareError::PermissionNotGranted);
        }
        if self.is_live() {
            return Err(ShareError::AlreadyLive);
        }
        self.state = ShareState::Live(target.clone());
        Ok(ShareEvent::Started(target))
    }

    /// Stops publishing. Idempotence is not assumed: stopping nothing is an error
    /// the caller should not hit, because it means the UI and the state disagree.
    pub fn stop(&mut self) -> Result<ShareEvent, ShareError> {
        if !self.is_live() {
            return Err(ShareError::NotLive);
        }
        self.state = ShareState::Idle;
        Ok(ShareEvent::Stopped(StopReason::UserAction))
    }
}
