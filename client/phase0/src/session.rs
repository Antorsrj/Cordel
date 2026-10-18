//! Room and media wiring for the Phase 0 shell.
//!
//! Everything about capture, audio devices, publishing, receiving and frame
//! conversion lives in `cordel-media` (Client 103, ADR-103); this module only
//! drives it and puts the result on the Slint window. The shell used to carry its
//! own copies of the capturer call and of the pixel conversions, which is what
//! this convergence removes: one implementation of the media path, and the shell
//! inherits its fixes.
//!
//! One consequence of that split is worth knowing before editing: `slint::Image`
//! is not `Send`, so what crosses to the UI thread is the captured [`Frame`], and
//! the frame → image step runs where the image will be used. That is the same
//! step `cordel_media::render` measures, so the numbers in the status line are the
//! real product path, not a side measurement.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use cordel_media::capture::{CaptureSource, CaptureTarget, ScreenCapture, SessionKind, TargetKind};
use cordel_media::devices::AudioDeviceControl;
use cordel_media::frame::Frame;
use cordel_media::render::{FrameSink, SlintFrameSink};
use cordel_media::sdk::{
    publish_microphone, received_to_frame, remote_video_stream, ScreenPublisher, SdkAudio,
    SdkDesktopCapture,
};
use futures_util::StreamExt;
use livekit::prelude::*;
use slint::{SharedString, Weak};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::AppWindow;

/// How long a capture waits for a frame. It has to cover a person answering the
/// portal picker, which is the slowest step of the Wayland path.
const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(90);

/// How often the captured still is re-pushed to the published track, so a
/// subscriber that arrives later still receives something.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(500);

pub enum Command {
    Join,
    CaptureFrame,
    Leave,
}

struct Joined {
    room: Room,
    /// Held for the session. The platform audio module is reference counted, and
    /// its device selection is what allows the microphone to open at all.
    _audio: SdkAudio,
    stop: tokio::sync::watch::Sender<bool>,
}

pub async fn run(mut commands: UnboundedReceiver<Command>, ui: Weak<AppWindow>) {
    let mut joined: Option<Joined> = None;
    let mut captured: Option<Frame> = None;
    let mut publishing = false;

    while let Some(command) = commands.recv().await {
        match command {
            Command::Join => {
                if joined.is_some() {
                    continue;
                }
                set_busy(&ui, true);
                match join_room().await {
                    Ok((session, mut events)) => {
                        let ui_events = ui.clone();
                        tokio::spawn(async move {
                            while let Some(event) = events.recv().await {
                                handle_room_event(event, &ui_events).await;
                            }
                        });
                        set_status(
                            &ui,
                            "Joined. Microphone is publishing. Remote audio uses the platform output device.",
                        );
                        set_joined(&ui, true);
                        joined = Some(session);
                        if captured.is_some() && !publishing {
                            match publish_frame(joined.as_ref(), captured.as_ref()).await {
                                Ok(()) => publishing = true,
                                Err(err) => set_status(
                                    &ui,
                                    &format!(
                                        "Joined, but the captured frame was not published: {err}"
                                    ),
                                ),
                            }
                        }
                    }
                    Err(err) => set_status(&ui, &format!("Join failed: {err}")),
                }
                set_busy(&ui, false);
            }
            Command::CaptureFrame => {
                set_busy(&ui, true);
                let session = SessionKind::detect();
                set_status(
                    &ui,
                    &format!(
                        "Capturing one frame on the {} session. Confirm the system picker if it appears.",
                        session_label(session)
                    ),
                );
                match capture_one_frame(TargetKind::Screen) {
                    Ok(frame) => {
                        show_frame(
                            &ui,
                            frame.clone(),
                            session,
                            if joined.is_some() {
                                "Publishing that still frame."
                            } else {
                                "Join to publish it."
                            },
                        );
                        captured = Some(frame);
                        if joined.is_some() && !publishing {
                            match publish_frame(joined.as_ref(), captured.as_ref()).await {
                                Ok(()) => publishing = true,
                                Err(err) => set_status(
                                    &ui,
                                    &format!("Frame captured, publish failed: {err}"),
                                ),
                            }
                        }
                    }
                    Err(err) => set_status(&ui, &format!("Screen capture failed: {err}")),
                }
                set_busy(&ui, false);
            }
            Command::Leave => {
                if let Some(session) = joined.take() {
                    let _ = session.stop.send(true);
                    let _ = session.room.close().await;
                    drop(session._audio);
                }
                publishing = false;
                set_joined(&ui, false);
                set_status(&ui, "Left the room. The microphone is released.");
            }
        }
    }
}

