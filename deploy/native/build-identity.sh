#!/usr/bin/env bash
# Build the published ADR-002 identity revision on Ubuntu without installing or starting it.
set -euo pipefail

readonly revision=cf373b62dfb270badb137c20e5d380949c3e9b4d
readonly rust_version=1.99.1
readonly repository=https://github.com/Antorsrj/Cordel.git
readonly build_user=cordel-build
readonly base=/var/lib/cordel-identity-build
readonly home="$base/home"
readonly workroot="$base/work"
readonly artifact_dir="$base/artifacts/$revision"
readonly artifact="$artifact_dir/cordel-identity"

[[ $EUID -eq 0 ]] || { echo 'Run through Azure Run Command or as root.' >&2; exit 1; }
[[ -r /etc/os-release ]] || { echo 'Cannot identify operating system.' >&2; exit 1; }
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == ubuntu && ${VERSION_ID:-} == 24.04 ]] || { echo 'Requires Ubuntu 24.04.' >&2; exit 1; }
for tool in useradd runuser git curl sha256sum install; do
  command -v "$tool" >/dev/null 2>&1 || { printf 'Required build host tool missing: %s\n' "$tool" >&2; exit 1; }
done
for tool in cc pkg-config; do
  command -v "$tool" >/dev/null 2>&1 || { printf 'Required compiler tool missing: %s\n' "$tool" >&2; exit 1; }
done

install -d -o root -g root -m 0755 "$base"
if getent passwd "$build_user" >/dev/null; then
  [[ $(getent passwd "$build_user" | cut -d: -f6) == "$home" ]] || {
    echo 'Existing cordel-build account has an unexpected home; refusing to modify it.' >&2; exit 1;
  }
else
  useradd --system --home-dir "$home" --create-home --shell /usr/sbin/nologin "$build_user"
fi
for directory in "$home" "$workroot"; do
  if [[ -e $directory ]]; then
    [[ -d $directory && $(stat -c '%U:%G' "$directory") == "$build_user:$build_user" ]] || {
      printf 'Existing build directory has unexpected type or ownership: %s\n' "$directory" >&2; exit 1;
    }
  else
    install -d -o "$build_user" -g "$build_user" -m 0700 "$directory"
  fi
done

if [[ -e $artifact_dir ]]; then
  [[ ! -L $artifact_dir && ! -L $artifact && ! -L "$artifact_dir/manifest.txt" && -x $artifact && -f $artifact && -f "$artifact_dir/manifest.txt" ]] || {
    echo 'Revision artifact directory exists but is incomplete; refusing to overwrite it.' >&2; exit 1;
  }
  recorded_revision=$(awk -F= '$1 == "revision" {print $2}' "$artifact_dir/manifest.txt")
  recorded_rust=$(awk -F= '$1 == "rustc" {print $2}' "$artifact_dir/manifest.txt")
  recorded_sha=$(awk -F= '$1 == "sha256" {print $2}' "$artifact_dir/manifest.txt")
  actual_sha=$(sha256sum "$artifact" | awk '{print $1}')
  [[ $recorded_revision == "$revision" && $recorded_rust == "$rust_version" && $recorded_sha == "$actual_sha" ]] || {
    echo 'Existing revision artifact failed manifest verification; refusing to overwrite it.' >&2; exit 1;
  }
  printf 'revision=%s\nrustc=%s\nartifact=%s\nsha256=%s\nreused=yes\n' "$revision" "$rust_version" "$artifact" "$actual_sha"
  exit 0
fi

work=$(mktemp -d "$workroot/run.XXXXXXXX")
chown "$build_user:$build_user" "$work"
cleanup() { rm -rf -- "$work"; }
trap cleanup EXIT

set +e
runuser -u "$build_user" -- env -i \
  PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin LANG=C.UTF-8 \
  HOME="$home" CARGO_HOME="$home/.cargo" RUSTUP_HOME="$home/.rustup" \
  WORK="$work" REVISION="$revision" REPOSITORY="$repository" RUST_VERSION="$rust_version" \
  bash -s >"$work/build.log" 2>&1 <<'BUILD'
set -euo pipefail
cd "$WORK"
curl --fail --location --silent --show-error https://sh.rustup.rs -o rustup-init.sh
sh rustup-init.sh -y --no-modify-path --default-toolchain none --profile minimal
"$HOME/.cargo/bin/rustup" toolchain install "$RUST_VERSION" --profile minimal --no-self-update
git clone --filter=blob:none --no-checkout "$REPOSITORY" source
git -C source checkout --quiet --detach "$REVISION"
checked_out=$(git -C source rev-parse HEAD)
[[ $checked_out == "$REVISION" ]]
rustc_version=$("$HOME/.cargo/bin/rustc" "+$RUST_VERSION" --version | awk '{print $2}')
[[ $rustc_version == "$RUST_VERSION" ]]
"$HOME/.cargo/bin/cargo" "+$RUST_VERSION" build --locked --release --manifest-path source/server/identity/Cargo.toml
install -m 0755 source/server/identity/target/release/cordel-identity "$WORK/cordel-identity"
printf '%s\n' "$rustc_version" > "$WORK/rustc-version"
BUILD
result=$?
set -e
if [[ $result -ne 0 ]]; then
  printf 'Build failed (exit=%s). No artifact was installed.\n' "$result" >&2
  printf 'Last 40 lines of the public-source build log:\n' >&2
  tail -n 40 "$work/build.log" >&2
  exit "$result"
fi

[[ -x "$work/cordel-identity" ]] || { echo 'Expected executable missing; refusing artifact publication.' >&2; exit 1; }
rustc_version=$(cat "$work/rustc-version")
[[ $rustc_version == "$rust_version" ]] || { echo 'Unexpected compiler version; refusing artifact publication.' >&2; exit 1; }
sha=$(sha256sum "$work/cordel-identity" | awk '{print $1}')
install -d -o root -g root -m 0755 "$artifact_dir"
install -o root -g root -m 0755 "$work/cordel-identity" "$artifact"
printf 'revision=%s\nrustc=%s\nsha256=%s\n' "$revision" "$rustc_version" "$sha" > "$work/manifest.txt"
install -o root -g root -m 0644 "$work/manifest.txt" "$artifact_dir/manifest.txt"
verified_sha=$(sha256sum "$artifact" | awk '{print $1}')
[[ $verified_sha == "$sha" ]] || { echo 'Installed artifact checksum mismatch.' >&2; exit 1; }

printf 'revision=%s\nrustc=%s\nartifact=%s\nsha256=%s\nreused=no\n' "$revision" "$rustc_version" "$artifact" "$verified_sha"
