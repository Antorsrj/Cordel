# ADR-012: Adopt GitHub Flow with protected main

**Status:** Accepted

**Delivery stage:** Repository process

**Date:** 2026-10-21

**Decider:** Project owner

## Context

The official repository is `https://github.com/Antorsrj/Cordel.git`. Pull requests [#1](https://github.com/Antorsrj/Cordel/pull/1) through [#7](https://github.com/Antorsrj/Cordel/pull/7) merged from `feature/*` task branches (`feature/adr-101-phase0`, `feature/adr-102-client-recovery`, `feature/session-catchup-contract`, and `feature/adr-002-identity`). Commit `acde05e` merged `feature/adr-103-linux-media-spike` into `develop`. The prefix `feature/` is replaced by `feat/`.

From 2026-10-18 until 2026-10-21, when the Protect Main ruleset was created, later changes landed directly on `main`, and `main` had no branch protection. That ruleset is now active on the default branch. `.github/workflows/phase0-linux.yml` still runs on `push` to `main` and on `workflow_dispatch`. It has no `pull_request` trigger. Its only job is `phase0`: format, Clippy, tests, and a release build of `client/phase0`. A workflow-level `paths:` filter also watches `client/media`, `client/state`, `protocol`, and `docs/contracts/fixtures`. The rest of the workspace, including `server/identity`, is outside that job. A pull request does not receive this check.

[Issue #8](https://github.com/Antorsrj/Cordel/issues/8) tracks this flow. The active ruleset covers deletion, force-push, the pull request, one approval from a non-author, and squash merges into `main`, for everyone except the repository-admin bypass actor. [Issue #9](https://github.com/Antorsrj/Cordel/issues/9) tracks pull-request CI and the required status checks, which are not active yet.

## Decision

Adopt GitHub Flow. `main` stays green.

- One task branch per task, cut from `main`, with the prefix `feat/`, `fix/`, `docs/`, or `chore/`.
- The link to an issue is mandatory. The pull request cites that issue. When no issue exists, the agent asks Antors to open one. Agents do not open a GitHub Issue themselves.
- Every change enters through a pull request. The ruleset requires one approval from a non-author. The pull request rule sets `require_extra_approval_for_unattributed_changes: true`. The Reviewer agent's review remains mandatory as the team work rule in this ADR, because every change enters through a pull request with review, and that review does not count as the GitHub approval. The ruleset lists a repository-admin bypass actor. The team does not depend on the bypass, and its use is at the owner's discretion. Required status checks are not active; they stay pending until issue #9.
- The integration is a squash merge. The task branch is deleted after the merge (automatic once maintainers enable it).
- A version is a tag on `main`. A release branch exists only when two versions must be maintained in parallel.
- A push needs the owner's approval. The push goes to the task branch. Direct pushes and force-pushes to `main` are prohibited for everyone except the repository-admin bypass actor.

This ADR does not edit workflows, product code, or GitHub settings.

### Ruleset Protect Main

The ruleset **Protect Main** is active on the default branch. This ADR does not change repository settings.

Active, for everyone except the repository-admin bypass actor. The bypass list is Repository admin (Roles) with Always allow. A repository read of the ruleset returned `bypass_actors: null`. The team does not depend on the bypass, and its use is at the owner's discretion.

- Deletion of `main` is restricted.
- Force-pushes to `main` are blocked.
- A pull request is required before merging.
- The ruleset requires one approval from a non-author. The pull request rule sets `require_extra_approval_for_unattributed_changes: true`.
- The ruleset allows only squash merges into `main`; repository-level merge commits and rebase merges are still enabled.

Not active:

- Required status checks are not active, so the task branch is not required to be up to date. Both stay pending until [issue #9](https://github.com/Antorsrj/Cordel/issues/9).
- The task branch is deleted after the merge (automatic once maintainers enable it).

### Pull-request CI

[Issue #9](https://github.com/Antorsrj/Cordel/issues/9) tracks the pull-request CI. It asks for a `pull_request` trigger to `main`, and keeps `push` to `main` and `workflow_dispatch`. The only job today is `phase0` in `.github/workflows/phase0-linux.yml`. Issue #9 adds separate jobs for `server/identity`, `protocol`, and `client/state`, each running `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked`, beside that `phase0` job. The identity job uses a PostgreSQL 16 service and `CORDEL_TEST_DATABASE_URL` for the ignored database tests. The LiveKit test that needs `CORDEL_TEST_LIVEKIT_*` stays out of CI unless a later change adds it. Issue #9 also asks for a Cargo cache, the pinned Rust 1.99.1 toolchain, and stable job names that maintainers can select as required checks under issue #8.

Issue #9 handles the `paths:` filter in one of two ways: include `server/**` and the workflow file, or remove the filter from the jobs that branch protection requires, so `main` does not wait for a check that never ran.

An aggregator job is proposed for issue #9; maintainers confirm when implementing. One shape is a job `ci` in the same workflow. The workflow trigger has no `paths:` filter. A path classification step decides which crate jobs run, because GitHub Actions has no per-job `paths:` key. `ci` uses `if: always()` and needs `phase0`, `protocol`, `client-state`, and `server-identity`. It fails when any needed job failed or was cancelled. It succeeds when every needed job succeeded or was skipped. A docs-only pull request would skip the crate jobs and `ci` would still report success. Maintainers may instead follow issue #9's own path options and require those stable job names directly. This ADR does not choose the required check names.

## Implementation boundary

- **Owns:** task branches, the mandatory issue link, pull requests, the approval rule above, squash merge, tags, the active Protect Main ruleset, and the required status checks that remain pending in issue #9.
- **Handoff:** [AGENTS.md](../../../AGENTS.md) and [WORKSTREAMS.md](../WORKSTREAMS.md) tell contributors to follow this flow. Maintainers still apply the workflow change and the required status checks in issue #9. This ADR does not change the ruleset.

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
- The Protect Main ruleset blocks a direct push, a force-push, and deletion of `main` for everyone except the repository-admin bypass actor. The team does not depend on the bypass, and its use is at the owner's discretion. A change enters through a pull request and a squash merge. Required status checks are not part of that ruleset; they stay pending until issue #9.
- This ADR does not join the 16 first-test mandatory decisions.

## Verification and handoff

- The owner accepted this flow on 2026-10-21, including the prefixes, the mandatory issue link, squash merge, and tags on `main`. The aggregator is not part of that acceptance. The ruleset requires one approval from a non-author. The Reviewer agent's review remains the team work rule in this ADR, because every change enters through a pull request with review, and that review does not count as the GitHub approval. The repository-admin bypass actor is on the bypass list with Always allow. The team does not depend on the bypass, and its use is at the owner's discretion.
- The Protect Main ruleset is active with the rules listed above. Issues [#8](https://github.com/Antorsrj/Cordel/issues/8) and [#9](https://github.com/Antorsrj/Cordel/issues/9) remain the tracking issues. This change does not edit the ruleset.
- Done still needs the issue #9 workflow and the required status checks chosen when that issue is implemented. This change does not perform those steps.

**Related:** [ADR-106](../client/ADR-106-license-distribution.md), [issue #8](https://github.com/Antorsrj/Cordel/issues/8), [issue #9](https://github.com/Antorsrj/Cordel/issues/9).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [AGENTS.md](../../../AGENTS.md).

**Complexity:** 2/5
