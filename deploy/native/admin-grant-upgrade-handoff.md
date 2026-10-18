# ADR-002 admin-grant upgrade handoff

**Recorded state: 2026-10-18, after the row-lock repair and focused live smoke passed.** Use this note if the Codex or Cloud Shell session is lost. The owner approved the isolated test upgrade without a verified backup of the disposable `test-admin`/`Cordel-Test` data. This does not authorize admitting real users or treating ADR-007 as complete.

The source-verified API is commit `70bfd9f052273854f2a4db9aca9c25df420fd1ce`. The owner built it on the Ubuntu VM with Rust 1.99.1. Its revision-specific artifact is `/var/lib/cordel-identity-build/artifacts/70bfd9f052273854f2a4db9aca9c25df420fd1ce/cordel-identity`, SHA-256 `34411cc5f19717fbcda6e5082fbdc155a57fe1ea281c0b8654c25ca4152f36da`.

The first upgrade invocation applied migration `0002_admin_grant.sql` and granted the runtime role its new table privileges, then stopped on a script verification bug **before the binary switch**. At that point, a separate read-only Azure Run Command confirmed all of the following:

- `cordel-api` is active with the **old** installed binary SHA-256 `dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5`.
- Migration versions are `1,2`; both `admin_grants` and `admin_audit` exist.
- `cordel_identity_app` has `SELECT`, `INSERT`, and `UPDATE` on `admin_grants` and `INSERT` on `admin_audit`.
- The old-binary rollback copy does not yet exist, because the script stopped before creating it.

The script concatenated PostgreSQL booleans into text and expected `t`; PostgreSQL returned `true`. The corrected script evaluates a SQL `CASE` over exact migration versions and privileges and accepts the already-applied second migration. Its SHA-256 is `c58855d1f03958ffde29d67c5d4d790afd5a87e0a8eb06efed5b8339a7d81393`. **Do not rerun either upgrade script on the VM now.**

## Completed resume and current gate

The owner ran the corrected script from commit `56a379f1c098f6fdc28d638108f1869bc851f920` after Cloud Shell verified its SHA-256. Azure Run Command reported `upgrade=complete`, source revision `70bfd9f052273854f2a4db9aca9c25df420fd1ce`, new binary SHA-256 `34411cc5f19717fbcda6e5082fbdc155a57fe1ea281c0b8654c25ca4152f36da`, migrations `1,2`, and rollback copy `/usr/local/bin/cordel-identity.rollback-cf373b62dfb270badb137c20e5d380949c3e9b4d`. The script checked that the API service was active and its local unauthenticated admin-grant route returned HTTP 401. The backup exception remained explicit for disposable test data. Independent host and public HTTPS checks passed. The focused disposable-admin grant and invitation checks passed after the row-lock repair described below.

## Historical resume command — already completed

The command below records exactly how the stopped upgrade was resumed. **Do not run it again on the completed VM.** The corrected script checked the old binary SHA-256, service state, artifact manifest, exact database counts, test identity, and migration ledger before touching the service. It reran SQLx migrations as a no-op and repeated the idempotent grants, then stored the verified old binary, switched to the new binary, and probed the local admin-grant route. Migration 0002 remains additive after a binary rollback.

The owner downloaded the corrected script in Azure Cloud Shell, verified its digest, and invoked it under Bash. The feature branch can move; the SHA-256 check was mandatory. The script contained no credentials; the VM read its protected migration URL locally.

```bash
(
set -euo pipefail
cordel_upgrade_script="$HOME/upgrade-admin-grant-resume.sh"
curl -fsSL --proto '=https' --tlsv1.2 \
  'https://raw.githubusercontent.com/Antorsrj/Cordel/56a379f1c098f6fdc28d638108f1869bc851f920/deploy/native/upgrade-admin-grant-test.sh' \
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

The success output above is recorded in `deploy/records/deployment-record.md` and `docs/adr/STATUS.md`.

The owner independently confirmed the new binary and rollback hashes, active services, migrations `1,2`, loopback API listener, HTTPS/TLS, public registration HTTP 403, and unauthenticated admin-grant HTTP 401. Direct port reachability has not been rechecked after the upgrade. The focused own-password grant and invitation test with the disposable admin passed after the repair below. ADR-002 remains In progress until its remaining gates are complete.

To reproduce that focused check with another disposable account, download [azure-admin-grant-smoke.py](../azure-admin-grant-smoke.py) into a new Cloud Shell session, verify SHA-256 `b179fd9c729cc485dec1e7e446cd94788c097303856d7f4860e3ed3b60e931a2`, and run it with Python 3. It accepts only this Azure test origin and the disposable `test-admin` login. It prompts for the password without echoing, does not print tokens or invite codes, and creates one one-use invitation that expires in one hour. It tests ordinary app-bearer denial, own-password unlock, invitation issuance with a grant, lock, post-lock denial, and logout. Do not run it with a real user account or a production database. Record only the status lines in this repository, not secrets.

**First live smoke result:** login 200, ordinary app-bearer invitation denial 401, unlock 500, logout 200. It did not create an invitation. A read-only `SET ROLE cordel_identity_app` probe confirmed `SELECT ... FOR SHARE` on `accounts` fails with `permission denied for table accounts`; `memberships` likewise lacks the required column-level `UPDATE` privilege. The guarded [row-lock grant repair](fix-admin-grant-row-lock.sh) was then run from pinned commit `4b29f51e725a69feb85ace46aed690f878fbb23f` after SHA-256 verification (`469315843bdef78953c19bafb4c07fc0fd075446b90d3df77c0b4383ea251702`). Azure Run Command reported `subject_row_lock=1`, `row_lock_grants=applied`, and `api_service=active`. The repair granted `UPDATE` only on the `created_at` columns of `accounts` and `memberships`; it did not restart or replace the API.

The owner reran the checksum-verified smoke script against disposable `test-admin`. It reported login 200, ordinary app-bearer invitation denial 401, own-password unlock 200, one-use one-hour invitation issuance 200, explicit lock 200, revoked-grant invitation denial 401, logout 200, and `PASS admin_grant_smoke`. No credentials or invitation code were printed. The issued test invitation expires after one hour. This is focused live validation, not invite redemption, backup/restore, real-user admission, or integrated product proof. Do not rerun the binary upgrade.
