# ADR-106: Resolve UI and repository licensing before further publication

**Status:** Accepted

**Delivery stage:** First-test mandatory

**Date:** 2026-10-18

**Decider:** Project owner

## Context

The official public GitHub repository already contains Linux Phase 0 client, client state, and shared protocol source. It was published without a project license. GitHub visibility alone grants no general right to reuse the code. The native prototype uses Slint. Cordel needs an explicit license for its own source and a separate valid path for distributing Slint with a client.

## Decision

License Cordel-authored source and documentation under Apache License 2.0. Keep copyrights with their individual contributors. The owner selected this path on 2026-10-18 and confirmed that the other existing code contributor agreed. The root `LICENSE`, `NOTICE`, and Rust package `license` metadata express this choice. Keep the official repository public; this choice does not itself authorize a Git push or a binary release.

Use Slint's royalty-free desktop/mobile/web license for the desktop client. The checked-in Phase 0 lockfile resolves `slint` and `slint-build` to 1.18.1 even though the manifest specifies `1.14` as the minimum compatible minor version. [Slint's v1.18.1 license](https://github.com/slint-ui/slint/blob/v1.18.1/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md) allows desktop distribution under this path if the application displays either the `AboutSlint` widget in an About screen reachable from the top-level menu (or in a splash screen if no About screen exists), or the Slint attribution badge on a public webpage. Cordel chooses the in-app About screen. Slint keeps its own license; Cordel's Apache-2.0 grant does not cover Slint. Recheck the locked version and corresponding terms before release.

Do not publish a client binary until the packaged application's About screen, attribution, and complete third-party notice inventory are checked. The present Phase 0 prototype has no About screen, so binary distribution remains open. No embedded-system use is authorized by the royalty-free path. Keep third-party assets and code under their original terms, and do not add assets with unknown rights.

## Implementation boundary

- **Owns:** Cordel's source license, contributor agreement record, third-party license inventory, and package/About attribution requirements. It does not choose product architecture or publish code on its own.
- **Handoff:** use `LICENSE`, `NOTICE`, [third-party inventory](../../legal/THIRD_PARTY.md), package metadata, and the About-screen requirement. Release packaging must generate a complete target-specific dependency and native-library inventory.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Apache-2.0 Cordel code plus royalty-free Slint **(chosen)** | Broad code reuse and a desktop binary path without GPLv3 distribution | Slint attribution, separate third-party rights, and release notices must be maintained |
| GPLv3 path | Clear open-source distribution path | Copyleft obligations for distributed binary |
| Paid Slint license | Different support/terms | Cost and contract decision |
| Replace Slint after Phase 0 | Avoid Slint license constraints | New UI feasibility work |

## Consequences

- Client and protocol code was published before a license was selected. The decision and its license/notice files were subsequently published and are now in `main`.
- A successful Slint technical spike does not prove packaged attribution or all transitive native-library notices.
- The owner-selected repository URL is configured as local `origin`. Inspect remote state before publication. A push still needs the owner's approval and follows [ADR-012](../process/ADR-012-github-flow.md): a task branch and a pull request, never a direct push or force-push to `main`. This ADR does not authorize deployment or a client binary.

## Acceptance and handoff

- The owner's Apache-2.0 and public-repository decisions and existing contributor agreement are recorded above.
- Root `LICENSE`, `NOTICE`, package metadata, and the known dependency inventory are present on `main`. Recheck them when preparing a release.
- Before any client binary release, confirm the package includes complete license/NOTICE material and that its About screen exposes `AboutSlint` from the top-level menu. Record an actual packaged-client check in [STATUS](../STATUS.md).

**References:** [Slint v1.18.1 licensing choices](https://github.com/slint-ui/slint/blob/v1.18.1/LICENSE.md), [Slint royalty-free license 2.0](https://github.com/slint-ui/slint/blob/v1.18.1/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md), [GitHub's no-license guidance](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository), [Apache-2.0 text](https://www.apache.org/licenses/LICENSE-2.0).

**Shared handoff:** [workstream map](../WORKSTREAMS.md).

**Complexity:** 3/5
