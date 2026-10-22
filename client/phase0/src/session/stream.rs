/// Captures the screen continuously and republishes it as a screen-share
/// track.
///
/// The capturer of the WebRTC has thread affinity, and a task of the runtime
/// can move between workers between awaits. Everything that touches the
/// capturer therefore lives in one dedicated thread: it lists the sources, it
/// starts the capture, and it asks for a frame. Another task receives the
/// frames, publishes them, and reports the measured rate. A frame of a
/// 2560x1440 screen is about 14 MB, and converting and encoding it costs real
/// time, so the rate is capped here rather than by the interface.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cordel_media::capture::{CaptureSource, CaptureTarget, ScreenCapture, TargetKind};
use cordel_media::frame::{fit_display, Frame};
use cordel_media::sdk::{ScreenPublisher, SdkDesktopCapture};

use super::*;

/// The rates the shell offers. A screen share of 2560x1440 costs about 14 MB
/// per frame, and converting and encoding it takes real time, so the rate is
/// the user's choice.
/// The default rate. It is the safest one for a full-screen capture.
pub(crate) const DEFAULT_FRAME_RATE: u32 = 15;

/// A Share screen click while a publication is already open.
///
/// The click does not publish a second track and it does not restart the one
/// that is live. Stop sharing ends it. A restart would be a new track id,
/// which is the publication the viewer was stacking.
pub(super) fn accept_share_click(publication_open: bool) -> Result<(), &'static str> {
    if publication_open {
        Err("Already sharing the screen. Stop sharing before sharing it again.")
    } else {
        Ok(())
    }
}

/// The interval between published frames for a given rate.
fn interval_for_rate(rate: u32) -> Duration {
    Duration::from_millis((1000 / rate.max(1)) as u64)
}

/// How long the first frame may take. It has to cover the portal picker.
const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(90);

/// How often the capture thread asks for a frame. The capturer of the WebRTC
/// is a polling loop: asking too rarely lets the capture expire, and the
/// answer is a temporary error. The frames that arrive faster than the chosen
/// rate are dropped before they cost anything.
const CAPTURE_POLL: Duration = Duration::from_millis(16);

/// How often the measured rate is reported.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// How often the local preview gets a frame. The stage throttles what it
/// paints, and a frame of a full-screen capture is about 14 MB, so the preview
/// follows a slower pace than the share.
const LOCAL_FRAME_INTERVAL: Duration = Duration::from_millis(250);

/// The handle of a running share. The capture thread stops when this is
/// signalled.
pub(super) struct StreamShare {
    stop: tokio::sync::watch::Sender<bool>,
    running: Arc<AtomicBool>,
    /// The published track. It is kept so stopping the share takes exactly
    /// that one off the stage, instead of guessing which track belongs to the
    /// screen.
    pub(super) publication: livekit::prelude::LocalTrackPublication,
    /// Pictures from this publication carry this epoch. Stopping the share
    /// ends the local tile at the same epoch.
    pub(super) epoch: u64,
}

