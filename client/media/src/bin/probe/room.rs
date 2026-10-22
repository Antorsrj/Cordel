fn join(args: &[String]) -> Result<()> {
    let url = option(args, "--url")
        .or_else(|| std::env::var("CORDEL_LIVEKIT_URL").ok())
        .context("--url or CORDEL_LIVEKIT_URL is required")?;
    let token = option(args, "--token")
        .or_else(|| std::env::var("CORDEL_LIVEKIT_TOKEN").ok())
        .context("--token or CORDEL_LIVEKIT_TOKEN is required")?;
    let seconds = number(args, "--seconds", 15)?;
    let share = flag(args, "--share");
    // A headless run has no audio device and no display, so the probe can
    // publish a synthetic screen track instead of capturing one.
    let no_audio = flag(args, "--no-audio");
    let synthetic = flag(args, "--synthetic-screen");
    let source = requested_source(args)?;

    #[cfg(feature = "sdk")]
    {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(join_room(
            &url, &token, seconds, share, source, no_audio, synthetic,
        ))
    }
    #[cfg(not(feature = "sdk"))]
    {
        let _ = (url, token, seconds, share, source, no_audio, synthetic);
        Err(anyhow!(
            "joining needs the SDK: rebuild with --features sdk"
        ))
    }
}

#[cfg(feature = "sdk")]
async fn join_room(
    url: &str,
    token: &str,
    seconds: u64,
    share: bool,
    source: CaptureSource,
    no_audio: bool,
    synthetic: bool,
) -> Result<()> {
    use cordel_media::capture::ScreenCapture;
    use cordel_media::devices::AudioDeviceControl;
    use cordel_media::frame::{synthetic_bgra, Frame};
    use std::sync::Arc;
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
    // A participant that joins later does not get a ParticipantConnected event
    // for the ones already there; the room itself is the source of truth.
    for participant in room.remote_participants().values() {
        println!("participant already here: {}", participant.identity());
    }

    let mut audio = None;
    if no_audio {
        println!("audio skipped (headless run)");
    } else {
        let mut device = SdkAudio::new()?;
        device.start_recording()?;
        let microphone = publish_microphone(&room, "microphone").await?;
        println!("published microphone (sid {})", microphone.sid());
        audio = Some(device);
    }

    // Screen share: the publisher's resolution comes from the first real frame, so
    // the client never has to guess what the capturer will produce.
    let mut shared = None;
    let mut synthetic_phase: u8 = 0;
    if share && synthetic {
        let first = synthetic_bgra(640, 360, 0);
        let publisher = Arc::new(ScreenPublisher::new(
            first.width(),
            first.height(),
            "synthetic-screen",
        ));
        publisher.publish(&room).await?;
        publisher.push(&first)?;
        println!(
            "published synthetic screen share at {}x{}",
            first.width(),
            first.height()
        );
        let (sender, receiver) = mpsc::channel::<Frame>();
        drop(sender);
        shared = Some((None, receiver, publisher));
    } else if share {
        let mut backend = SdkDesktopCapture::with_source(TargetKind::Screen, source)?;
        println!(
            "capture path: {} (requested {})",
            backend.source().as_str(),
            source.as_str()
        );
        let target = match backend.targets()?.into_iter().next() {
            Some(target) => target,
            None if backend.source() == CaptureSource::Portal => {
                CaptureTarget::new(0, "portal picker", TargetKind::Screen)
            }
            None => return Err(anyhow!("no screen to share")),
        };
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
        shared = Some((Some(backend), receiver, Arc::new(publisher)));
    }

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut followed: std::collections::HashSet<String> = std::collections::HashSet::new();
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
                other => println!("room event: {other:?}"),
            }
        }

        if let Some((backend, receiver, publisher)) = shared.as_mut() {
            if let Some(backend) = backend.as_mut() {
                backend.tick()?;
                while let Ok(frame) = receiver.try_recv() {
                    if let Err(error) = publisher.push(&frame) {
                        eprintln!("dropping frame: {error}");
                    }
                }
            } else {
                // A synthetic source keeps producing frames, the way a real
                // capture does. A single frame is not enough for the encoder
                // to carry the track to the server.
                synthetic_phase = synthetic_phase.wrapping_add(1);
                let frame = synthetic_bgra(640, 360, synthetic_phase);
                if let Err(error) = publisher.push(&frame) {
                    eprintln!("dropping frame: {error}");
                }
            }
        }

        // One long-lived reader per remote track. A new stream per loop would
        // drop the receiver the track was feeding, and the frames would stop.
        for remote in room.remote_participants().values() {
            for publication in remote.track_publications().values() {
                let Some(livekit::prelude::RemoteTrack::Video(video)) = publication.track() else {
                    continue;
                };
                let key = publication.sid().to_string();
                if !followed.insert(key) {
                    continue;
                }
                let identity = remote.identity();
                tokio::spawn(async move {
                    let mut stream = remote_video_stream(&video);
                    while let Some(frame) = stream.next().await {
                        match received_to_frame(&frame) {
                            Some(frame) => println!(
                                "received {}x{} from {}",
                                frame.width(),
                                frame.height(),
                                identity
                            ),
                            None => {
                                println!("received a frame in a layout this spike does not handle")
                            }
                        }
                    }
                });
            }
        }

        tokio::time::sleep(Duration::from_millis(33)).await;
    }

    if let Some(device) = audio.as_mut() {
        device.stop_recording()?;
    }
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
