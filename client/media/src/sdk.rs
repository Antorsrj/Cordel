//! LiveKit Rust SDK adapters (feature `sdk`).
//!
//! Everything OS- and network-specific lives here, and everything here is a thin
//! mapping onto the `livekit` 0.9 SDK: the shared layers see only
//! [`AudioDeviceControl`] and [`ScreenCapture`], never a WebRTC type.
//!
//! This module is compiled only with `--features sdk`, because the SDK builds
//! WebRTC's C++ sources. `docs/spikes/media-phase0.md` records what the toolchain
//! required on Linux and which parts of this module were actually exercised.
//!
//! Two design notes that matter for the product, not just for the spike:
//!
//! 1. **Microphone capture is driven by the SDK's platform audio module**
//!    (`RtcAudioSource::Device`), which is also where device enumeration and
//!    live switching live. Cordel therefore does not need its own audio device
//!    layer on Linux; [`SdkAudio`] is the whole of it.
//! 2. **Video capture is pull-based.** WebRTC's desktop capturer only produces a
//!    frame when the caller asks, so [`SdkDesktopCapture::tick`] exists and the
//!    client must drive it from the same loop that renders. A portal/PipeWire
//!    backend will instead push frames on its own thread; the interface says so
//!    rather than hiding the difference.

use std::fmt;

use livekit::options::TrackPublishOptions;
use livekit::prelude::{
    LocalAudioTrack, LocalTrack, LocalTrackPublication, LocalVideoTrack, RemoteVideoTrack,
    TrackSource,
};
use livekit::webrtc::audio_source::RtcAudioSource;
use livekit::webrtc::desktop_capturer::{
    DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
};
use livekit::webrtc::prelude::*;
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use livekit::{
    AudioProcessingOptions, PlatformAudio, PlayoutDeviceId, RecordingDeviceId, Room, RoomError,
};

use crate::capture::{
    CaptureError, CaptureSource, CaptureTarget, FrameCallback, ScreenCapture, SessionKind,
    TargetKind,
};
use crate::devices::{AudioDevice, AudioDeviceControl, DeviceError, Direction};
use crate::frame::{Frame, FrameError, PixelFormat};

fn device_err(context: &str, error: impl fmt::Debug) -> DeviceError {
    DeviceError::Audio(format!("{context}: {error:?}"))
}

/// Microphone and speaker selection backed by the SDK's platform audio module.
///
/// Kept behind the shared trait so the client code and its tests never mention
/// `PlatformAudio`, and so a second platform can supply its own implementation.
pub struct SdkAudio {
    audio: PlatformAudio,
}

impl SdkAudio {
    pub fn new() -> Result<Self, DeviceError> {
        PlatformAudio::new()
            .map(|audio| Self { audio })
            .map_err(|e| device_err("initialising platform audio", e))
    }

    /// Whether the platform can do echo cancellation, gain control and noise
    /// suppression in hardware rather than in the WebRTC audio pipeline.
    pub fn hardware_processing_available(&self) -> (bool, bool, bool) {
        (
            self.audio.is_hardware_aec_available(),
            self.audio.is_hardware_agc_available(),
            self.audio.is_hardware_ns_available(),
        )
    }

    /// Chooses between hardware and software echo cancellation / gain / noise
    /// suppression. A voice call needs these; a screen-share-only session does not.
    ///
    /// This is deliberately not on [`AudioDeviceControl`]: ADR-103 does not mention
    /// audio processing at all, which is a gap the spike report records.
    pub fn configure_processing(&mut self, prefer_hardware: bool) -> Result<(), DeviceError> {
        self.audio
            .configure_audio_processing(AudioProcessingOptions {
                echo_cancellation: true,
                noise_suppression: true,
                auto_gain_control: true,
                prefer_hardware_processing: prefer_hardware,
            })
            .map_err(|e| device_err("configuring audio processing", e))
    }
}

