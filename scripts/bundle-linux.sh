#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target_dir="${CARGO_TARGET_DIR:-target}"
version="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"name":"waku","version":"\([^"]*\)".*/\1/p')"
target_triple="$(rustc -vV | sed -n 's/^host: //p')"
# The top-level directory inside the tarball keeps the goddard-<v>-<triple> name:
# already-installed updaters validate that exact root before swapping. Only the
# published archive filename carries the product name.
package="goddard-${version}-${target_triple}"
archive="$target_dir/release/Goddard-${version}-${target_triple}.tar.gz"
staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT

cargo build --locked --release \
  --package waku --bin goddard --bin goddard-updater --bin goddard_js_repl \
  --package waku-daemon --bin goddard-daemon \
  --package waku-computer-use --bin goddard_computer_use

package_dir="$staging/$package"
bun scripts/cua-driver.ts bundle "$package_dir/bin" "$package_dir/share/goddard" release
install -Dm755 "$target_dir/release/goddard" "$package_dir/bin/goddard"
install -Dm755 "$target_dir/release/goddard-updater" "$package_dir/bin/goddard-updater"
install -Dm755 "$target_dir/release/goddard-daemon" "$package_dir/bin/goddard-daemon"
install -Dm644 resources/linux/org.goddardai.app.desktop \
  "$package_dir/share/applications/org.goddardai.app.desktop"
install -Dm644 resources/linux/self-update-v1 \
  "$package_dir/share/goddard/self-update-v1"
install -Dm644 website/public/app-icon.png \
  "$package_dir/share/icons/hicolor/256x256/apps/org.goddardai.app.png"
install -Dm644 LICENSE "$package_dir/share/licenses/goddard/LICENSE"

mkdir -p "$(dirname "$archive")"
tar -C "$staging" -czf "$archive" "$package"
printf 'Created %s\n' "$archive"
