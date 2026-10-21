//! Room and media wiring for the Phase 0 shell.
//!
//! Everything about capture, audio devices, publishing, receiving and frame
//! conversion lives in `cordel-media` (Client 103, ADR-103); this module only
//! drives it. The shell used to carry its own copies of the capturer call and of
//! the pixel conversions, which is what this convergence removed: one
//! implementation of the media path, and the shell inherits its fixes.
//!
//! It also never touches a Slint type. `slint::Image` and the stage model are
//! bound to the UI thread, so this side sends owned [`MediaEvent`]s — frames,
//! share ends, states, status lines — and the UI thread drains them. That is why
//! every remote track can be followed here without any UI knowledge.
//!
//! One task owns the room, and it waits on the commands *and* on the room's own
//! events in the same `select!`. `livekit::Room` is not `Clone`, so watching its
//! events from a second task would mean giving that task the only handle to the
//! session the shell is still publishing on.

use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use cordel_media::capture::{CaptureSource, CaptureTarget, ScreenCapture, SessionKind, TargetKind};
use cordel_media::devices::AudioDeviceControl;
use cordel_media::frame::Frame;
use cordel_media::sdk::{
    publish_microphone, received_to_frame, remote_video_stream, ScreenPublisher, SdkAudio,
    SdkDesktopCapture,
};
use cordel_state::{MediaAdmissionClient, NativeHttpsTransport};
use futures_util::StreamExt;
use livekit::prelude::*;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::watch;

use crate::share::{MediaEvent, LOCAL_SHARE_KEY};
use crate::ShareState;

/// How long a capture waits for a frame. It has to cover a person answering the
/// portal picker, which is the slowest step of the Wayland path.
const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(90);

/// How often the captured still is re-pushed to the published track, so a
/// subscriber that arrives later still receives something.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(500);

/// How often a remote share is allowed to hand a frame to the UI. A screen share
/// does not need 30 fps to look live on this stage, and the conversion runs on the
/// UI thread.
const SHARE_FRAME_INTERVAL: Duration = Duration::from_millis(66);

pub enum Command {
    Join,
    CaptureFrame,
    Leave,
}

/// A joined room, plus what the shell has to remember about it.
struct Joined {
    room: Room,
    room_events: UnboundedReceiver<RoomEvent>,
    /// The shares being followed: media track key -> publishing identity. It is
    /// what lets a reconnection and a disconnect attribute tiles correctly.
    tracked: HashMap<String, String>,
    /// Held for the session. The platform audio module is reference counted, and
    /// its device selection is what allows the microphone to open at all.
    _audio: SdkAudio,
    /// Stops the still-frame publisher of the *current* capture. A second
    /// capture replaces the publication, because the frame decides the track
    /// size.
    publisher: Option<tokio::sync::watch::Sender<bool>>,
}

impl Joined {
    /// Stops pushing frames and takes the share off the stage. Used when the
    /// capture is replaced and when leaving.
    ///
    /// Without the unpublish, every capture left its track published and the
    /// other side collected one tile per capture.
    async fn stop_publishing(&mut self) {
        if let Some(stop) = self.publisher.take() {
            let _ = stop.send(true);
        }
        let participant = self.room.local_participant();
        let screenshares: Vec<_> = participant
            .track_publications()
            .values()
            .filter(|publication| publication.source() == TrackSource::Screenshare)
            .map(|publication| publication.sid())
            .collect();
        for sid in screenshares {
            let _ = participant.unpublish_track(&sid).await;
        }
    }
}

/// What woke the loop up.
enum Incoming {
    /// `None` means the shell is gone and this task can stop.
    Command(Option<Command>),
    /// `None` means the room channel ended, which is a dropped session. Boxed:
    /// a room event is much larger than a command, and this is a loop-local value.
    Room(Option<Box<RoomEvent>>),
}

