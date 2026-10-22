#!/bin/sh
# Prints the build environment for a release tag: `alpha` for vX.Y.Z-alpha.N,
# `beta` for vX.Y.Z-beta.N, and nothing for vX.Y.Z. Any other tag fails, so
# a release can't ship with the wrong label.
set -eu

. "$(dirname "$0")/../../scripts/version.sh"

tag=${1:?usage: build-env.sh <tag>}

case $tag in
  v*) version_env "${tag#v}" && exit 0 ;;
esac
echo "Unsupported tag $tag: use vX.Y.Z, vX.Y.Z-alpha.N or vX.Y.Z-beta.N" >&2
exit 1
