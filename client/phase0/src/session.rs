use std::time::Duration;

use futures_util::StreamExt;
use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::webrtc::prelude::{I420Buffer, VideoBuffer, VideoFrame, VideoRotation};
use livekit::webrtc::video_frame::native::VideoFrameBufferExt;
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::{RtcVideoSource, VideoResolution};
use livekit::webrtc::video_stream::native::NativeVideoStream;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer, SharedString, Weak};
use tokio::sync::mpsc;

use crate::pixels::{downscale_rgba, i420_to_rgba, I420View};
use crate::screen::{capture_one_frame, CapturePath, CapturedFrame, PREVIEW_MAX_WIDTH};
use crate::AppWindow;

pub enum Command {
    Join,
    CaptureFrame,
    Leave,
}

struct Joined {
    room: Room,
    _audio: PlatformAudio,
    stop: tokio::sync::watch::Sender<bool>,
}

pub async fn run(mut commands: mpsc::UnboundedReceiver<Command>, ui: Weak<AppWindow>) {
    let mut joined: Option<Joined> = None;
    let mut screen: Option<CapturedFrame> = None;
    let mut pumping = false;

    while let Some(command) = commands.recv().await {
        match command {
            Command::Join => {
                if joined.is_some() {
                    continue;
                }
                set_busy(&ui, true);
                match join_room().await {
                    Ok((session, mut events)) => {
                        let ui_events = ui.clone();
                        tokio::spawn(async move {
                            while let Some(event) = events.recv().await {
                                handle_room_event(event, &ui_events).await;
                            }
                        });
                        set_status(
                            &ui,
                            "Joined. Microphone is publishing. Remote audio uses the platform output device.",
                        );
                        set_joined(&ui, true);
                        joined = Some(session);
                        if screen.is_some() && !pumping {
                            if let Err(err) = publish_still(joined.as_ref(), screen.as_ref()).await
                            {
                                set_status(
                                    &ui,
                                    &format!(
                                        "Joined, but the captured frame was not published: {err}"
                                    ),
                                );
                            } else {
                                pumping = true;
                            }
                        }
                    }
                    Err(err) => set_status(&ui, &format!("Join failed: {err}")),
                }
                set_busy(&ui, false);
            }
            Command::CaptureFrame => {
                set_busy(&ui, true);
                let path = CapturePath::detect();
                set_status(
                    &ui,
                    &format!(
                        "Capturing one frame through {}. Confirm the system picker if it appears.",
                        path.label()
                    ),
                );
                match capture_one_frame(path) {
                    Ok(captured) => {
                        show_preview(&ui, &captured);
                        set_status(
                            &ui,
                            &format!(
                                "Captured one {} frame {}x{} ({} copies: capturer, I420, preview). {}",
                                captured.path.label(),
                                captured.planes.width,
                                captured.planes.height,
                                captured.copy_count,
                                if joined.is_some() {
                                    "Publishing that still frame."
                                } else {
                                    "Join to publish it."
                                }
                            ),
                        );
                        screen = Some(captured);
                        if joined.is_some() && !pumping {
                            if let Err(err) = publish_still(joined.as_ref(), screen.as_ref()).await
                            {
                                set_status(&ui, &format!("Frame captured, publish failed: {err}"));
                            } else {
                                pumping = true;
                            }
                        }
                    }
                    Err(err) => set_status(&ui, &format!("Screen capture failed: {err}")),
                }
                set_busy(&ui, false);
            }
            Command::Leave => {
                if let Some(session) = joined.take() {
                    let _ = session.stop.send(true);
                    let _ = session.room.close().await;
                    drop(session._audio);
                }
                pumping = false;
                set_joined(&ui, false);
                set_status(&ui, "Left the room. The microphone is released.");
            }
        }
    }
}

async fn join_room() -> anyhow::Result<(Joined, mpsc::UnboundedReceiver<RoomEvent>)> {
    let url = std::env::var("CORDEL_LIVEKIT_URL")
        .map_err(|_| anyhow::anyhow!("CORDEL_LIVEKIT_URL is not set"))?;
    let token = std::env::var("CORDEL_LIVEKIT_TOKEN")
        .map_err(|_| anyhow::anyhow!("CORDEL_LIVEKIT_TOKEN is not set"))?;

    let audio = PlatformAudio::new().map_err(|err| anyhow::anyhow!("platform audio: {err}"))?;
    if let Some(device) = audio.recording_devices().next() {
        audio
            .set_recording_device(&device.id)
            .map_err(|err| anyhow::anyhow!("microphone: {err}"))?;
        log::info!("microphone: {}", device.name);
    }
    if let Some(device) = audio.playout_devices().next() {
        audio
            .set_playout_device(&device.id)
            .map_err(|err| anyhow::anyhow!("speaker: {err}"))?;
        log::info!("speaker: {}", device.name);
    }

    let (room, events) = Room::connect(&url, &token, RoomOptions::default()).await?;
    let track = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
    room.local_participant()
        .publish_track(
            LocalTrack::Audio(track),
            TrackPublishOptions {
                source: TrackSource::Microphone,
                ..Default::default()
            },
        )
        .await?;

    let (stop, _) = tokio::sync::watch::channel(false);
    Ok((
        Joined {
            room,
            _audio: audio,
            stop,
        },
        events,
    ))
}