/// Captures a single frame through whichever backend `cordel-media` picks for
/// this session, and waits for the portal grant if that is the path.
fn capture_one_frame(kind: TargetKind) -> anyhow::Result<Frame> {
    let source = CaptureSource::Auto.resolve(SessionKind::detect());
    let mut backend = SdkDesktopCapture::with_source(kind, source)?;
    let target = match backend.targets()?.into_iter().next() {
        Some(target) => target,
        // The portal lists nothing before the grant, because the picker decides
        // the surface; the placeholder title is only used for logging.
        None if backend.source() == CaptureSource::Portal => {
            CaptureTarget::new(0, "portal picker", kind)
        }
        None => return Err(anyhow!("no capture source was listed")),
    };

    let (sender, receiver) = mpsc::channel::<Frame>();
    backend.start(
        &target,
        Box::new(move |frame| {
            let _ = sender.send(frame);
        }),
    )?;

    let deadline = Instant::now() + FIRST_FRAME_DEADLINE;
    let outcome = loop {
        // The portal handshake only progresses while `tick` keeps being called,
        // so this runs it even before the first frame arrives.
        backend.tick()?;
        match receiver.recv_timeout(Duration::from_millis(16)) {
            Ok(frame) => break Ok(frame),
            Err(RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(RecvTimeoutError::Timeout) => {
                break Err(anyhow!(
                    "no screen frame arrived; the portal or X11 source was not confirmed"
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                break Err(anyhow!("screen capture stopped before a frame"))
            }
        }
    };
    backend.stop()?;
    outcome
}

async fn join_room() -> anyhow::Result<(Joined, UnboundedReceiver<RoomEvent>)> {
    let url = std::env::var("CORDEL_LIVEKIT_URL")
        .map_err(|_| anyhow::anyhow!("CORDEL_LIVEKIT_URL is not set"))?;
    let token = std::env::var("CORDEL_LIVEKIT_TOKEN")
        .map_err(|_| anyhow::anyhow!("CORDEL_LIVEKIT_TOKEN is not set"))?;

    // Opening the platform audio selects the system microphone and speakers.
    // WebRTC's Linux audio device module refuses to initialise either direction
    // until a device has been selected, so this is what makes the microphone open
    // and remote audio audible at all.
    let mut audio = SdkAudio::new().context("platform audio")?;
    let (microphone, speakers) = audio.devices_selected();
    log::info!("audio devices selected: microphone={microphone} speakers={speakers}");
    audio.start_recording().context("microphone")?;

    let (room, events) = Room::connect(&url, &token, RoomOptions::default()).await?;
    publish_microphone(&room, "microphone")
        .await
        .context("publishing the microphone")?;

    let (stop, _) = tokio::sync::watch::channel(false);
    Ok((
        Joined {
            room,
            _audio: audio,
            stop,
        },
        events,
    ))
}

/// Publishes the captured frame as a screen-share track and keeps pushing it, so
/// a subscriber that arrives later still gets the picture.
async fn publish_frame(joined: Option<&Joined>, captured: Option<&Frame>) -> anyhow::Result<()> {
    let (Some(joined), Some(captured)) = (joined, captured) else {
        return Ok(());
    };
    if captured.width() == 0 || captured.height() == 0 {
        anyhow::bail!("captured frame has no dimensions");
    }

    // The publisher's resolution comes from the frame the capturer actually
    // produced, so the client never has to guess what the picker will hand it.
    let publisher = Arc::new(ScreenPublisher::new(
        captured.width(),
        captured.height(),
        "screen-frame",
    ));
    publisher.publish(&joined.room).await?;
    publisher.push(captured)?;

    let mut stop = joined.stop.subscribe();
    let frame = Arc::new(captured.clone());
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PUBLISH_INTERVAL);
        loop {
            tokio::select! {
                _ = stop.changed() => break,
                _ = ticker.tick() => {
                    if let Err(err) = publisher.push(&frame) {
                        log::warn!("screen frame not pushed: {err}");
                    }
                }
            }
        }
    });
    Ok(())
}

