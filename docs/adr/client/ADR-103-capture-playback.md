# ADR-103: Isolate screen and audio devices behind platform adapters

**Status:** Accepted

**Date:** 2026-10-17

**Decider:** Project owner

## Context

LiveKit forwards tracks but does not remove OS-specific capture and playback work. Screen APIs, consent, audio-device behavior, and package dependencies differ on Linux, macOS, and Windows. The first test needs up to two simultaneous screen-share publishers without harming voice quality.

## Decision

Define a common client interface for selecting a microphone and screen/window, starting and stopping capture, producing frames/samples for LiveKit, and rendering remote tracks. Implement Linux first with separate X11 and Wayland portal/PipeWire paths as needed. Validate macOS ScreenCaptureKit and Windows Graphics Capture in their later platform gates. Always require a visible user action and active OS permission before publishing microphone or screen content. Show when a track is live and stop it promptly when sharing ends or permission is lost.

Treat voice as higher priority than screen frame rate under CPU or network pressure. Negotiate codec, resolution, frame rate, simulcast layers, and bitrate based on measured text readability and host egress; do not hard-code the architecture document's sizing assumptions as product guarantees. Slint remote-frame rendering must be proven in Phase 0 before relying on it for the product UI.

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

- Linux: two-way audio, X11 and Wayland screen/window selection, publish and remote render, stop/restart, hotplug, and permission denial.
- Later OS gates: equivalent macOS and Windows tests, including installer launch and OS permission prompts.
- Load test two concurrent screen shares and verify voice continuity, text readability, client CPU, and server egress.

**References:** [Apple ScreenCaptureKit](https://developer.apple.com/documentation/ScreenCaptureKit), [Windows Graphics Capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture), [XDG ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html).
