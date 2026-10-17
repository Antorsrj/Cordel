# Journal client behavior

**Status:** accepted product behavior under [ADR-107](../adr/client/ADR-107-journal-ux.md). Slint, multiwindow rendering, and each OS adapter still require the gates in ADR-101/103.

This text is the repository handoff for the approved Journal mock direction. No mock image is required to understand or implement it. The mock files remain outside Git. UI copy, spacing, typography, and icons can be refined without changing the behavior below.

## Information hierarchy

1. **Community and channel navigation:** show the active community, text channels, voice channels, and the user's current voice connection. Channel names come from server data. Every member can create channels. Admin-only community/member controls appear only when the server reports an admin role.
2. **Conversation:** show the current text channel, ordered messages, formatted text, safe links, protected inline image previews, and composer. The user can hide and restore this area; hide does not delete a draft or change the selected channel.
3. **People:** a visible or one-action-reachable roster shows who is online, who is in a voice room, and which confirmed members are speaking, muted, or sharing. Group people by voice room where possible and list online members outside voice separately. Never infer online status solely from a stale visual tile.
4. **Media:** show screen shares for the current voice room. Share identity follows the media track, not the tile position. Voice controls and the current connection state remain accessible while chat or a share is hidden.

## Required states

| State | Main window | Separate window | Expected behavior |
| --- | --- | --- | --- |
| No active share | Conversation and people have the space; no empty video stage | None | Voice room and roster still show who is connected |
| Two active shares | Both shares render concurrently; either can be emphasized, with chat and people still reachable | None | The user can identify each publisher and hear voice without interruption |
| One detached share | Remaining share stays live in the main window | Detached share stays live | Detach changes presentation only; one account session and one voice connection remain |

When the user closes a detached window while its source is still active, dock that share back into the main window. If the source ended or access was revoked, remove the detached view and show the remaining state. A normal network interruption marks views reconnecting and restores them from the refreshed share catalog. Closing a view never stops another person's publication and never leaves the voice room. The user may hide the conversation during any state and restore it with the same draft and scroll position where locally available.

## Message behavior

- The sender's pending message has a clear sending state. Server acknowledgement replaces that state with the authoritative message and per-channel order. A failed send remains recoverable and does not silently duplicate on retry.
- Restricted formatting includes paragraphs, bold, italic, inline code, lists, code blocks, and safe links. Raw HTML is never interpreted. URL cards are not required for the first test because Server 011 does not fetch arbitrary sites.
- An image message shows its author and timestamp with an inline preview, useful alt text or filename fallback, loading/error state, and an explicit action to open the protected original. A revoked user cannot fetch new protected bytes. Image storage and limits are gated by Server 011.
- The conversation supports long history and incremental loading without freezing typing, scrolling, audio, or video.

## Navigation, resize, and accessibility

- The active text channel and voice room are visually distinct. Voice participation does not force the user to leave a text channel. A share tile labels the publisher and sharing state.
- On wide windows, the channel index, conversation, media, and people area can coexist. On narrow windows, collapse secondary areas behind labeled controls; the conversation, roster, and voice controls remain one action away. No fixed pixel breakpoint is a product requirement; test the smallest window each OS will support.
- Use keyboard-reachable controls with visible focus, accessible names for icon-only actions, readable contrast, and non-color indicators for online/speaking/muted/sharing state. Screen-reader announcements for roster changes should avoid a burst for every heartbeat or frame.
- Permission denial, room revocation, source-ended, and network reconnect are distinct states. The UI does not auto-publish a microphone or screen in response to a layout action.

## Disposable sample data

The reference mocks used the community name `Raid Against The Machine`; text channels `hall-de-entrada`, `sala_de_estar`, `raspberry_pi`, `artificialinteligence`, and `fi`; and voice channels including `Sala de Estar VOZ`, `Os chatos da AI`, and `AFK_LDT`. Example members included `aralrj`, `Dudle`, `GiggMaster`, `gralrj`, `Tonho`, and `Coe`. These names are optional test fixtures only. They are not seed data, reserved names, or product defaults. The original screenshots truncated some labels, so no contributor should infer canonical full names from them.

## Handoff checks

- Use fake chat/presence/media data to demonstrate all three states before server integration. Then repeat with two real remote screen tracks and one detached native window on Linux X11 and Wayland.
- Exercise a crowded roster, 10,000-message history, formatted chat, an authorized image, a broken image, and a revoked image. Check keyboard path and screen-reader names.
- Disconnect and reconnect with a draft, an open detached share, and a pending message. Verify the draft survives, the pending message is deduplicated, and the roster/share catalog resynchronizes.
- Repeat the OS-specific window, device, and accessibility checks on macOS and Windows before claiming those platforms are supported. The browser diagnostic client is allowed a narrower interface.
