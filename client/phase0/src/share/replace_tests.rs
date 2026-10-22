//! One screen share per participant.
//!
//! Issue #25: each click on Share screen published a new track. The viewer
//! logged that the previous track had ended, and the stage still kept every
//! tile, because the decoder delivered another picture afterwards and that
//! picture created the tile again.

use super::events::announce;
use super::*;

fn picture(slots: &FrameSlots, key: &str, publisher: &str) -> MediaEvent {
    announce(
        slots,
        key,
        publisher,
        ShareState::Live,
        cordel_media::frame::synthetic_bgra(320, 180, 1),
        0,
    )
    .expect("a picture is announced")
}

fn stage() -> (Rc<Shares>, FrameSlots) {
    let slots = FrameSlots::default();
    (Shares::new(slots.clone()), slots)
}

fn keys(shares: &Shares) -> Vec<String> {
    (0..shares.model().row_count())
        .filter_map(|row| shares.model().row_data(row))
        .map(|tile| tile.key.to_string())
        .collect()
}

/// The UI thread's order: every end in the batch, then every picture that was
/// already queued. This is the drain in `main`.
fn drain(shares: &Shares, events: Vec<MediaEvent>) {
    let mut pictures = Vec::new();
    for event in events {
        match event {
            MediaEvent::Frame {
                key,
                publisher,
                state,
            } => pictures.push((key, publisher, state)),
            other => {
                let _ = shares.apply(other);
            }
        }
    }
    for (key, publisher, state) in pictures {
        let _ = shares.apply(MediaEvent::Frame {
            key,
            publisher,
            state,
        });
    }
}

#[test]
fn ten_clicks_leave_one_tile_and_stopping_removes_it() {
    let publisher = "6f885a53-41ee-41b9-b5d8-a4f7bec009bc";
    let (shares, slots) = stage();

    // The end events are not required for the count. Ten track ids from one
    // person are still one tile, and the replaced renderers are released.
    for click in 0..10 {
        let key = format!("{publisher}/TR_{click}");
        shares.apply(picture(&slots, &key, publisher));
    }
    assert_eq!(keys(&shares), vec![format!("{publisher}/TR_9")]);
    assert!(!shares.renderer_held(&format!("{publisher}/TR_0")));

    // Each click ends the previous track. The drain applies that end before
    // the picture that was already queued, and the decoder then delivers one
    // more picture of the track the session has already logged as ended.
    let (shares, slots) = stage();
    for click in 0..10 {
        let key = format!("{publisher}/TR_{click}");
        if click > 0 {
            let previous = format!("{publisher}/TR_{}", click - 1);
            let queued = picture(&slots, &previous, publisher);
            drain(
                &shares,
                vec![
                    queued,
                    MediaEvent::Ended {
                        key: previous.clone(),
                        epoch: 0,
                    },
                ],
            );
            shares.apply(picture(&slots, &previous, publisher));
        }
        shares.apply(picture(&slots, &key, publisher));
        assert_eq!(
            keys(&shares),
            vec![key],
            "click {click} must replace the previous share, not stack a tile"
        );
    }

    let current = format!("{publisher}/TR_9");
    let queued = picture(&slots, &current, publisher);
    drain(
        &shares,
        vec![
            queued,
            MediaEvent::Ended {
                key: current.clone(),
                epoch: 0,
            },
        ],
    );
    shares.apply(picture(&slots, &current, publisher));
    assert_eq!(
        keys(&shares),
        Vec::<String>::new(),
        "stopping removes the tile, and a late picture does not bring it back"
    );
    assert!(
        !shares.renderer_held(&current),
        "the ended share releases its renderer"
    );

    // Someone else, and a new track after the stop, are each one tile. A late
    // picture of the stopped track still is not.
    let other = "other-participant";
    shares.apply(picture(&slots, &format!("{other}/TR_a"), other));
    shares.apply(picture(&slots, &format!("{publisher}/TR_new"), publisher));
    shares.apply(picture(&slots, &current, publisher));
    let mut shown = keys(&shares);
    shown.sort();
    let mut expected = vec![format!("{other}/TR_a"), format!("{publisher}/TR_new")];
    expected.sort();
    assert_eq!(shown, expected);
}

fn picture_at(slots: &FrameSlots, key: &str, publisher: &str, epoch: u64) -> MediaEvent {
    announce(
        slots,
        key,
        publisher,
        ShareState::Live,
        cordel_media::frame::synthetic_bgra(320, 180, 1),
        epoch,
    )
    .expect("a picture is announced")
}

fn ended(key: &str, epoch: u64) -> MediaEvent {
    MediaEvent::Ended {
        key: key.to_string(),
        epoch,
    }
}