impl AudioDeviceControl for SdkAudio {
    fn backend_name(&self) -> &'static str {
        "livekit-platform-audio"
    }

    fn recording_devices(&self) -> Result<Vec<AudioDevice>, DeviceError> {
        Ok(self
            .audio
            .recording_devices()
            .map(|d| AudioDevice {
                index: d.index,
                id: d.id.as_str().to_string(),
                name: d.name,
                direction: Direction::Recording,
            })
            .collect())
    }

    fn playout_devices(&self) -> Result<Vec<AudioDevice>, DeviceError> {
        Ok(self
            .audio
            .playout_devices()
            .map(|d| AudioDevice {
                index: d.index,
                id: d.id.as_str().to_string(),
                name: d.name,
                direction: Direction::Playout,
            })
            .collect())
    }

    fn set_recording_device(&mut self, id: &str) -> Result<(), DeviceError> {
        // `from_unchecked_guid` is how the SDK documents building an id back from
        // a persisted string; the setter validates that the device exists.
        self.audio
            .set_recording_device(&RecordingDeviceId::from_unchecked_guid(id))
            .map_err(|e| device_err("selecting recording device", e))
    }

    fn switch_recording_device(&mut self, id: &str) -> Result<(), DeviceError> {
        self.audio
            .switch_recording_device(&RecordingDeviceId::from_unchecked_guid(id))
            .map_err(|e| device_err("switching recording device", e))
    }

    fn set_playout_device(&mut self, id: &str) -> Result<(), DeviceError> {
        self.audio
            .set_playout_device(&PlayoutDeviceId::from_unchecked_guid(id))
            .map_err(|e| device_err("selecting playout device", e))
    }

    fn switch_playout_device(&mut self, id: &str) -> Result<(), DeviceError> {
        self.audio
            .switch_playout_device(&PlayoutDeviceId::from_unchecked_guid(id))
            .map_err(|e| device_err("switching playout device", e))
    }

    fn start_recording(&mut self) -> Result<(), DeviceError> {
        self.audio
            .start_recording()
            .map_err(|e| device_err("starting microphone capture", e))
    }

    fn stop_recording(&mut self) -> Result<(), DeviceError> {
        self.audio
            .stop_recording()
            .map_err(|e| device_err("stopping microphone capture", e))
    }
}

/// Screen or window capture through WebRTC's desktop capturer.
///
/// On Linux the capturer's backend is whatever `webrtc-sys` was built with, and
/// the `DesktopCaptureSourceType` request selects between two different code
/// paths: `Generic` is the portal-aware capturer (the XDG ScreenCast portal and
/// PipeWire), while `Screen` and `Window` select the direct X11 capturers. The
/// spike report records which path this machine was actually driven down.
pub struct SdkDesktopCapture {
    capturer: DesktopCapturer,
    kind: TargetKind,
    source: CaptureSource,
    glib_pump: bool,
    started: bool,
}

impl SdkDesktopCapture {
    /// Session-appropriate default: the portal on Wayland, the direct capturer
    /// otherwise. Use [`Self::with_source`] to force one path.
    pub fn new(kind: TargetKind) -> Result<Self, CaptureError> {
        Self::with_source(kind, CaptureSource::Auto)
    }

    /// Requests a specific capture path.
    ///
    /// Requesting the portal on an X11-only session, or the X11 capturer on a
    /// Wayland session, is expected to fail rather than silently fall back: the
    /// caller needs to know which path it really got, because that is the whole
    /// distinction the Phase 0 gate is asking about.
    pub fn with_source(kind: TargetKind, source: CaptureSource) -> Result<Self, CaptureError> {
        let source = source.resolve(SessionKind::detect());
        let source_type = match source {
            // `Auto` was resolved just above; it is grouped here only to keep the
            // match total.
            CaptureSource::Auto | CaptureSource::Portal => DesktopCaptureSourceType::Generic,
            CaptureSource::Screen => DesktopCaptureSourceType::Screen,
            CaptureSource::Window => DesktopCaptureSourceType::Window,
        };
        let mut options = DesktopCapturerOptions::new(source_type);
        options.set_include_cursor(true);
        DesktopCapturer::new(options)
            .map(|capturer| Self {
                capturer,
                kind,
                source,
                glib_pump: true,
                started: false,
            })
            .ok_or_else(|| {
                CaptureError::Unavailable(
                    "WebRTC could not create a desktop capturer for this session; \
                     on Wayland the desktop portal has to be installed and running"
                        .to_string(),
                )
            })
    }

    /// The capture path actually requested, after `Auto` was resolved.
    pub fn source(&self) -> CaptureSource {
        self.source
    }

    /// Turns the GLib main-context pump off, which reproduces the stall that
    /// `docs/spikes/media-phase0.md` records. It exists for that demonstration;
    /// leave it on.
    pub fn set_glib_pump(&mut self, enabled: bool) {
        self.glib_pump = enabled;
    }

