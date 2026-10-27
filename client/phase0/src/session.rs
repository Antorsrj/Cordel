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

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use cordel_media::capture::{SessionKind, TargetKind};
use cordel_media::devices::AudioDeviceControl;
use cordel_media::quality::ScreenShareBitrate;
use cordel_media::sdk::{publish_microphone, received_to_frame, remote_video_stream, SdkAudio};
use cordel_state::{MediaAdmissionClient, NativeHttpsTransport};
use futures_util::StreamExt;
use livekit::prelude::*;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::watch;

use crate::share::{MediaEvent, MediaOut, LOCAL_SHARE_KEY};
use crate::ShareState;

/// How often a remote share is allowed to hand a frame to the UI. A screen share
/// does not need 30 fps to look live on this stage, and the conversion runs on the
/// UI thread.
const SHARE_FRAME_INTERVAL: Duration = Duration::from_millis(66);

pub enum Command {
    Join,
    CaptureFrame {
        rate: u32,
        quality: ScreenShareBitrate,
    },
    StopShare,
    Leave,
}

/// A joined room, plus what the shell has to remember about it.
struct Joined {
    room: Room,
    room_events: UnboundedReceiver<RoomEvent>,
    /// The remote shares being decoded. Stopping one drops its video stream.
    follows: follow::RemoteFollows,
    /// Held for the session. The platform audio module is reference counted, and
    /// its device selection is what allows the microphone to open at all.
    _audio: SdkAudio,
    /// The screen share in progress, if any. A second click does not publish
    /// another track; Stop sharing is what ends this one.
    share: Option<stream::StreamShare>,
}

