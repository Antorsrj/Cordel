//! What the media side sends to the interface, and how it reaches it.
//!
//! The media side never touches a Slint type. It announces its pictures here
//! and the interface thread takes them. The pictures themselves wait in a slot
//! per share: the room delivers frames faster than any interface can paint
//! them, and a frame of a screen is megabytes of pixels, so only the newest
//! picture of a share is worth keeping. The queue carries a short note about
//! it instead of the pixels, and the events that end a share are never stuck
//! behind a backlog of pictures.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use cordel_media::frame::Frame;

use crate::ShareState;

/// What the media side sends to the UI thread.
pub enum MediaEvent {
    /// A new picture for a share. The picture itself waits in the frame slots;
    /// this event only says that there is one. The first one creates the tile,
    /// and a later one updates the state, so the local tile can go from preview
    /// to sharing.
    Frame {
        key: String,
        publisher: String,
        state: ShareState,
    },
    /// The source stopped publishing, or access to it was revoked.
    Ended { key: String },
    /// The same share, new connection state. Used for reconnection, which is a
    /// presentation change and not a new share.
    State { key: String, state: ShareState },
    /// A line for the status area. It travels this queue so the UI thread has one
    /// writer of the status text, in the order things happened.
    Status(String),
    /// A join, a capture or a leave is in flight, so the buttons stand down.
    Busy(bool),
    /// Whether the room is joined. Enables leave and disables join.
    Joined(bool),
}

/// The newest picture of each share, waiting for the interface.
///
/// The media side writes here and the interface takes the picture out. Only one
/// picture per share is kept: a picture that is replaced before the interface
/// reaches it is dropped, because a stale picture is worth less than a
/// current one.
pub type FrameSlots = Arc<Mutex<HashMap<String, Frame>>>;

/// The way the media side reaches the interface.
///
/// It carries the events and the waiting pictures. Keeping them together means
/// the media side cannot announce a picture the interface would never find.
#[derive(Clone)]
pub struct MediaOut {
    events: Sender<MediaEvent>,
    slots: FrameSlots,
}

impl MediaOut {
    pub fn new(events: Sender<MediaEvent>) -> Self {
        Self {
            events,
            slots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The waiting pictures, for the interface side.
    pub fn slots(&self) -> FrameSlots {
        self.slots.clone()
    }

    /// Sends an event that carries no picture.
    ///
    /// It answers whether the queue took it. A closed queue means the
    /// interface is gone, and the media side stops instead of working for
    /// nobody.
    pub fn send(&self, event: MediaEvent) -> bool {
        self.events.send(event).is_ok()
    }

    /// Announces a new picture of a share.
    ///
    /// The picture waits in its slot and the note travels in the queue. A
    /// picture that is replaced before the interface takes it is dropped: the
    /// interface shows the current picture instead of an old one.
    pub fn frame(&self, key: &str, publisher: &str, state: ShareState, frame: Frame) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.insert(key.to_string(), frame);
        }
        self.send(MediaEvent::Frame {
            key: key.to_string(),
            publisher: publisher.to_string(),
            state,
        });
    }
}
