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
//! probe mic --yes [--device ID]          open the microphone, select/hot-swap devices
//! probe targets [--source S]          capture sources (needs --features sdk)
//! probe capture --yes [--source S]    capture real frames and measure them
//! probe join --url URL --token TOKEN   join a room and publish (needs --features sdk)
//! ```
//!
//! `S` is `auto | portal | screen | window`, and defaults to `auto`, which
//! follows the session: the portal on Wayland, the direct capturer otherwise. The
//! explicit values exist because a Wayland session that also runs XWayland can
//! satisfy both, and separating them is the distinction the gate is about.
//! `--windows` is accepted as the older spelling of `--source window`.
//!
//! On Linux the portal path only progresses while the GLib main context runs.
//! The backend gets that from the LiveKit SDK's `glib-main-loop` feature, which
//! the crate enables in `Cargo.toml`; the report records the runs before and
//! after that was wired up, including the stall with no loop.

use anyhow::{anyhow, Context, Result};
use cordel_media::capture::{CaptureSource, CaptureTarget, SessionKind, TargetKind};
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

/// What kind of surface to ask for. X11 lists screens and windows separately,
/// while the portal picker decides on its own, so this only matters off-portal.
fn target_kind(args: &[String]) -> TargetKind {
    if flag(args, "--windows") || option(args, "--source").as_deref() == Some("window") {
        TargetKind::Window
    } else {
        TargetKind::Screen
    }
}

/// The capture path the user asked for. `--windows` is the older spelling of
/// `--source window`, kept so the recorded commands keep working.
fn requested_source(args: &[String]) -> Result<CaptureSource> {
    if flag(args, "--windows") {
        return Ok(CaptureSource::Window);
    }
    match option(args, "--source") {
        None => Ok(CaptureSource::Auto),
        Some(value) => CaptureSource::parse(&value).with_context(|| {
            format!("unknown --source `{value}`: use auto, portal, screen or window")
        }),
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
        // The id is printed because it is the value `mic --device` and the
        // client's stored preference take; the index is only a position in this
        // run's list and changes when devices are added or removed.
        let recording = audio.recording_devices()?;
        let playout = audio.playout_devices()?;
        for device in &recording {
            println!(
                "  [recording {:>2}] {}\n                   id: {}",
                device.index, device.name, device.id
            );
        }
        for device in &playout {
            println!(
                "  [playout   {:>2}] {}\n                   id: {}",
                device.index, device.name, device.id
            );
        }
        if recording.iter().any(|device| device.id.is_empty()) {
            println!(
                "note: this backend reports no per-device ids, so asking for anything\n\
                 \x20     other than the default cannot be honoured; see\n\
                 \x20     docs/spikes/media-phase0.md"
            );
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
        // `new` already selected the system defaults; this reports what it got,
        // because an unselected direction is silent for reasons nothing logs.
        let (mic_selected, speakers_selected) = audio.devices_selected();
        println!("devices selected: microphone={mic_selected} speakers={speakers_selected}");
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
    let kind = target_kind(args);
    let source = requested_source(args)?;

    #[cfg(feature = "sdk")]
    {
        use cordel_media::capture::ScreenCapture;
        use cordel_media::sdk::SdkDesktopCapture;

        let backend = SdkDesktopCapture::with_source(kind, source)?;
        println!("backend: {}", backend.backend_name());
        println!(
            "capture path: {} (requested {})",
            backend.source().as_str(),
            source.as_str()
        );
        let targets = backend.targets()?;
        if targets.is_empty() {
            if backend.source() == CaptureSource::Portal {
                println!(
                    "no targets listed: on the portal path the picker chooses the \
                     surface, and the list is empty until capture starts"
                );
            } else {
                println!("no targets reported");
            }
        }
        for target in targets {
            println!("  [{}] {:?} {}", target.id, target.kind, target.title);
        }
        Ok(())
    }
    #[cfg(not(feature = "sdk"))]
    {
        let _ = (kind, source);
        Err(anyhow!(
            "capture targets need the SDK: rebuild with --features sdk"
        ))
    }
}

include!("probe/capture.rs");
include!("probe/room.rs");
