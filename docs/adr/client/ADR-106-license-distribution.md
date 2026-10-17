# ADR-106: Resolve UI and repository licensing before further publication

**Status:** Proposed

**Delivery stage:** First-test mandatory

**Date:** 2026-10-17

**Decider:** Project owner

## Context

The official public GitHub repository now contains the Linux Phase 0 client, client state, and shared protocol source, but the owner has not chosen Cordel's license. The proposed native UI is Slint. Its current official licensing paths include GPLv3 and a royalty-free desktop/mobile/web license with attribution conditions; a paid path also exists. Repository visibility and publication terms cannot be inferred from a URL.

## Proposed decision

Before further publishing application code or binaries, choose the intended license for Cordel and a compatible Slint license path. For a public repository that should permit broad reuse without requiring the whole binary to be GPLv3, evaluate Slint's current royalty-free terms and required attribution against the owner's distribution plan. If the owner wants GPLv3 distribution, the GPL path may be suitable. Record the exact dependency versions, license notices, attribution location, and any non-code assets' terms. Have the final selection reviewed against the current official terms before release.

Until the owner selects a license, do not label Cordel as MIT, Apache-2.0, GPLv3, proprietary, or “open source” merely because a GitHub repository exists. This ADR does not decide repository visibility, release channel, signing, or package format.

## Implementation boundary

- **Owns:** license research, owner decision record, third-party notice inventory, and package/About attribution requirements. It does not choose product architecture or publish code on its own.
- **Handoff:** provide current official terms, a recommended compatible path, and the exact files/notices required before any application code or binary publication.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| Royalty-free Slint terms plus owner-selected code license | Flexible app terms | Attribution and terms must be followed |
| GPLv3 path | Clear open-source distribution path | Copyleft obligations for distributed binary |
| Paid Slint license | Different support/terms | Cost and contract decision |
| Replace Slint after Phase 0 | Avoid Slint license constraints | New UI feasibility work |

## Consequences

- Client and protocol code was published before this gate was resolved. Resolve the license choice before further publication; drafting ADRs and private prototypes may continue.
- A successful Slint technical spike does not settle distribution rights.
- The owner-selected repository URL is configured as local `origin`, and client/protocol source is already public. Repository state and license terms must be inspected before further code or binary publication. No push/deployment is authorized by this ADR.

## Acceptance and handoff

- Owner chooses Cordel's license and confirms desired repository visibility and distribution model.
- Review current Slint terms, source/binary obligations, attribution, and third-party notices; add the chosen license and notice files before further code publication.
- Confirm package artifacts and About screen satisfy the chosen UI license.

**References:** [Slint licensing choices](https://slint.dev/get-started), [Slint terms](https://slint.dev/terms-and-conditions).

**Shared handoff:** [workstream map](../WORKSTREAMS.md).

**Complexity:** 3/5
