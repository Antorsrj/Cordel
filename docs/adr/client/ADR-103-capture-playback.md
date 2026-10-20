# ADR-103: Isolate screen and audio devices behind platform adapters

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

LiveKit forwards tracks but does not remove OS-specific capture and playback work. Screen APIs, consent, audio-device behavior, and package dependencies differ on Linux, macOS, and Windows. The first test needs up to two simultaneous screen-share publishers without harming voice quality.

## Decision

Define a common client interface for selecting a microphone and screen/window, starting and stopping capture, producing frames/samples for LiveKit, and rendering remote tracks. Implement Linux first with separate X11 and Wayland portal/PipeWire paths as needed. Validate macOS ScreenCaptureKit and Windows Graphics Capture in their later platform gates. Always require a visible user action and active OS permission before publishing microphone or screen content. Show when a track is live and stop it promptly when sharing ends or permission is lost.

Treat voice as higher priority than screen frame rate under CPU or network pressure. Negotiate codec, resolution, frame rate, simulcast layers, and bitrate based on measured text readability and host egress; do not hard-code the architecture document's sizing assumptions as product guarantees. Subscribe to and render two independent remote screen tracks at once. A detached view reuses the active media session and track subscription; it must not publish another track, rejoin the room, or duplicate full-resolution decoding merely because it moved windows. Select lower receive layers when a share is small or hidden, while preserving a readable enlarged view. Slint windows have separate global instances, and borrowed GPU textures cannot be assumed shareable between windows; keep shared media state in Rust and measure the actual frame-transfer path. Slint two-track and multiwindow remote-frame rendering must be proven in Phase 0 before relying on it for the Journal UI. Feed current-room speaker and mute track events to the Journal roster locally rather than asking Axum to synthesize them.

## Implementation boundary

- **Owns:** common media adapter interface, Linux X11/Wayland capture/audio adapter, LiveKit publish/subscribe control, remote frame surfaces, and detach/dock media plumbing. Client 107 owns window layout and controls; Client 101 owns app shell and UI feasibility. Client 108/109 own later macOS/Windows adapters.
- **Handoff:** expose stable track IDs and view handles so a fake renderer can test two integrated views and one detached view. Publish Linux permission and performance evidence, plus interface conformance tests reusable on later platforms.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Shared orchestration plus OS adapters **(chosen)** | One product behavior with correct OS permissions | Adapter work on each platform |
| One cross-platform capture crate only | Smaller code surface if it works | Wayland and permission gaps may block release |
| Separate media app per OS | Platform optimization | Duplicate logic and inconsistent behavior |

## Consequences

- Platform support is staged: Linux, then macOS, then Windows. A future browser client uses browser media APIs under ADR-104.
- Device hotplug, screen changes, and revoked permission are normal runtime events, not fatal crashes.
- The exact capture library and encoding settings are selected from measured prototype results.

## Verification and handoff

The [initial Linux investigation](../../spikes/media-phase0-initial.md) is preserved separately from the [later media and room evidence](../../spikes/media-phase0.md).

- Linux: two-way audio, X11 and Wayland screen/window selection, publish and remote render, stop/restart, hotplug, permission denial, and current-room speaker/mute event delivery from the pinned Rust SDK.
- Later OS gates: equivalent macOS and Windows tests, including installer launch and OS permission prompts.
- Load test two concurrent screen shares watched by one client and verify voice continuity, text readability, client CPU, and server egress. Detach and dock one share while both continue, then end one source and confirm the remaining view stays live.

**References:** [Apple ScreenCaptureKit](https://developer.apple.com/documentation/ScreenCaptureKit), [Windows Graphics Capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture), [XDG ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html).

**UI feasibility references:** [Slint globals across windows](https://docs.slint.dev/latest/docs/slint/reference/language/globals/), [borrowed OpenGL texture sharing](https://docs.slint.dev/latest/docs/rust/slint/struct.BorrowedOpenGLTextureBuilder), [LiveKit active-speaker events](https://docs.livekit.io/reference/client-sdk-js/enums/RoomEvent.html).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
