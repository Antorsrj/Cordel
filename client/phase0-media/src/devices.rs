//! Audio device selection and hot-swap.
//!
//! ADR-103 requires microphone selection and device hotplug to be normal runtime
//! events rather than crashes, and `architecture.md` requires OS behavior to stay
//! behind an interface. The LiveKit Rust SDK already exposes a platform audio
//! module that covers enumeration, selection and switching while recording, so
//! this trait is a deliberately thin mirror of it: the shared layer talks to
//! [`AudioDeviceControl`], and the `sdk` module maps it onto the SDK.

use std::fmt;

/// Microphone or speaker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Recording,
    Playout,
}

/// A device the user can pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Index in the current list; may change when devices are added or removed.
    pub index: usize,
    /// Stable identifier (GUID) to persist and to select by.
    pub id: String,
    pub name: String,
    pub direction: Direction,
}

#[derive(Debug)]
pub enum DeviceError {
    /// No audio backend in this build or on this host.
    Unavailable(String),
    UnknownDevice(String),
    /// The audio layer rejected the operation (device busy, stream error, ...).
    Audio(String),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceError::Unavailable(why) => write!(f, "audio unavailable: {why}"),
            DeviceError::UnknownDevice(id) => write!(f, "unknown audio device {id}"),
            DeviceError::Audio(why) => write!(f, "audio error: {why}"),
        }
    }
}

impl std::error::Error for DeviceError {}

/// Device control used by the client. Mirrors `livekit::PlatformAudio` where the
/// SDK provides the behavior.
pub trait AudioDeviceControl: Send {
    fn backend_name(&self) -> &'static str;

    fn recording_devices(&self) -> Result<Vec<AudioDevice>, DeviceError>;
    fn playout_devices(&self) -> Result<Vec<AudioDevice>, DeviceError>;

    /// Selects the microphone used by the *next* capture start.
    fn set_recording_device(&mut self, id: &str) -> Result<(), DeviceError>;

    /// Switches the microphone while a capture is already running. ADR-103 calls
    /// hot-swap a normal event, so it gets its own operation instead of relying on
    /// stop/start, which would drop audio.
    fn switch_recording_device(&mut self, id: &str) -> Result<(), DeviceError>;

    fn set_playout_device(&mut self, id: &str) -> Result<(), DeviceError>;
    fn switch_playout_device(&mut self, id: &str) -> Result<(), DeviceError>;

    fn start_recording(&mut self) -> Result<(), DeviceError>;
    fn stop_recording(&mut self) -> Result<(), DeviceError>;
}

/// Honest placeholder for builds without the SDK feature.
pub struct UnavailableAudio {
    reason: String,
}

impl UnavailableAudio {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn fail(&self) -> DeviceError {
        DeviceError::Unavailable(self.reason.clone())
    }
}

impl AudioDeviceControl for UnavailableAudio {
    fn backend_name(&self) -> &'static str {
        "unavailable"
    }

    fn recording_devices(&self) -> Result<Vec<AudioDevice>, DeviceError> {
        Err(self.fail())
    }

    fn playout_devices(&self) -> Result<Vec<AudioDevice>, DeviceError> {
        Err(self.fail())
    }

    fn set_recording_device(&mut self, _id: &str) -> Result<(), DeviceError> {
        Err(self.fail())
    }

    fn switch_recording_device(&mut self, _id: &str) -> Result<(), DeviceError> {
        Err(self.fail())
    }

    fn set_playout_device(&mut self, _id: &str) -> Result<(), DeviceError> {
        Err(self.fail())
    }

    fn switch_playout_device(&mut self, _id: &str) -> Result<(), DeviceError> {
        Err(self.fail())
    }

    fn start_recording(&mut self) -> Result<(), DeviceError> {
        Err(self.fail())
    }

    fn stop_recording(&mut self) -> Result<(), DeviceError> {
        Err(self.fail())
    }
}
