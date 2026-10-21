# ADR-002 identity deployment record and handoff

This records the completed standalone identity installation and admin-grant upgrade on the Ubuntu 24.04 Azure test VM. The [initial install procedure](identity-initial-install-history.md) is historical and must not be rerun on this VM. ADR-003 authorization, ADR-009 browser admission, client integration, backups, and other first-test ADRs remain separate gates. Do not admit real users until those gates and a reviewed release are complete.

**Current Azure state (owner-reported, 2026-10-18):** the pinned identity binary has been built. `setup-identity-postgres-empty-test.sh` created the empty `cordel_identity` database, two roles, and protected credential files. The backup/restore gate was explicitly skipped for this isolated test. The owner-reported migration applied one schema migration and verified seven identity tables with zero accounts. The owner-reported installer verified its SHA-256, installed the API binary/unit and Caddy `/v1/*` route, created `/root/Cordel.caddy.pre-identity`, and initially left `cordel-api` disabled. **Do not rerun database setup, the first migration, or installation.** A later VM check found the API active on `127.0.0.1:3000` and a local HTTP 403 for public registration. A Cloud Shell request through public HTTPS returned HTTP 403 with `public_registration_disabled`; the media root still returned HTTP 200 with valid TLS. Cloud Shell TCP probes found public ports 3000, 5432, and 7880 blocked. The owner then enabled `cordel-api` for boot. After a VM reboot, PostgreSQL, Caddy, LiveKit, and the API were active, the API still listened on `127.0.0.1:3000`, and public HTTPS still returned HTTP 403 with valid TLS. The Cloud Shell SSH source changed, so the NSG allowlist was narrowed to its new `/32`. Entra SSH and administrator `sudo` worked; the SSH ED25519 fingerprint matched an independent Azure Run Command check. The owner then created the disposable `test-admin` account and `Cordel-Test` community from an interactive Entra SSH session; the host CLI reported an audit record. The database is now nonempty. A later isolated Cloud Shell client login/`me`/refresh/logout smoke test passed; product integration remains.

## Completed test upgrade: account-bound Management Mode

Commit `70bfd9f052273854f2a4db9aca9c25df420fd1ce` contains the source-verified `0002_admin_grant.sql` migration and the new API. The VM now has migrations `1,2`, the new runtime grants, and the admin-grant API binary. The first upgrade script stopped on a verification bug; the corrected script completed and reported the new binary SHA-256. **Use the [upgrade handoff](admin-grant-upgrade-handoff.md) for the exact output and remaining gates. Do not rerun either upgrade script.** The initial install and migration scripts below are for a fresh VM and must not be rerun on this one.

The first authenticated unlock returned HTTP 500 because the restricted runtime role cannot run `SELECT ... FOR SHARE` on `accounts`. A read-only `SET ROLE` probe reproduced `permission denied for table accounts`. PostgreSQL requires `UPDATE` on at least one column of every row-locked table. The [guarded repair](fix-admin-grant-row-lock.sh) grants `UPDATE` only on the inert `created_at` columns of `accounts` and `memberships`, then checks the same joined row lock as the API. The owner ran the checksum-verified repair, and its runtime-role row-lock probe passed while the API stayed active. The focused external retry then passed own-password unlock, ordinary-bearer denial, one-use invitation issuance, explicit lock, revoked-grant denial, and logout. It did not change the API binary or service. The canonical upgrade script now includes these grants and probe for a separate VM at the recorded pre-upgrade state; **do not rerun that script on the current VM**.

At the time of the admin-grant upgrade, the database contained one disposable account, community, membership, and session. A regular disposable `test-user` was later created through a one-use invitation. The former backup exception applied only while the database was empty. This test-only procedure requires the explicit script argument `accept_disposable_test_data_without_backup=true`; use it only when the owner has decided that this disposable state may be lost and the lack of a verified backup is accepted. Do not use this exception for real users or a production-like database. The upgrade script independently checks the exact recorded test snapshot and the previous binary SHA-256 `dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5` before it changes anything.

