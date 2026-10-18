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
    /// Identifier (GUID) to persist and to select by.
    ///
    /// Not guaranteed to be usable: WebRTC's PulseAudio ADM on Linux leaves the
    /// GUID empty for every device, which leaves that backend able to address
    /// only its default entry. Check for an empty `id` before offering a device
    /// as a persistable choice; see `sdk::SdkAudio`.
    pub id: String,
    pub name: String,
    pub direction: Direction,
}

/// The device to use when the caller has no stored preference.
///
/// Backends put their system default first, and WebRTC's PulseAudio ADM on Linux
/// builds its list that way by construction: entry 0 is "default", followed by
/// one entry per source. Preferring index 0 keeps that assumption in one place,
/// with the first entry as the fallback for a backend that orders its list
/// differently.
pub fn default_device(devices: &[AudioDevice]) -> Option<&AudioDevice> {
    devices
        .iter()
        .find(|device| device.index == 0)
        .or_else(|| devices.first())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn device(index: usize, id: &str) -> AudioDevice {
        AudioDevice {
            index,
            id: id.to_string(),
            name: format!("device {index}"),
            direction: Direction::Recording,
        }
    }

    #[test]
    fn default_device_prefers_the_entry_the_backend_calls_default() {
        // The PulseAudio ADM lists "default" first; ordering the list is not
        // enough, because the index is what identifies it.
        let devices = [device(1, ""), device(0, ""), device(2, "")];
        assert_eq!(default_device(&devices).map(|d| d.index), Some(0));
    }

    #[test]
    fn default_device_falls_back_to_the_first_entry() {
        let devices = [device(3, "c"), device(4, "d")];
        assert_eq!(default_device(&devices).map(|d| d.index), Some(3));
        assert!(default_device(&[]).is_none());
    }
}
