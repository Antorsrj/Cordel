# ADR-002 admin-grant upgrade handoff

**Recorded state: 2026-10-18, after a stopped upgrade.** Use this note if the Codex or Cloud Shell session is lost. The owner approved the isolated test upgrade without a verified backup of the disposable `test-admin`/`Cordel-Test` data. This does not authorize admitting real users or treating ADR-007 as complete.

The source-verified API is commit `70bfd9f052273854f2a4db9aca9c25df420fd1ce`. The owner built it on the Ubuntu VM with Rust 1.99.1. Its revision-specific artifact is `/var/lib/cordel-identity-build/artifacts/70bfd9f052273854f2a4db9aca9c25df420fd1ce/cordel-identity`, SHA-256 `34411cc5f19717fbcda6e5082fbdc155a57fe1ea281c0b8654c25ca4152f36da`.

The first upgrade invocation applied migration `0002_admin_grant.sql` and granted the runtime role its new table privileges, then stopped on a script verification bug **before the binary switch**. A separate read-only Azure Run Command confirmed all of the following:

- `cordel-api` is active with the **old** installed binary SHA-256 `dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5`.
- Migration versions are `1,2`; both `admin_grants` and `admin_audit` exist.
- `cordel_identity_app` has `SELECT`, `INSERT`, and `UPDATE` on `admin_grants` and `INSERT` on `admin_audit`.
- The old-binary rollback copy does not yet exist, because the script stopped before creating it.

The script concatenated PostgreSQL booleans into text and expected `t`; PostgreSQL returned `true`. The corrected script evaluates a SQL `CASE` over exact migration versions and privileges and accepts the already-applied second migration. Its SHA-256 is `c58855d1f03958ffde29d67c5d4d790afd5a87e0a8eb06efed5b8339a7d81393`. **Do not rerun the earlier script** with SHA-256 `e65f7a96fae9e3874ed5eeed14167312a47d7084a00376b43e13ac7ba3750558`.

## Safe resume

Before resuming, repeat the read-only diagnostic if the VM may have changed. Confirm the old installed binary and service state, migrations `1,2`, all four privileges, and the disposable data snapshot. The corrected script also checks the old binary SHA-256, service state, artifact manifest, exact database counts, test identity, and migration ledger before touching the service. It reruns SQLx migrations as a no-op and repeats the idempotent grants, then stores the verified old binary, switches to the new binary, and probes the local admin-grant route. It restores the old binary if the new service does not stay active or the route probe fails. Migration 0002 remains additive after a binary rollback.

From Azure Cloud Shell, obtain the corrected script from the repository's feature branch, verify its digest, and invoke it under Bash. The feature branch may move, so **the SHA-256 check is mandatory**. The script has no credentials; the VM reads its protected migration URL locally.

```bash
(
set -euo pipefail
cordel_upgrade_script="$HOME/upgrade-admin-grant-resume.sh"
curl -fsSL --proto '=https' --tlsv1.2 \
  'https://raw.githubusercontent.com/Antorsrj/Cordel/feature/adr-002-identity/deploy/native/upgrade-admin-grant-test.sh' \
  -o "$cordel_upgrade_script"
printf '%s  %s\n' \
  'c58855d1f03958ffde29d67c5d4d790afd5a87e0a8eb06efed5b8339a7d81393' \
  "$cordel_upgrade_script" | sha256sum -c -
upgrade_contents=$(cat "$cordel_upgrade_script")
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts "bash -s -- accept_disposable_test_data_without_backup=true <<'CORDEL_UPGRADE_SCRIPT'
$upgrade_contents
CORDEL_UPGRADE_SCRIPT" \
  --query 'value[].message' -o tsv
)
```

Expected success output includes `upgrade=complete`, source revision `70bfd9f052273854f2a4db9aca9c25df420fd1ce`, binary SHA-256 `34411cc5f19717fbcda6e5082fbdc155a57fe1ea281c0b8654c25ca4152f36da`, and `migrations=1,2`. If it fails, **do not rerun blindly**. Inspect the installed binary digest, API service, migration ledger, privileges, and rollback path first. Record the result in `deploy/records/deployment-record.md` and `docs/adr/STATUS.md`.

After successful replacement, verify public HTTPS still returns `public_registration_disabled` for `POST /v1/accounts`, that the unauthenticated admin-grant route returns HTTP 401, and that raw API and PostgreSQL ports remain blocked. A focused own-password grant and invitation test with the disposable admin is still required before declaring this change live. ADR-002 remains In progress until its remaining gates are complete.