    /// The capturer's own view of what is available, before any filtering.
    pub fn source_count(&self) -> usize {
        self.capturer.get_source_list().len()
    }
}

impl ScreenCapture for SdkDesktopCapture {
    fn backend_name(&self) -> &'static str {
        "libwebrtc-desktop-capturer"
    }

    fn targets(&self) -> Result<Vec<CaptureTarget>, CaptureError> {
        let kind = self.kind;
        Ok(self
            .capturer
            .get_source_list()
            .into_iter()
            .map(|s| CaptureTarget::new(s.id(), s.title(), kind))
            .collect())
    }

    fn start(
        &mut self,
        target: &CaptureTarget,
        on_frame: FrameCallback,
    ) -> Result<(), CaptureError> {
        // On the portal path the picker chooses the surface, and the source list
        // can be empty until a grant exists, so there is no id to select here:
        // passing one would fail before the portal is ever asked. `None` starts
        // the capturer's own session and lets the portal decide.
        let selected = if self.source == CaptureSource::Portal {
            None
        } else {
            Some(
                self.capturer
                    .get_source_list()
                    .into_iter()
                    .find(|s| s.id() == target.id)
                    .ok_or(CaptureError::UnknownTarget(target.id))?,
            )
        };

        let mut on_frame = on_frame;
        self.capturer
            .start_capture(selected, move |result| match result {
                Ok(frame) => {
                    let (width, height) =
                        (frame.width().max(0) as u32, frame.height().max(0) as u32);
                    match Frame::new(
                        width,
                        height,
                        frame.stride(),
                        PixelFormat::Bgra8,
                        frame.data().to_vec(),
                    ) {
                        Ok(frame) => on_frame(frame),
                        Err(error) => eprintln!("media probe: captured frame rejected ({error})"),
                    }
                }
                Err(error) => eprintln!("media probe: capture error ({error:?})"),
            });
        self.started = true;
        Ok(())
    }

    fn tick(&mut self) -> Result<(), CaptureError> {
        if self.started {
            // See `pump_glib_main_context`: on Linux the portal handshake only
            // progresses while someone iterates the GLib main context, and the
            // host loop that calls `tick` is where that belongs.
            if self.glib_pump {
                pump_glib_main_context();
            }
            // This is the call that actually asks WebRTC for a frame; the callback
            // handed to `start` runs synchronously from here on this thread.
            self.capturer.capture_frame();
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        // WebRTC keeps the session until the capturer is dropped, so this only
        // stops delivering frames. Said plainly because it surprises people.
        self.started = false;
        Ok(())
    }
}

/// Publishes captured frames as a screen-share track.
pub struct ScreenPublisher {
    source: NativeVideoSource,
    track: LocalVideoTrack,
    width: u32,
    height: u32,
}

impl ScreenPublisher {
    /// `width`/`height` must match the frames pushed later.
    pub fn new(width: u32, height: u32, name: &str) -> Self {
        let source = NativeVideoSource::new(
            livekit::webrtc::video_source::VideoResolution { width, height },
            true,
        );
        let track =
            LocalVideoTrack::create_video_track(name, RtcVideoSource::Native(source.clone()));
        Self {
            source,
            track,
            width,
            height,
        }
    }

    /// The resolution the track was created with. WebRTC may adapt the stream
    /// after negotiation, so this is the starting point, not a guarantee.
    pub fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Publishes the track to `room` with the screen-share source label, which is
    /// what tells the other side this is a screen and not a camera.
    pub async fn publish(&self, room: &Room) -> Result<LocalTrackPublication, RoomError> {
        room.local_participant()
            .publish_track(
                LocalTrack::Video(self.track.clone()),
                TrackPublishOptions {
                    source: TrackSource::Screenshare,
                    ..Default::default()
                },
            )
            .await
    }

    /// Sends one captured frame.
    ///
    /// The captured layout is BGRA and the encoder wants I420, so this uses
    /// WebRTC's own libyuv binding rather than a hand-written loop. libyuv calls
    /// the input "ARGB" even though the bytes in memory are B,G,R,A — the name is
    /// historical and the byte order is what matters.
    ///
    /// The buffer is sized from the frame, so a capturer that changes resolution
    /// mid-share is handled here rather than rejected; WebRTC scales to the
    /// encoding layers it negotiated.
    pub fn push(&self, frame: &Frame) -> Result<(), FrameError> {
        if frame.format() != PixelFormat::Bgra8 {
            return Err(FrameError::UnexpectedFormat {
                expected: PixelFormat::Bgra8,
                actual: frame.format(),
            });
        }
        let mut buffer = I420Buffer::new(frame.width(), frame.height());
        let (stride_y, stride_u, stride_v) = buffer.strides();
        {
            let (y, u, v) = buffer.data_mut();
            livekit::webrtc::native::yuv_helper::argb_to_i420(
                frame.data(),
                frame.stride(),
                y,
                stride_y,
                u,
                stride_u,
                v,
                stride_v,
                frame.width() as i32,
                frame.height() as i32,
            );
        }
        let video_frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        self.source.capture_frame(&video_frame);
        Ok(())
    }

