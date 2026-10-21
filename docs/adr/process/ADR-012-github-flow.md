# ADR-012: Adopt GitHub Flow with protected main

**Status:** Accepted

**Delivery stage:** Repository process

**Date:** 2026-10-21

**Decider:** Project owner

## Context

The official repository is `https://github.com/Antorsrj/Cordel.git`. Pull requests [#1](https://github.com/Antorsrj/Cordel/pull/1) through [#7](https://github.com/Antorsrj/Cordel/pull/7) merged from `feature/*` task branches (`feature/adr-101-phase0`, `feature/adr-102-client-recovery`, `feature/session-catchup-contract`, and `feature/adr-002-identity`). Commit `acde05e` merged `feature/adr-103-linux-media-spike` into `develop`. The prefix `feature/` is replaced by `feat/`.

Since 2026-10-18, later changes have landed directly on `main`. `main` has no branch protection. `.github/workflows/phase0-linux.yml` runs on `push` to `main` and on `workflow_dispatch`. It has no `pull_request` trigger. Its only job is `phase0`: format, Clippy, tests, and a release build of `client/phase0`. A workflow-level `paths:` filter also watches `client/media`, `client/state`, `protocol`, and `docs/contracts/fixtures`. The rest of the workspace, including `server/identity`, is outside that job. A pull request does not receive this check.

[Issue #8](https://github.com/Antorsrj/Cordel/issues/8) tracks this flow and the branch protection settings. [Issue #9](https://github.com/Antorsrj/Cordel/issues/9) tracks pull-request CI beyond the Phase 0 client.

## Decision

Adopt GitHub Flow. `main` stays green.

- One task branch per task, cut from `main`, with the prefix `feat/`, `fix/`, `docs/`, or `chore/`.
- The link to an issue is mandatory. The pull request cites that issue. When no issue exists, the agent asks Antors to open one. Agents do not open a GitHub Issue themselves.
- Every change enters through a pull request. Applicable CI passes, and at least one review is required from Antorsrj, arleylambert, or the Reviewer agent. That reviewer set stays as written. A change to who may review is still a pending decision for Antors.
- The integration is a squash merge. The task branch is deleted after the merge (automatic once maintainers enable it).
- A version is a tag on `main`. A release branch exists only when two versions must be maintained in parallel.
- A push needs the owner's approval. The push goes to the task branch. Direct pushes and force-pushes to `main` are prohibited.

This ADR does not edit workflows, product code, or GitHub settings.

### Branch protection maintainers apply

Maintainers configure these settings on `main`. This change does not apply them.

- A pull request is required before merging, with at least one approval.
- Required status checks must pass, and the task branch must be up to date with `main` before merging.
- Squash merge is the only merge method. Merge commits and rebase merges are disabled.
- The task branch is deleted automatically after the merge.
- Force-pushes to `main` are blocked.
- Deletion of `main` is blocked.

### Pull-request CI

[Issue #9](https://github.com/Antorsrj/Cordel/issues/9) tracks the pull-request CI. It asks for a `pull_request` trigger to `main`, and keeps `push` to `main` and `workflow_dispatch`. The only job today is `phase0` in `.github/workflows/phase0-linux.yml`. Issue #9 adds separate jobs for `server/identity`, `protocol`, and `client/state`, each running `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked`, beside that `phase0` job. The identity job uses a PostgreSQL 16 service and `CORDEL_TEST_DATABASE_URL` for the ignored database tests. The LiveKit test that needs `CORDEL_TEST_LIVEKIT_*` stays out of CI unless a later change adds it. Issue #9 also asks for a Cargo cache, the pinned Rust 1.99.1 toolchain, and stable job names that maintainers can select as required checks under issue #8.

Issue #9 handles the `paths:` filter in one of two ways: include `server/**` and the workflow file, or remove the filter from the jobs branch protection requires, so `main` does not wait for a check that never ran.

An aggregator job is proposed for issue #9; maintainers confirm when implementing. One shape is a job `ci` in the same workflow. The workflow trigger has no `paths:` filter. A path classification step decides which crate jobs run, because GitHub Actions has no per-job `paths:` key. `ci` uses `if: always()` and needs `phase0`, `protocol`, `client-state`, and `server-identity`. It fails when any needed job failed or was cancelled. It succeeds when every needed job succeeded or was skipped. A docs-only pull request would skip the crate jobs and `ci` would still report success. Maintainers may instead follow issue #9's own path options and require those stable job names directly. This ADR does not choose the required check names.

## Implementation boundary

- **Owns:** task branches, the mandatory issue link, pull requests, the reviewer set above, squash merge, tags, and the branch protection and CI settings maintainers apply.
- **Handoff:** [AGENTS.md](../../../AGENTS.md) and [WORKSTREAMS.md](../WORKSTREAMS.md) tell contributors to follow this flow. Maintainers apply the GitHub settings and the workflow change in issue #9. This ADR does not apply them.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| GitHub Flow **(chosen)** | One green `main`, with review before integration | Needs branch protection. The pull-request check shape stays open in issue #9 |
| Git Flow | Separates integration and release | `develop` and long-lived release branches for a single product line. Commit `acde05e` already showed that shape |
| Direct push to `main` **(practice since 2026-10-18)** | Fewer steps | Review is not required, and `phase0` sees the commit only after it is on `main` |

## Consequences

- Contributors integrate through a task branch and a pull request.
- The index in [docs/adr/README.md](../README.md) still withholds authority to create a remote repository, to direct-push or force-push to `main`, to deploy, or to publish a release or client binary. A task-branch push and a pull request need the owner's approval, as described in this ADR.
- [ADR-106](../client/ADR-106-license-distribution.md) remains the license decision: Apache-2.0 for Cordel source and documentation, the Slint royalty-free desktop path, the About-screen gate before a client binary, and the rule that public visibility is not a reuse grant. ADR-106 still does not authorize deployment or a binary release. Its consequence "Inspect remote state and obtain the owner's push approval before publication. No push or deployment is authorized by this ADR." no longer forbids every push. Owner approval before a push remains. The push procedure moves here: a task branch and a pull request, never a direct push or force-push to `main`.
- Branch protection is not configured yet. Until maintainers apply it, GitHub still accepts a direct push to `main`. The project rule is already this decision. The pull-request workflow stays with issue #9.
- This ADR does not join the 16 first-test mandatory decisions.

## Verification and handoff

- The owner accepted this flow on 2026-10-21, including the prefixes, the mandatory issue link, squash merge, tags on `main`, and the branch protection list. The aggregator is not part of that acceptance. Issues [#8](https://github.com/Antorsrj/Cordel/issues/8) and [#9](https://github.com/Antorsrj/Cordel/issues/9) are the tracking issues.
- The reviewer set remains Antorsrj, arleylambert, or the Reviewer agent. Antors has a pending decision before that set changes.
- Done still needs the maintainer settings and the issue #9 workflow, including required checks chosen when that issue is implemented and a direct push to `main` that branch protection rejects. This change does not perform those steps.

**Related:** [ADR-106](../client/ADR-106-license-distribution.md), [issue #8](https://github.com/Antorsrj/Cordel/issues/8), [issue #9](https://github.com/Antorsrj/Cordel/issues/9).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [AGENTS.md](../../../AGENTS.md).

**Complexity:** 2/5
