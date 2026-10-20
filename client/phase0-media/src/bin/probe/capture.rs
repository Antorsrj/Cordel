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
        use cordel_phase0_media::capture::ScreenCapture;
        use cordel_phase0_media::sdk::SdkDesktopCapture;
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
        let mut sink = cordel_phase0_media::render::SlintFrameSink::new();

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
                    use cordel_phase0_media::render::FrameSink;
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
            use cordel_phase0_media::render::FrameSink;
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
    use cordel_phase0_media::capture::ScreenCapture;
    use cordel_phase0_media::devices::AudioDeviceControl;
    use cordel_phase0_media::frame::Frame;
    use cordel_phase0_media::sdk::{
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
    receiver: &std::sync::mpsc::Receiver<cordel_phase0_media::frame::Frame>,
    backend: &mut dyn cordel_phase0_media::capture::ScreenCapture,
    patience: Duration,
) -> Result<cordel_phase0_media::frame::Frame> {
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