async fn handle_room_event(event: RoomEvent, ui: &Weak<AppWindow>) {
    let RoomEvent::TrackSubscribed { track, .. } = event else {
        return;
    };
    let RemoteTrack::Video(track) = track else {
        return;
    };
    let ui = ui.clone();
    tokio::spawn(async move {
        let mut stream = remote_video_stream(&track);
        let Some(frame) = stream.next().await else {
            return;
        };
        let Some(frame) = received_to_frame(&frame) else {
            set_status(
                &ui,
                "A remote video frame arrived in a layout this build does not handle.",
            );
            return;
        };
        let (width, height) = (frame.width(), frame.height());
        let _ = ui.upgrade_in_event_loop(move |ui| match present(&frame) {
            Ok((image, summary)) => {
                ui.set_frame(image);
                ui.set_status(
                    format!("Remote frame rendered in Slint, {width}x{height}. {summary}").into(),
                );
            }
            Err(err) => {
                ui.set_status(format!("A remote frame could not be converted: {err}").into())
            }
        });
    });
}

/// Shows a freshly captured frame, reporting what the conversion cost.
fn show_frame(ui: &Weak<AppWindow>, frame: Frame, session: Option<SessionKind>, note: &str) {
    let size = format!("{}x{}", frame.width(), frame.height());
    let label = session_label(session);
    let note = note.to_string();
    let _ = ui.upgrade_in_event_loop(move |ui| match present(&frame) {
        Ok((image, summary)) => {
            ui.set_frame(image);
            ui.set_status(format!("Captured one {label} frame ({size}); {summary}. {note}").into());
        }
        Err(err) => ui.set_status(format!("Frame captured, render failed: {err}").into()),
    });
}

/// Runs the shared frame → Slint image step and returns the image with its cost.
///
/// The sink is created per frame here, which is the pessimistic case the spike
/// measured (a fresh pixel buffer each time). A client that keeps one sink per
/// track pays the conversion only.
fn present(frame: &Frame) -> Result<(slint::Image, String), cordel_media::frame::FrameError> {
    let mut sink = SlintFrameSink::new();
    sink.present(frame)?;
    Ok((sink.image().unwrap_or_default(), sink.stats().summary()))
}

/// `unknown` when the process cannot tell which session it runs on, which is
/// itself worth showing: the capture path differs between the two.
fn session_label(session: Option<SessionKind>) -> &'static str {
    session.map(SessionKind::as_str).unwrap_or("unknown")
}

fn set_status(ui: &Weak<AppWindow>, text: &str) {
    let text: SharedString = text.into();
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_status(text));
}

fn set_joined(ui: &Weak<AppWindow>, joined: bool) {
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_joined(joined));
}

fn set_busy(ui: &Weak<AppWindow>, busy: bool) {
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_busy(busy));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_label_names_the_unknown_case_instead_of_hiding_it() {
        assert_eq!(session_label(Some(SessionKind::Wayland)), "wayland");
        assert_eq!(session_label(Some(SessionKind::X11)), "x11");
        assert_eq!(session_label(None), "unknown");
    }
}
