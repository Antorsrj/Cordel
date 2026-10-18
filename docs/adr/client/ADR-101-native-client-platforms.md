# ADR-101: Build a shared native Rust client, validating Linux first

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

Cordel needs native clients for Linux, macOS, and Windows. Linux is the first priority, macOS second, and Windows later. Capture, audio devices, permissions, and packaging are OS-specific. A web test client exists separately; it does not justify making the native client a webview.

## Decision

Use Rust for shared application state, REST/WS, auth flow, and LiveKit orchestration. Use Slint as the **candidate** native UI and the LiveKit Rust SDK for media. The client provides the product UI. Every account starts in ordinary member mode; a designated admin may unlock Management Mode in Settings with their own password and a short-lived server grant. The headless server enforces each operation. Users may create text and voice channels but cannot manage community settings or members. Place screen capture, device handling, OS permissions, credential storage, and packaging behind platform adapters. Keep the server protocol platform-neutral. Do not assume a Linux build proves macOS or Windows support.

Phase 0 is a Linux feasibility gate, including X11 and Wayland as distinct capture paths. Demonstrate a two-person voice call and publish one captured screen frame. Then use two remote share publishers to receive and render both tracks simultaneously in Slint, detach one into a second window, and exercise a representative long chat list with the Journal layout in ADR-107. Measure audio continuity, CPU/RAM, frame copies, text readability, scroll behavior, and accessibility. If Slint or the capture path fails the gate, record a superseding ADR before building the whole client around it. Validate macOS next and Windows later under ADR-108/109.

## Implementation boundary

- **Owns:** shared Rust app shell, UI framework feasibility, Linux Phase 0, and platform adapter interfaces. Client 102 owns network/session state; Client 103 owns shared media interfaces and Linux media; Client 107 owns Journal presentation. Client 108/109 own later macOS/Windows delivery.
- **Handoff:** use fake service/media feeds for Linux Phase 0. Publish build instructions and measured X11/Wayland two-track/multiwindow evidence before other client work depends on Slint. Keep platform interfaces stable for Client 108/109.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Shared Rust plus Slint **(chosen conditionally)** | Native UI and shared language | Video rendering and dense chat need proof |
| Tauri/webview | Rich web UI and browser reuse | Changes the intended native-client model |
| Separate native clients per OS | Deep platform fit | Three UI and state implementations |

## Consequences

- The proposed Slint UI is not a blanket promise of performance or accessibility. The Linux prototype is a stop/go test.
- Linux support requires X11 and Wayland-specific evidence. macOS and Windows remain target platforms under ADR-108/109 and cannot be advertised as tested until their separate gates pass.
- UI licensing and repository terms are resolved under ADR-106 before distribution.

## Verification and handoff

- Record Phase 0 artifacts: build steps, two-way audio, capture and two-track remote render including a detached window, 10,000-message UI test, X11/Wayland results, and measurements.
- At each later OS gate, test microphone and screen permission prompts, device changes, sleep/wake, installer launch, and real network media.

**References:** [Slint desktop](https://slint.dev/desktop), [LiveKit Rust SDK](https://github.com/livekit/rust-sdks), [XDG ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
