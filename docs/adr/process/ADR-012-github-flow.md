# ADR-012: Adopt GitHub Flow with protected main

**Status:** Accepted

**Delivery stage:** Repository process

**Date:** 2026-10-21

**Decider:** Project owner

## Context

The official repository is `https://github.com/Antorsrj/Cordel.git`. Pull requests [#1](https://github.com/Antorsrj/Cordel/pull/1) through [#7](https://github.com/Antorsrj/Cordel/pull/7) merged from `feature/*` task branches (`feature/adr-101-phase0`, `feature/adr-102-client-recovery`, `feature/session-catchup-contract`, and `feature/adr-002-identity`). Commit `acde05e` merged `feature/adr-103-linux-media-spike` into `develop`. The prefix `feature/` is replaced by `feat/`.

From 2026-10-18 until 2026-10-21, when the Protect Main ruleset was created, later changes landed directly on `main`, and `main` had no branch protection. That ruleset is now active on the default branch. When this ADR was accepted, `.github/workflows/phase0-linux.yml` ran on `push` to `main` and on `workflow_dispatch`. It had no `pull_request` trigger. Its only job was `phase0`: format, Clippy, tests, and a release build of `client/phase0`. A workflow-level `paths:` filter also watched `client/media`, `client/state`, `protocol`, and `docs/contracts/fixtures`. The rest of the workspace, including `server/identity`, was outside that job. A pull request did not receive this check.

[Issue #8](https://github.com/Antorsrj/Cordel/issues/8) tracks this flow. The active ruleset covers deletion, force-push, the pull request, one approval from a non-author, and squash merges into `main`, for everyone except the repository-admin bypass actor. [Issue #9](https://github.com/Antorsrj/Cordel/issues/9) tracks pull-request CI and the required status checks, which are not active yet.

## Decision

Adopt GitHub Flow. `main` stays green.

- One task branch per task, cut from `main`, with the prefix `feat/`, `fix/`, `docs/`, or `chore/`.
- The link to an issue is mandatory. The pull request cites that issue. When no issue exists, the agent asks Antors to open one. Agents do not open a GitHub Issue themselves.
- Every change enters through a pull request. Applicable CI passes, and at least one review is required from Antorsrj, arleylambert, or the Reviewer agent. The ruleset requires one approval from a non-author. The pull request rule sets `require_extra_approval_for_unattributed_changes: true`. Required status checks are not active. [Issue #9](https://github.com/Antorsrj/Cordel/issues/9) names them in [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml); the owner still adds them to the ruleset.
- The integration is a squash merge. Automatic deletion of the task branch after the merge is active.
- A version is a tag on `main`. A release branch exists only when two versions must be maintained in parallel.
- A push needs the owner's approval. The push goes to the task branch. Direct pushes and force-pushes to `main` are prohibited for everyone except the repository-admin bypass actor. Agents never use the bypass, even when they act through an admin account: no direct push, force-push, or merge without the required approval. Only the owner uses it, personally, as a manual action outside the agent workflow.

This ADR does not edit workflows, product code, or GitHub settings.

### Ruleset Protect Main

The ruleset **Protect Main** is active on the default branch. This ADR does not change repository settings.

Active, for everyone except the repository-admin bypass actor. The bypass list is Repository admin (Roles) with Always allow, confirmed by the owner in repository settings on 2026-10-21; API reads without admin permission do not return bypass_actors. The team does not depend on the bypass, and its use is at the owner's discretion. Agents never use the bypass, even when they act through an admin account: no direct push, force-push, or merge without the required approval. Only the owner uses it, personally, as a manual action outside the agent workflow.

- Deletion of `main` is restricted.
- Force-pushes to `main` are blocked.
- A pull request is required before merging.
- The ruleset requires one approval from a non-author. The pull request rule sets `require_extra_approval_for_unattributed_changes: true`.
- The ruleset allows only squash merges into `main`; repository-level merge commits and rebase merges are still enabled.

Automatic deletion of the task branch after the merge is active. That setting is repository-level (`delete_branch_on_merge`), not a ruleset rule.

Not active:

- Required status checks are not active, so the task branch is not required to be up to date. The check names are in [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml). The owner adds them to this ruleset; this ADR does not.

### Pull-request CI

[Issue #9](https://github.com/Antorsrj/Cordel/issues/9) is implemented by [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml). The workflow runs on `pull_request` to `main`, on `push` to `main`, and on `workflow_dispatch`. It has no `paths` filter. Every job runs on every pull request, so a required check is not skipped.

Each crate job runs `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked` on Rust 1.99.1. The Cargo cache is keyed by that toolchain and the crate `Cargo.lock`. `server-identity` starts PostgreSQL 16, creates `cordel_identity_test_ci`, sets `CORDEL_TEST_DATABASE_URL`, and runs the ignored database tests one at a time. `disposable_database_media_drain_flow` stays out because it needs a running LiveKit and `CORDEL_TEST_LIVEKIT_*`. The `phase0` job keeps the Linux desktop packages, clang 21, `xvfb-run`, and the release build. Ignored Phase 0 tests that need a local Axum server and `CORDEL_PHASE0_TEST_*` stay manual, including the network-recovery measurement tracked by issue #10. `client-media` tests `--features ui`. The `phase0` job compiles `client/media` with `sdk` and `ui`, so CI does not download WebRTC twice. `cargo-audit` reads every crate lockfile.

There is no aggregator job. The jobs always run, and branch protection can require their check names directly:

- `CI / protocol`
- `CI / client-state`
- `CI / client-media`
- `CI / server-identity`
- `CI / phase0`
- `CI / cargo-audit`

Adding those names to the Protect Main ruleset is a repository-settings step for the owner. This ADR does not edit the ruleset.

## Implementation boundary

- **Owns:** task branches, the mandatory issue link, pull requests, the approval rule above, squash merge, tags, the active Protect Main ruleset, and the required status checks that remain pending in issue #9.
- **Handoff:** [AGENTS.md](../../../AGENTS.md) and [WORKSTREAMS.md](../WORKSTREAMS.md) tell contributors to follow this flow. The workflow change is [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml). The owner still adds its check names to the Protect Main ruleset. This ADR does not change the ruleset.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| GitHub Flow **(chosen)** | One green `main`, with review before integration | Protect Main is active. Required status checks stay open in issue #9 |
| Git Flow | Separates integration and release | `develop` and long-lived release branches for a single product line. Commit `acde05e` already showed that shape |
| Direct push to `main` **(practice since 2026-10-18)** | Fewer steps | Review is not required, and `phase0` sees the commit only after it is on `main` |

## Consequences

- Contributors integrate through a task branch and a pull request.
- The index in [docs/adr/README.md](../README.md) still withholds authority to create a remote repository, to direct-push or force-push to `main`, to deploy, or to publish a release or client binary. A task-branch push and a pull request need the owner's approval, as described in this ADR.
- [ADR-106](../client/ADR-106-license-distribution.md) remains the license decision: Apache-2.0 for Cordel source and documentation, the Slint royalty-free desktop path, the About-screen gate before a client binary, and the rule that public visibility is not a reuse grant. ADR-106 still does not authorize deployment or a binary release. Its consequence "Inspect remote state and obtain the owner's push approval before publication. No push or deployment is authorized by this ADR." no longer forbids every push. Owner approval before a push remains. The push procedure moves here: a task branch and a pull request, never a direct push or force-push to `main`.
- The Protect Main ruleset blocks a direct push, a force-push, and deletion of `main` for everyone except the repository-admin bypass actor. The team does not depend on the bypass, and its use is at the owner's discretion. Agents never use the bypass, even when they act through an admin account: no direct push, force-push, or merge without the required approval. Only the owner uses it, personally, as a manual action outside the agent workflow. A change enters through a pull request and a squash merge. Required status checks are not part of that ruleset yet. The workflow names them; the owner adds them in repository settings.
- This ADR does not join the 16 first-test mandatory decisions.

## Verification and handoff

- The owner accepted this flow on 2026-10-21, including the prefixes, the mandatory issue link, squash merge, and tags on `main`. The aggregator is not part of that acceptance. Applicable CI passes, and at least one review is required from Antorsrj, arleylambert, or the Reviewer agent. The ruleset requires one approval from a non-author. The repository-admin bypass actor is on the bypass list with Always allow. The team does not depend on the bypass, and its use is at the owner's discretion. Agents never use the bypass, even when they act through an admin account: no direct push, force-push, or merge without the required approval. Only the owner uses it, personally, as a manual action outside the agent workflow.
- The Protect Main ruleset is active with the rules listed above. Issues [#8](https://github.com/Antorsrj/Cordel/issues/8) and [#9](https://github.com/Antorsrj/Cordel/issues/9) remain the tracking issues. This change does not edit the ruleset.
- The issue #9 workflow is [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml), with the check names listed above. Done still needs the owner to add those checks to the Protect Main ruleset. This change does not edit the ruleset.

**Related:** [ADR-106](../client/ADR-106-license-distribution.md), [issue #8](https://github.com/Antorsrj/Cordel/issues/8), [issue #9](https://github.com/Antorsrj/Cordel/issues/9).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [AGENTS.md](../../../AGENTS.md).

**Complexity:** 2/5
