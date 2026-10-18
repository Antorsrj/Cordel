# ADR-002 identity deployment preparation

This procedure installs the current `server/identity` crate on the existing Ubuntu 24.04 VM. It is a deployment gate for the standalone identity component, not an integrated Cordel release: ADR-003 authorization, ADR-009 browser admission, client session handling, backups, and first-test mandatory ADR completion remain separate. Do not expose an invitation flow to test users until those gates and a reviewed release are complete.

**Current Azure state (owner-reported, 2026-10-18):** the pinned identity binary has been built. `setup-identity-postgres-empty-test.sh` created the empty `cordel_identity` database, two roles, and protected credential files. The backup/restore gate was explicitly skipped for this isolated test. The owner-reported migration applied one schema migration and verified seven identity tables with zero accounts. The owner-reported installer verified its SHA-256, installed the API binary/unit and Caddy `/v1/*` route, created `/root/Cordel.caddy.pre-identity`, and initially left `cordel-api` disabled. **Do not rerun database setup, the first migration, or installation.** A later VM check found the API active on `127.0.0.1:3000` and a local HTTP 403 for public registration. A Cloud Shell request through public HTTPS returned HTTP 403 with `public_registration_disabled`; the media root still returned HTTP 200 with valid TLS. Cloud Shell TCP probes found public ports 3000, 5432, and 7880 blocked. The owner then enabled `cordel-api` for boot. After a VM reboot, PostgreSQL, Caddy, LiveKit, and the API were active, the API still listened on `127.0.0.1:3000`, and public HTTPS still returned HTTP 403 with valid TLS. The Cloud Shell SSH source changed, so the NSG allowlist was narrowed to its new `/32`. Entra SSH and administrator `sudo` worked; the SSH ED25519 fingerprint matched an independent Azure Run Command check. The owner then created the disposable `test-admin` account and `Cordel-Test` community from an interactive Entra SSH session; the host CLI reported an audit record. The database is now nonempty. A later isolated Cloud Shell client login/`me`/refresh/logout smoke test passed; product integration remains.

## Next upgrade: account-bound Management Mode

Commit `70bfd9f052273854f2a4db9aca9c25df420fd1ce` contains the source-verified `0002_admin_grant.sql` migration and the new API. The VM now has migrations `1,2`, the new runtime grants, and the admin-grant API binary. The first upgrade script stopped on a verification bug; the corrected script completed and reported the new binary SHA-256. **Use the [upgrade handoff](admin-grant-upgrade-handoff.md) for the exact output and remaining live checks. Do not rerun either upgrade script.** The initial install and migration scripts below are for a fresh VM and must not be rerun on this one.

The first authenticated unlock returned HTTP 500 because the restricted runtime role cannot run `SELECT ... FOR SHARE` on `accounts`. A read-only `SET ROLE` probe reproduced `permission denied for table accounts`. PostgreSQL requires `UPDATE` on at least one column of every row-locked table. The [guarded repair](fix-admin-grant-row-lock.sh) grants `UPDATE` only on the inert `created_at` columns of `accounts` and `memberships`, then checks the same joined row lock as the API. It does not change the API binary or service. The canonical upgrade script now includes these grants and probe for a separate VM at the recorded pre-upgrade state; **do not rerun that script on the current VM**.

The database now contains one disposable account, community, membership, and session. The former backup exception applied only while the database was empty. This test-only procedure requires the explicit script argument `accept_disposable_test_data_without_backup=true`; use it only when the owner has decided that this disposable state may be lost and the lack of a verified backup is accepted. Do not use this exception for real users or a production-like database. The upgrade script independently checks the exact recorded test snapshot and the previous binary SHA-256 `dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5` before it changes anything.

For a VM at the recorded pre-upgrade state with the earlier API and only migration `1`, first upload `build-identity-admin-grant.sh` to the current Cloud Shell home directory. The current VM has already built this artifact; **do not repeat this stage while resuming its partial upgrade**. Run Command builds the exact commit above on Ubuntu 24.04 into a revision-specific artifact directory without installing it or changing the database. The explicit `$HOME` path avoids Azure CLI treating a missing local `@file` as script text:

```bash
(
set -e
test -s "$HOME/build-identity-admin-grant.sh" || { echo 'Upload the build script to this Cloud Shell session first'; exit 1; }
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts "@$HOME/build-identity-admin-grant.sh" \
  --query 'value[].message' --output tsv
)
```

For a VM at that exact pre-upgrade state, review the reported revision and SHA-256. Then obtain the **current corrected** `upgrade-admin-grant-test.sh`, verify its SHA-256 against the handoff, and invoke it with the explicit test-data acknowledgment. Pass the reviewed file to `bash -s` as a quoted here-document rather than using Azure `--parameters`: Azure may prepend `export arg1=...` before a script's shebang, causing Bash syntax to run under `/bin/sh`. The current VM must use the [resume handoff](admin-grant-upgrade-handoff.md).

