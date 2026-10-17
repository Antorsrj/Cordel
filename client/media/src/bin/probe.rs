//! Phase 0 media probe for ADR-103.
//!
//! A command line harness over the spike code, meant to be run by a human and to
//! print evidence rather than opinions. Every command that touches a capture
//! source requires an explicit `--yes`, because `AGENTS.md` forbids capturing a
//! screen or microphone without an explicit user action; the flag is how the
//! probe refuses to be an exception to that rule.
//!
//! Commands:
//!
//! ```text
//! probe summary                        what this build can do, and on which session
//! probe consent                        the capture-consent state machine, no hardware
//! probe synthetic [--frames N] [...]   frame conversion (+ Slint image) cost, no hardware
//! probe devices                        audio devices (needs --features sdk)
//! probe mic --yes [--switch-to ID]      open the microphone, hot-swap it while open
//! probe targets [--windows]            capture sources (needs --features sdk)
//! probe capture --yes [...]            capture real frames and measure them
//! probe join --url URL --token TOKEN   join a room and publish (needs --features sdk)
//! ```

use anyhow::{anyhow, Context, Result};
use cordel_media::capture::{CaptureTarget, SessionKind, TargetKind};
use cordel_media::consent::{Permission, ShareController, ShareEvent, StopReason};
use cordel_media::frame::{synthetic_bgra, Frame};
#[cfg(feature = "sdk")]
use std::time::Duration;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("summary");
    let rest = &args[args.len().min(1)..];
    match command {
        "summary" => summary(),
        "consent" => consent_demo(),
        "synthetic" => synthetic(rest),
        "devices" => devices(),
        "mic" => mic(rest),
        "targets" => targets(rest),
        "capture" => capture(rest),
        "join" => join(rest),
        other => Err(anyhow!(
            "unknown command `{other}`\navailable: summary, consent, synthetic, devices, targets, capture, join"
        )),
    }
}

// ---------------------------------------------------------------------------
// argument helpers (hand-rolled: the probe has no business pulling a CLI crate)
// ---------------------------------------------------------------------------

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn option(args: &[String], name: &str) -> Option<String> {
    let index = args.iter().position(|a| a == name)?;
    args.get(index + 1).cloned()
}

fn number(args: &[String], name: &str, default: u64) -> Result<u64> {
    match option(args, name) {
        None => Ok(default),
        Some(value) => value
            .parse()
            .with_context(|| format!("{name} expects a number, got `{value}`")),
    }
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

fn summary() -> Result<()> {
    println!("cordel phase0 media probe (ADR-103) — spike build");
    println!();
    println!("build capabilities");
    for (name, enabled) in cordel_media::build_capabilities() {
        println!("  [{}] {name}", if enabled { "x" } else { " " });
    }

    println!();
    match SessionKind::detect() {
        Some(kind) => println!(
            "linux session: {} (capture path depends on this)",
            kind.as_str()
        ),
        None => println!("linux session: none detected (no WAYLAND_DISPLAY and no DISPLAY)"),
    }

    println!();
    println!("audio backend     : {}", audio_backend_name());
    println!("capture backend   : {}", capture_backend_name());
    Ok(())
}

#[cfg(feature = "sdk")]
fn audio_backend_name() -> &'static str {
    "livekit-platform-audio (--features sdk)"
}
#[cfg(not(feature = "sdk"))]
fn audio_backend_name() -> &'static str {
    "unavailable: rebuild with --features sdk"
}

#[cfg(feature = "sdk")]
fn capture_backend_name() -> &'static str {
    "libwebrtc-desktop-capturer (--features sdk)"
}
#[cfg(not(feature = "sdk"))]
fn capture_backend_name() -> &'static str {
    "unavailable: rebuild with --features sdk"
}

