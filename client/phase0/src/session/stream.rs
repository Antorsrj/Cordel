/// Captures the screen continuously and republishes it as a screen-share
/// track.
///
/// The capturer delivers frames on its own thread, or asks for one at a time
/// through `tick`, depending on the backend. A dedicated task holds both the
/// capturer and the publisher: it advances the backend, forwards the frames
/// that arrive, and stops the capturer when the share ends. The number of
/// frames per second is measured and reported, because a screen share that
/// only shows a still frame is a failure the user can see.
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cordel_media::capture::{CaptureSource, CaptureTarget, ScreenCapture, TargetKind};
use cordel_media::frame::Frame;
use cordel_media::sdk::{ScreenPublisher, SdkDesktopCapture};

use super::*;

/// How often the task asks the backend for a frame and forwards what arrived.
const STREAM_INTERVAL: Duration = Duration::from_millis(66);

/// How long the first frame may take. It has to cover the portal picker.
const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(90);

/// How often the measured rate is reported.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// The handle of a running share. The task that holds the capturer stops it
/// when this is signalled.
pub(super) struct StreamShare {
    stop: tokio::sync::watch::Sender<bool>,
}

impl StreamShare {
    /// Ask the share to end. The task stops the capturer and releases the
    /// capture permission.
    pub(super) fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

/// Start a continuous screen share on the joined room.
///
/// The first frame decides the track size, because the client never has to
/// guess what the picker will hand it.
pub(super) async fn start_share(
    joined: &mut Joined,
    kind: TargetKind,
    events: &Sender<MediaEvent>,
) -> anyhow::Result<()> {
    let source = CaptureSource::Auto.resolve(SessionKind::detect());
    let mut backend = SdkDesktopCapture::with_source(kind, source)?;
    let target: CaptureTarget = match backend.targets()?.into_iter().next() {
        Some(target) => target,
        // The portal lists nothing before the grant, because the picker decides
        // the surface; the placeholder title is only used for logging.
        None if backend.source() == CaptureSource::Portal => {
            CaptureTarget::new(0, "portal picker", kind)
        }
        None => anyhow::bail!("no capture source was listed"),
    };

    let (sender, receiver) = mpsc::channel::<Frame>();
    backend.start(
        &target,
        Box::new(move |frame| {
            let _ = sender.send(frame);
        }),
    )?;

    // The first frame defines the track.
    let first = wait_for_first_frame(&receiver, &mut backend)?;
    let publisher = Arc::new(ScreenPublisher::new(
        first.width(),
        first.height(),
        "screen-stream",
    ));
    publisher.publish(&joined.room).await?;
    publisher.push(&first)?;
    let _ = events.send(MediaEvent::Status(format!(
        "Sharing the screen at {}x{} on the {} session.",
        first.width(),
        first.height(),
        session_label(SessionKind::detect()),
    )));
    let _ = events.send(MediaEvent::Frame {
        key: LOCAL_SHARE_KEY.into(),
        publisher: "you".into(),
        state: ShareState::Live,
        frame: first,
    });

    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let report = events.clone();
    tokio::spawn(async move {
        let mut backend = backend;
        let mut last_report = Instant::now();
        let mut frames = 0u32;
        loop {
            if stopped.changed().await.is_err() {
                break;
            }
            if let Err(error) = backend.tick() {
                let _ = report.send(MediaEvent::Status(format!(
                    "Screen capture stopped: {error}"
                )));
                break;
            }
            loop {
                match receiver.try_recv() {
                    Ok(frame) => {
                        frames += 1;
                        if let Err(error) = publisher.push(&frame) {
                            log::warn!("screen frame not pushed: {error}");
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            if last_report.elapsed() >= REPORT_INTERVAL {
                let seconds = last_report.elapsed().as_secs_f32();
                let rate = frames as f32 / seconds;
                frames = 0;
                last_report = Instant::now();
                let _ = report.send(MediaEvent::Status(format!(
                    "Screen share running at {rate:.1} frames per second."
                )));
            }
            tokio::time::sleep(STREAM_INTERVAL).await;
        }
        // The capturer must release the capture permission.
        let _ = backend.stop();
    });

    joined.share = Some(StreamShare { stop });
    Ok(())
}

/// Wait for the first frame, advancing the backend while it waits.
fn wait_for_first_frame(
    receiver: &mpsc::Receiver<Frame>,
    backend: &mut dyn ScreenCapture,
) -> anyhow::Result<Frame> {
    let deadline = Instant::now() + FIRST_FRAME_DEADLINE;
    while Instant::now() < deadline {
        // A pull-based backend only produces a frame when it is asked.
        backend.tick()?;
        if let Ok(frame) = receiver.try_recv() {
            return Ok(frame);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    anyhow::bail!("the screen produced no frame within {FIRST_FRAME_DEADLINE:?}")
}