**Historical command for a VM at the recorded pre-upgrade state only.** With the earlier API and only migration `1`, first upload `build-identity-admin-grant.sh` to the current Cloud Shell home directory. The current VM has already built this artifact; **do not repeat this stage while resuming its partial upgrade**. Run Command builds the exact commit above on Ubuntu 24.04 into a revision-specific artifact directory without installing it or changing the database. The explicit `$HOME` path avoids Azure CLI treating a missing local `@file` as script text:

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

For a VM at that exact historical pre-upgrade state, review the reported revision and SHA-256. Then obtain the corrected `upgrade-admin-grant-test.sh`, verify its SHA-256 against the handoff, and invoke it with the explicit test-data acknowledgment. Pass the reviewed file to `bash -s` as a quoted here-document rather than using Azure `--parameters`: Azure may prepend `export arg1=...` before a script's shebang, causing Bash syntax to run under `/bin/sh`. The current VM must use the [resume handoff](admin-grant-upgrade-handoff.md).

```bash
(
set -e
test -s "$HOME/upgrade-admin-grant-test.sh" || { echo 'Upload the upgrade script to this Cloud Shell session first'; exit 1; }
printf '%s  %s\n' 'c58855d1f03958ffde29d67c5d4d790afd5a87e0a8eb06efed5b8339a7d81393' "$HOME/upgrade-admin-grant-test.sh" | sha256sum -c -
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

## Media admission gate (not deployed)

The Server 005 media service and the Server 006 admission gate are local
source only. Nothing in this section has been applied to the Azure test VM.
The Caddy change below is the deployment step that the gate depends on, and
it must not be applied before the identity binary that serves the gate is
installed and the LiveKit API settings are present.

The gate closes the path into a room for a moderated account. LiveKit itself
cannot deny a token it already issued, so Axum answers the question before the
reverse proxy hands the signaling upgrade to LiveKit.

The identity service opens a second loopback listener for the gate, separate
from the public `/v1/*` path:

```ini
# cordel-identity service
Environment=CORDEL_MEDIA_GATE_ADDR=127.0.0.1:3001
Environment=CORDEL_LIVEKIT_API_KEY=<livekit api key>
Environment=CORDEL_LIVEKIT_API_SECRET=<livekit api secret>
Environment=CORDEL_LIVEKIT_URL=wss://media.example
Environment=CORDEL_LIVEKIT_API_URL=http://127.0.0.1:7880
```

The gate is a host-local port. It must not be reachable from the Internet,
and the LiveKit signaling port must stay blocked, so the only way into a room
is through the gate. The Caddy configuration becomes:

```caddy
    handle /v1/* {
        reverse_proxy 127.0.0.1:3000
    }
    handle {
        # The signaling path is gated by Axum, so a moderated account cannot
        # enter a room with a token it already holds.
        @livekit path /rtc /rtc/*
        handle @livekit {
            forward_auth 127.0.0.1:3001 {
                uri /v1/livekit/authorize
                method GET
                header_up X-Cordel-Livekit-Token {query.access_token}
            }
            reverse_proxy 10.77.1.4:7880
        }
        handle {
            reverse_proxy 10.77.1.4:7880
        }
    }
```

The LiveKit SDK sends the access token in the `Authorization` header, not in
the query string, and the reverse proxy copies the request headers to the
gate subrequest. The gate therefore reads the token from `Authorization`, and
the `header_up` line only covers a client that puts it in the query. Either
way the token stays out of the URL and out of the access log.

After the change, check three things before admitting real users: a valid
admission still joins, a stale generation is refused with 403, and the LiveKit
port is still unreachable from outside the host. The local script
`deploy/native/local-lxc-gate.sh` runs exactly that check against a real
proxy.

## Current evidence and next gate

The [deployment record](../records/deployment-record.md) has the observed checks and the [ADR status ledger](../../docs/adr/STATUS.md) tracks completion. The installed ADR-002 binary passed focused own-password grant, invitation, and disposable-user redemption checks. ADR-003 policy source is verified in isolation but not deployed. Off-host restore and integrated client/media checks remain open.
