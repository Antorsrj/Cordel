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
    let mut epochs = 0;
    let mut receivers = Vec::new();
    for click in 0..10 {
        let watched = follows.watch(publisher, &format!("TR_{click}"), true, &mut epochs);
        assert!(watched.fresh);
        assert_eq!(watched.epoch, click + 1);
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
    let mut epochs = 0;
    let ana = follows.watch("ana", "screen", true, &mut epochs);
    let bruno = follows.watch("bruno", "screen", true, &mut epochs);
    let camera = follows.watch("ana", "camera", false, &mut epochs);
    assert!(ana.ended.is_empty() && bruno.ended.is_empty() && camera.ended.is_empty());
    assert_eq!(follows.keys().len(), 3);
    assert!(!*ana.stop.borrow());
}

/// Leave drops the room, and Join used to build a new follow set whose epochs
/// started at 1 again. The stage still holds the tombstone, so the counter
/// that both the local share and the remote follows draw from keeps moving.
#[test]
fn rejoining_issues_a_newer_epoch_than_the_share_that_ended() {
    let mut epochs = 0;
    let mut follows = follow::RemoteFollows::new();
    let first = follows.watch("ana", "TR_screen", true, &mut epochs);
    assert_eq!(first.epoch, 1);
    let local = issue_epoch(&mut epochs);
    assert_eq!(local, 2);
    assert_eq!(follows.end_all(), vec![("ana/TR_screen".to_string(), 1)]);

    let mut follows = follow::RemoteFollows::new();
    let again = follows.watch("ana", "TR_screen", true, &mut epochs);
    let again_local = issue_epoch(&mut epochs);
    assert_eq!(again.epoch, 3);
    assert_eq!(again_local, 4);
}

/// Leave has to tell the stage about every remote share. Dropping the room
/// stops the decoders, and without these events the tiles and renderers stay.
#[test]
fn leaving_emits_an_end_for_every_remote_share() {
    let (tx, rx) = std::sync::mpsc::channel();
    let events = MediaOut::new(tx);
    let mut epochs = 0;
    let mut follows = follow::RemoteFollows::new();
    follows.watch("ana", "TR_a", true, &mut epochs);
    follows.watch("bruno", "TR_b", true, &mut epochs);
    emit_remote_ends(&mut follows, &events);
    drop(events);

    let mut ends = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            MediaEvent::Ended { key, epoch } => ends.push((key, epoch)),
            MediaEvent::Frame { .. }
            | MediaEvent::State { .. }
            | MediaEvent::Status(_)
            | MediaEvent::Busy(_)
            | MediaEvent::Joined(_) => panic!("leave should only end the remote shares"),
        }
    }
    ends.sort();
    assert_eq!(
        ends,
        vec![("ana/TR_a".to_string(), 1), ("bruno/TR_b".to_string(), 2)]
    );
    assert!(follows.keys().is_empty());
}

/// `Command::Leave` is what calls `emit_remote_ends`. The helper test above
/// still passes if that call is removed; this one sends the command.
#[tokio::test]
async fn leave_command_emits_an_end_for_every_remote_share() {
    let mut epochs = 0;
    let mut follows = follow::RemoteFollows::new();
    follows.watch("ana", "TR_a", true, &mut epochs);
    follows.watch("bruno", "TR_b", true, &mut epochs);
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let events = MediaOut::new(event_tx);
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_access_tx, access_rx) = watch::channel(None);
    command_tx
        .send(Command::Leave)
        .expect("the command is sent");
    drop(command_tx);

    run_session(command_rx, events, access_rx, Some(follows)).await;

    let mut ends = Vec::new();
    let mut joined = None;
    while let Ok(event) = event_rx.try_recv() {
        match event {
            MediaEvent::Ended { key, epoch } => ends.push((key, epoch)),
            MediaEvent::Joined(value) => joined = Some(value),
            MediaEvent::Status(_) => {}
            MediaEvent::Frame { .. } | MediaEvent::State { .. } | MediaEvent::Busy(_) => {
                panic!("leave should end the remote shares and mark the room left")
            }
        }
    }
    ends.sort();
    assert_eq!(
        ends,
        vec![("ana/TR_a".to_string(), 1), ("bruno/TR_b".to_string(), 2)]
    );
    assert_eq!(joined, Some(false));
}