impl Joined {
    /// Stops the share and takes it off the stage. Used when the user stops
    /// sharing and when leaving.
    ///
    /// The local tile ends at this publication's epoch, so a picture still in
    /// flight cannot put it back. The next Share screen uses a newer epoch.
    async fn stop_publishing(&mut self, events: &MediaOut) {
        let Some(share) = self.share.take() else {
            return;
        };
        let epoch = share.epoch;
        share.stop();
        let participant = self.room.local_participant();
        let sid = share.publication.sid();
        match participant.unpublish_track(&sid).await {
            Ok(_) => log::info!("the screen track {sid} left the stage"),
            Err(error) => {
                log::warn!("the screen share track was not taken off the stage: {error}")
            }
        }
        let _ = events.send(MediaEvent::Ended {
            key: LOCAL_SHARE_KEY.into(),
            epoch,
        });
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
    commands: UnboundedReceiver<Command>,
    events: MediaOut,
    access: watch::Receiver<Option<String>>,
) {
    run_session(commands, events, access, None).await
}

/// `seeded_follows` is how the Leave command is tested without a LiveKit room.
/// A real session replaces it with that session's follows, and Leave emits
/// from this one value, so the test and the room take the same path.
async fn run_session(
    mut commands: UnboundedReceiver<Command>,
    events: MediaOut,
    access: watch::Receiver<Option<String>>,
    mut seeded_follows: Option<follow::RemoteFollows>,
) {
    let mut joined: Option<Joined> = None;
    // Shares outlives every room, and a tombstone is never lowered. This
    // counter is the process's, so Leave and Join cannot hand the stage a
    // picture it still treats as ended.
    let mut publication_epoch = 0u64;

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
                let Some(event) = event else {
                    // The room channel ended without a Leave command. The
                    // session is gone, and the tiles should not outlive it.
                    if let Some(mut session) = joined.take() {
                        session.stop_publishing(&events).await;
                        emit_remote_ends(&mut session.follows, &events);
                    }
                    let _ = events.send(MediaEvent::Joined(false));
                    let _ = events.send(MediaEvent::Status("The room connection ended.".into()));
                    continue;
                };
                let Some(session) = joined.as_mut() else {
                    continue;
                };
                handle_room_event(session, *event, &events, &mut publication_epoch);
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
                            register_remote_shares(&mut session, &events, &mut publication_epoch);
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
                Command::CaptureFrame { rate, quality } => {
                    busy(&events, true);
                    if let Some(session) = joined.as_mut() {
                        match stream::start_share(
                            session,
                            TargetKind::Screen,
                            rate,
                            quality,
                            &events,
                            &mut publication_epoch,
                        )
                        .await
                        {
                            Ok(()) => {}
                            Err(err) => {
                                let _ = events.send(MediaEvent::Status(format!(
                                    "Screen share did not start: {err}"
                                )));
                            }
                        }
                    } else {
                        let _ = events.send(MediaEvent::Status(
                            "Join a room before sharing the screen.".into(),
                        ));
                    }
                    busy(&events, false);
                }
                Command::StopShare => {
                    if let Some(session) = joined.as_mut() {
                        session.stop_publishing(&events).await;
                    }
                    let _ = events.send(MediaEvent::Status("Stopped sharing the screen.".into()));
                }
                Command::Leave => {
                    if let Some(mut session) = joined.take() {
                        session.stop_publishing(&events).await;
                        // One slot for the test's follows and the room's. The
                        // emit below is the only one, so dropping this call
                        // drops both.
                        seeded_follows = Some(std::mem::replace(
                            &mut session.follows,
                            follow::RemoteFollows::new(),
                        ));
                        let _ = session.room.close().await;
                        drop(session._audio);
                    }
                    // Remote tiles are not the local publication. Without
                    // these ends they stay on the stage, renderer included,
                    // after the room is gone.
                    if let Some(mut follows) = seeded_follows.take() {
                        emit_remote_ends(&mut follows, &events);
                    }
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
mod follow;
pub(crate) mod stream;

/// Follows the room while it is joined: shares appearing, shares ending,
/// participants leaving and the connection dropping.
///
/// Keys are the media track identity, so a tile keeps its identity while another
/// share ends or is detached (`docs/ux/journal.md`).
fn handle_room_event(session: &mut Joined, event: RoomEvent, events: &MediaOut, epochs: &mut u64) {
    match event {
        RoomEvent::TrackSubscribed {
            track,
            publication,
            participant,
        } => {
            let RemoteTrack::Video(track) = track else {
                return;
            };
            follow_video(
                session,
                participant.identity().to_string(),
                publication.sid().to_string(),
                publication.source(),
                track,
                events,
                epochs,
            );
        }
        RoomEvent::TrackUnsubscribed {
            track,
            publication,
            participant,
        } => {
            if matches!(track, RemoteTrack::Video(_)) {
                let publisher = participant.identity().to_string();
                let key = share_key(&publisher, publication.sid());
                end_followed(session, events, &key);
            }
        }
        RoomEvent::TrackUnpublished {
            publication,
            participant,
        } => {
            let publisher = participant.identity().to_string();
            let key = share_key(&publisher, publication.sid());
            log::info!("a share ended: {key}");
            end_followed(session, events, &key);
        }
        RoomEvent::ParticipantDisconnected(participant) => {
            let leaving = participant.identity().to_string();
            for (key, epoch) in session.follows.end_publisher(&leaving) {
                let _ = events.send(MediaEvent::Ended { key, epoch });
            }
        }
        RoomEvent::Reconnecting => {
            for key in session.follows.keys() {
                let _ = events.send(MediaEvent::State {
                    key,
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
            // tiles come back under the same publishers. The old decoders stop
            // here, or they would keep the tiles the end events removed.
            emit_remote_ends(&mut session.follows, events);
            register_remote_shares(session, events, epochs);
            let _ = events.send(MediaEvent::Status(
                "Reconnected. The share list was refreshed.".into(),
            ));
        }
        _ => {}
    }
}

/// Adds every video publication the room already has. Called on joining (someone
/// may be sharing already) and after a reconnection.
fn register_remote_shares(session: &mut Joined, events: &MediaOut, epochs: &mut u64) {
    for participant in session.room.remote_participants().values() {
        let publisher = participant.identity().to_string();
        for publication in participant.track_publications().values() {
            if let Some(RemoteTrack::Video(track)) = publication.track() {
                follow_video(
                    session,
                    publisher.clone(),
                    publication.sid().to_string(),
                    publication.source(),
                    track,
                    events,
                    epochs,
                );
            }
        }
    }
}

/// Follows one remote video track. A screen share replaces the previous screen
/// share from the same participant and stops that decoder.
fn follow_video(
    session: &mut Joined,
    publisher: String,
    sid: String,
    source: TrackSource,
    track: RemoteVideoTrack,
    events: &MediaOut,
    epochs: &mut u64,
) {
    let exclusive = source == TrackSource::Screenshare;
    let watched = session.follows.watch(&publisher, &sid, exclusive, epochs);
    for (key, epoch) in watched.ended {
        log::info!("a share ended: {key}");
        let _ = events.send(MediaEvent::Ended { key, epoch });
    }
    if !watched.fresh {
        return;
    }
    let key = share_key(&publisher, &sid);
    log::info!("a share appeared: {key}");
    pump_share(
        track,
        key,
        publisher,
        watched.epoch,
        watched.stop,
        events.clone(),
    );
}

/// Forwards one remote share's frames until the track ends or the follow is
/// stopped. Leaving the function drops the track and the stream, which is what
/// releases the decoder and its native queue.
fn pump_share(
    track: RemoteVideoTrack,
    key: String,
    publisher: String,
    epoch: u64,
    mut stop: tokio::sync::watch::Receiver<bool>,
    events: MediaOut,
) {
    tokio::spawn(async move {
        let mut stream = remote_video_stream(&track);
        let mut last = Instant::now() - SHARE_FRAME_INTERVAL;
        loop {
            tokio::select! {
                biased;
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        break;
                    }
                }
                frame = stream.next() => {
                    let Some(frame) = frame else {
                        break;
                    };
                    if *stop.borrow() {
                        break;
                    }
                    if last.elapsed() < SHARE_FRAME_INTERVAL {
                        continue;
                    }
                    last = Instant::now();
                    let Some(frame) = received_to_frame(&frame) else {
                        log::warn!(
                            "a frame from {publisher} uses a layout this build does not handle"
                        );
                        continue;
                    };
                    // The stage shrinks its own copy. The detached window needs
                    // this captured frame, so the reduction happens there.
                    events.frame(&key, &publisher, ShareState::Live, frame, epoch);
                }
            }
        }
    });
}

/// Tells the stage that every remote share of this room has ended.
fn emit_remote_ends(follows: &mut follow::RemoteFollows, events: &MediaOut) {
    for (key, epoch) in follows.end_all() {
        let _ = events.send(MediaEvent::Ended { key, epoch });
    }
}

fn end_followed(session: &mut Joined, events: &MediaOut, key: &str) {
    if let Some(epoch) = session.follows.end(key) {
        let _ = events.send(MediaEvent::Ended {
            key: key.to_string(),
            epoch,
        });
    }
}

/// A share's identity is the publishing participant and the media track, never the
/// tile position.
fn share_key(identity: &str, sid: impl std::fmt::Display) -> String {
    format!("{identity}/{sid}")
}

/// The next publication epoch.
///
/// The stage tombstone for a track only moves forward, and it outlives the
/// room. A counter that started again at zero would make the local preview
/// after Leave and Join the same epoch the stage had already ended.
fn issue_epoch(epochs: &mut u64) -> u64 {
    *epochs = epochs.saturating_add(1);
    *epochs
}

fn busy(events: &MediaOut, busy: bool) {
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
