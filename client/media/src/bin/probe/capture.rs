fn capture(args: &[String]) -> Result<()> {
    let kind = target_kind(args);
    let source = requested_source(args)?;
    let frames = number(args, "--frames", 60)?;
    let patience = number(args, "--patience", 10)?;
    let out = option(args, "--out");

    if !flag(args, "--yes") {
        println!("this would capture your screen and write a file.");
        println!("re-run with --yes if that is what you want:");
        println!(
            "  probe capture --yes --frames {frames} --source {}{}",
            source.as_str(),
            out.map(|o| format!(" --out {o}")).unwrap_or_default()
        );
        return Ok(());
    }

    #[cfg(feature = "sdk")]
    {
        use cordel_media::capture::ScreenCapture;
        use cordel_media::sdk::SdkDesktopCapture;
        use std::sync::mpsc;

        let mut backend = SdkDesktopCapture::with_source(kind, source)?;
        let requested = backend.source();
        let available = backend.targets()?;
        println!(
            "backend: {}; capture path: {} (requested {})",
            backend.backend_name(),
            requested.as_str(),
            source.as_str()
        );
        // The portal handshake runs on the GLib main loop the SDK's
        // `glib-main-loop` feature starts; the backend no longer pumps it.
        let target = match option(args, "--target") {
            Some(id) => available
                .iter()
                .find(|t| t.id.to_string() == id)
                .cloned()
                .with_context(|| format!("no such capture target `{id}`"))?,
            None => match available.first().cloned() {
                Some(target) => target,
                // The portal path exposes no list before the grant, because the
                // picker decides the surface; the name is only used for logging.
                None if requested == CaptureSource::Portal => {
                    CaptureTarget::new(0, "portal picker", kind)
                }
                None => return Err(anyhow!("no capture target available")),
            },
        };
        println!("capturing [{}] {}", target.id, target.title);

        let (sender, receiver) = mpsc::channel();
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
            Some(frame) => {
                println!(
                    "frame size {}x{}, stride {}, {} bytes ({} bytes of pixels)",
                    frame.width(),
                    frame.height(),
                    frame.stride(),
                    frame.data().len(),
                    frame.rgba8_len()
                );
                println!("first frame content: {}", frame_stats(frame));
            }
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
        let _ = (kind, source, frames, out, patience);
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

/// Aggregate statistics over one captured frame.
///
/// This is how the probe shows that a captured frame is a real picture rather
/// than a black or uninitialised buffer, without printing or keeping the picture:
/// one pass, plus a coarse 12-bit colour histogram. The gate asks for the
/// *display* of a screen frame, so "frames arrived" alone is not evidence enough
/// to claim it.
#[cfg(feature = "sdk")]
fn frame_stats(frame: &Frame) -> String {
    let rgba = frame.to_rgba8();
    let (pixels, _) = rgba.as_chunks::<4>();
    if pixels.is_empty() {
        return "empty frame".to_string();
    }

    let count = pixels.len() as f64;
    let mut sum = [0f64; 3];
    let mut sum_sq = [0f64; 3];
    let mut min = [u8::MAX; 3];
    let mut max = [0u8; 3];
    let mut histogram = vec![0u32; 4096];

    for pixel in pixels {
        for (channel, target) in sum.iter_mut().enumerate() {
            let value = pixel[channel];
            *target += value as f64;
            sum_sq[channel] += (value as f64) * (value as f64);
            min[channel] = min[channel].min(value);
            max[channel] = max[channel].max(value);
        }
        let key = ((pixel[0] as usize >> 4) << 8)
            | ((pixel[1] as usize >> 4) << 4)
            | (pixel[2] as usize >> 4);
        histogram[key] += 1;
    }

    let mean = |channel: usize| sum[channel] / count;
    let sd = |channel: usize| {
        let m = mean(channel);
        (sum_sq[channel] / count - m * m).max(0.0).sqrt()
    };
    let list = |f: &dyn Fn(usize) -> f64| {
        (0..3)
            .map(|c| format!("{:.1}", f(c)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let colours = histogram.iter().filter(|&&n| n > 0).count();

    format!(
        "mean rgb [{}], sd [{}], min [{}, {}, {}], max [{}, {}, {}], {colours} distinct coarse colours (12-bit)",
        list(&mean),
        list(&sd),
        min[0],
        min[1],
        min[2],
        max[0],
        max[1],
        max[2]
    )
}
