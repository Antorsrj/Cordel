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
