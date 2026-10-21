#!/usr/bin/env python3
"""Reset a disposable account password through the server's own command.

The server reads the password from the terminal, so this drives it with a
pseudo terminal. It is a local development helper: the account is disposable.
"""

import os
import pty
import select
import subprocess
import sys


def drive(command, secret: str) -> int:
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "dumb"
        os.execvp(command[0], command)
    written = 0
    buffer = b""
    while True:
        ready, _, _ = select.select([fd], [], [], 20)
        if not ready:
            break
        try:
            chunk = os.read(fd, 1024)
        except OSError:
            break
        if not chunk:
            break
        buffer += chunk
        if b"password" in buffer.lower() and written < 2:
            os.write(fd, secret.encode() + b"\n")
            written += 1
    _, status = os.waitpid(pid, 0)
    return os.waitstatus_to_exitcode(status)


if __name__ == "__main__":
    login = sys.argv[1]
    secret = sys.argv[2]
    code = drive(
        ["./server/identity/target/debug/cordel-identity", "recover-admin", login],
        secret,
    )
    print(f"exit={code}")
    sys.exit(code)