impl StreamShare {
    /// Ask the share to end. The capture thread stops and releases the capture
    /// permission.
    pub(super) fn stop(&self) {
        let _ = self.stop.send(true);
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Start a continuous screen share on the joined room.
///
/// The first frame decides the track size, because the client never has to
/// guess what the picker will hand it.
pub(super) async fn start_share(
    joined: &mut Joined,
    kind: TargetKind,
    rate: u32,
    events: &MediaOut,
    epochs: &mut u64,
) -> anyhow::Result<()> {
    // A second click must not publish another track. Stop sharing is what ends
    // the one that is already up. Replacing it here is what gave every click a
    // new track id, and the viewer then had one tile per click.
    if let Err(message) = accept_share_click(joined.share.is_some()) {
        let _ = events.send(MediaEvent::Status(message.to_owned()));
        return Ok(());
    }
    // Only a click that will open a publication mentions the picker. A second
    // click is the no-op above, and it must not claim a share is starting.
    let session = SessionKind::detect();
    let _ = events.send(MediaEvent::Status(format!(
        "Sharing the screen on the {} session. Confirm the system picker if it \
         appears.",
        super::session_label(session)
    )));
    let epoch = super::issue_epoch(epochs);
    let stream_interval = interval_for_rate(rate);

    let source = CaptureSource::Auto.resolve(SessionKind::detect());
    let (frame_tx, frame_rx) = mpsc::channel::<Frame>();
    let running = Arc::new(AtomicBool::new(true));
    let capture_running = running.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    // Tudo que toca o capturador vive nesta thread.
    let capture = std::thread::spawn(move || {
        let mut backend = match SdkDesktopCapture::with_source(kind, source) {
            Ok(backend) => backend,
            Err(error) => {
                let _ = ready_tx.send(Err(format!("{error:?}")));
                return;
            }
        };
        let target: CaptureTarget = match backend.targets() {
            Ok(targets) => match targets.into_iter().next() {
                Some(target) => target,
                // The portal lists nothing before the grant, because the picker
                // decides the surface; the placeholder title is only used for
                // logging.
                None if backend.source() == CaptureSource::Portal => {
                    CaptureTarget::new(0, "portal picker", kind)
                }
                None => {
                    let _ = ready_tx.send(Err("no capture source was listed".to_owned()));
                    return;
                }
            },
            Err(error) => {
                let _ = ready_tx.send(Err(format!("{error:?}")));
                return;
            }
        };
        let sender = frame_tx.clone();
        if let Err(error) = backend.start(
            &target,
            Box::new(move |frame| {
                let _ = sender.send(frame);
            }),
        ) {
            let _ = ready_tx.send(Err(format!("{error:?}")));
            return;
        }
        let _ = ready_tx.send(Ok(()));
        while capture_running.load(Ordering::Relaxed) {
            if let Err(error) = backend.tick() {
                log::warn!("screen capture error ({error:?})");
            }
            std::thread::sleep(CAPTURE_POLL);
        }
        // The capturer must release the capture permission.
        let _ = backend.stop();
    });

    // The capture thread reports whether it could start.
    match ready_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Ok(())) => {}
        Ok(Err(message)) => {
            let _ = capture.join();
            anyhow::bail!("the capture did not start: {message}");
        }
        Err(_) => {
            let _ = capture.join();
            anyhow::bail!("the capture did not start");
        }
    }

    // The first frame defines the track.
    let first = wait_for_first_frame(&frame_rx)?;

    let publisher = Arc::new(
        ScreenPublisher::new(first.width(), first.height(), "screen-stream")
            .with_frame_rate(rate as f64),
    );
    let publication = publisher.publish(&joined.room).await?;
    log::info!(
        "the screen track {} was published at {}x{}",
        publication.sid(),
        first.width(),
        first.height()
    );
    publisher.push(&first)?;
    let _ = events.send(MediaEvent::Status(format!(
        "Sharing the screen at {}x{} on the {} session.",
        first.width(),
        first.height(),
        session_label(SessionKind::detect()),
    )));
    // A previa local e a mesma imagem que vai para o outro lado, so que
    // reduzida: a interface pinta ela escalada.
    events.frame(
        LOCAL_SHARE_KEY,
        "you",
        ShareState::Live,
        fit_display(first, UI_FRAME_WIDTH),
        epoch,
    );

    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let report = events.clone();
    tokio::spawn(async move {
        let mut last_report = Instant::now();
        let mut local = Instant::now() - LOCAL_FRAME_INTERVAL;
        let mut frames = 0u32;
        loop {
            // Waiting for the end of the share and the capture run together:
            // waiting only for the value to change would stall the task after
            // the first frame, and the screen would stop moving.
            tokio::select! {
                _ = stopped.changed() => break,
                _ = tokio::time::sleep(stream_interval) => {}
            }
            // Only the last frame of the interval is published. Dropping the
            // rest keeps the rate low and the interface responsive.
            let mut latest = None;
            loop {
                match frame_rx.try_recv() {
                    Ok(frame) => latest = Some(frame),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            if let Some(frame) = latest {
                frames += 1;
                if let Err(error) = publisher.push(&frame) {
                    log::warn!("screen frame not pushed: {error}");
                }
                // The local preview is the same frame that goes to the other
                // side, so it has to move with the stream.
                if local.elapsed() >= LOCAL_FRAME_INTERVAL {
                    local = Instant::now();
                    report.frame(
                        LOCAL_SHARE_KEY,
                        "you",
                        ShareState::Live,
                        fit_display(frame, UI_FRAME_WIDTH),
                        epoch,
                    );
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
        }
    });

    joined.share = Some(StreamShare {
        stop,
        running,
        publication,
        epoch,
    });
    Ok(())
}

/// Wait for the first frame the capture thread produces.
fn wait_for_first_frame(receiver: &mpsc::Receiver<Frame>) -> anyhow::Result<Frame> {
    let deadline = Instant::now() + FIRST_FRAME_DEADLINE;
    while Instant::now() < deadline {
        if let Ok(frame) = receiver.try_recv() {
            return Ok(frame);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    anyhow::bail!("the screen produced no frame within {FIRST_FRAME_DEADLINE:?}")
}
