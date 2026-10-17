# Cordel

Cordel is a self-hosted community app for text chat, voice rooms, and screen sharing. The first version is an invite-only community with `user` and `admin` roles. The server is headless; desktop and browser clients provide the interface, including admin controls.

**Project status:** Under development. This repository contains the architecture, ADRs, shared protocol, client state library, and a Linux Phase 0 client. These pieces are not yet an integrated product. The Azure test host runs the media foundation, but it is not ready for community use. See the [ADR status ledger](docs/adr/STATUS.md) for completion evidence and open gates.

## First-test scope

- One Linux server with Axum, PostgreSQL, Caddy, and self-hosted LiveKit. The current test deployment uses native `systemd` services on an Azure East US VM; the architecture also describes a generic VM or home server.
- A Linux native client first, plus a diagnostic browser client. macOS and Windows are later validation targets. Shared native logic is written in Rust.
- Invite-only accounts with a login and password. Admins can invite people and manage community settings and members. Members can create text and voice rooms.
- The Journal interface has a people roster, a collapsible conversation area, and screen shares that can stay in the client or open in a separate window.
- End-to-end encryption is planned before final release; it is not part of the first test.

The 15-person, two-concurrent-share example is a sizing target to measure, not a verified capacity limit. User-facing integrated product testing begins only after every [first-test mandatory ADR](docs/adr/README.md) is complete. Isolated component tests run during implementation.

## Start here

| If you need to... | Read |
| --- | --- |
| Understand the product goal | [Project vision (Portuguese)](Plano%20de%20Desenvolvimento%20-%20Revisado.md) |
| Understand deployment and network paths | [Architecture](architecture.md) and [deployment guide](deploy/README.md) |
| Pick an implementation task | [ADR index](docs/adr/README.md), [workstream map](docs/adr/WORKSTREAMS.md), and [status ledger](docs/adr/STATUS.md) |
| Implement a server/client seam | [Versioned application contract](docs/contracts/v1.md) |
| Implement the interface | [Journal UX specification](docs/ux/journal.md) |
| Contribute with an agent or IDE | [Agent and contributor instructions](AGENTS.md) |

Accepted ADRs record decisions, not completed software. Proposed ADRs have gates that must be resolved before their features can be claimed. Each ADR identifies its implementation boundary, verification, and complexity.

## Repository layout

| Path | Purpose |
| --- | --- |
| [`client/phase0`](client/phase0/README.md) | Linux UI, audio, and screen-capture feasibility work |
| [`client/state`](client/state/README.md) | Client session and reconnect state |
| [`protocol`](protocol/README.md) | Shared versioned Rust/JSON types |
| [`docs/adr`](docs/adr/README.md) | Server and client decisions and status |
| [`docs/contracts`](docs/contracts/v1.md) | Cross-component API and event contract |
| [`deploy`](deploy/README.md) | Native Linux deployment scripts and runbooks |

Each Rust crate has its own build instructions. The repository does not yet provide a complete server/client startup command.

## Contributing

Select one ADR and read its dependencies and contract handoffs before editing. Keep the change scoped, use disposable test data, and record actual format, lint, test, and platform results. Do not treat a passing component test as a completed product test. The repository has no selected Cordel license yet; [ADR-106](docs/adr/client/ADR-106-license-distribution.md) tracks that decision before further application-source publication.
