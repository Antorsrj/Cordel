use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context};
use livekit::webrtc::desktop_capturer::{
    CaptureError, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions, DesktopFrame,
};

use crate::pixels::{bgra_to_i420, bgra_to_rgba, downscale_rgba, I420Planes};

pub const PREVIEW_MAX_WIDTH: usize = 960;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePath {
    WaylandPortal,
    X11,
    Unknown,
}

impl CapturePath {
    pub fn detect() -> Self {
        match std::env::var("XDG_SESSION_TYPE").ok().as_deref() {
            Some("wayland") => Self::WaylandPortal,
            Some("x11") => Self::X11,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WaylandPortal => "wayland-portal",
            Self::X11 => "x11",
            Self::Unknown => "unknown",
        }
    }

    fn source_type(self) -> DesktopCaptureSourceType {
        match self {
            Self::WaylandPortal => DesktopCaptureSourceType::Generic,
            Self::X11 | Self::Unknown => DesktopCaptureSourceType::Screen,
        }
    }
}

pub struct CapturedFrame {
    pub path: CapturePath,
    pub planes: I420Planes,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    /// Capturer buffer, I420 planes, then the preview RGBA buffer.
    pub copy_count: u32,
}

pub fn capture_one_frame(path: CapturePath) -> anyhow::Result<CapturedFrame> {
    let (tx, rx) = mpsc::channel();
    let source_type = path.source_type();
    thread::spawn(move || {
        let result = capture_on_thread(source_type);
        let _ = tx.send(result);
    });

    rx.recv_timeout(Duration::from_secs(120))
        .context("screen capture timed out waiting for a frame or the portal dialog")?
        .map(|raw| finish_frame(path, raw))
}

struct RawFrame {
    width: usize,
    height: usize,
    bgra: Vec<u8>,
}

fn finish_frame(path: CapturePath, raw: RawFrame) -> CapturedFrame {
    let rgba = bgra_to_rgba(&raw.bgra, raw.width * 4, raw.width, raw.height);
    let (preview_rgba, preview_width, preview_height) =
        downscale_rgba(&rgba, raw.width, raw.height, PREVIEW_MAX_WIDTH);
    let planes = bgra_to_i420(&raw.bgra, raw.width * 4, raw.width, raw.height);
    CapturedFrame {
        path,
        planes,
        preview_rgba,
        preview_width: preview_width as u32,
        preview_height: preview_height as u32,
        copy_count: 3,
    }
}

fn capture_on_thread(source_type: DesktopCaptureSourceType) -> anyhow::Result<RawFrame> {
    let (frame_tx, frame_rx) = mpsc::channel();
    let callback = move |result: Result<DesktopFrame, CaptureError>| {
        let frame = match result {
            Ok(frame) => frame,
            Err(CaptureError::Temporary) => return,
            Err(CaptureError::Permanent) => {
                let _ = frame_tx.send(Err(anyhow!("screen capture failed permanently")));
                return;
            }
        };
        let width = frame.width();
        let height = frame.height();
        if width <= 1 || height <= 1 {
            return;
        }
        let stride = frame.stride() as usize;
        let data = frame.data();
        let row_bytes = width as usize * 4;
        if stride < row_bytes {
            let _ = frame_tx.send(Err(anyhow!(
                "capture stride {stride} is shorter than a BGRA row"
            )));
            return;
        }
        let mut bgra = Vec::with_capacity(row_bytes * height as usize);
        for row in 0..height as usize {
            let start = row * stride;
            bgra.extend_from_slice(&data[start..start + row_bytes]);
        }
        let _ = frame_tx.send(Ok(RawFrame {
            width: width as usize,
            height: height as usize,
            bgra,
        }));
    };

    let mut options = DesktopCapturerOptions::new(source_type);
    options.set_include_cursor(false);
    let mut capturer = DesktopCapturer::new(options).context("desktop capturer was not created")?;
    let selected = if source_type == DesktopCaptureSourceType::Generic {
        None
    } else {
        capturer.get_source_list().into_iter().next()
    };
    capturer.start_capture(selected, callback);

    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        capturer.capture_frame();
        match frame_rx.recv_timeout(Duration::from_millis(16)) {
            Ok(frame) => return frame,
            Err(RecvTimeoutError::Timeout) if std::time::Instant::now() < deadline => {}
            Err(RecvTimeoutError::Timeout) => {
                return Err(anyhow!(
                    "no screen frame arrived; the portal or X11 source was not confirmed"
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("screen capture stopped before a frame"));
            }
        }
    }
}
