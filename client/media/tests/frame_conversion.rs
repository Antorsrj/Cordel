//! Pixel conversion and frame validation, verified without any hardware.

use cordel_media::frame::{
    fit_display, resize_bgra, synthetic_bgra, Frame, FrameError, PixelFormat,
};

fn rgba(frame: &Frame) -> Vec<u8> {
    frame.to_rgba8()
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * width + x) * 4) as usize;
    [
        rgba[index],
        rgba[index + 1],
        rgba[index + 2],
        rgba[index + 3],
    ]
}

#[test]
fn bgra_swap_reverses_red_and_blue() {
    // Two padded rows: 2 pixels of data and 4 bytes of padding each.
    let stride = 2 * 4 + 4;
    let mut data = vec![0xAAu8; stride * 2];
    for row in 0..2 {
        let start = row * stride;
        data[start..start + 8].copy_from_slice(&[1, 2, 3, 255, 4, 5, 6, 255]);
    }
    let frame = Frame::new(2, 2, stride as u32, PixelFormat::Bgra8, data).expect("valid frame");

    let converted = rgba(&frame);
    assert_eq!(converted.len(), 2 * 2 * 4);
    assert_eq!(pixel(&converted, 2, 0, 0), [3, 2, 1, 255]);
    assert_eq!(pixel(&converted, 2, 1, 0), [6, 5, 4, 255]);
    // The padding bytes (0xAA) must not leak into the result.
    assert_eq!(pixel(&converted, 2, 0, 1), [3, 2, 1, 255]);
    assert_eq!(pixel(&converted, 2, 1, 1), [6, 5, 4, 255]);
}

#[test]
fn rgba_passthrough_drops_the_padding() {
    let mut data = vec![0u8; 2 * 4];
    data[0..4].copy_from_slice(&[9, 8, 7, 6]);
    data[4..8].copy_from_slice(&[5, 4, 3, 2]);
    let frame = Frame::new(2, 1, 2 * 4, PixelFormat::Rgba8, data).expect("valid frame");
    assert_eq!(rgba(&frame), vec![9, 8, 7, 6, 5, 4, 3, 2]);
}

fn i420(width: u32, height: u32, y: u8, u: u8, v: u8) -> Frame {
    let chroma = width.div_ceil(2) as usize * height.div_ceil(2) as usize;
    let mut data = vec![y; width as usize * height as usize];
    data.extend(std::iter::repeat_n(u, chroma));
    data.extend(std::iter::repeat_n(v, chroma));
    Frame::packed(width, height, PixelFormat::I420, data).expect("valid i420 frame")
}

#[test]
fn i420_black_and_white_map_to_the_ends_of_the_range() {
    let black = rgba(&i420(2, 2, 16, 128, 128));
    assert_eq!(pixel(&black, 2, 0, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&black, 2, 1, 1), [0, 0, 0, 255]);

    let white = rgba(&i420(2, 2, 235, 128, 128));
    assert_eq!(pixel(&white, 2, 0, 0), [255, 255, 255, 255]);
}

#[test]
fn i420_colour_survives_the_round_trip_through_the_ui_layout() {
    // BT.601 limited-range values for pure red.
    let red = rgba(&i420(2, 2, 82, 90, 240));
    let [r, g, b, a] = pixel(&red, 2, 0, 0);
    assert!(r >= 250, "red channel was {r}");
    assert!(g <= 6, "green channel was {g}");
    assert!(b <= 6, "blue channel was {b}");
    assert_eq!(a, 255);
}

/// A remote screen is I420. `resize_bgra` cannot read it and returns the
/// original, which is the 2560x1440 picture that cost about 135 ms to paint.
/// Four of those in one drain are the 545 ms warnings from issue #25.
#[test]
fn a_remote_screen_is_shrunk_before_the_interface_converts_it() {
    let frame = i420(2560, 1440, 16, 128, 128);
    let untouched = resize_bgra(frame.clone(), 960);
    assert_eq!(
        untouched.width(),
        2560,
        "resize_bgra leaves an I420 frame at the captured size"
    );
    let fitted = fit_display(frame, 960);
    assert!(fitted.width() <= 960);
    assert_eq!(fitted.width() % 2, 0);
    assert_eq!(fitted.height() % 2, 0);
    assert_eq!(fitted.format(), PixelFormat::I420);
    assert!(fitted.pixels() <= 960 * 540);
    let rgba = rgba(&fitted);
    assert_eq!(pixel(&rgba, fitted.width(), 0, 0), [0, 0, 0, 255]);
}

#[test]
fn i420_handles_odd_dimensions() {
    // 3x3 has chroma planes of 2x2, which is where naive code reads out of bounds.
    let frame = i420(3, 3, 235, 128, 128);
    assert_eq!(frame.data().len(), 9 + 4 + 4);
    let rgba = rgba(&frame);
    assert_eq!(rgba.len(), 3 * 3 * 4);
    assert_eq!(pixel(&rgba, 3, 2, 2), [255, 255, 255, 255]);
}

#[test]
fn write_rgba8_refuses_a_wrong_sized_buffer() {
    let frame = synthetic_bgra(4, 4, 0);
    let mut short = vec![0u8; 16];
    assert_eq!(
        frame.write_rgba8(&mut short),
        Err(FrameError::LengthMismatch {
            expected: 64,
            actual: 16
        })
    );
}

#[test]
fn frame_rejects_impossible_layouts() {
    assert_eq!(
        Frame::new(0, 4, 16, PixelFormat::Rgba8, vec![0; 64]),
        Err(FrameError::ZeroDimension {
            width: 0,
            height: 4
        })
    );
    assert_eq!(
        Frame::new(4, 4, 8, PixelFormat::Rgba8, vec![0; 8 * 4]),
        Err(FrameError::StrideTooSmall {
            stride: 8,
            minimum: 16
        })
    );
    assert_eq!(
        Frame::new(4, 4, 16, PixelFormat::Rgba8, vec![0; 10]),
        Err(FrameError::LengthMismatch {
            expected: 64,
            actual: 10
        })
    );
}

#[test]
fn converting_into_a_reused_buffer_matches_a_fresh_conversion() {
    let frame = synthetic_bgra(8, 4, 7);
    let fresh = frame.to_rgba8();

    let mut reused = vec![0xAAu8; 4096];
    frame.to_rgba8_into(&mut reused);
    assert_eq!(reused.len(), fresh.len());
    assert_eq!(
        reused, fresh,
        "reusing a larger buffer must not change the result"
    );
}

#[test]
fn synthetic_frames_are_actually_different() {
    let first = synthetic_bgra(8, 8, 0).to_rgba8();
    let second = synthetic_bgra(8, 8, 1).to_rgba8();
    assert_ne!(first, second);
}