    pub async fn unpublish(&self, room: &Room) -> Result<LocalTrackPublication, RoomError> {
        room.local_participant()
            .unpublish_track(&self.track.sid())
            .await
    }
}

/// Publishes the microphone.
///
/// Uses the SDK's `Device` audio source, so capture runs through the platform
/// audio module selected in [`SdkAudio`]. The caller is expected to have started
/// recording first; see the gate order in [`crate::consent`].
pub async fn publish_microphone(room: &Room, name: &str) -> Result<LocalAudioTrack, RoomError> {
    let track = LocalAudioTrack::create_audio_track(name, RtcAudioSource::Device);
    room.local_participant()
        .publish_track(
            LocalTrack::Audio(track.clone()),
            TrackPublishOptions {
                source: TrackSource::Microphone,
                ..Default::default()
            },
        )
        .await?;
    Ok(track)
}

/// Turns a subscribed video track into a stream of frames.
pub fn remote_video_stream(track: &RemoteVideoTrack) -> NativeVideoStream {
    NativeVideoStream::new(track.rtc_track())
}

/// Converts a received frame into the shared [`Frame`] type.
///
/// Returns `None` for layouts this spike does not handle yet. The planes are
/// copied into a tight buffer because the SDK's strides are not guaranteed to be
/// what the conversion loop expects.
pub fn received_to_frame(frame: &BoxVideoFrame) -> Option<Frame> {
    let buffer = frame.buffer.as_ref().as_i420()?;
    let (width, height) = (buffer.width(), buffer.height());
    let (chroma_width, chroma_height) = (buffer.chroma_width(), buffer.chroma_height());
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data();

    let mut data = Vec::with_capacity(
        width as usize * height as usize + 2 * chroma_width as usize * chroma_height as usize,
    );
    let copy_plane = |data: &mut Vec<u8>, plane: &[u8], stride: u32, rows: u32, row_bytes: u32| {
        for row in 0..rows {
            let start = (row * stride) as usize;
            data.extend_from_slice(&plane[start..start + row_bytes as usize]);
        }
    };
    copy_plane(&mut data, plane_y, stride_y, height, width);
    copy_plane(&mut data, plane_u, stride_u, chroma_height, chroma_width);
    copy_plane(&mut data, plane_v, stride_v, chroma_height, chroma_width);

    Frame::packed(width, height, PixelFormat::I420, data).ok()
}

/// Pumps the default GLib main context once, without blocking.
///
/// WebRTC's portal capturer drives the XDG ScreenCast handshake over GDBus and
/// delivers every completion — `CreateSession`, `SelectSources`, `Start`,
/// `OpenPipeWireRemote` — on the thread-default GLib main context. Nothing in the
/// LiveKit SDK iterates that context, so on Linux the whole handshake is queued
/// and never sent unless the thread that owns the capturer pumps it. The visible
/// symptom is subtle: the capturer reports one placeholder source and every
/// frame request returns `ERROR_TEMPORARY`, which is the "no frame yet" path, not
/// a reported failure. `docs/spikes/media-phase0.md` records a run with and
/// without this pump.
///
/// Declared by hand because the crate has no GLib binding and the library is
/// already linked for the portal backend; see `build.rs`.
#[cfg(target_os = "linux")]
fn pump_glib_main_context() {
    use std::ffi::c_void;

    extern "C" {
        fn g_main_context_iteration(context: *mut c_void, may_block: i32) -> i32;
    }

    // A null context means the global default, which is what GDBus uses when no
    // thread-default context has been pushed.
    unsafe {
        g_main_context_iteration(std::ptr::null_mut(), 0);
    }
}

/// The portal paths on other platforms live in their own adapters.
#[cfg(not(target_os = "linux"))]
fn pump_glib_main_context() {}