fn consent_demo() -> Result<()> {
    let target = CaptureTarget::new(1, "whole screen", TargetKind::Screen);
    let mut controller = ShareController::new();

    println!(
        "initial            : live={} indicator={}",
        controller.is_live(),
        controller.indicator_visible()
    );
    println!(
        "start without grant: {:?}",
        controller.start(target.clone())
    );
    controller.set_permission(Permission::Granted);
    println!("after grant, start : {:?}", controller.start(target));
    println!(
        "while live         : live={} indicator={}",
        controller.is_live(),
        controller.indicator_visible()
    );
    println!(
        "second start       : {:?}",
        controller.start(CaptureTarget::new(2, "second", TargetKind::Window))
    );
    let event = controller.set_permission(Permission::Denied);
    println!("permission revoked : {event:?}");
    println!(
        "after revoke       : live={} indicator={}",
        controller.is_live(),
        controller.indicator_visible()
    );

    assert!(matches!(
        event,
        Some(ShareEvent::Stopped(StopReason::PermissionLost))
    ));
    assert!(!controller.is_live());
    Ok(())
}

fn synthetic(args: &[String]) -> Result<()> {
    let frames = number(args, "--frames", 120)?;
    let width = number(args, "--width", 1920)? as u32;
    let height = number(args, "--height", 1080)? as u32;

    println!("synthetic frames: {width}x{height}, {frames} frames");
    println!("(no hardware involved: this isolates conversion and image cost)");

    // Frames are generated up front so that generation cost is not charged to the
    // measurement; both paths below see exactly the same frames.
    let pool: Vec<Frame> = (0..8).map(|i| synthetic_bgra(width, height, i)).collect();

    let mut converted = Vec::new();
    let convert_started = Instant::now();
    for index in 0..frames {
        pool[index as usize % pool.len()].to_rgba8_into(&mut converted);
    }
    let convert_elapsed = convert_started.elapsed();
    let per_frame = convert_elapsed / frames.max(1) as u32;
    println!(
        "BGRA -> RGBA into a reused buffer          : total {:.1} ms, {:.3} ms/frame, {:.1} MP/s",
        convert_elapsed.as_secs_f64() * 1000.0,
        per_frame.as_secs_f64() * 1000.0,
        (frames as f64 * width as f64 * height as f64) / convert_elapsed.as_secs_f64() / 1e6,
    );

    #[cfg(feature = "ui")]
    {
        use cordel_media::render::{FrameSink, SlintFrameSink};
        let mut sink = SlintFrameSink::new();
        let started = Instant::now();
        for index in 0..frames {
            sink.present(&pool[index as usize % pool.len()])?;
        }
        let elapsed = started.elapsed();
        println!(
            "frame -> slint::Image (fresh buffer/frame): total {:.1} ms, {:.3} ms/frame",
            elapsed.as_secs_f64() * 1000.0,
            elapsed.as_secs_f64() * 1000.0 / frames.max(1) as f64,
        );
        println!("  {}", sink.stats().summary());
        if sink.image().is_none() {
            return Err(anyhow!("sink produced no image"));
        }
    }

    #[cfg(not(feature = "ui"))]
    println!("frame -> slint::Image: not built (rebuild with --features ui)");

    Ok(())
}

fn devices() -> Result<()> {
    #[cfg(feature = "sdk")]
    {
        use cordel_media::devices::AudioDeviceControl;
        use cordel_media::sdk::SdkAudio;

        let audio = SdkAudio::new()?;
        println!("backend: {}", audio.backend_name());
        for device in audio.recording_devices()? {
            println!("  [recording {:>2}] {}", device.index, device.name);
        }
        for device in audio.playout_devices()? {
            println!("  [playout   {:>2}] {}", device.index, device.name);
        }
        Ok(())
    }
    #[cfg(not(feature = "sdk"))]
    {
        Err(anyhow!(
            "audio devices need the SDK: rebuild with --features sdk"
        ))
    }
}

