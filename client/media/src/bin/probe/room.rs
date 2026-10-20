fn join(args: &[String]) -> Result<()> {
    let url = option(args, "--url")
        .or_else(|| std::env::var("CORDEL_LIVEKIT_URL").ok())
        .context("--url or CORDEL_LIVEKIT_URL is required")?;
    let token = option(args, "--token")
        .or_else(|| std::env::var("CORDEL_LIVEKIT_TOKEN").ok())
        .context("--token or CORDEL_LIVEKIT_TOKEN is required")?;
    let seconds = number(args, "--seconds", 15)?;
    let share = flag(args, "--share");
    let source = requested_source(args)?;

    #[cfg(feature = "sdk")]
    {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(join_room(&url, &token, seconds, share, source))
    }
    #[cfg(not(feature = "sdk"))]
    {
        let _ = (url, token, seconds, share, source);
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
) -> Result<()> {
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
