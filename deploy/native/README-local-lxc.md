# Local LXC validation for Cordel

Disposable scripts that stand in for the Azure test VM. They target a local
Ubuntu 24.04 LXC or VM and never touch a deployed environment, a production
database, or real user data.

| File | Purpose |
| --- | --- |
| `local-lxc-common.sh` | Shared helpers: checkout discovery, logging, disk space. |
| `local-lxc-provision.sh` | Installs the toolchain and runs the server, client-state, and Phase 0 suites. |
| `local-lxc-e2e.py` | Creates a disposable channel and a second member through the public API. |
| `local-lxc-e2e.sh` | End-to-end ADR-004 chat proof: two clients exchanging live messages. |

## Prerequisites

The LXC needs `sudo`, about 20 GB of free disk, and outbound access to GitHub,
crates.io, apt, and the LLVM repository. The provisioning script installs the
same package list, clang 21 toolchain, and Rust version as the Linux CI, so a
passing local run means the same sources build the same way.

## Usage

```sh
scp deploy/native/local-lxc-provision.sh \
    deploy/native/local-lxc-common.sh \
    deploy/native/local-lxc-e2e.sh \
    deploy/native/local-lxc-e2e.py \
    cordel@<host>:/tmp/

ssh cordel@<host>
bash /tmp/local-lxc-provision.sh   # toolchain, database, test suites
bash /tmp/local-lxc-e2e.sh         # end-to-end live chat proof
```

Run them with `bash <file>`. A file copied with `scp` has no execute bit, and
`./file.sh` fails with "Permission denied".

Both scripts are idempotent. They reuse an existing checkout, refresh it from
`origin/main`, and create a new disposable database per run. Set
`CORDEL_REPO_DIR` to force a checkout path.

## Environment variables

| Variable | Effect |
| --- | --- |
| `CORDEL_REPO_DIR` | Use this checkout instead of searching for one. |
| `CORDEL_SKIP_PHASE0=1` | Skip the Phase 0 build when the disk is short. |
| `CORDEL_FULL_DEBUG=1` | Keep debug symbols in the Cargo build. |
| `CORDEL_PHASE0_TEST_*` | Consumed by the opt-in Phase 0 chat tests. |

## Last validated

On 2026-10-21 a disposable Ubuntu 24.04 LXC passed both scripts. The
provisioning run passed the five ignored server flows, the client-state suite,
the Phase 0 suite under Xvfb, and the release build. The end-to-end run
created a channel and a second member through the public API, passed both
opt-in chat tests including the two-client exchange, and confirmed that the
Phase 0 window opens with the chat surface.

## Limits

This is local evidence, not deployment evidence. It does not cover public DNS,
TLS termination, NSG or firewall rules, external TURN and ICE, reboot recovery,
off-host backup and restore, a real capacity measurement, or macOS and Windows.
Those gates still need the Azure test VM and a second network.
