# ADR-007: Make the single host recoverable before real users

**Status:** Proposed

**Date:** 2026-10-17

**Decider:** Project owner for loss and recovery targets

## Context

The first deployment has one host and one PostgreSQL database. Loss of a VM, disk, home power, router, or ISP can interrupt both chat and voice. The owner has not chosen how much message history may be lost or how quickly service must recover. Those targets determine backup frequency and storage cost.

## Proposed decision

Use pinned container images, health checks, versioned SQLx migrations, private service networks, structured logs, resource metrics, and documented upgrade/rollback procedures. Back up PostgreSQL consistently, encrypt copies, and store them outside the host's failure domain. Keep the host configuration and secret-recovery instructions under separate restricted backup. Before inviting real users, restore a backup into an isolated host and verify login and message history.

For the initial test, propose a **24-hour recovery point target** and a **4-hour recovery time target** as planning assumptions, with a nightly logical database backup, seven daily copies, and four weekly copies. These are targets to approve and prove in a drill, not guarantees. If losing up to a day of messages is unacceptable, add more frequent backups or WAL archiving. If attachments are added later, their object store and metadata must be backed up and restored consistently.

Keep PostgreSQL, LiveKit control, and Axum's private listener off the public Internet. Document cloud firewall/router rules, DNS, certificate renewal, media ports, backup location, operator access, and a recovery runbook. Host-side admin management from ADR-002/003 must work during recovery.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Nightly encrypted off-host dump **(proposed test baseline)** | Simple for a small database | Up to one day's data loss |
| Frequent dumps or WAL archiving | Lower loss window | More storage and restore complexity |
| Local-only backups | Fast to configure | Does not survive host or site loss |

## Consequences

- Voice rooms end on host restart; clients must obtain fresh admission after service recovery.
- Backups are useful only after a restore drill and application-level verification.
- The owner must approve recovery targets before this ADR becomes accepted for real users.

## Acceptance and handoff

- Demonstrate encrypted off-host backup, retention pruning, restore to an isolated host, and successful app login/history read.
- Record measured recovery time and recoverable timestamp; compare them with approved targets.
- Verify certificate renewal, resource/egress alerts, and rollback procedure without exposing secrets.

**Related:** [ADR-001](ADR-001-single-host-deployment.md), [ADR-004](ADR-004-durable-chat.md).
