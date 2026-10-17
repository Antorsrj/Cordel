# ADR-008: Keep the server ready for future end-to-end encryption

**Status:** Proposed

**Date:** 2026-10-17

**Decider:** Project owner before the final release

## Context

The first test may use transport encryption without E2EE. Before the final release, text, voice, and screen sharing need E2EE. LiveKit supports E2EE for media when clients supply keys, but Cordel's durable text chat is stored through Axum/PostgreSQL, not LiveKit data channels. The server must not claim E2EE merely because it uses HTTPS or WebRTC.

## Proposed decision

Keep the chat wire and storage model able to carry versioned encrypted content envelopes and the minimum routing metadata separately. For the final release, clients encrypt message bodies before upload; PostgreSQL stores ciphertext, message order, IDs, timestamps, and authorization metadata. The server checks membership and routes ciphertext but does not possess content keys. The media path uses client-side LiveKit E2EE after the native and browser SDKs prove the required support and interoperability. Key generation, distribution, rotation, and recovery belong to clients under ADR-105, with server endpoints limited to opaque public-key or encrypted-key-envelope delivery if needed.

Do not silently label pre-E2EE plaintext history as encrypted. Before migration, choose whether old messages are re-encrypted by authorized clients, retained with an explicit legacy label, or removed. Reassess server-side search, moderation of content, abuse handling, backups, and support diagnostics under ciphertext.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Client-encrypted text plus media E2EE **(proposed)** | Server cannot read content | Key lifecycle, search, and migration complexity |
| TLS and WebRTC transport encryption only | Simple first test | Does not meet final-release requirement |
| Server-managed shared keys | Easier recovery | Server can decrypt; not the intended E2EE boundary |

## Consequences

- The first test can launch without E2EE, but protocol versioning and content representation must not prevent adding it.
- Membership changes must trigger key-rotation policy; removing server access alone does not erase keys already held by a former member.
- This ADR does not choose cryptographic primitives or recovery policy before an interoperable prototype and threat-model review.

## Acceptance and handoff

- Prove two authorized clients can exchange encrypted stored text and encrypted voice/screen media while Axum, PostgreSQL, and LiveKit cannot read plaintext.
- Test a new device, member removal, lost device, browser/native interoperability, old plaintext history, backup restore, and protocol downgrade attempts.
- Accept only after the client key lifecycle in ADR-105 is resolved and reviewed.

**References:** [LiveKit encryption overview](https://docs.livekit.io/transport/encryption/).
