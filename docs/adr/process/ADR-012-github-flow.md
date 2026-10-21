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
- The integration is a squash merge. GitHub deletes the task branch after the merge.
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

### Pull-request CI and the required check

[Issue #9](https://github.com/Antorsrj/Cordel/issues/9) adds the `pull_request` trigger and crate jobs. The job that exists today is `phase0` in workflow `Phase 0 Linux build`. Issue #9 adds stable jobs `protocol`, `client-state`, and `server-identity` beside `phase0`. Those four jobs are the work CI runs. They are not the branch-protection required check.

The required check is one aggregator job, `ci`, in the same workflow file. The workflow trigger has no `paths:` filter, so a pull request always starts the workflow. `push` to `main` and `workflow_dispatch` stay. A path classification step decides which crate jobs run. Each crate job skips when its paths did not change. GitHub Actions has no per-job `paths:` key, so the filter lives in that classification step, not on the workflow trigger.

`ci` uses `if: always()` and needs `phase0`, `protocol`, `client-state`, and `server-identity`. It fails when any needed job failed or was cancelled. It succeeds when every needed job succeeded or was skipped. Branch protection requires only `Phase 0 Linux build / ci`. It does not require `phase0` by name. The workflow `name` stays `Phase 0 Linux build` so that check string stays stable.

A docs-only pull request skips the four crate jobs. `ci` still reports success, so branch protection does not wait for `phase0`, which the path filter did not run. An always-green companion workflow would also report on a docs-only pull request, and it would stay green when `phase0` failed. The aggregator reports on every pull request and still fails the pull request when a crate job fails. That is the check maintainers require. They add it only after the `ci` job exists. Requiring today's path-filtered `phase0` job would leave a docs-only pull request waiting for a check that never runs.

## Implementation boundary

- **Owns:** task branches, the mandatory issue link, pull requests, the reviewer set above, squash merge, tags, and the branch protection and CI settings maintainers apply.
- **Handoff:** [AGENTS.md](../../../AGENTS.md) and [WORKSTREAMS.md](../WORKSTREAMS.md) tell contributors to follow this flow. Maintainers apply the GitHub settings and the workflow change in issue #9. This ADR does not apply them.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| GitHub Flow **(chosen)** | One green `main`, with review before integration | Needs branch protection and a pull-request check that always reports |
| Git Flow | Separates integration and release | `develop` and long-lived release branches for a single product line. Commit `acde05e` already showed that shape |
| Direct push to `main` **(practice since 2026-10-18)** | Fewer steps | Review is not required, and `phase0` sees the commit only after it is on `main` |

## Consequences

- Contributors integrate through a task branch and a pull request.
- The index sentence "No ADR authorizes creating a remote repository, pushing, deploying, or publishing." in [docs/adr/README.md](../README.md) is replaced. Creating a remote repository, a direct push or force-push to `main`, and deploying stay unauthorized. A task-branch push and a pull request need the owner's approval, as described in this ADR.
- [ADR-106](../client/ADR-106-license-distribution.md) remains the license decision: Apache-2.0 for Cordel source and documentation, the Slint royalty-free desktop path, the About-screen gate before a client binary, and the rule that public visibility is not a reuse grant. ADR-106 still does not authorize deployment or a binary release. Its consequence "Inspect remote state and obtain the owner's push approval before publication. No push or deployment is authorized by this ADR." no longer forbids every push. Owner approval before a push remains. The push procedure moves here: a task branch and a pull request, never a direct push or force-push to `main`.
- Branch protection and the `ci` job are not configured yet. Until maintainers apply them, GitHub still accepts a direct push to `main`. The project rule is already this decision.
- This ADR does not join the 16 first-test mandatory decisions.

## Verification and handoff

- The owner accepted this flow on 2026-10-21, including the prefixes, the mandatory issue link, squash merge, tags on `main`, the branch protection list, and the `ci` aggregator. Issues [#8](https://github.com/Antorsrj/Cordel/issues/8) and [#9](https://github.com/Antorsrj/Cordel/issues/9) are the tracking issues.
- The reviewer set remains Antorsrj, arleylambert, or the Reviewer agent. Antors has a pending decision before that set changes.
- Done still needs the maintainer settings and the issue #9 workflow, including a docs-only pull request that receives `Phase 0 Linux build / ci` and a direct push to `main` that branch protection rejects. This change does not perform those steps.

**Related:** [ADR-106](../client/ADR-106-license-distribution.md), [issue #8](https://github.com/Antorsrj/Cordel/issues/8), [issue #9](https://github.com/Antorsrj/Cordel/issues/9).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [AGENTS.md](../../../AGENTS.md).

**Complexity:** 2/5