/// Opens the microphone, and optionally switches device while it is open.
///
/// This is the half of "two-way audio" that can be checked without a server: the
/// capture device actually opens, and hot-swapping the microphone does not require
/// stopping and restarting capture.
fn mic(args: &[String]) -> Result<()> {
    let seconds = number(args, "--seconds", 3)?;
    let switch_to = option(args, "--switch-to");

    if !flag(args, "--yes") {
        println!("this would open your microphone for {seconds} s (nothing is recorded to disk).");
        println!("re-run with --yes to actually do it.");
        return Ok(());
    }

    #[cfg(feature = "sdk")]
    {
        use cordel_media::devices::AudioDeviceControl;
        use cordel_media::sdk::SdkAudio;

        let mut audio = SdkAudio::new()?;
        if let Some(id) = option(args, "--device") {
            audio.set_recording_device(&id)?;
            println!("selected recording device {id}");
        }

        let (aec, agc, ns) = audio.hardware_processing_available();
        println!("hardware processing available: aec={aec} agc={agc} ns={ns}");
        let prefer_hardware = flag(args, "--hardware-processing");
        audio.configure_processing(prefer_hardware)?;
        println!("audio processing configured (prefer_hardware={prefer_hardware})");

        audio.start_recording()?;
        println!("microphone open for {seconds} s");
        std::thread::sleep(Duration::from_secs(seconds.min(5)));

        if let Some(id) = switch_to {
            audio.switch_recording_device(&id)?;
            println!("switched microphone to {id} while recording (no stop/start)");
            std::thread::sleep(Duration::from_secs(2));
        }

        audio.stop_recording()?;
        println!("microphone closed cleanly");
        Ok(())
    }
    #[cfg(not(feature = "sdk"))]
    {
        let _ = (seconds, switch_to);
        Err(anyhow!(
            "microphone capture needs the SDK: rebuild with --features sdk"
        ))
    }
}

fn targets(args: &[String]) -> Result<()> {
    let kind = if flag(args, "--windows") {
        TargetKind::Window
    } else {
        TargetKind::Screen
    };

    #[cfg(feature = "sdk")]
    {
        use cordel_media::capture::ScreenCapture;
        use cordel_media::sdk::SdkDesktopCapture;

        let backend = SdkDesktopCapture::new(kind)?;
        println!("backend: {}", backend.backend_name());
        let targets = backend.targets()?;
        if targets.is_empty() {
            println!("no targets reported");
        }
        for target in targets {
            println!("  [{}] {:?} {}", target.id, target.kind, target.title);
        }
        Ok(())
    }
    #[cfg(not(feature = "sdk"))]
    {
        let _ = kind;
        Err(anyhow!(
            "capture targets need the SDK: rebuild with --features sdk"
        ))
    }
}