```bash
(
set -e
test -s "$HOME/upgrade-admin-grant-test.sh" || { echo 'Upload the upgrade script to this Cloud Shell session first'; exit 1; }
printf '%s  %s\n' '3e2bbde9704afea39574861f7be0a2ca9d8a1af566b3532b57ec751920e3b8cb' "$HOME/upgrade-admin-grant-test.sh" | sha256sum -c -
upgrade_script=$(cat "$HOME/upgrade-admin-grant-test.sh")
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts "bash -s -- accept_disposable_test_data_without_backup=true <<'CORDEL_UPGRADE_SCRIPT'
$upgrade_script
CORDEL_UPGRADE_SCRIPT" \
  --query 'value[].message' --output tsv
)
```

The script applies migration 0002 using the protected migration URL, grants the runtime role `SELECT`/`INSERT`/`UPDATE` on `admin_grants` and `INSERT` on `admin_audit`, verifies those privileges and the two-entry migration ledger, then stops the service and atomically replaces the binary. It preserves the verified old binary at `/usr/local/bin/cordel-identity.rollback-cf373b62dfb270badb137c20e5d380949c3e9b4d`. If the new service fails to start, it restores that binary and attempts to restart the service. Migration 0002 is additive; binary rollback does not roll back the schema. A failed or interrupted run requires local inspection before retrying. Do not admit real users based on this isolated upgrade.

## Preconditions

- Use an approved maintenance window and authorized VM access. Check the current VM, PostgreSQL health, disk space, and deployment record first.
- Build the reviewed source commit `cf373b62dfb270badb137c20e5d380949c3e9b4d` on Ubuntu 24.04 using the procedure below. Cloud Shell runs Azure Linux 3 and must not be assumed ABI-compatible with this guest. Record the source revision, Rust version, build command, and SHA-256. Never transfer credentials with the executable.
- The build uses the published Cordel repository and pins its full source revision. Do not build from an unreviewed source copy.
- For a normal deployment, confirm a verified PostgreSQL backup and restore procedure before schema changes. The owner explicitly authorized skipping that gate on 2026-10-18 for this isolated first test while the identity database and roles were absent. This exception permits a disposable technical deployment only: do not invite real users, promise recovery, or mark ADR-007 Done. Never use a production-like user database for a test.
- Existing Caddy routes media traffic to LiveKit. The installer adds `/v1/*` to loopback Axum and keeps other paths routed to LiveKit. It validates a candidate Caddyfile before replacing the live file.

## Read-only Azure guest preflight

From Azure Cloud Shell, stage the reviewed `identity-preflight.sh` file in the current directory, then invoke it against the existing VM:

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @identity-preflight.sh \
  --query 'value[].message' --output tsv
```

Run Command executes as root. This script only inspects state: Ubuntu version, Caddy/PostgreSQL/LiveKit and identity unit states, identity database existence from PostgreSQL catalog metadata, identity file presence and environment key names/mode, Caddy `/v1` route/validation, root filesystem space, and build-tool availability/version. It does not print environment values, Caddy contents, database rows, or credentials. The identity database catalog check can report unavailable when PostgreSQL is stopped or its local peer connection is unavailable. Do not infer overall deployment readiness from one output line.

## Build the pinned identity binary on the VM

The build script checks Ubuntu 24.04, uses the dedicated unprivileged `cordel-build` account, installs Rust 1.99.1 into that account's isolated Rust/Cargo directories, checks out commit `cf373b62dfb270badb137c20e5d380949c3e9b4d`, and runs `cargo +1.99.1 build --locked --release`. It does not install the binary as a service or change the database. Required host tools are `git`, `curl`, `cc`, and `pkg-config`. The VM initially lacked `cc` and `pkg-config`; `prepare-identity-build-host.sh` installed `build-essential`, `pkg-config`, `git`, and `curl` through Ubuntu apt. It did not install Rust globally.

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @prepare-identity-build-host.sh \
  --query 'value[].message' --output tsv
```

Building on the VM avoids copying a binary across environments. It downloads the pinned Rust toolchain and Cargo dependencies and uses VM CPU, disk, and network. Allow several GB of free disk and potentially tens of minutes; check available space and the maintenance window first. The isolated Cargo cache and checked-out build data remain under `/var/lib/cordel-identity-build` to make reruns efficient. The script preserves an existing artifact and reuses it only when its revision manifest and SHA-256 agree; incomplete or mismatched artifact paths stop for operator review.

Stage the reviewed `build-identity.sh` in Azure Cloud Shell, then invoke it:

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @build-identity.sh \
  --query 'value[].message' --output tsv