async fn publish_still(
    joined: Option<&Joined>,
    captured: Option<&CapturedFrame>,
) -> anyhow::Result<()> {
    let Some(joined) = joined else {
        return Ok(());
    };
    let Some(captured) = captured else {
        return Ok(());
    };
    let width = captured.planes.width;
    let height = captured.planes.height;
    if width == 0 || height == 0 {
        anyhow::bail!("captured frame has no even dimensions");
    }

    let mut buffer = I420Buffer::new(width, height);
    {
        let (y, u, v) = buffer.data_mut();
        copy_plane(y, &captured.planes.y);
        copy_plane(u, &captured.planes.u);
        copy_plane(v, &captured.planes.v);
    }
    let frame = VideoFrame {
        rotation: VideoRotation::VideoRotation0,
        timestamp_us: 0,
        frame_metadata: None,
        buffer,
    };
    let source = NativeVideoSource::new(VideoResolution { width, height }, true);
    let track =
        LocalVideoTrack::create_video_track("screen-frame", RtcVideoSource::Native(source.clone()));
    joined
        .room
        .local_participant()
        .publish_track(
            LocalTrack::Video(track),
            TrackPublishOptions {
                source: TrackSource::Screenshare,
                ..Default::default()
            },
        )
        .await?;

    let mut stop = joined.stop.subscribe();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                _ = stop.changed() => break,
                _ = ticker.tick() => {
                    source.capture_frame(&frame);
                }
            }
        }
    });
    Ok(())
}

fn rgba_from_video_buffer(buffer: &dyn VideoBuffer) -> Option<Vec<u8>> {
    if let Some(native) = buffer.as_native() {
        return Some(rgba_from_i420(&native.to_i420()));
    }
    buffer.as_i420().map(rgba_from_i420)
}

fn rgba_from_i420(buffer: &I420Buffer) -> Vec<u8> {
    let (y_plane, u_plane, v_plane) = buffer.data();
    let (stride_y, stride_u, stride_v) = buffer.strides();
    i420_to_rgba(I420View {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        stride_y: stride_y as usize,
        stride_u: stride_u as usize,
        stride_v: stride_v as usize,
        width: buffer.width() as usize,
        height: buffer.height() as usize,
    })
}

fn copy_plane(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}

async fn handle_room_event(event: RoomEvent, ui: &Weak<AppWindow>) {
    let RoomEvent::TrackSubscribed { track, .. } = event else {
        return;
    };
    let RemoteTrack::Video(track) = track else {
        return;
    };
    let ui = ui.clone();
    tokio::spawn(async move {
        let mut stream = NativeVideoStream::new(track.rtc_track());
        if let Some(frame) = stream.next().await {
            let Some(rgba) = rgba_from_video_buffer(frame.buffer.as_ref()) else {
                set_status(
                    &ui,
                    "A remote video frame arrived in an unsupported buffer format.",
                );
                return;
            };
            let (rgba, width, height) = downscale_rgba(
                &rgba,
                frame.buffer.width() as usize,
                frame.buffer.height() as usize,
                PREVIEW_MAX_WIDTH,
            );
            let _ = ui.upgrade_in_event_loop(move |ui| {
                ui.set_frame(image_from_rgba(&rgba, width, height));
                ui.set_status(format!("Remote frame rendered in Slint, {width}x{height}.").into());
            });
        }
    });
}

fn show_preview(ui: &Weak<AppWindow>, captured: &CapturedFrame) {
    let rgba = captured.preview_rgba.clone();
    let width = captured.preview_width as usize;
    let height = captured.preview_height as usize;
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_frame(image_from_rgba(&rgba, width, height)));
}

fn image_from_rgba(rgba: &[u8], width: usize, height: usize) -> Image {
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width as u32, height as u32);
    let bytes = pixels.make_mut_bytes();
    let n = bytes.len().min(rgba.len());
    bytes[..n].copy_from_slice(&rgba[..n]);
    Image::from_rgba8(pixels)
}

fn set_status(ui: &Weak<AppWindow>, text: &str) {
    let text: SharedString = text.into();
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_status(text));
}

fn set_joined(ui: &Weak<AppWindow>, joined: bool) {
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_joined(joined));
}

fn set_busy(ui: &Weak<AppWindow>, busy: bool) {
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_busy(busy));
}
