#!/bin/sh
""":"
exec python3 "$0" "$@"
":"""
from __future__ import annotations

# Create the ADR-002 database roles and root-only secrets for the test VM.

import os
import pty
import re
import select
import secrets
import shutil
import stat
import subprocess
import sys
import time
import argparse


REVISION = "cf373b62dfb270badb137c20e5d380949c3e9b4d"
EXPECTED_BINARY_SHA = "dcc97ecceda9024b6895c4331b907ae92653562f2490272d242b505a39a2f4c5"
DATABASE = "cordel_identity"
APP_ROLE = "cordel_identity_app"
MIGRATOR_ROLE = "cordel_identity_migrator"
GATE_MARKER = "/var/lib/cordel-identity-backup-gate/verified"
MIGRATION_ENV = "/root/cordel-identity-migration.env"
RUNTIME_ENV = "/etc/cordel/identity.env"
MAX_GATE_AGE_SECONDS = 7 * 24 * 60 * 60


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def require_root_ubuntu() -> None:
    if os.geteuid() != 0:
        fail("Run through Azure Run Command or as root.")
    try:
        values = {}
        with open("/etc/os-release", encoding="utf-8") as source:
            for line in source:
                if "=" in line:
                    key, value = line.rstrip().split("=", 1)
                    values[key] = value.strip('"')
    except OSError:
        fail("Cannot identify operating system.")
    if values.get("ID") != "ubuntu" or values.get("VERSION_ID") != "24.04":
        fail("Requires Ubuntu 24.04.")


def require_tools() -> None:
    for name in ("systemctl", "runuser", "psql", "openssl", "sha256sum"):
        if shutil.which(name) is None:
            fail(f"Required setup tool missing: {name}")


