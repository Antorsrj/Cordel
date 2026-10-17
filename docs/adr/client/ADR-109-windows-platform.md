# ADR-109: Deliver the native client on Windows after macOS

**Status:** Accepted

**Delivery stage:** Platform roadmap after first test

**Date:** 2026-10-17

**Decider:** Project owner

## Context

Windows is the third native target. The first integrated product test uses Linux and the diagnostic browser; macOS validation follows first. Windows capture, audio devices, permission behavior, graphics interop, packaging, and detached windows need direct evidence. Shared Rust logic should not fork by OS.

## Decision

Use the shared client application, session state, protocol, Journal UI, and LiveKit orchestration from ADR-101/102/103/107. Implement Windows-specific screen/window capture and device handling behind the established platform adapter. Evaluate Windows Graphics Capture with the pinned Windows SDK and Rust bindings in a disposable spike; choose a maintained binding or a small reviewed native bridge based on actual capture and rendering results. Require a user action before microphone or screen publication and respect OS permission and selection behavior. Package a launchable Windows application and document minimum supported Windows versions after dependency testing.

The Windows client must show two simultaneous remote screen tracks, detach one into a second window, preserve voice and chat, and reconnect after network loss or sleep/wake. No Windows-specific client protocol is allowed. If the shared UI renderer fails this platform gate, update ADR-101 with measured evidence and a compatible alternative before claiming Windows support.

## Implementation boundary

- **Owns:** Windows capture/audio/device adapter, build and packaging configuration, and Windows-specific tests. Client 101 owns the shared shell, Client 103 owns the adapter interface, and Client 107 owns Journal behavior.
- **Handoff:** publish a tested Windows build recipe, minimum OS version, installer plan, and evidence for source selection, two views, detach/dock, device changes, and sleep/wake. Do not modify server payloads solely for Windows.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Shared Rust app plus Windows adapter **(chosen)** | One product behavior with native capture | Graphics interop and packaging proof required |
| Separate Windows application | Platform-specific flexibility | Duplicates state, protocol, and UX work |
| Browser-only Windows access | Faster test access | Does not satisfy the native Windows target |

## Consequences

- Windows does not block the first Linux/browser product test, but cannot be advertised as supported until this gate passes.
- Packaging and distribution must also satisfy ADR-106's license decision.
- The adapter may use Windows-specific code without changing shared client or server semantics.

## Verification and handoff

- Build and launch from a clean Windows machine at the documented minimum OS version. Exercise screen/window selection, permission denial/regrant, microphone selection, device hotplug, and sleep/wake.
- Run two clients on separate networks, publish a screen, receive two shares, detach/dock one, and measure audio continuity, CPU, memory, frame copies, and screen text readability.
- Test invite/login, chat gap recovery, presence resync, admin UI visibility, kick/ban denial, and protected image display against the shared v1 contract. Record versions and results; do not infer Windows support from Linux or macOS tests.

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md); [Journal behavior](../../ux/journal.md).

**Complexity:** 4/5
