//! The decoded-frame to Slint-image path (Phase 0 spike B).
//!
//! ADR-103's gate needs to know whether a received screen frame can be shown by
//! the product UI without making the rest of the interface stutter. That is a
//! question about per-frame cost, so this module **measures** it instead of
//! asserting it: [`SlintFrameSink`] records color conversion and image
//! construction time per frame and exposes them through [`RenderStats`].
//!
//! Building a [`slint::Image`] needs no window and no display backend, so this
//! path is measurable headlessly. Putting the image on an actual window is a
//! separate step and is reported separately in `docs/spikes/media-phase0.md`.

use crate::frame::{Frame, FrameError};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::time::{Duration, Instant};

/// Aggregated cost of presenting frames.
///
/// Three buckets, because in a real client they are three different decisions:
/// bringing the pixels into the UI layout, handing the buffer to the toolkit, and
/// retiring the previous frame. The third one is easy to forget and shows up as
/// unexplained time in a naive measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    pub frames: u64,
    pub pixels: u64,
    pub bytes: u64,
    /// Bringing pixels into the layout the UI reads.
    pub convert: Duration,
    /// Building the value handed to the UI. Nearly free here because the buffer is
    /// moved rather than copied; a windowed renderer adds its own GPU upload at
    /// draw time, which this headless measurement does not include.
    pub handoff: Duration,
    /// Dropping the previous frame once the new one takes its place.
    pub retire: Duration,
    pub slowest_frame: Duration,
}

impl RenderStats {
    pub fn total(&self) -> Duration {
        self.convert + self.handoff + self.retire
    }

    fn mean(total: Duration, frames: u64) -> Option<Duration> {
        (frames > 0).then(|| total / frames as u32)
    }

    pub fn mean_frame(&self) -> Option<Duration> {
        Self::mean(self.total(), self.frames)
    }

    pub fn mean_convert(&self) -> Option<Duration> {
        Self::mean(self.convert, self.frames)
    }

    pub fn mean_handoff(&self) -> Option<Duration> {
        Self::mean(self.handoff, self.frames)
    }

    pub fn mean_retire(&self) -> Option<Duration> {
        Self::mean(self.retire, self.frames)
    }

    /// Throughput of the whole path in megapixels per second.
    pub fn megapixels_per_second(&self) -> Option<f64> {
        let secs = self.total().as_secs_f64();
        (secs > 0.0).then(|| self.pixels as f64 / secs / 1_000_000.0)
    }

    /// Human-readable one-liner for the probe output.
    pub fn summary(&self) -> String {
        let ms = |d: Option<Duration>| match d {
            Some(d) => format!("{:.3} ms", d.as_secs_f64() * 1000.0),
            None => "n/a".to_string(),
        };
        format!(
            "{} frames, {:.1} MP total | mean per frame {} (convert {}, handoff {}, retire {}) | slowest {} | {:.1} MP/s",
            self.frames,
            self.pixels as f64 / 1_000_000.0,
            ms(self.mean_frame()),
            ms(self.mean_convert()),
            ms(self.mean_handoff()),
            ms(self.mean_retire()),
            ms(Some(self.slowest_frame)),
            self.megapixels_per_second().unwrap_or(0.0),
        )
    }
}

/// Where frames go once they arrive.
///
/// A real client would route this into a Slint property; the spike keeps the
/// interface so the receive loop, the conversion and the panel are separable and
/// each one can be measured and tested on its own.
pub trait FrameSink {
    fn present(&mut self, frame: &Frame) -> Result<(), FrameError>;
    fn stats(&self) -> RenderStats;
    /// Last image handed to the UI, if any.
    fn image(&self) -> Option<Image>;
}

/// [`FrameSink`] backed by a Slint image, the way the client would show a remote
/// share.
///
/// Cost per frame is two steps, deliberately kept visible in [`RenderStats`]:
/// a color conversion straight into the pixel buffer, then the image the toolkit
/// reads. Note that a fresh pixel buffer per frame is the *pessimistic* case; a
/// client that keeps one panel and overwrites it pays the convert step only.
#[derive(Default)]
pub struct SlintFrameSink {
    stats: RenderStats,
    image: Option<Image>,
}

impl SlintFrameSink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FrameSink for SlintFrameSink {
    fn present(&mut self, frame: &Frame) -> Result<(), FrameError> {
        let started = Instant::now();

        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(frame.width(), frame.height());
        frame.write_rgba8(buffer.make_mut_bytes())?;
        let converted = Instant::now();

        let image = Image::from_rgba8(buffer);
        let handed_off = Instant::now();

        // Retiring the previous frame is a real cost and belongs in the numbers:
        // a client that drops the old image on every frame pays it every frame.
        self.image = Some(image);
        let retired = Instant::now();

        let frame_time = retired - started;
        self.stats.frames += 1;
        self.stats.pixels += frame.pixels();
        self.stats.bytes += frame.rgba8_len() as u64;
        self.stats.convert += converted - started;
        self.stats.handoff += handed_off - converted;
        self.stats.retire += retired - handed_off;
        if frame_time > self.stats.slowest_frame {
            self.stats.slowest_frame = frame_time;
        }
        Ok(())
    }

    fn stats(&self) -> RenderStats {
        self.stats
    }

    fn image(&self) -> Option<Image> {
        self.image.clone()
    }
}