```

The build completed on 2026-10-18 and reported source revision `cf373b62dfb270badb137c20e5d380949c3e9b4d`, Rust 1.99.1, artifact `/var/lib/cordel-identity-build/artifacts/cf373b62dfb270badb137c20e5d380949c3e9b4d/cordel-identity`, and SHA-256 `dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5` (`reused=no`). Verify the digest on the guest before installation.

For a future fresh deployment, run the disposable restore gate before creating its real identity database. The current Azure test skipped this gate with owner authorization; do not run it now as a substitute for backing up the existing database. The gate verifies a pinned binary, creates two uniquely named disposable PostgreSQL databases, migrates one, inserts unusable synthetic rows, dumps it, restores into the other, and compares the restored rows and migration ledger. It retains both databases and a root-only local dump for operator review. It never changes an existing `cordel_identity` database, creates no real accounts, and does not prove off-host recovery or complete ADR-007. For a fresh deployment, upload the latest `identity-backup-restore-gate.sh` to Cloud Shell and run:

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @identity-backup-restore-gate.sh \
  --query 'value[].message' --output tsv
```

The gate writes a root-owned mode `0600` marker tied to the tested binary and dump. Its retained test databases and dump need a separate reviewed cleanup decision. For this isolated first test only, the owner authorized skipping this gate while the identity database, roles, secret files, and API installation were absent. `setup-identity-postgres-empty-test.sh` carried the explicit bypass and verified those conditions. It ran successfully on 2026-10-18. This command is retained for a fresh replacement host only; do not run it again on the current VM:

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @setup-identity-postgres-empty-test.sh \
  --query 'value[].message' --output tsv
```

The completed command created the real but empty identity database, two restricted PostgreSQL roles, and root-only runtime/migration credential files. It did not migrate schema, start the API, create accounts, or verify backup. Its output contained file paths and role names only. The script intentionally refuses to overwrite a partial setup. Do not invite real users until an encrypted off-host backup and isolated restore procedure exists.

For a fresh deployment after successful setup, upload `migrate-identity-test.sh` to Cloud Shell and run the first migration without parameters. This stage passed on the current VM on 2026-10-18; do not rerun it there. The test-only filename carries the bypass. It verifies the database and roles, refuses an existing schema, applies the pinned migration with the migrator role, grants only the table operations required by the API to the runtime role, and confirms there are no accounts:

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @migrate-identity-test.sh \
  --query 'value[].message' --output tsv
```

For a fresh deployment after migration, upload the current `install-identity.sh` to Cloud Shell and invoke it without parameters. This stage passed on the current VM on 2026-10-18; do not rerun it there. A new Cloud Shell session does not retain previously uploaded scripts, but the VM's database and build artifact persist. In this test mode the installer selects the pinned guest artifact path and verifies its digest. It also checks the empty first schema, validates both Caddy configurations before changing files, and creates `/root/Cordel.caddy.pre-identity` as a root-only rollback copy. It refuses an existing copy or partial identity install. If a later step fails, it attempts to restore Caddy and remove the binary and unit it just created; inspect the remaining service account and state directory before any retry.

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts @install-identity.sh \
  --query 'value[].message' --output tsv
```

The install script still leaves the API disabled and stopped. Review its route and service unit before starting the API. Do not paste build logs, environment values, or database content into Cloud Shell output or chat.

## Credentials and database roles

`setup-identity-postgres-empty-test.sh` runs the reviewed Python setup with the explicit empty-test bypass. It generates distinct passwords and a JWT signing secret on the VM, uses the PostgreSQL `\password` prompt so plaintext role passwords do not appear in SQL command text, and writes `/root/cordel-identity-migration.env` and `/etc/cordel/identity.env` as root-owned mode `0600`. It refuses existing roles, database, secret files, or API installation. It does not print secrets. `migrate-identity-test.sh` uses only the migration-owner URL for schema changes and grants the runtime role the minimum operations used by the current API; it does not grant runtime access to `host_audit`. Do not rerun either script after a partial failure or manually create the same roles. Inspect PostgreSQL and protected files locally before recovery.

The API uses the runtime role URL from `/etc/cordel/identity.env`. The host-side first-admin and recovery commands must use the migration-owner URL from `/root/cordel-identity-migration.env` because they write `host_audit`. Do not put either URL in a shell argument, chat, repository, or console log. The test database currently has no off-host backup. Establish and verify one before admitting real users.

## Install and start

The installer creates a root-only Caddy rollback copy. Do not create that path manually before running it, because the installer refuses to overwrite an existing copy. If using an interactive host session, first inspect current services and listeners without displaying secrets:

```bash
sudo systemctl is-active caddy postgresql
sudo ss -lntp
```

The build script leaves the verified executable on the VM in its revision-specific artifact directory. Use the Azure Run Command invocation above or, when using SSH, invoke the installer directly with that artifact path and the recorded digest:

```bash
sudo bash install-identity.sh \
  /var/lib/cordel-identity-build/artifacts/cf373b62dfb270badb137c20e5d380949c3e9b4d/cordel-identity \
  dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5
