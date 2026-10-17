//! The capture-consent rules from `AGENTS.md`, expressed as tests.
//!
//! These are not UI details: "never capture or publish without an explicit user
//! action and an active OS permission, and keep the state visible" is a product
//! invariant, so it is checked rather than trusted.

use cordel_media::capture::{CaptureTarget, TargetKind};
use cordel_media::consent::{Permission, ShareController, ShareError, ShareEvent, StopReason};

fn screen() -> CaptureTarget {
    CaptureTarget::new(1, "whole screen", TargetKind::Screen)
}

#[test]
fn a_fresh_controller_publishes_nothing() {
    let controller = ShareController::new();
    assert_eq!(controller.permission(), Permission::Unknown);
    assert!(!controller.is_live());
    assert!(!controller.indicator_visible());
}

#[test]
fn starting_without_a_grant_is_refused() {
    let mut controller = ShareController::new();
    assert_eq!(
        controller.start(screen()),
        Err(ShareError::PermissionNotGranted)
    );

    controller.set_permission(Permission::Denied);
    assert_eq!(
        controller.start(screen()),
        Err(ShareError::PermissionNotGranted)
    );
    assert!(!controller.is_live());
}

#[test]
fn a_grant_alone_does_not_start_capture() {
    // The permission is not the action: someone has to ask to share.
    let mut controller = ShareController::new();
    controller.set_permission(Permission::Granted);
    assert!(!controller.is_live());
    assert!(!controller.indicator_visible());
}

#[test]
fn starting_twice_is_an_error_the_ui_can_show() {
    let mut controller = ShareController::new();
    controller.set_permission(Permission::Granted);
    assert_eq!(
        controller.start(screen()),
        Ok(ShareEvent::Started(screen()))
    );
    assert!(controller.indicator_visible());
    assert_eq!(controller.start(screen()), Err(ShareError::AlreadyLive));
}

#[test]
fn losing_permission_while_live_stops_the_share() {
    let mut controller = ShareController::new();
    controller.set_permission(Permission::Granted);
    controller.start(screen()).expect("granted share starts");

    assert_eq!(
        controller.set_permission(Permission::Denied),
        Some(ShareEvent::Stopped(StopReason::PermissionLost))
    );
    assert!(!controller.is_live());
    assert!(
        !controller.indicator_visible(),
        "the indicator must go away with the capture"
    );
}

#[test]
fn stopping_is_reported_and_only_works_while_live() {
    let mut controller = ShareController::new();
    assert_eq!(controller.stop(), Err(ShareError::NotLive));

    controller.set_permission(Permission::Granted);
    controller.start(screen()).expect("granted share starts");
    assert_eq!(
        controller.stop(),
        Ok(ShareEvent::Stopped(StopReason::UserAction))
    );
    assert_eq!(controller.stop(), Err(ShareError::NotLive));
}

#[test]
fn the_state_says_which_target_is_being_shared() {
    let mut controller = ShareController::new();
    controller.set_permission(Permission::Granted);
    let window = CaptureTarget::new(7, "a window", TargetKind::Window);
    controller
        .start(window.clone())
        .expect("granted share starts");
    assert_eq!(
        controller.state(),
        &cordel_media::consent::ShareState::Live(window)
    );
}
