# ADR-011: Store restricted rich chat and private image attachments

**Status:** Proposed

**Date:** 2026-10-17

**Decider:** Project owner for image storage and operating cost

## Context

The approved Journal mocks include formatted chat and inline images. Earlier first-test plans excluded attachments. Image messages now need durable bytes, per-channel access checks, backup coverage, safe decoding, and a path toward E2EE. The first installation is one host; no external object-storage service is required.

## Proposed decision

Store message text as a bounded, versioned structured content format that supports plain text, emphasis, lists, code, and links. Renderers treat it as data, not HTML. Reject raw HTML and unsupported elements; validate size and structure on write. Keep message ordering, idempotency, and catch-up from ADR-004. Do not fetch arbitrary URLs from the server to generate link cards in the first test; a safe clickable link is sufficient.

Add an authenticated image-upload flow. For the first host, store image bytes in a private directory on a persistent data volume and attachment metadata in PostgreSQL, linked to the message/channel/author. Accept only a reviewed set of raster formats, impose configurable byte and pixel limits, inspect actual content rather than trusting filename or MIME claims, and reject active content such as SVG. Process images with a maintained decoder to normalize previews and remove metadata where feasible. Serve previews and originals through an Axum endpoint that checks current channel membership and moderation status on every request; do not expose the storage directory or permanent public URLs. Use short-lived responses and cache policy that prevents a revoked user from receiving new protected bytes through stale shared caches.

Treat upload and message creation as a two-step operation with temporary objects, expiration, and cleanup, so an orphaned upload cannot grow without bound. A deleted message or removed image loses API access promptly; define physical retention in the operating policy. Back up database metadata and image bytes as one recoverable set under ADR-007. Keep the attachment record able to carry a versioned encrypted payload later; pre-E2EE plaintext images must be handled by the migration policy in ADR-008/105.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Private host volume plus PostgreSQL metadata **(proposed)** | Fits one-node deployment without another service | Coordinated backup, storage quotas, and cleanup |
| PostgreSQL large objects/byte arrays | One backup boundary | Database and backups grow quickly with media |
| External S3-compatible object store now | Scales storage separately | Additional service, credentials, and cost for first test |

## Consequences

- The earlier “no attachment store” assumption is superseded for the approved image-chat feature. A private data volume is enough for the first host; arbitrary files remain outside scope.
- Image size limits and storage quotas must be set from test data and host capacity before enabling uploads. The existing 50 GB disk estimate must be rechecked.
- This proposal must pass its acceptance gate before an implementer can claim the image-chat behavior complete.

## Acceptance and handoff

- Approve the host-volume choice and initial upload/retention limits. Demonstrate formatted text and image messages across native and diagnostic browser clients.
- Test invalid formats, oversized and decompression-bomb inputs, broken previews, concurrent uploads, orphan cleanup, channel revocation during download, and unauthorized direct-object access.
- Restore PostgreSQL plus image files to an isolated host and verify that no message points to a missing image after a consistent backup.

**Related:** [ADR-004](ADR-004-durable-chat.md), [ADR-007](ADR-007-backup-operations.md), [ADR-107](../client/ADR-107-journal-ux.md).