pub async fn run(
    mut commands: UnboundedReceiver<Command>,
    events: Sender<MediaEvent>,
    access: watch::Receiver<Option<String>>,
) {
    let mut joined: Option<Joined> = None;

    loop {
        let incoming = match joined.as_mut() {
            Some(session) => tokio::select! {
                command = commands.recv() => Incoming::Command(command),
                event = session.room_events.recv() => Incoming::Room(event.map(Box::new)),
            },
            None => match commands.recv().await {
                Some(command) => Incoming::Command(Some(command)),
                None => return,
            },
        };

        match incoming {
            Incoming::Command(None) => return,
            Incoming::Room(event) => {
                let Some(session) = joined.as_mut() else {
                    continue;
                };
                match event {
                    Some(event) => handle_room_event(session, *event, &events),
                    None => {
                        // The room channel ended without a Leave command. The
                        // session is gone, and the tiles should not outlive it.
                        let stale: Vec<String> =
                            session.tracked.drain().map(|(key, _)| key).collect();
                        for key in stale {
                            let _ = events.send(MediaEvent::Ended { key });
                        }
                        let _ = events.send(MediaEvent::Joined(false));
                        let _ =
                            events.send(MediaEvent::Status("The room connection ended.".into()));
                        let _ = events.send(MediaEvent::Ended {
                            key: LOCAL_SHARE_KEY.into(),
                        });
                        joined = None;
                    }
                }
            }
            Incoming::Command(Some(command)) => match command {
                Command::Join => {
                    if joined.is_some() {
                        continue;
                    }
                    busy(&events, true);
                    match join_room(&events, access.borrow().clone()).await {
                        Ok(mut session) => {
                            // Someone may be sharing already: the stage follows the
                            // room, not only what happens after this client joins.
                            register_remote_shares(&session.room, &events, &mut session.tracked);
                            let _ = events.send(MediaEvent::Joined(true));
                            // Two clients can share one screen during testing, so the
                            // status line says which identity this window joined as.
                            let identity = session.room.local_participant().identity();
                            let _ = events.send(MediaEvent::Status(format!(
                                "Joined as {identity}. The microphone is publishing and remote \
                                 audio goes to the platform output device."
                            )));
                            joined = Some(session);
                        }
                        Err(err) => {
                            let _ = events.send(MediaEvent::Status(format!("Join failed: {err}")));
                        }
                    }
                    busy(&events, false);
                }
                Command::CaptureFrame => {
                    busy(&events, true);
                    let session = SessionKind::detect();
                    let _ = events.send(MediaEvent::Status(format!(
                        "Capturing one frame on the {} session. Confirm the system picker if it \
                         appears.",
                        session_label(session)
                    )));
                    match capture_one_frame(TargetKind::Screen) {
                        Ok(frame) => {
                            let size = format!("{}x{}", frame.width(), frame.height());
                            let mut state = ShareState::Preview;
                            if let Some(session) = joined.as_mut() {
                                match publish_frame(session, &frame, &events).await {
                                    Ok(()) => state = ShareState::Live,
                                    Err(err) => {
                                        let _ = events.send(MediaEvent::Status(format!(
                                            "Frame captured, but not published: {err}"
                                        )));
                                    }
                                }
                            }
                            let _ = events.send(MediaEvent::Frame {
                                key: LOCAL_SHARE_KEY.into(),
                                publisher: "you".into(),
                                state,
                                frame,
                            });
                            if state == ShareState::Preview && joined.is_none() {
                                let _ = events.send(MediaEvent::Status(format!(
                                    "Captured one {} frame ({size}). Join, then capture again to publish.",
                                    session_label(session)
                                )));
                            }
                        }
                        Err(err) => {
                            let _ = events
                                .send(MediaEvent::Status(format!("Screen capture failed: {err}")));
                        }
                    }
                    busy(&events, false);
                }
                Command::Leave => {
                    if let Some(mut session) = joined.take() {
                        session.stop_publishing().await;
                        let _ = session.room.close().await;
                        drop(session._audio);
                    }
                    // A later join must never publish a frame captured in an
                    // earlier room or account session.
                    let _ = events.send(MediaEvent::Ended {
                        key: LOCAL_SHARE_KEY.into(),
                    });
                    let _ = events.send(MediaEvent::Joined(false));
                    let _ = events.send(MediaEvent::Status(
                        "Left the room. The microphone is released.".into(),
                    ));
                }
            },
        }
    }
}

include!("session/publish.rs");

