//! Capture/audio adapter behaviour that can be checked without hardware:
//! which session path Linux needs, and that a build without a backend says so
//! instead of pretending.

use cordel_media::capture::{
    CaptureError, CaptureSource, CaptureTarget, ScreenCapture, SessionKind, TargetKind,
    UnavailableCapture,
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
fn auto_follows_the_session_and_explicit_paths_do_not() {
    // This is the rule the whole capture-backend choice rests on: on Wayland
    // `auto` must request the portal, because the direct capturer is the X11 one
    // and would be asking XWayland to be Wayland.
    assert_eq!(
        CaptureSource::Auto.resolve(Some(SessionKind::Wayland)),
        CaptureSource::Portal
    );
    assert_eq!(
        CaptureSource::Auto.resolve(Some(SessionKind::X11)),
        CaptureSource::Screen
    );
    assert_eq!(CaptureSource::Auto.resolve(None), CaptureSource::Screen);

    // An explicit request is never rewritten, so the probe can force either path
    // on the same machine and the evidence means what it says.
    for requested in [
        CaptureSource::Portal,
        CaptureSource::Screen,
        CaptureSource::Window,
    ] {
        for session in [Some(SessionKind::Wayland), Some(SessionKind::X11), None] {
            assert_eq!(requested.resolve(session), requested);
        }
    }
}

#[test]
fn the_source_words_round_trip() {
    for source in [
        CaptureSource::Auto,
        CaptureSource::Portal,
        CaptureSource::Screen,
        CaptureSource::Window,
    ] {
        assert_eq!(CaptureSource::parse(source.as_str()), Some(source));
    }
    assert_eq!(CaptureSource::parse("x11"), None);
    assert_eq!(CaptureSource::parse(""), None);
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