```

The script checks checksum, schema, and host preconditions, creates a dedicated `cordel` service account, installs a systemd unit, and updates Caddy. It leaves the API disabled and stopped. Review `/etc/systemd/system/cordel-api.service` and the Caddy route structure without printing `/etc/cordel/identity.env`.

The migration script must have reported `identity_schema=applied`, `identity_tables=7`, `migrations=1`, and `accounts=0` before this installation. Verify `/etc/cordel/identity.env` is root-owned mode `0600` without displaying its contents. Then start and inspect:

From Azure Cloud Shell, the following separate Run Command stage starts the already installed service and prints its listener for loopback review. It does not enable the service at boot or print credentials:

```bash
az vm run-command invoke \
  --subscription f77cc05e-0316-4c39-9ee2-74b5d9078f0a \
  --resource-group rg-cordel-test-eastus \
  --name vm-cordel-test-01 \
  --command-id RunShellScript \
  --scripts 'set -eu
systemctl start cordel-api.service
systemctl is-active cordel-api.service
ss -lnt | grep 3000' \
  --query 'value[].message' --output tsv
```

Check the output before the public HTTPS proof. If the service fails to start, inspect unit status and logs on the VM without displaying environment files. Do not rerun installation or migration.

For an interactive SSH session, the equivalent initial checks are:

```bash
sudo systemctl start cordel-api.service
sudo systemctl --no-pager --full status cordel-api.service
sudo journalctl -u cordel-api.service -n 50 --no-pager
curl -i --silent --show-error https://cordel-media-f77cc05e.eastus.cloudapp.azure.com/v1/accounts \
  -H 'content-type: application/json' -d '{}'
```

The last request should return the documented `public_registration_disabled` error envelope. Confirm `/v1` is served through Caddy, raw port 3000 is loopback-only, PostgreSQL remains loopback-only, and port 7880 remains blocked externally. Do not log request bodies or credentials. Never test host bootstrap or recovery on an instance with existing user data unless following a reviewed recovery procedure.

After those checks pass, enable boot startup with `sudo systemctl enable cordel-api.service`, reboot only within the approved maintenance window, and verify the API and Caddy routes again. The installer leaves the unit disabled so it cannot start against an unmigrated database.

## First admin and operational limits

After the code/source review, migration, route, and secret checks pass, an authorized host operator may create one disposable admin for the isolated login proof. This CLI requires an interactive terminal; Azure Run Command is not a password-entry channel. Do not invite real users before backup and recovery are verified. Use the migration-owner credentials because the command writes `host_audit`:

```bash
sudo -i
set -a
source /root/cordel-identity-migration.env
set +a
/usr/local/bin/cordel-identity bootstrap-admin test-admin Cordel-Test
unset DATABASE_URL
exit
```

The binary prompts for the password twice and writes a `host_audit` record. This command created the initial community and first admin on the current VM on 2026-10-18; **do not run it again there**. For a fresh replacement, run it once only after verifying the target database state. `recover-admin` also prompts and revokes sessions, and must follow the ADR-002 authorized host recovery procedure. Never put passwords in command arguments. The current Axum service is not sufficient to represent the finished Cordel product and does not implement ADR-003 authorization or ADR-009 browser controls.

## Rollback

For a failed deployment, stop the API and restore Caddy routing first:

```bash
sudo systemctl stop cordel-api.service
sudo cp -a /root/Cordel.caddy.pre-identity /etc/caddy/Cordel.caddy
sudo caddy validate --config /etc/caddy/Cordel.caddy --adapter caddyfile
sudo systemctl restart caddy.service
sudo systemctl disable cordel-api.service
```

Keep `/usr/local/bin/cordel-identity`, the unit, and environment file quarantined with root-only permissions until the failure is understood. Do not delete the database or migration records as part of application rollback. The migration is additive and there is no reviewed down migration; restore database state only from the verified backup under the database recovery procedure. If the API may have created accounts, invites, or sessions, preserve its data and audit trail. Revert the binary only from a separately recorded, checksum-verified prior release.

## Evidence to record

Record source revision, binary SHA-256, Rust/build versions, PostgreSQL version, migration result, Caddy validation result, service account and unit state, loopback listener checks, external `/v1/accounts` disabled-registration result, and rollback artifact locations. Do not record secret values, password hashes, invite codes, bearer tokens, raw request bodies, or database contents. This procedure does not prove full application integration, two-network behavior, backup recovery, or completion of ADR-002.
