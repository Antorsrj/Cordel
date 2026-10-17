# ADR-108: Deliver the native client on macOS after the Linux test

**Status:** Accepted

**Delivery stage:** Platform roadmap after first test

**Date:** 2026-10-17

**Decider:** Project owner

## Context

macOS is the second native target. The first integrated product test uses Linux and the diagnostic browser. A successful Linux build does not prove macOS capture permissions, audio devices, rendering, packaging, or detached-window behavior. Shared Rust logic should be reused rather than forked.

## Decision

Use the shared client application, session state, protocol, Journal UI, and LiveKit orchestration from ADR-101/102/103/107. Implement macOS-specific screen/window capture and consent behind the established platform adapter. Evaluate ScreenCaptureKit with the pinned macOS SDK and Rust bindings in a disposable spike; choose the maintained binding or a small reviewed native bridge based on measured frame delivery and permission behavior. Keep microphone access behind explicit user action and OS permission. Package a launchable macOS application and document minimum supported macOS versions after dependency testing.

The macOS client must show two simultaneous remote screen tracks, detach one into a second window, preserve voice and chat, and reconnect after network loss or sleep/wake. No macOS-specific client protocol is allowed. If the shared UI renderer fails this platform gate, update ADR-101 with measured evidence and a compatible alternative before claiming macOS support.

## Implementation boundary

- **Owns:** macOS capture/permission/audio/device adapter, build and packaging configuration, and macOS-specific tests. Client 101 owns the shared shell, Client 103 owns the platform adapter interface, and Client 107 owns Journal behavior.
- **Handoff:** publish a tested macOS build recipe, minimum OS version, signing/notarization plan if distribution requires it, and evidence for permission prompts, two views, detach/dock, device changes, and sleep/wake. Do not modify server payloads solely for macOS.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Shared Rust app plus macOS adapter **(chosen)** | One product behavior with native capture/permissions | Binding and packaging proof required |
| Separate macOS application | Unrestricted platform UI choices | Duplicates state, protocol, and UX work |
| Browser-only macOS access | Faster test access | Does not satisfy the native macOS target |

## Consequences

- macOS does not block the first Linux/browser product test, but cannot be advertised as supported until this gate passes.
- Packaging and distribution must also satisfy ADR-106's license decision.
- The adapter may use macOS-specific code without changing shared client or server semantics.

## Verification and handoff

- Build and launch from a clean macOS machine at the documented minimum OS version. Exercise screen/window selection, consent denial/regrant, microphone selection, device hotplug, and sleep/wake.
- Run two clients on separate networks, publish a screen, receive two shares, detach/dock one, and measure audio continuity, CPU, memory, frame copies, and screen text readability.
- Test invite/login, chat gap recovery, presence resync, admin UI visibility, kick/ban denial, and protected image display against the shared v1 contract. Record versions and results; do not infer macOS support from Linux tests.

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md); [Journal behavior](../../ux/journal.md).

**Complexity:** 4/5
