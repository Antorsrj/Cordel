#!/usr/bin/env bash
# Install only the Ubuntu packages required to build the pinned identity binary.
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo 'Run as root through Azure Run Command or SSH.' >&2; exit 1; }
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || {
  echo 'Requires Ubuntu 24.04.' >&2; exit 1;
}

if ! command -v cc >/dev/null 2>&1 || ! command -v pkg-config >/dev/null 2>&1 ||
   ! command -v git >/dev/null 2>&1 || ! command -v curl >/dev/null 2>&1; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-upgrade build-essential pkg-config git curl
fi

for tool in cc pkg-config git curl; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'Build tool remains missing: %s\n' "$tool" >&2; exit 1;
  }
done

printf 'Identity build prerequisites available.\n'
cc --version | head -n 1
pkg-config --version
git --version
curl --version | head -n 1
df -hP / | awk 'NR <= 2 {print}'
