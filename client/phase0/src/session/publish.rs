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
    let access_token =
        access_token.ok_or_else(|| anyhow!("sign in before joining a voice channel"))?;
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
    events: &MediaOut,
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
        share: None,
    })
}
