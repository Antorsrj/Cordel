use super::events::announce;
use super::*;

fn stage() -> (Rc<Shares>, FrameSlots) {
    let slots = FrameSlots::default();
    (Shares::new(slots.clone()), slots)
}

fn frame_event(slots: &FrameSlots, key: &str, publisher: &str, phase: u8) -> MediaEvent {
    announce(
        slots,
        key,
        publisher,
        ShareState::Live,
        cordel_media::frame::synthetic_bgra(320, 180, phase),
    )
    .expect("the first picture of a share is announced")
}

fn stage_keys(shares: &Shares) -> Vec<String> {
    (0..shares.model().row_count())
        .filter_map(|i| shares.model().row_data(i))
        .map(|t| t.key.to_string())
        .collect()
}

#[test]
fn a_new_share_from_the_same_publisher_replaces_their_previous_tile() {
    let (shares, slots) = stage();
    shares.apply(frame_event(&slots, "ana/TR_1", "ana", 0));
    shares.apply(frame_event(&slots, "bruno/TR_1", "bruno", 200));
    assert_eq!(stage_keys(&shares), ["ana/TR_1", "bruno/TR_1"]);

    // Starting a new share gives the same publisher a new track key; it
    // replaces their existing tile instead of stacking a second one.
    shares.apply(frame_event(&slots, "ana/TR_2", "ana", 40));
    assert_eq!(stage_keys(&shares), ["ana/TR_2", "bruno/TR_1"]);
    assert!(shares
        .apply(MediaEvent::Ended {
            key: "ana/TR_1".into()
        })
        .is_none());
    assert_eq!(stage_keys(&shares), ["ana/TR_2", "bruno/TR_1"]);

    // Replacing while detached updates the detached tile in place too.
    shares.detach("ana/TR_2");
    shares.apply(frame_event(&slots, "ana/TR_3", "ana", 80));
    assert_eq!(stage_keys(&shares), ["bruno/TR_1"]);
    assert_eq!(
        shares.detached_tile().map(|t| t.key.to_string()),
        Some("ana/TR_3".into())
    );
    assert_eq!(
        shares.apply(MediaEvent::Ended {
            key: "ana/TR_3".into()
        }),
        Some("ana stopped sharing.".into())
    );
    assert!(shares.detached_tile().is_none());
}

#[test]
fn stopping_and_restarting_a_share_never_accumulates_duplicate_tiles() {
    let (shares, slots) = stage();
    for round in 1..=4u8 {
        let key = format!("ana/TR_{round}");
        shares.apply(frame_event(&slots, &key, "ana", round * 20));
        assert_eq!(
            stage_keys(&shares),
            [key.clone()],
            "spectator sees exactly 1 share for the participant on round {round}"
        );
        assert_eq!(
            shares.apply(MediaEvent::Ended { key }),
            Some("ana stopped sharing.".into())
        );
        assert!(
            stage_keys(&shares).is_empty(),
            "tile disappears when the participant stops sharing"
        );
    }
}
