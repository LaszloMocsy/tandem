#!/bin/sh
# Prints the build environment for a release tag: `alpha` for vX.Y.Z-alpha.N,
# `beta` for vX.Y.Z-beta.N, and nothing for vX.Y.Z. Any other tag fails, so
# a release can't ship with the wrong label.
set -eu

tag=${1:?usage: build-env.sh <tag>}
version='v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
number='(0|[1-9][0-9]*)'

fail() {
  echo "Unsupported tag $tag: use vX.Y.Z, vX.Y.Z-alpha.N or vX.Y.Z-beta.N" >&2
  exit 1
}

# grep matches line by line, so refuse anything that could hold a newline.
case $tag in
  *[!A-Za-z0-9.-]*) fail ;;
esac

if printf '%s\n' "$tag" | grep -Eqx "$version"; then
  echo ''
elif printf '%s\n' "$tag" | grep -Eqx "$version-alpha\.$number"; then
  echo alpha
elif printf '%s\n' "$tag" | grep -Eqx "$version-beta\.$number"; then
  echo beta
else
  fail
fi
