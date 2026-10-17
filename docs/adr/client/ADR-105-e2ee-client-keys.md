# ADR-105: Put future E2EE keys under client control

**Status:** Proposed

**Delivery stage:** Final-release roadmap

**Date:** 2026-10-17

**Decider:** Project owner before the final release

## Context

The first test may omit E2EE. The final release needs E2EE for stored text, chat images, voice, and screen sharing. LiveKit's media E2EE requires clients to enable encryption and distribute keys; the server cannot provide E2EE merely by enabling TLS. Device loss, new devices, membership changes, and mixed browser/native clients make key lifecycle a product decision.

## Proposed decision

Assign each client device a cryptographic identity. Encrypt text content and image attachments on the sender device before they reach Axum and decrypt only on authorized recipient devices. Use a versioned ciphertext envelope so the storage protocol can evolve. Enable LiveKit media E2EE with keys delivered through a client-controlled authenticated exchange, not a plaintext room key supplied by Axum. Treat voice and screen tracks in the same room as one E2EE policy. Rotate the relevant keys when a member loses access; recognize that previously received plaintext or keys cannot be retroactively erased.

Choose a reviewed key agreement and recovery design only after an interoperable proof with the pinned Rust and browser SDKs. Define how a new device gains access, how users verify devices, what happens after total key loss, and whether the project can recover historical messages. Do not silently fall back to plaintext when a participant cannot use E2EE.

## Implementation boundary

- **Owns:** proposed device identity, key exchange/storage/recovery, content encryption, and native/browser media interoperability. Server 008 owns the ciphertext envelope and server boundary.
- **Handoff:** produce an interoperable prototype, threat model, and failure behavior for lost/revoked devices. Do not claim E2EE from transport encryption or a single-client demo.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Client-controlled device keys **(proposed)** | Server cannot read protected content | More onboarding and recovery work |
| Server-issued shared room key | Easy group setup | Server can decrypt media, defeating desired boundary |
| Transport encryption only | Simple first test | Does not meet final-release requirement |

## Consequences

- Search and content moderation that depend on server plaintext must be redesigned or omitted for E2EE content.
- The first test's plaintext history needs an explicit migration or labeling decision before the final release.
- Key storage must use OS-protected facilities where available; browser storage and cross-device recovery require a dedicated security review.

## Acceptance and handoff

- Demonstrate Rust↔Rust and Rust↔browser encrypted text, image attachments, audio, and screen sharing; inspect server/DB/image/media process data for absence of plaintext.
- Test new device, revoked member, lost key, backup restore, key rotation, and refusal of an unencrypted participant.
- Publish a threat model and an independently reviewable protocol before calling E2EE complete.

**Related:** [ADR-008](../server/ADR-008-e2ee-server-boundary.md), [LiveKit encryption](https://docs.livekit.io/transport/encryption/).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [v1 application contract](../../contracts/v1.md).

**Complexity:** 5/5