/// Follows the room while it is joined: shares appearing, shares ending,
/// participants leaving and the connection dropping.
///
/// Keys are the media track identity, so a tile keeps its identity while another
/// share ends or is detached (`docs/ux/journal.md`).
fn handle_room_event(session: &mut Joined, event: RoomEvent, events: &Sender<MediaEvent>) {
    match event {
        RoomEvent::TrackSubscribed {
            track,
            publication,
            participant,
        } => {
            let RemoteTrack::Video(track) = track else {
                return;
            };
            let publisher = participant.identity().to_string();
            let key = share_key(&publisher, publication.sid());
            if session
                .tracked
                .insert(key.clone(), publisher.clone())
                .is_none()
            {
                pump_share(track, key, publisher, events.clone());
            }
        }
        RoomEvent::TrackUnsubscribed {
            track,
            publication,
            participant,
        } => {
            if matches!(track, RemoteTrack::Video(_)) {
                let publisher = participant.identity().to_string();
                end_share(
                    events,
                    &mut session.tracked,
                    &share_key(&publisher, publication.sid()),
                );
            }
        }
        RoomEvent::TrackUnpublished {
            publication,
            participant,
        } => {
            let publisher = participant.identity().to_string();
            end_share(
                events,
                &mut session.tracked,
                &share_key(&publisher, publication.sid()),
            );
        }
        RoomEvent::ParticipantDisconnected(participant) => {
            let leaving = participant.identity().to_string();
            let keys: Vec<String> = session
                .tracked
                .iter()
                .filter(|(_, publisher)| **publisher == leaving)
                .map(|(key, _)| key.clone())
                .collect();
            for key in keys {
                end_share(events, &mut session.tracked, &key);
            }
        }
        RoomEvent::Reconnecting => {
            for key in session.tracked.keys() {
                let _ = events.send(MediaEvent::State {
                    key: key.clone(),
                    state: ShareState::Reconnecting,
                });
            }
            let _ = events.send(MediaEvent::Status(
                "The connection dropped. The shares are marked reconnecting.".into(),
            ));
        }
        RoomEvent::Reconnected => {
            // A full reconnect re-issues track identifiers, so the old keys cannot
            // be reused: the catalog is rebuilt from what the room reports, and the
            // tiles come back under the same publishers.
            let stale: Vec<String> = session.tracked.drain().map(|(key, _)| key).collect();
            for key in stale {
                let _ = events.send(MediaEvent::Ended { key });
            }
            register_remote_shares(&session.room, events, &mut session.tracked);
            let _ = events.send(MediaEvent::Status(
                "Reconnected. The share list was refreshed.".into(),
            ));
        }
        _ => {}
    }
}

/// Adds every video publication the room already has. Called on joining (someone
/// may be sharing already) and after a reconnection.
fn register_remote_shares(
    room: &Room,
    events: &Sender<MediaEvent>,
    tracked: &mut HashMap<String, String>,
) {
    for participant in room.remote_participants().values() {
        let publisher = participant.identity().to_string();
        for publication in participant.track_publications().values() {
            if let Some(RemoteTrack::Video(track)) = publication.track() {
                let key = share_key(&publisher, publication.sid());
                if tracked.insert(key.clone(), publisher.clone()).is_none() {
                    pump_share(track, key, publisher.clone(), events.clone());
                }
            }
        }
    }
}

/// Forwards one remote share's frames to the UI thread for as long as the track
/// delivers them.
fn pump_share(track: RemoteVideoTrack, key: String, publisher: String, events: Sender<MediaEvent>) {
    tokio::spawn(async move {
        let mut stream = remote_video_stream(&track);
        let mut last = Instant::now() - SHARE_FRAME_INTERVAL;
        while let Some(frame) = stream.next().await {
            if last.elapsed() < SHARE_FRAME_INTERVAL {
                continue;
            }
            last = Instant::now();
            let Some(frame) = received_to_frame(&frame) else {
                log::warn!("a frame from {publisher} uses a layout this build does not handle");
                continue;
            };
            if events
                .send(MediaEvent::Frame {
                    key: key.clone(),
                    publisher: publisher.clone(),
                    state: ShareState::Live,
                    frame,
                })
                .is_err()
            {
                return;
            }
        }
    });
}

fn end_share(events: &Sender<MediaEvent>, tracked: &mut HashMap<String, String>, key: &str) {
    if tracked.remove(key).is_some() {
        let _ = events.send(MediaEvent::Ended {
            key: key.to_string(),
        });
    }
}

/// A share's identity is the publishing participant and the media track, never the
/// tile position.
fn share_key(identity: &str, sid: impl std::fmt::Display) -> String {
    format!("{identity}/{sid}")
}

fn busy(events: &Sender<MediaEvent>, busy: bool) {
    let _ = events.send(MediaEvent::Busy(busy));
}

/// `unknown` when the process cannot tell which session it runs on, which is
/// itself worth showing: the capture path differs between the two.
fn session_label(session: Option<SessionKind>) -> &'static str {
    session.map(SessionKind::as_str).unwrap_or("unknown")
}

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