/// Leave and Join used to start the publication counter at zero again. The
/// stage tombstone is still the epoch that ended, so that picture never
/// becomes a tile. The next epoch does.
#[test]
fn a_rejoin_needs_a_newer_epoch_or_the_local_preview_stays_blank() {
    let (shares, slots) = stage();
    let key = LOCAL_SHARE_KEY;
    shares.apply(picture_at(&slots, key, "you", 1));
    assert_eq!(keys(&shares), vec![key.to_string()]);
    assert_eq!(
        shares.apply(ended(key, 1)).as_deref(),
        Some("you stopped sharing.")
    );
    assert!(keys(&shares).is_empty());
    assert!(!shares.renderer_held(key));

    shares.apply(picture_at(&slots, key, "you", 1));
    assert!(
        keys(&shares).is_empty(),
        "reusing the ended epoch must not bring the local preview back"
    );
    assert!(!shares.renderer_held(key));

    shares.apply(picture_at(&slots, key, "you", 2));
    assert_eq!(keys(&shares), vec![key.to_string()]);
    assert!(shares.renderer_held(key));
}

/// Stop sharing, then Share screen again. The late picture of the stopped
/// publication stays gone, and the new epoch is the preview.
#[test]
fn stopping_then_sharing_again_shows_the_newer_epoch_only() {
    let (shares, slots) = stage();
    let key = LOCAL_SHARE_KEY;
    shares.apply(picture_at(&slots, key, "you", 4));
    let queued = picture_at(&slots, key, "you", 4);
    drain(&shares, vec![queued, ended(key, 4)]);
    shares.apply(picture_at(&slots, key, "you", 4));
    assert!(keys(&shares).is_empty());
    assert!(!shares.renderer_held(key));

    shares.apply(picture_at(&slots, key, "you", 5));
    assert_eq!(keys(&shares), vec![key.to_string()]);
    assert!(shares.renderer_held(key));
}

/// A reconnect ends the publication the drop belonged to. A picture from that
/// epoch does not restore the tile, a newer epoch of the same track does, and
/// the old end does not remove the restored share.
#[test]
fn a_reconnect_restores_the_share_only_at_a_newer_epoch() {
    let (shares, slots) = stage();
    let key = "ana/TR_screen";
    shares.apply(picture_at(&slots, key, "ana", 3));
    shares.apply(MediaEvent::State {
        key: key.to_string(),
        state: ShareState::Reconnecting,
    });
    assert_eq!(keys(&shares), vec![key.to_string()]);

    let queued = picture_at(&slots, key, "ana", 3);
    drain(&shares, vec![queued, ended(key, 3)]);
    shares.apply(picture_at(&slots, key, "ana", 3));
    assert!(
        keys(&shares).is_empty(),
        "the publication that dropped stays ended"
    );
    assert!(!shares.renderer_held(key));

    shares.apply(picture_at(&slots, key, "ana", 7));
    assert_eq!(keys(&shares), vec![key.to_string()]);
    assert!(shares.apply(ended(key, 3)).is_none());
    assert_eq!(keys(&shares), vec![key.to_string()]);
    assert!(shares.renderer_held(key));
}

/// The detached window follows the publication. Ending that epoch closes it
/// and releases the renderer; a newer epoch comes back on the stage.
#[test]
fn a_detached_share_ends_with_its_epoch_and_a_newer_one_returns() {
    let (shares, slots) = stage();
    let key = "ana/TR_screen";
    shares.apply(picture_at(&slots, key, "ana", 2));
    shares.detach(key);
    assert!(keys(&shares).is_empty());
    assert_eq!(
        shares.detached_tile().map(|tile| tile.key.to_string()),
        Some(key.to_string())
    );
    assert!(shares.renderer_held(key));

    let queued = picture_at(&slots, key, "ana", 2);
    drain(&shares, vec![queued, ended(key, 2)]);
    assert!(shares.detached_tile().is_none());
    assert!(!shares.renderer_held(key));
    shares.apply(picture_at(&slots, key, "ana", 2));
    assert!(shares.detached_tile().is_none());
    assert!(keys(&shares).is_empty());

    shares.apply(picture_at(&slots, key, "ana", 6));
    assert_eq!(keys(&shares), vec![key.to_string()]);
    assert!(shares.detached_tile().is_none());
    assert!(shares.renderer_held(key));
}

/// Leave emits an end for each remote share. The stage drops the tile, the
/// detached window and the renderer, and a picture of that epoch stays gone.
#[test]
fn leaving_removes_remote_tiles_including_a_detached_one() {
    let (shares, slots) = stage();
    shares.apply(picture_at(&slots, "ana/TR_a", "ana", 1));
    shares.apply(picture_at(&slots, "bruno/TR_b", "bruno", 2));
    shares.detach("ana/TR_a");
    assert!(shares.renderer_held("ana/TR_a"));
    assert!(shares.renderer_held("bruno/TR_b"));

    shares.apply(ended("ana/TR_a", 1));
    shares.apply(ended("bruno/TR_b", 2));
    assert!(keys(&shares).is_empty());
    assert!(shares.detached_tile().is_none());
    assert!(!shares.renderer_held("ana/TR_a"));
    assert!(!shares.renderer_held("bruno/TR_b"));

    shares.apply(picture_at(&slots, "ana/TR_a", "ana", 1));
    shares.apply(picture_at(&slots, "bruno/TR_b", "bruno", 2));
    assert!(keys(&shares).is_empty());
    assert!(shares.detached_tile().is_none());
}
