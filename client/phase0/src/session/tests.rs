
use super::*;

#[test]
fn session_label_names_the_unknown_case_instead_of_hiding_it() {
    assert_eq!(session_label(Some(SessionKind::Wayland)), "wayland");
    assert_eq!(session_label(Some(SessionKind::X11)), "x11");
    assert_eq!(session_label(None), "unknown");
}

#[test]
fn a_share_key_is_the_publisher_and_the_track() {
    assert_eq!(share_key("ana", "TR_123"), "ana/TR_123");
}
