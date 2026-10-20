/// Publishes captured frames as a screen-share track.
pub struct ScreenPublisher {
    source: NativeVideoSource,
    track: LocalVideoTrack,
    width: u32,
    height: u32,
}

impl ScreenPublisher {
    /// `width`/`height` must match the frames pushed later.
    pub fn new(width: u32, height: u32, name: &str) -> Self {
        let source = NativeVideoSource::new(
            livekit::webrtc::video_source::VideoResolution { width, height },
            true,
        );
        let track =
            LocalVideoTrack::create_video_track(name, RtcVideoSource::Native(source.clone()));
        Self {
            source,
            track,
            width,
            height,
        }
    }

    /// The resolution the track was created with. WebRTC may adapt the stream
    /// after negotiation, so this is the starting point, not a guarantee.
    pub fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Publishes the track to `room` with the screen-share source label, which is
    /// what tells the other side this is a screen and not a camera.
    pub async fn publish(&self, room: &Room) -> Result<LocalTrackPublication, RoomError> {
        room.local_participant()
            .publish_track(
                LocalTrack::Video(self.track.clone()),
                TrackPublishOptions {
                    source: TrackSource::Screenshare,
                    ..Default::default()
                },
            )
            .await
    }

    /// Sends one captured frame.
    ///
    /// The captured layout is BGRA and the encoder wants I420, so this uses
    /// WebRTC's own libyuv binding rather than a hand-written loop. libyuv calls
    /// the input "ARGB" even though the bytes in memory are B,G,R,A — the name is
    /// historical and the byte order is what matters.
    ///
    /// The buffer is sized from the frame, so a capturer that changes resolution
    /// mid-share is handled here rather than rejected; WebRTC scales to the
    /// encoding layers it negotiated.
    pub fn push(&self, frame: &Frame) -> Result<(), FrameError> {
        if frame.format() != PixelFormat::Bgra8 {
            return Err(FrameError::UnexpectedFormat {
                expected: PixelFormat::Bgra8,
                actual: frame.format(),
            });
        }
        let mut buffer = I420Buffer::new(frame.width(), frame.height());
        let (stride_y, stride_u, stride_v) = buffer.strides();
        {
            let (y, u, v) = buffer.data_mut();
            livekit::webrtc::native::yuv_helper::argb_to_i420(
                frame.data(),
                frame.stride(),
                y,
                stride_y,
                u,
                stride_u,
                v,
                stride_v,
                frame.width() as i32,
                frame.height() as i32,
            );
        }
        let video_frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        self.source.capture_frame(&video_frame);
        Ok(())
    }

    pub async fn unpublish(&self, room: &Room) -> Result<LocalTrackPublication, RoomError> {
        room.local_participant()
            .unpublish_track(&self.track.sid())
            .await
    }
}

/// Publishes the microphone.
///
/// Uses the SDK's `Device` audio source, so capture runs through the platform
/// audio module selected in [`SdkAudio`]. The caller is expected to have started
/// recording first; see the gate order in [`crate::consent`].
pub async fn publish_microphone(room: &Room, name: &str) -> Result<LocalAudioTrack, RoomError> {
    let track = LocalAudioTrack::create_audio_track(name, RtcAudioSource::Device);
    room.local_participant()
        .publish_track(
            LocalTrack::Audio(track.clone()),
            TrackPublishOptions {
                source: TrackSource::Microphone,
                ..Default::default()
            },
        )
        .await?;
    Ok(track)
}

/// Turns a subscribed video track into a stream of frames.
pub fn remote_video_stream(track: &RemoteVideoTrack) -> NativeVideoStream {
    NativeVideoStream::new(track.rtc_track())
}

/// Converts a received frame into the shared [`Frame`] type.
///
/// Returns `None` for layouts this spike does not handle yet. The planes are
/// copied into a tight buffer because the SDK's strides are not guaranteed to be
/// what the conversion loop expects.
pub fn received_to_frame(frame: &BoxVideoFrame) -> Option<Frame> {
    let buffer = frame.buffer.as_ref().as_i420()?;
    let (width, height) = (buffer.width(), buffer.height());
    let (chroma_width, chroma_height) = (buffer.chroma_width(), buffer.chroma_height());
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data();

    let mut data = Vec::with_capacity(
        width as usize * height as usize + 2 * chroma_width as usize * chroma_height as usize,
    );
    let copy_plane = |data: &mut Vec<u8>, plane: &[u8], stride: u32, rows: u32, row_bytes: u32| {
        for row in 0..rows {
            let start = (row * stride) as usize;
            data.extend_from_slice(&plane[start..start + row_bytes as usize]);
        }
    };
    copy_plane(&mut data, plane_y, stride_y, height, width);
    copy_plane(&mut data, plane_u, stride_u, chroma_height, chroma_width);
    copy_plane(&mut data, plane_v, stride_v, chroma_height, chroma_width);

    Frame::packed(width, height, PixelFormat::I420, data).ok()
}
