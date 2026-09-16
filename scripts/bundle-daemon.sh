#!/usr/bin/env bash

# Packages goddard-daemon alone for SSH remotes: the desktop fetches
# goddard-daemon-<version>-<target-triple>.tar.gz from the release bucket
# when a remote host needs the daemon installed or upgraded. Runs on the
# platform runners the Release workflow already uses, so the triple always
# matches the machine that built it.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target_dir="${CARGO_TARGET_DIR:-target}"
version="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"name":"waku","version":"\([^"]*\)".*/\1/p')"
target_triple="$(rustc -vV | sed -n 's/^host: //p')"

cargo build --locked --release --package waku-daemon --bin goddard-daemon

binary="$target_dir/release/goddard-daemon${EXE_SUFFIX:-}"
archive="goddard-daemon-${version}-${target_triple}.tar.gz"
staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT

mkdir -p "$staging/bin"
install -m 0755 "$binary" "$staging/bin/goddard-daemon"
tar -C "$staging" -czf "$target_dir/release/$archive" bin
(cd "$target_dir/release" && shasum -a 256 "$archive" >"$archive.sha256")
printf 'Created %s\n' "$target_dir/release/$archive"