def read_gate() -> dict[str, str]:
    try:
        info = os.stat(GATE_MARKER, follow_symlinks=False)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o600:
            fail("Disposable backup/restore gate marker has unexpected type, owner, or mode.")
        with open(GATE_MARKER, encoding="ascii") as source:
            fields = dict(line.rstrip("\n").split("=", 1) for line in source if "=" in line)
    except (OSError, ValueError):
        fail("A verified disposable backup/restore gate is required first.")
    try:
        age = int(time.time()) - int(fields.get("verified_at_epoch", "0"))
    except ValueError:
        age = MAX_GATE_AGE_SECONDS + 1
    if (
        fields.get("result") != "passed"
        or fields.get("revision") != REVISION
        or fields.get("binary_sha256") != EXPECTED_BINARY_SHA
        or age < 0
        or age > MAX_GATE_AGE_SECONDS
    ):
        fail("Disposable backup/restore gate is missing, stale, or for a different build.")
    dump_file = fields.get("dump_file", "")
    if not dump_file.startswith(f"{os.path.dirname(GATE_MARKER)}/run.") or not os.path.isfile(dump_file):
        fail("Disposable backup artifact is unavailable; rerun the restore gate for review.")
    try:
        dump_sha = subprocess.run(
            ["sha256sum", dump_file], check=True, text=True, capture_output=True
        ).stdout.split()[0]
    except (OSError, subprocess.CalledProcessError, IndexError):
        fail("Could not verify the disposable backup artifact.")
    if dump_sha != fields.get("disposable_dump_sha256"):
        fail("Disposable backup artifact checksum does not match its gate marker.")
    source_db = fields.get("source_database", "")
    restored_db = fields.get("restored_database", "")
    db_pattern = re.compile(r"cordel_identity_backup_gate_[0-9]+_[0-9a-f]{8}_(?:src|dst)\Z")
    if not db_pattern.fullmatch(source_db) or not db_pattern.fullmatch(restored_db):
        fail("Disposable restore gate database names are invalid.")
    catalog_sql = (
        "SELECT count(*) FROM pg_database WHERE datname IN "
        f"('{source_db}', '{restored_db}')"
    )
    try:
        db_count = subprocess.run(
            ["runuser", "-u", "postgres", "--", "psql", "-XAt", "-v", "ON_ERROR_STOP=1", "-d", "postgres", "-c", catalog_sql],
            check=True,
            text=True,
            capture_output=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        fail("Could not verify retained disposable restore databases.")
    if db_count != "2":
        fail("Disposable restore databases are not both retained; rerun the restore gate.")
    return fields


def check_binary() -> None:
    path = f"/var/lib/cordel-identity-build/artifacts/{REVISION}/cordel-identity"
    try:
        digest = subprocess.run(
            ["sha256sum", path], check=True, text=True, capture_output=True
        ).stdout.split()[0]
    except (OSError, subprocess.CalledProcessError, IndexError):
        fail("Pinned identity build artifact is unavailable.")
    if digest != EXPECTED_BINARY_SHA:
        fail("Pinned identity build artifact checksum mismatch.")


def check_database_absent() -> None:
    sql = (
        "SELECT (SELECT count(*) FROM pg_database WHERE datname = 'cordel_identity') || '|' || "
        "(SELECT count(*) FROM pg_roles WHERE rolname = 'cordel_identity_app') || '|' || "
        "(SELECT count(*) FROM pg_roles WHERE rolname = 'cordel_identity_migrator')"
    )
    try:
        result = subprocess.run(
            ["runuser", "-u", "postgres", "--", "psql", "-XAt", "-v", "ON_ERROR_STOP=1", "-d", "postgres", "-c", sql],
            check=True,
            text=True,
            capture_output=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        fail("Could not inspect local PostgreSQL catalog.")
    if result != "0|0|0":
        fail("Identity database or role already exists; refusing to alter existing state.")


def assert_secret_paths_absent() -> None:
    for path in (MIGRATION_ENV, RUNTIME_ENV):
        if os.path.lexists(path):
            fail("Identity secret file path already exists; refusing to overwrite it.")
    if not os.path.isdir(os.path.dirname(RUNTIME_ENV)):
        fail("/etc/cordel must exist before identity secret setup.")


def write_secret_file(path: str, contents: str) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    fd = os.open(path, flags, 0o600)
    try:
        os.fchmod(fd, 0o600)
        os.fchown(fd, 0, 0)
        with os.fdopen(fd, "w", encoding="ascii", closefd=False) as output:
            output.write(contents)
            output.flush()
            os.fsync(fd)
    finally:
        os.close(fd)


def await_pattern(master: int, pattern: re.Pattern[bytes], timeout: int = 30) -> bytes:
    output = bytearray()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if pattern.search(output):
            return bytes(output)
        readable, _, _ = select.select([master], [], [], max(0.0, deadline - time.monotonic()))
        if not readable:
            break
        try:
            chunk = os.read(master, 4096)
        except OSError:
            break
        if not chunk:
            break
        output.extend(chunk)
    raise RuntimeError("PostgreSQL interactive command did not reach its expected prompt.")


def send(master: int, value: str) -> None:
    os.write(master, value.encode("ascii") + b"\n")


def set_role_password(master: int, role: str, password: str) -> None:
    send(master, f"\\password {role}")
    await_pattern(master, re.compile(rb"Enter new password[^\r\n]*:", re.I))
    send(master, password)
    await_pattern(master, re.compile(rb"Enter it again:", re.I))
    send(master, password)
    await_pattern(master, re.compile(rb"postgres=[#>] "))


def create_roles_and_database(migrator_password: str, app_password: str) -> None:
    pid, master = pty.fork()
    if pid == 0:
        environment = {
            "PATH": "/usr/sbin:/usr/bin:/sbin:/bin",
            "LC_ALL": "C",
            "PGOPTIONS": "-c password_encryption=scram-sha-256",
        }
        os.execvpe(
            "runuser",
            [
                "runuser", "-u", "postgres", "--", "psql", "-X", "-v", "ON_ERROR_STOP=1", "-d", "postgres"
            ],
            environment,
        )
    try:
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        send(master, f"CREATE ROLE {MIGRATOR_ROLE} NOLOGIN;")
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        set_role_password(master, MIGRATOR_ROLE, migrator_password)
        send(master, f"ALTER ROLE {MIGRATOR_ROLE} LOGIN;")
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        send(master, f"CREATE ROLE {APP_ROLE} NOLOGIN;")
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        set_role_password(master, APP_ROLE, app_password)
        send(master, f"ALTER ROLE {APP_ROLE} LOGIN;")
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        send(master, f"CREATE DATABASE {DATABASE} OWNER {MIGRATOR_ROLE};")
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        send(master, f"REVOKE CONNECT ON DATABASE {DATABASE} FROM PUBLIC;")
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        send(master, f"GRANT CONNECT ON DATABASE {DATABASE} TO {MIGRATOR_ROLE}, {APP_ROLE};")
        await_pattern(master, re.compile(rb"postgres=[#>] "))
        send(master, "\\q")
        _, status = os.waitpid(pid, 0)
        if status != 0:
            raise RuntimeError("PostgreSQL role/database setup did not complete.")
    except Exception as error:  # Do not echo PTY contents or exception output.
        try:
            os.kill(pid, 15)
        except ProcessLookupError:
            pass
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
        raise RuntimeError("PostgreSQL role/database setup failed; preserve secret files for operator review.") from error
    finally:
        os.close(master)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Create Cordel identity database roles and root-only secrets."
    )
    parser.add_argument(
        "deployment_mode",
        nargs="?",
        choices=("require-backup-gate", "skip-backup-gate-for-empty-test-db"),
        default="require-backup-gate",
        help="use the default restore gate or explicitly bypass it for an empty test database",
    )
    args = parser.parse_args()
    require_root_ubuntu()
    require_tools()
    if subprocess.run(["systemctl", "is-active", "--quiet", "postgresql"]).returncode != 0:
        fail("PostgreSQL is not active.")
    check_binary()
    check_database_absent()
    assert_secret_paths_absent()
    if os.path.lexists("/etc/systemd/system/cordel-api.service") or os.path.lexists(
        "/usr/local/bin/cordel-identity"
    ):
        fail("Identity service is already installed; refusing first-time database setup.")
    skip_backup_gate = args.deployment_mode == "skip-backup-gate-for-empty-test-db"
    if not skip_backup_gate:
        read_gate()

    migrator_password = secrets.token_hex(32)
    app_password = secrets.token_hex(32)
    jwt_secret = secrets.token_hex(32)
    migration_url = f"postgresql://{MIGRATOR_ROLE}:{migrator_password}@127.0.0.1:5432/{DATABASE}"
    runtime_url = f"postgresql://{APP_ROLE}:{app_password}@127.0.0.1:5432/{DATABASE}"
    write_secret_file(MIGRATION_ENV, f"DATABASE_URL={migration_url}\n")
    write_secret_file(RUNTIME_ENV, f"DATABASE_URL={runtime_url}\nCORDEL_JWT_SECRET_HEX={jwt_secret}\n")
    try:
        create_roles_and_database(migrator_password, app_password)
    except RuntimeError:
        fail("PostgreSQL setup failed. Secret files remain root-only; inspect local PostgreSQL state before retrying.")
    print(f"result=created\ndatabase={DATABASE}\nroles={MIGRATOR_ROLE},{APP_ROLE}\n")
    print(f"migration_credentials_file={MIGRATION_ENV}\nruntime_credentials_file={RUNTIME_ENV}")
    print("database_schema=not_migrated")
    print(
        "backup_gate=skipped_for_empty_test_database"
        if skip_backup_gate
        else "backup_gate=disposable_restore_passed"
    )


if __name__ == "__main__":
    main()
