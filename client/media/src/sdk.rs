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
//!    live switching live. [`SdkAudio`] is therefore the whole Linux audio
//!    device layer, with two caveats the spike found and records: WebRTC's
//!    PulseAudio ADM refuses to initialise *either* direction until a device has
//!    been selected (see [`SdkAudio::new`]), and it reports no per-device GUIDs
//!    on Linux, so only the backend's default entry can actually be addressed.
//! 2. **Video capture is pull-based.** WebRTC's desktop capturer only produces a
//!    frame when the caller asks, so [`SdkDesktopCapture::tick`] exists and the
//!    client must drive it from the same loop that renders. A portal/PipeWire
//!    backend will instead push frames on its own thread; the interface says so
//!    rather than hiding the difference. The portal handshake that a Wayland
//!    session goes through needs the GLib main loop to run, which this crate gets
//!    from the SDK's `glib-main-loop` feature instead of driving itself.

use std::fmt;

use livekit::options::{TrackPublishOptions, VideoEncoding};
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
use crate::devices::{default_device, AudioDevice, AudioDeviceControl, DeviceError, Direction};
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
    /// Whether a microphone has been picked in this instance. WebRTC will not
    /// initialise recording without one; see [`Self::start_recording`].
    recording_device_selected: bool,
    /// Whether a speaker has been picked in this instance; see [`Self::new`].
    playout_device_selected: bool,
}

impl SdkAudio {
    /// Opens the platform audio layer and selects the system defaults.
    ///
    /// Selecting here is not cosmetic. WebRTC's PulseAudio ADM fails
    /// `InitRecording()` and `InitPlayout()` while its own "device specified"
    /// flag is unset — it does not fall back to the backend's default device —
    /// and it says so only in a C++ log line nothing collects. The visible
    /// symptoms are a microphone that refuses to open (`init_recording failed`)
    /// and remote audio that never plays, both with no explanation.
    /// `docs/spikes/media-phase0.md` records the failing and the working run.
    ///
    /// A direction with no hardware at all is not an error: a machine can have
    /// speakers and no microphone. That direction stays unselected, and opening
    /// it later fails with a message that names the missing device.
    pub fn new() -> Result<Self, DeviceError> {
        let audio =
            PlatformAudio::new().map_err(|e| device_err("initialising platform audio", e))?;
        let mut audio = Self {
            audio,
            recording_device_selected: false,
            playout_device_selected: false,
        };
        audio.recording_device_selected = audio.select_default(Direction::Recording)?;
        audio.playout_device_selected = audio.select_default(Direction::Playout)?;
        Ok(audio)
    }

    /// Which directions have a device selected, as `(microphone, speakers)`.
    ///
    /// Both should be true after [`Self::new`] on a machine that has the
    /// hardware; a `false` is why that direction would stay silent.
    pub fn devices_selected(&self) -> (bool, bool) {
        (self.recording_device_selected, self.playout_device_selected)
    }

    /// Picks the backend's default device for one direction.
    ///
    /// Returns `false` when the backend lists no device in that direction. An
    /// error means the backend refused a device it had just listed, which is a
    /// real failure rather than an empty machine.
    fn select_default(&mut self, direction: Direction) -> Result<bool, DeviceError> {
        let devices = match direction {
            Direction::Recording => self.recording_devices()?,
            Direction::Playout => self.playout_devices()?,
        };
        let Some(id) = default_device(&devices).map(|device| device.id.clone()) else {
            return Ok(false);
        };
        match direction {
            Direction::Recording => self.set_recording_device(&id)?,
            Direction::Playout => self.set_playout_device(&id)?,
        }
        Ok(true)
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
            .map_err(|e| device_err("selecting recording device", e))?;
        self.recording_device_selected = true;
        Ok(())
    }

    fn switch_recording_device(&mut self, id: &str) -> Result<(), DeviceError> {
        self.audio
            .switch_recording_device(&RecordingDeviceId::from_unchecked_guid(id))
            .map_err(|e| device_err("switching recording device", e))?;
        self.recording_device_selected = true;
        Ok(())
    }

    fn set_playout_device(&mut self, id: &str) -> Result<(), DeviceError> {
        self.audio
            .set_playout_device(&PlayoutDeviceId::from_unchecked_guid(id))
            .map_err(|e| device_err("selecting playout device", e))?;
        self.playout_device_selected = true;
        Ok(())
    }

    fn switch_playout_device(&mut self, id: &str) -> Result<(), DeviceError> {
        self.audio
            .switch_playout_device(&PlayoutDeviceId::from_unchecked_guid(id))
            .map_err(|e| device_err("switching playout device", e))?;
        self.playout_device_selected = true;
        Ok(())
    }

    fn start_recording(&mut self) -> Result<(), DeviceError> {
        // A client can reach this without a microphone ever having been picked:
        // it has no stored preference, or [`Self::new`] found no device and one
        // has been plugged in since. WebRTC refuses to initialise recording
        // without a selection, so the default is picked here rather than
        // returning the ADM's unhelpful "init_recording failed".
        if !self.recording_device_selected && !self.select_default(Direction::Recording)? {
            return Err(DeviceError::Unavailable(
                "the audio backend reports no microphone".to_string(),
            ));
        }
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

include!("sdk/capture.rs");
include!("sdk/publish.rs");
