//! The frame -> Slint image path, checked headlessly (no window, no display).
#![cfg(feature = "ui")]

use cordel_media::frame::{synthetic_bgra, Frame, PixelFormat};
use cordel_media::render::{FrameSink, SlintFrameSink};

#[test]
fn presenting_frames_produces_an_image_and_additive_stats() {
    let mut sink = SlintFrameSink::new();
    for index in 0..5 {
        sink.present(&synthetic_bgra(64, 48, index))
            .expect("presenting a valid frame");
    }

    let stats = sink.stats();
    assert_eq!(stats.frames, 5);
    assert_eq!(stats.pixels, 5 * 64 * 48);
    assert_eq!(stats.bytes, 5 * 64 * 48 * 4);
    assert!(sink.image().is_some(), "the UI must have something to draw");
    assert_eq!(stats.total(), stats.convert + stats.handoff + stats.retire);
    assert!(stats
        .mean_frame()
        .is_some_and(|mean| mean > std::time::Duration::ZERO));
}

#[test]
fn the_decoder_layout_reaches_the_ui_too() {
    // Same path a received (I420) frame takes, so a regression in the receive side
    // shows up as a failing test rather than as a black panel.
    let width = 16;
    let height = 8;
    let chroma = (width / 2) as usize * (height / 2) as usize;
    let mut data = vec![200u8; (width * height) as usize];
    data.extend(std::iter::repeat_n(90u8, chroma));
    data.extend(std::iter::repeat_n(240u8, chroma));
    let frame = Frame::packed(width, height, PixelFormat::I420, data).expect("valid frame");

    let mut sink = SlintFrameSink::new();
    sink.present(&frame).expect("presenting a decoded frame");
    assert_eq!(sink.stats().frames, 1);
    assert!(sink.image().is_some());

    let rgba = frame.to_rgba8();
    assert!(
        rgba[0] > 200,
        "the frame should be mostly red, got {}",
        rgba[0]
    );
    assert_eq!(rgba[3], 255);
}
