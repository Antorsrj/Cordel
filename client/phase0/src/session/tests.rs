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

#[test]
fn sharing_again_does_not_open_a_second_publication() {
    assert_eq!(
        stream::accept_share_click(true),
        Err("Already sharing the screen. Stop sharing before sharing it again.")
    );
    assert!(stream::accept_share_click(false).is_ok());
}

/// Ten screen tracks from one participant keep one decoder. Each new track
/// tells the previous one to stop, which is what drops its video stream.
#[test]
fn ten_screen_tracks_from_one_participant_keep_one_decoder() {
    let publisher = "6f885a53-41ee-41b9-b5d8-a4f7bec009bc";
    let mut follows = follow::RemoteFollows::new();
    let mut receivers = Vec::new();
    for click in 0..10 {
        let watched = follows.watch(publisher, &format!("TR_{click}"), true);
        assert!(watched.fresh);
        assert_eq!(watched.ended.len(), if click == 0 { 0 } else { 1 });
        receivers.push(watched.stop);
    }
    assert_eq!(follows.keys().len(), 1);
    for stopped in &receivers[..9] {
        assert!(
            *stopped.borrow(),
            "the previous screen decoder was told to stop"
        );
    }
    assert!(!*receivers[9].borrow());

    let epoch = follows
        .end(&format!("{publisher}/TR_9"))
        .expect("the live share");
    assert!(epoch >= 1);
    assert!(*receivers[9].borrow(), "stopping releases the last decoder");
    assert!(follows.keys().is_empty());

    // A different person, and a non-screen video, are not the same publication.
    let mut follows = follow::RemoteFollows::new();
    let ana = follows.watch("ana", "screen", true);
    let bruno = follows.watch("bruno", "screen", true);
    let camera = follows.watch("ana", "camera", false);
    assert!(ana.ended.is_empty() && bruno.ended.is_empty() && camera.ended.is_empty());
    assert_eq!(follows.keys().len(), 3);
    assert!(!*ana.stop.borrow());
}
