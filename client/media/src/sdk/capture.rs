/// Screen or window capture through WebRTC's desktop capturer.
///
/// On Linux the capturer's backend is whatever `webrtc-sys` was built with, and
/// the `DesktopCaptureSourceType` request selects between two different code
/// paths: `Generic` is the portal-aware capturer (the XDG ScreenCast portal and
/// PipeWire), while `Screen` and `Window` select the direct X11 capturers. The
/// spike report records which path this machine was actually driven down.
pub struct SdkDesktopCapture {
    capturer: DesktopCapturer,
    kind: TargetKind,
    source: CaptureSource,
    started: bool,
}

impl SdkDesktopCapture {
    /// Session-appropriate default: the portal on Wayland, the direct capturer
    /// otherwise. Use [`Self::with_source`] to force one path.
    pub fn new(kind: TargetKind) -> Result<Self, CaptureError> {
        Self::with_source(kind, CaptureSource::Auto)
    }

    /// Requests a specific capture path.
    ///
    /// Requesting the portal on an X11-only session, or the X11 capturer on a
    /// Wayland session, is expected to fail rather than silently fall back: the
    /// caller needs to know which path it really got, because that is the whole
    /// distinction the Phase 0 gate is asking about.
    pub fn with_source(kind: TargetKind, source: CaptureSource) -> Result<Self, CaptureError> {
        let source = source.resolve(SessionKind::detect());
        let source_type = match source {
            // `Auto` was resolved just above; it is grouped here only to keep the
            // match total.
            CaptureSource::Auto | CaptureSource::Portal => DesktopCaptureSourceType::Generic,
            CaptureSource::Screen => DesktopCaptureSourceType::Screen,
            CaptureSource::Window => DesktopCaptureSourceType::Window,
        };
        let mut options = DesktopCapturerOptions::new(source_type);
        options.set_include_cursor(true);
        DesktopCapturer::new(options)
            .map(|capturer| Self {
                capturer,
                kind,
                source,
                started: false,
            })
            .ok_or_else(|| {
                CaptureError::Unavailable(
                    "WebRTC could not create a desktop capturer for this session; \
                     on Wayland the desktop portal has to be installed and running"
                        .to_string(),
                )
            })
    }

    /// The capture path actually requested, after `Auto` was resolved.
    pub fn source(&self) -> CaptureSource {
        self.source
    }

    /// The capturer's own view of what is available, before any filtering.
    pub fn source_count(&self) -> usize {
        self.capturer.get_source_list().len()
    }
}

/// The WebRTC desktop capturer reports a temporary failure while a capture is
/// still in progress, which is the normal state between a request and its
/// frame. Repeating that message for every frame would flood the terminal.
fn is_temporary(error: &livekit::webrtc::desktop_capturer::CaptureError) -> bool {
    matches!(
        error,
        livekit::webrtc::desktop_capturer::CaptureError::Temporary
    )
}

impl ScreenCapture for SdkDesktopCapture {
    fn backend_name(&self) -> &'static str {
        "libwebrtc-desktop-capturer"
    }

    fn targets(&self) -> Result<Vec<CaptureTarget>, CaptureError> {
        let kind = self.kind;
        Ok(self
            .capturer
            .get_source_list()
            .into_iter()
            .map(|s| CaptureTarget::new(s.id(), s.title(), kind))
            .collect())
    }

    fn start(
        &mut self,
        target: &CaptureTarget,
        on_frame: FrameCallback,
    ) -> Result<(), CaptureError> {
        // On the portal path the picker chooses the surface, and the source list
        // can be empty until a grant exists, so there is no id to select here:
        // passing one would fail before the portal is ever asked. `None` starts
        // the capturer's own session and lets the portal decide.
        let selected = if self.source == CaptureSource::Portal {
            None
        } else {
            Some(
                self.capturer
                    .get_source_list()
                    .into_iter()
                    .find(|s| s.id() == target.id)
                    .ok_or(CaptureError::UnknownTarget(target.id))?,
            )
        };

        let mut on_frame = on_frame;
        self.capturer
            .start_capture(selected, move |result| match result {
                Ok(frame) => {
                    let (width, height) =
                        (frame.width().max(0) as u32, frame.height().max(0) as u32);
                    match Frame::new(
                        width,
                        height,
                        frame.stride(),
                        PixelFormat::Bgra8,
                        frame.data().to_vec(),
                    ) {
                        Ok(frame) => on_frame(frame),
                        Err(error) => eprintln!("media probe: captured frame rejected ({error})"),
                    }
                }
                Err(error) => {
                    // Um erro temporario significa que o pedido anterior ainda
                    // esta em andamento. Repetir a mensagem a cada quadro
                    // inundaria o terminal sem acrescentar nada.
                    if !is_temporary(&error) {
                        eprintln!("media capture error ({error:?})");
                    }
                }
            });
        self.started = true;
        Ok(())
    }

    fn tick(&mut self) -> Result<(), CaptureError> {
        if self.started {
            // This is the call that actually asks WebRTC for a frame; the callback
            // handed to `start` runs synchronously from here on this thread. The
            // portal handshake that has to complete first is driven by the GLib
            // main loop the SDK's `glib-main-loop` feature runs; see the crate
            // documentation and `docs/spikes/media-phase0.md`.
            self.capturer.capture_frame();
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        // WebRTC keeps the session until the capturer is dropped, so this only
        // stops delivering frames. Said plainly because it surprises people.
        self.started = false;
        Ok(())
    }
}
