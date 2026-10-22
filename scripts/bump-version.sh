#!/bin/sh
# Sets the app version everywhere it lives: [workspace.package] in Cargo.toml,
# desktop/tauri.conf.json and the workspace crates in Cargo.lock. Run it on a
# new version/vX.Y.Z branch:
#
#   sh scripts/bump-version.sh 0.2.0          # or 0.2.0-alpha.1, 0.2.0-beta.1
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/version.sh"

cargo_toml=$root/Cargo.toml
tauri_conf=$root/desktop/tauri.conf.json
cargo_lock=$root/Cargo.lock

fail() {
  echo "bump-version: $*" >&2
  exit 1
}

[ $# -eq 1 ] || fail 'usage: bump-version.sh <X.Y.Z | X.Y.Z-alpha.N | X.Y.Z-beta.N>'
new=${1#v}
version_env "$new" >/dev/null ||
  fail "unsupported version $1: use X.Y.Z, X.Y.Z-alpha.N or X.Y.Z-beta.N"

current=$(version_from_cargo "$cargo_toml")
[ -n "$current" ] || fail 'no [workspace.package] version in Cargo.toml'
[ "$(version_from_tauri "$tauri_conf")" = "$current" ] ||
  fail "tauri.conf.json and Cargo.toml disagree; fix them by hand first"
version_gt "$new" "$current" || fail "$new is not higher than the current $current"

# A clean start keeps the bump's diff to the version lines.
if ! git -C "$root" diff --quiet HEAD -- Cargo.toml Cargo.lock desktop/tauri.conf.json; then
  fail 'Cargo.toml, Cargo.lock or tauri.conf.json has uncommitted changes'
fi

# Rewrites file $1 with sed script $2 through a temp file, and fails unless
# exactly one line changed.
edit() {
  tmp="$1.bump.$$"
  sed "$2" "$1" >"$tmp"
  changed=$(diff "$1" "$tmp" | grep -c '^>' || true)
  if [ "$changed" -ne 1 ]; then
    rm -f "$tmp"
    fail "expected to change one line in $1, found $changed"
  fi
  mv "$tmp" "$1"
}

old=$(printf '%s' "$current" | sed 's/\./\\./g')
edit "$cargo_toml" "/^\[workspace\.package\]/,/^\[/s/^version = \"$old\"$/version = \"$new\"/"
edit "$tauri_conf" "s/^  \"version\": \"$old\",$/  \"version\": \"$new\",/"

# Rewrites only the workspace crates' entries; dependencies stay as they are.
cargo update --workspace --offline --quiet --manifest-path "$cargo_toml" ||
  fail 'cargo update --workspace failed; Cargo.toml and tauri.conf.json are already bumped'

branch=$(git -C "$root" branch --show-current)
echo "Bumped $current -> $new in Cargo.toml, desktop/tauri.conf.json and Cargo.lock."
if [ "$branch" != "version/v$new" ]; then
  echo "Note: you're on '$branch', not 'version/v$new'."
fi
echo
echo 'Next:'
echo "  1. Commit the bump on version/v$new."
echo "  2. Merge feature branches into version/v$new."
echo "  3. Open a pull request from version/v$new into main."
echo "  4. After it merges, tag the merge commit v$new to start the release."
