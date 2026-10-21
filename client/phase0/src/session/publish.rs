/// Captures a single frame through whichever backend `cordel-media` picks for this
/// session, and waits for the portal grant if that is the path.
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
        // The portal handshake only progresses while `tick` keeps being called, so
        // this runs it even before the first frame arrives.
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

/// The room credentials for one join.
///
/// The prototype keeps the environment override, because the media spike
/// scripts and a manual room test supply a token directly. Otherwise the
/// client asks Axum for a scoped admission, which is the product path: the
/// server decides which account may enter which voice channel.
async fn room_credentials(access_token: Option<String>) -> anyhow::Result<(String, String)> {
    if let Ok(token) = std::env::var("CORDEL_LIVEKIT_TOKEN") {
        let url = std::env::var("CORDEL_LIVEKIT_URL")
            .map_err(|_| anyhow!("CORDEL_LIVEKIT_URL is not set"))?;
        return Ok((url, token));
    }
    let channel_id = std::env::var("CORDEL_VOICE_CHANNEL_ID")
        .map_err(|_| anyhow!("CORDEL_VOICE_CHANNEL_ID is not set"))?;
    let access_token = access_token.ok_or_else(|| anyhow!("sign in before joining a voice channel"))?;
    let origin = std::env::var("CORDEL_API_ORIGIN")
        .map_err(|_| anyhow!("CORDEL_API_ORIGIN is not set"))?;
    let admission = tokio::task::spawn_blocking(move || {
        let transport = admission_transport(&origin)?;
        let client = MediaAdmissionClient::new(transport);
        client
            .admission(&access_token, &channel_id)
            .map_err(|error| anyhow!("admission refused: {error}"))
    })
    .await??;
    Ok((admission.url, admission.token))
}

/// The transport for the admission route. A plain origin needs the
/// test-only `local-smoke` feature.
fn admission_transport(origin: &str) -> anyhow::Result<NativeHttpsTransport> {
    #[cfg(feature = "local-smoke")]
    if origin.starts_with("http://") {
        return NativeHttpsTransport::new_plain(origin)
            .map_err(|error| anyhow!("{error:?}"));
    }
    NativeHttpsTransport::new(origin).map_err(|error| anyhow!("{error:?}"))
}

async fn join_room(
    events: &Sender<MediaEvent>,
    access_token: Option<String>,
) -> anyhow::Result<Joined> {
    let (url, token) = room_credentials(access_token).await?;

    // Opening the platform audio selects the system microphone and speakers.
    // WebRTC's Linux audio device module refuses to initialise either direction
    // until a device has been selected, so this is what makes the microphone open
    // and remote audio audible at all.
    let mut audio = SdkAudio::new().context("platform audio")?;
    let (microphone, speakers) = audio.devices_selected();
    log::info!("audio devices selected: microphone={microphone} speakers={speakers}");
    let _ = events.send(MediaEvent::Status(format!(
        "Audio devices selected: microphone={microphone} speakers={speakers}. Joining."
    )));
    audio.start_recording().context("microphone")?;

    let (room, room_events) = Room::connect(&url, &token, RoomOptions::default()).await?;
    publish_microphone(&room, "microphone")
        .await
        .context("publishing the microphone")?;

    Ok(Joined {
        room,
        room_events,
        tracked: HashMap::new(),
        _audio: audio,
        publisher: None,
    })
}

/// Publishes the captured frame as a screen-share track and keeps pushing it, so a
/// subscriber that arrives later still gets the picture.
async fn publish_frame(
    joined: &mut Joined,
    captured: &Frame,
    events: &Sender<MediaEvent>,
) -> anyhow::Result<()> {
    if captured.width() == 0 || captured.height() == 0 {
        anyhow::bail!("captured frame has no dimensions");
    }
    joined.stop_publishing().await;

    // The publisher's resolution comes from the frame the capturer actually
    // produced, so the client never has to guess what the picker will hand it.
    let publisher = Arc::new(ScreenPublisher::new(
        captured.width(),
        captured.height(),
        "screen-frame",
    ));
    publisher.publish(&joined.room).await?;
    publisher.push(captured)?;

    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    joined.publisher = Some(stop);
    let frame = Arc::new(captured.clone());
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PUBLISH_INTERVAL);
        loop {
            tokio::select! {
                _ = stopped.changed() => break,
                _ = ticker.tick() => {
                    if let Err(err) = publisher.push(&frame) {
                        log::warn!("screen frame not pushed: {err}");
                    }
                }
            }
        }
    });
    let _ = events.send(MediaEvent::Status(format!(
        "Publishing the captured {}x{} frame as a screen-share track.",
        captured.width(),
        captured.height()
    )));
    Ok(())
}
