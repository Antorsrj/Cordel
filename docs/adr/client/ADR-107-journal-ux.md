# ADR-107: Use the Journal interface for community conversation

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The owner selected the Journal mock direction for the native client. The approved examples show a community channel index, readable chat with formatted text and images, a visible roster of connected people, and three media states: no active screen share, two shares in the client, and one share in a separate window while another remains in the client. The names in the reference screenshots are example community data, not product defaults.

## Decision

Use a text-forward Journal layout with a persistent community/channel navigation area, a conversation area that the user can hide and restore, and a readily accessible people area. The people area shows online members and their current voice room, where available; people who are online but not in voice remain discoverable. Show sharing from the reconciled server snapshot. Show speaking and mute indicators from the connected client's LiveKit room/track events for its current voice room only; omit unconfirmed indicators for other rooms. The roster is an interaction surface for finding people in a busy community, not just a numeric online count. The headless server provides state and enforces permissions. All clients start in ordinary member mode. A designated admin uses Settings to enter Management Mode with their own account password; only then does the client render community/member management controls. Ordinary members retain channel creation under ADR-003.

The conversation renders a restricted set of rich-text elements, safe links, and authorized inline image previews from ADR-011. Provide a readable loading state, a failed-image state, and an explicit open-image action. Do not render arbitrary HTML or treat a URL preview as proof that a remote site is safe. The user may hide the conversation to give media more space and restore it without losing scroll position or draft text.

When no screen share is active, conversation and people are the main surfaces; do not show an empty video stage. With two active shares, the user can view both at once inside the client, switch their relative prominence, and still reach chat and the people area. A share can be detached into a separate native window while another stays in the main window. Detach and dock change presentation only: they do not leave the voice room, start a second publication, or require a second account login. Closing a detached window docks an active share back into the main window; it must never stop another person's share or unexpectedly leave voice. If a source ends or access is revoked, remove its view promptly and show the remaining state.

Keep the state model and controls consistent on Linux, macOS, and Windows. Window chrome, shortcuts, and screen-reader integration may follow the OS. The browser diagnostic client in ADR-104 is not required to reproduce the full Journal layout. The prototype's sample community, channel, and person names are fixture data only. The [written Journal behavior spec](../../ux/journal.md) is the repository handoff; contributors do not need the mock files.

## Implementation boundary

- **Owns:** Journal view models, layout, Management Mode lock/unlock interactions, accessibility states, and local presentation preferences. Client 102 owns in-memory admin-grant state; Client 103 owns media surfaces; Server 010/011 own roster and image data.
- **Handoff:** test all three states with committed text fixtures and fake media tracks. Do not rely on external mock images, hardcode reference community names, or request server permissions based on UI visibility.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Journal layout **(chosen)** | Clear conversation hierarchy and room for simultaneous media | Dense screens and multiwindow rendering need validation |
| Video-first layout | Maximizes the current share | Hides community conversation and people |
| Conventional chat clone | Familiar | Does not meet the selected visual direction |

## Consequences

- ADR-010 supplies online, voice, and share state; ADR-011 supplies durable rich content and protected image access.
- ADR-103 must validate two concurrent subscriptions and a detachable renderer, not just two publishers.
- Persist local presentation preferences and draft/scroll state without making them server authorization state.
- Slint remains a candidate under ADR-101. If its rendering or multiwindow behavior fails the Linux gate, select a viable renderer/UI with a superseding ADR while preserving these user-facing behaviors.

## Verification and handoff

- Exercise the three approved states with a seeded test community, including a long member list, rich text, and image messages. Check narrow-window behavior, keyboard traversal, readable contrast, and screen-reader names for controls and presence indicators.
- With two real incoming screen tracks, verify both are visible, voice remains intelligible, one track can be detached and docked, and closing its window does not leave the call or stop the other track.
- Hide/show conversation during an active call and verify draft and scroll preservation. End a share, revoke access, disconnect, and reconnect while a view is detached.
- Verify an admin sees the ordinary member view on login, can unlock Management Mode through Settings with their own password, and returns to the ordinary view on explicit lock, grant expiry, logout, role removal, and restart. A regular user must never obtain management controls, even with a locally modified client.
- Measure CPU, memory, frame copies, and text readability on the Linux Phase 0 build. Repeat platform-specific window and accessibility checks on macOS and Windows before claiming support.

**Related:** [ADR-101](ADR-101-native-client-platforms.md), [ADR-103](ADR-103-capture-playback.md), [ADR-010](../server/ADR-010-presence-and-share-state.md), [ADR-011](../server/ADR-011-rich-chat-images.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 4/5
