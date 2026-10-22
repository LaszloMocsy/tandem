#!/bin/sh
# Tests build-env.sh. Run from anywhere: sh .github/scripts/build-env.test.sh
set -u

script="$(dirname "$0")/build-env.sh"
failed=0

expect() {
  tag=$1
  want=$2
  if got=$(sh "$script" "$tag" 2>/dev/null); then
    if [ "$got" != "$want" ]; then
      echo "FAIL $tag: got '$got', want '$want'"
      failed=1
    fi
  else
    echo "FAIL $tag: rejected, want '$want'"
    failed=1
  fi
}

reject() {
  if got=$(sh "$script" "$1" 2>/dev/null); then
    echo "FAIL $1: accepted as '$got', want rejected"
    failed=1
  fi
}

expect v0.2.0 ''
expect v1.10.3 ''
expect v0.2.0-alpha.1 alpha
expect v0.2.0-alpha.12 alpha
expect v0.2.0-beta.0 beta

reject v0.2.0-rc.1
reject v0.2.0-alpha
reject v0.2.0-alpha.01
reject v0.2.0-beta.1.1
reject v0.2.0-ALPHA.1
reject v0.2
reject 0.2.0
reject v01.2.0
reject 'v0.2.0
v0.2.0'
reject ''

if [ "$failed" -eq 0 ]; then echo 'build-env.sh: all tests passed'; fi
exit "$failed"
