//! Screen and window capture adapters.
//!
//! ADR-103 puts OS behavior behind an interface, with Linux X11 and Wayland
//! (portal/PipeWire) as separate paths. This module defines that interface and
//! nothing else: it never captures by itself. A backend implements
//! [`ScreenCapture`], and callers must pass through
//! [`crate::consent::ShareController`] before frames leave the machine.
//!
//! The Linux session kind is derived from the environment (`WAYLAND_DISPLAY`
//! versus `DISPLAY`) because the two paths differ in permission model: X11 lets
//! an app read the screen directly, while Wayland requires a desktop portal and
//! an explicit user grant.

use crate::frame::Frame;
use std::fmt;

/// What kind of surface a capture source represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Screen,
    Window,
}

/// A selectable capture source, as the UI would list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureTarget {
    pub id: u64,
    pub title: String,
    pub kind: TargetKind,
}

impl CaptureTarget {
    pub fn new(id: u64, title: impl Into<String>, kind: TargetKind) -> Self {
        Self {
            id,
            title: title.into(),
            kind,
        }
    }
}

/// Which Linux capture path this session needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKind {
    /// Legacy X11 path. XWayland also reports as X11 when `WAYLAND_DISPLAY` is
    /// unset for the process.
    X11,
    /// Wayland session: capture must go through the desktop portal (PipeWire).
    Wayland,
}

impl SessionKind {
    /// Pure decision function, so it is testable without touching the process
    /// environment.
    pub fn from_env(wayland_display: Option<&str>, display: Option<&str>) -> Option<Self> {
        let has = |v: Option<&str>| v.is_some_and(|s| !s.is_empty());
        if has(wayland_display) {
            Some(SessionKind::Wayland)
        } else if has(display) {
            Some(SessionKind::X11)
        } else {
            None
        }
    }

    /// Reads the current process environment.
    pub fn detect() -> Option<Self> {
        Self::from_env(
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            std::env::var("DISPLAY").ok().as_deref(),
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::X11 => "x11",
            SessionKind::Wayland => "wayland",
        }
    }
}

#[derive(Debug)]
pub enum CaptureError {
    /// No usable capture backend on this host or session.
    Unavailable(String),
    /// The OS or portal refused the capture request.
    PermissionDenied,
    /// The requested source is not in the current target list.
    UnknownTarget(u64),
    /// The backend failed after starting.
    Backend(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Unavailable(why) => write!(f, "capture unavailable: {why}"),
            CaptureError::PermissionDenied => write!(f, "capture permission was denied"),
            CaptureError::UnknownTarget(id) => write!(f, "unknown capture target {id}"),
            CaptureError::Backend(why) => write!(f, "capture backend error: {why}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Called for every captured frame. `Send` so a backend may deliver from its own
/// thread, which is what WebRTC's desktop capturer does.
pub type FrameCallback = Box<dyn FnMut(Frame) + Send>;

/// A capture backend. One instance drives one share at a time.
pub trait ScreenCapture: Send {
    /// Stable name for logs and the spike report, e.g. `"libwebrtc-desktop-capturer"`.
    fn backend_name(&self) -> &'static str;

    /// Lists what can be captured. On Wayland this is the portal grant, so it may
    /// require user interaction.
    fn targets(&self) -> Result<Vec<CaptureTarget>, CaptureError>;

    /// Begins capture of one target and delivers frames to `on_frame`.
    fn start(
        &mut self,
        target: &CaptureTarget,
        on_frame: FrameCallback,
    ) -> Result<(), CaptureError>;

    /// Advances a *pull-based* backend.
    ///
    /// This exists because the two Linux paths differ in who drives capture:
    /// a portal/PipeWire backend pushes frames on its own thread, while WebRTC's
    /// desktop capturer requires the caller to ask for each frame. The product UI
    /// must know which one it is talking to, so the interface states it instead of
    /// hiding it in a backend thread. Push-based backends leave this as a no-op.
    fn tick(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }

    /// Stops capture. Must be safe to call after a permission loss.
    fn stop(&mut self) -> Result<(), CaptureError>;
}

/// Honest placeholder used when the crate is built without a capture backend, so
/// the probe prints a clear "not built / not available" line instead of looking
/// like capture silently works.
pub struct UnavailableCapture {
    reason: String,
}

impl UnavailableCapture {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl ScreenCapture for UnavailableCapture {
    fn backend_name(&self) -> &'static str {
        "unavailable"
    }

    fn targets(&self) -> Result<Vec<CaptureTarget>, CaptureError> {
        Err(CaptureError::Unavailable(self.reason.clone()))
    }

    fn start(
        &mut self,
        _target: &CaptureTarget,
        _on_frame: FrameCallback,
    ) -> Result<(), CaptureError> {
        Err(CaptureError::Unavailable(self.reason.clone()))
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }
}