fn capture(args: &[String]) -> Result<()> {
    let kind = if flag(args, "--windows") {
        TargetKind::Window
    } else {
        TargetKind::Screen
    };
    let frames = number(args, "--frames", 60)?;
    let patience = number(args, "--patience", 10)?;
    let out = option(args, "--out");

    if !flag(args, "--yes") {
        println!("this would capture your screen and write a file.");
        println!("re-run with --yes if that is what you want:");
        println!(
            "  probe capture --yes --frames {frames}{}",
            out.map(|o| format!(" --out {o}")).unwrap_or_default()
        );
        return Ok(());
    }

    #[cfg(feature = "sdk")]
    {
        use cordel_media::capture::ScreenCapture;
        use cordel_media::sdk::SdkDesktopCapture;
        use std::sync::mpsc;

        let backend = SdkDesktopCapture::new(kind)?;
        let available = backend.targets()?;
        let target = match option(args, "--target") {
            Some(id) => available
                .iter()
                .find(|t| t.id.to_string() == id)
                .cloned()
                .context("no such capture target")?,
            None => available
                .first()
                .cloned()
                .context("no capture target available")?,
        };
        println!("capturing [{}] {}", target.id, target.title);

        let (sender, receiver) = mpsc::channel();
        let mut backend = backend;
        let started = Instant::now();
        backend.start(
            &target,
            Box::new(move |frame| {
                let _ = sender.send(frame);
            }),
        )?;

        #[cfg(feature = "ui")]
        let mut sink = cordel_media::render::SlintFrameSink::new();

        let mut received = 0u64;
        let mut first: Option<Frame> = None;
        let mut tick_total = Duration::ZERO;
        while received < frames && started.elapsed() < Duration::from_secs(patience) {
            let tick_started = Instant::now();
            backend.tick()?;
            tick_total += tick_started.elapsed();
            while let Ok(frame) = receiver.try_recv() {
                if first.is_none() {
                    first = Some(frame.clone());
                }
                #[cfg(feature = "ui")]
                {
                    use cordel_media::render::FrameSink;
                    sink.present(&frame)?;
                }
                received += 1;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        backend.stop()?;
        let elapsed = started.elapsed();

        println!(
            "captured {received} frames in {:.2} s ({:.1} fps), {:.3} ms/sec of capture-tick time",
            elapsed.as_secs_f64(),
            received as f64 / elapsed.as_secs_f64(),
            tick_total.as_secs_f64() * 1000.0 / elapsed.as_secs_f64(),
        );
        match &first {
            Some(frame) => println!(
                "frame size {}x{}, stride {}, {} bytes ({} bytes of pixels)",
                frame.width(),
                frame.height(),
                frame.stride(),
                frame.data().len(),
                frame.rgba8_len()
            ),
            None => {
                println!("no frame arrived: the capturer produced nothing (permission or backend)")
            }
        }

        #[cfg(feature = "ui")]
        {
            use cordel_media::render::FrameSink;
            println!("  {}", sink.stats().summary());
        }

        if let (Some(path), Some(frame)) = (out, first) {
            write_ppm(&path, &frame)?;
            println!("wrote {path} (not printed here: it is a picture of your screen)");
        }
        Ok(())
    }
    #[cfg(not(feature = "sdk"))]
    {
        let _ = (kind, frames, out, patience);
        Err(anyhow!(
            "capture needs the SDK: rebuild with --features sdk"
        ))
    }
}

/// Writes a P6 PPM so a frame can be looked at without adding an image crate.
#[cfg(feature = "sdk")]
fn write_ppm(path: &str, frame: &Frame) -> Result<()> {
    let rgba = frame.to_rgba8();
    let mut out = format!("P6\n{} {}\n255\n", frame.width(), frame.height()).into_bytes();
    let (pixels, _) = rgba.as_chunks::<4>();
    for pixel in pixels {
        out.extend_from_slice(&pixel[..3]);
    }
    std::fs::write(path, out).with_context(|| format!("writing {path}"))?;
    Ok(())
}

fn join(args: &[String]) -> Result<()> {
    let url = option(args, "--url")
        .or_else(|| std::env::var("CORDEL_LIVEKIT_URL").ok())
        .context("--url or CORDEL_LIVEKIT_URL is required")?;
    let token = option(args, "--token")
        .or_else(|| std::env::var("CORDEL_LIVEKIT_TOKEN").ok())
        .context("--token or CORDEL_LIVEKIT_TOKEN is required")?;
    let seconds = number(args, "--seconds", 15)?;
    let share = flag(args, "--share");

    #[cfg(feature = "sdk")]
    {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(join_room(&url, &token, seconds, share))
    }
    #[cfg(not(feature = "sdk"))]
    {
        let _ = (url, token, seconds, share);
        Err(anyhow!(
            "joining needs the SDK: rebuild with --features sdk"
        ))
    }
}

#[cfg(feature = "sdk")]
async fn join_room(url: &str, token: &str, seconds: u64, share: bool) -> Result<()> {
    use cordel_media::capture::ScreenCapture;
    use cordel_media::devices::AudioDeviceControl;
    use cordel_media::frame::Frame;
    use cordel_media::sdk::{
        publish_microphone, received_to_frame, remote_video_stream, ScreenPublisher, SdkAudio,
        SdkDesktopCapture,
    };
    use livekit::{Room, RoomEvent, RoomOptions};
    use std::sync::mpsc;
    use tokio_stream::StreamExt;

    let (room, mut events) = Room::connect(url, token, RoomOptions::default())
        .await
        .context("connecting to the room")?;
    println!("connected to {url}");

    let mut audio = SdkAudio::new()?;
    audio.start_recording()?;
    let microphone = publish_microphone(&room, "microphone").await?;
    println!("published microphone (sid {})", microphone.sid());

    // Screen share: the publisher's resolution comes from the first real frame, so
    // the client never has to guess what the capturer will produce.
    let mut shared = None;
    if share {
        let mut backend = SdkDesktopCapture::new(TargetKind::Screen)?;
        let target = backend
            .targets()?
            .into_iter()
            .next()
            .context("no screen to share")?;
        println!("sharing [{}] {}", target.id, target.title);
        let (sender, receiver) = mpsc::channel::<Frame>();
        backend.start(
            &target,
            Box::new(move |frame| {
                let _ = sender.send(frame);
            }),
        )?;

        let first = wait_for_frame(&receiver, &mut backend, Duration::from_secs(5))?;
        let publisher = ScreenPublisher::new(first.width(), first.height(), "screen");
        publisher.publish(&room).await?;
        publisher.push(&first)?;
        println!(
            "published screen share at {}x{}",
            first.width(),
            first.height()
        );
        shared = Some((backend, receiver, publisher));
    }

    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        while let Ok(event) = events.try_recv() {
            match event {
                RoomEvent::TrackSubscribed { participant, .. } => {
                    println!("subscribed to a track from {}", participant.identity())
                }
                RoomEvent::ParticipantConnected(participant) => {
                    println!("participant joined: {}", participant.identity())
                }
                RoomEvent::Disconnected { reason } => {
                    println!("disconnected: {reason:?}");
                    return Ok(());
                }
                _ => {}
            }
        }

        if let Some((backend, receiver, publisher)) = shared.as_mut() {
            backend.tick()?;
            while let Ok(frame) = receiver.try_recv() {
                if let Err(error) = publisher.push(&frame) {
                    eprintln!("dropping frame: {error}");
                }
            }
        }

        for remote in room.remote_participants().values() {
            for publication in remote.track_publications().values() {
                // One frame per remote track is enough here: the point is to prove
                // the receive path decodes and lands in the shared frame type. The
                // product loop drains the stream continuously.
                if let Some(livekit::prelude::RemoteTrack::Video(video)) = publication.track() {
                    let mut stream = remote_video_stream(&video);
                    if let Some(frame) = stream.next().await {
                        match received_to_frame(&frame) {
                            Some(frame) => println!(
                                "received {}x{} from {}",
                                frame.width(),
                                frame.height(),
                                remote.identity()
                            ),
                            None => {
                                println!("received a frame in a layout this spike does not handle")
                            }
                        }
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(33)).await;
    }

    audio.stop_recording()?;
    room.close().await?;
    Ok(())
}

#[cfg(feature = "sdk")]
fn wait_for_frame(
    receiver: &std::sync::mpsc::Receiver<cordel_media::frame::Frame>,
    backend: &mut dyn cordel_media::capture::ScreenCapture,
    patience: Duration,
) -> Result<cordel_media::frame::Frame> {
    let deadline = Instant::now() + patience;
    while Instant::now() < deadline {
        backend.tick()?;
        if let Ok(frame) = receiver.try_recv() {
            return Ok(frame);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    Err(anyhow!(
        "the capturer produced no frame within {patience:?}"
    ))
}
