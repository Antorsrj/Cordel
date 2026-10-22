//! Cordel Phase 0 media probe — ADR-103.
//!
//! This crate is a **spike**, not product code. It exists to answer the gate in
//! [`docs/adr/README.md`](https://github.com/Antorsrj/Cordel/blob/develop/docs/adr/README.md):
//! on Linux, prove two-way audio plus capture, publication, reception and display
//! of at least one screen frame *before* committing the product UI to Slint
//! (ADR-101) and before choosing capture libraries (ADR-103).
//!
//! Layering, following `architecture.md` §4 (shared logic stays OS-free):
//!
//! | Module | Depends on | Verified without hardware |
//! | --- | --- | --- |
//! | [`frame`] | nothing | yes — pixel conversion is unit-tested |
//! | [`consent`] | nothing | yes — capture-consent rules are unit-tested |
//! | [`quality`] | nothing | yes — bitrate presets are unit-tested |
//! | [`capture`] | nothing | trait + honest "unavailable" backend |
//! | [`devices`] | nothing | trait + honest "unavailable" backend |
//! | `render` (feature `ui`) | Slint | measured conversion + image construction |
//! | `sdk` (feature `sdk`) | LiveKit Rust SDK | needs the WebRTC toolchain |
//!
//! Hard rule from `AGENTS.md`: never capture or publish a screen or microphone
//! without an explicit user action and an active OS permission. [`consent`]
//! encodes that rule so a spike harness cannot quietly bypass it.

pub mod capture;
pub mod consent;
pub mod devices;
pub mod frame;
pub mod quality;

#[cfg(feature = "ui")]
pub mod render;

#[cfg(feature = "sdk")]
pub mod sdk;

/// One-line description of what this build can actually do, for the probe output.
pub fn build_capabilities() -> Vec<(&'static str, bool)> {
    vec![
        ("frame conversion (shared)", true),
        ("share consent rules (shared)", true),
        ("slint render path (feature `ui`)", cfg!(feature = "ui")),
        (
            "livekit sdk adapters (feature `sdk`)",
            cfg!(feature = "sdk"),
        ),
    ]
}
