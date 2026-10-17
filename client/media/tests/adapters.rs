//! Capture/audio adapter behaviour that can be checked without hardware:
//! which session path Linux needs, and that a build without a backend says so
//! instead of pretending.

use cordel_media::capture::{
    CaptureError, CaptureTarget, ScreenCapture, SessionKind, TargetKind, UnavailableCapture,
};
use cordel_media::devices::{AudioDeviceControl, UnavailableAudio};

#[test]
fn session_kind_prefers_wayland_when_the_session_is_wayland() {
    // A Wayland session usually also exports DISPLAY for XWayland; the Wayland
    // path is the one with the portal permission model, so it wins.
    assert_eq!(
        SessionKind::from_env(Some("wayland-1"), Some(":0")),
        Some(SessionKind::Wayland)
    );
    assert_eq!(
        SessionKind::from_env(None, Some(":0")),
        Some(SessionKind::X11)
    );
    assert_eq!(
        SessionKind::from_env(Some(""), Some(":0")),
        Some(SessionKind::X11)
    );
    assert_eq!(
        SessionKind::from_env(Some("wayland-0"), None),
        Some(SessionKind::Wayland)
    );
    assert_eq!(SessionKind::from_env(None, None), None);
    assert_eq!(SessionKind::from_env(Some(""), Some("")), None);
}

#[test]
fn a_missing_backend_reports_unavailable_rather_than_working() {
    let mut backend = UnavailableCapture::new("no SDK in this build");
    assert_eq!(backend.backend_name(), "unavailable");
    match backend.targets() {
        Err(CaptureError::Unavailable(reason)) => assert!(reason.contains("no SDK")),
        other => panic!("expected Unavailable, got {other:?}"),
    }
    let target = CaptureTarget::new(1, "x", TargetKind::Screen);
    assert!(backend.start(&target, Box::new(|_frame| {})).is_err());
    // Stopping something that never started must not be an error: a client tears
    // down uniformly.
    assert!(backend.stop().is_ok());
}

#[test]
fn a_missing_audio_backend_reports_unavailable() {
    let mut audio = UnavailableAudio::new("no SDK in this build");
    assert_eq!(audio.backend_name(), "unavailable");
    assert!(audio.recording_devices().is_err());
    assert!(audio.playout_devices().is_err());
    assert!(audio.set_recording_device("anything").is_err());
    assert!(audio.switch_recording_device("anything").is_err());
    assert!(audio.start_recording().is_err());
    assert!(audio.stop_recording().is_err());
}
