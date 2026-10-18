# Third-party licenses and release notices

Cordel-authored source and documentation use [Apache-2.0](../../LICENSE). The
license does not relicense dependencies, bundled native libraries, or assets
owned by others. The dependency inventory below reflects the checked-in source
and lockfiles on 2026-10-18. It is not a complete binary bill of materials.

| Component | Current checked-in version | License path and source | Cordel action |
| --- | --- | --- | --- |
| Slint runtime and build tooling in `client/phase0` | `1.18.1` in `client/phase0/Cargo.lock`; manifest allows compatible `1.x` versions from `1.14` | [Slint royalty-free license 2.0](https://github.com/slint-ui/slint/blob/v1.18.1/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md), chosen for Cordel desktop distribution | Preserve Slint's notices. Before distributing a client binary, expose its `AboutSlint` widget in an About screen reachable from the top-level menu, and verify that path in the packaged application. Recheck the terms if the locked version changes. |
| LiveKit Rust SDK in `client/phase0` | `0.9.5` in `client/phase0/Cargo.lock` | [LiveKit Rust SDK license and NOTICE](https://github.com/livekit/rust-sdks) (Apache-2.0) | Include applicable upstream copyright and NOTICE material when distributing a binary containing the SDK. Audit its native WebRTC download and transitive licenses for the exact package. |
| LiveKit server on the Azure test VM | `1.13.10` binary, installed separately from this repository | [LiveKit server license](https://github.com/livekit/livekit) (Apache-2.0) | Keep its upstream license and notice with a redistributed server binary. A VM installation is not a Cordel binary package. |
| Other Rust crates and native libraries | Versions in each crate's `Cargo.lock` | Their respective upstream license files | Generate and review a complete dependency/license inventory for each release target. Preserve required license texts and notices in the package. |

No third-party image, font, or mock asset is tracked as a Cordel project asset
at this stage. Journal mock images are outside the repository. Record an asset's
source and distribution terms before adding it to a release.

## Release checks

1. Build from reviewed lockfiles with `--locked`; record the package revision and target OS.
2. Generate an inventory of Rust crates and native components actually shipped
   for that target. Review each license and required NOTICE or attribution file.
3. Include the Cordel `LICENSE` and `NOTICE` plus applicable third-party texts
   with the package. Do not imply that Cordel's Apache-2.0 license covers Slint.
4. In a packaged desktop client, open About from the top-level menu and verify
   that `AboutSlint` is visible. The Phase 0 prototype has no About screen yet;
   do not distribute it as a release until this is implemented and checked.
5. Recheck Slint's terms and attribution whenever its locked version or the
   distribution model changes. Embedded-system distribution requires a separate
   license decision.
