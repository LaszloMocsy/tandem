# Version helpers shared by the release scripts. Source it, don't run it:
#   . scripts/version.sh
#
# Versions are X.Y.Z, X.Y.Z-alpha.N or X.Y.Z-beta.N, with no leading zeros.
# Tags are the same with a leading v. Helper variables start with _v_, since
# POSIX sh has no local variables.

VERSION_CORE='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
VERSION_NUMBER='(0|[1-9][0-9]*)'

# Prints the build environment for a version: alpha, beta, or nothing for a
# production version. Fails on any other version.
version_env() {
  # grep matches line by line, so refuse anything that could hold a newline.
  case $1 in
    '' | *[!A-Za-z0-9.-]*) return 1 ;;
  esac
  if printf '%s\n' "$1" | grep -Eqx "$VERSION_CORE"; then
    echo ''
  elif printf '%s\n' "$1" | grep -Eqx "$VERSION_CORE-alpha\.$VERSION_NUMBER"; then
    echo alpha
  elif printf '%s\n' "$1" | grep -Eqx "$VERSION_CORE-beta\.$VERSION_NUMBER"; then
    echo beta
  else
    return 1
  fi
}

# Prints "X Y Z rank N" for a valid version, where rank orders the channels:
# alpha 0, beta 1, production 2 (N is 0 for production).
version_fields() {
  _v_env=$(version_env "$1") || return 1
  case $_v_env in
    alpha) _v_rank=0 ;;
    beta) _v_rank=1 ;;
    *) _v_rank=2 ;;
  esac
  _v_n=0
  [ -n "$_v_env" ] && _v_n=${1##*.}
  echo "${1%%-*}" | awk -F. -v rank="$_v_rank" -v n="$_v_n" '{ print $1, $2, $3, rank, n }'
}

# Succeeds if version $1 is higher than version $2, by SemVer precedence:
# 0.2.0-alpha.1 < 0.2.0-alpha.2 < 0.2.0-beta.1 < 0.2.0 < 0.2.1.
version_gt() {
  _v_a=$(version_fields "$1") || return 2
  _v_b=$(version_fields "$2") || return 2
  printf '%s\n%s\n' "$_v_a" "$_v_b" | awk '
    NR == 1 { for (i = 1; i <= 5; i++) a[i] = $i }
    NR == 2 {
      for (i = 1; i <= 5; i++) {
        if (a[i] + 0 > $i + 0) exit 0
        if (a[i] + 0 < $i + 0) exit 1
      }
      exit 1
    }'
}

# Prints [workspace.package] version from the Cargo.toml at $1.
version_from_cargo() {
  sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$1"
}

# Prints the top-level version from the tauri.conf.json at $1.
version_from_tauri() {
  sed -n 's/^  "version": "\(.*\)",$/\1/p' "$1"
}
