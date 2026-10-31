#!/usr/bin/env bash
# Install a verified local package. No downloads, elevated privileges, or shell-profile edits.
set -euo pipefail
fail() { printf '%s\n' "Error: $*" >&2; exit 1; }
if [ "$#" -ne 2 ] || [ "$1" != --prefix ]; then
  printf '%s\n' 'Usage: bash install.sh --prefix /absolute/user/prefix' >&2
  exit 2
fi
prefix=$2
case "$prefix" in /*) ;; *) fail 'The installation prefix must be absolute.' ;; esac
case "$prefix" in /|*//*|*/../*|*/..|*/./*|*/.) fail 'Use a specific normalized installation prefix.' ;; esac
prefix=${prefix%/}
root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
for item in VERSION TARGET REVISION SHA256SUMS bin/pm-agent; do
  [ -f "$root/$item" ] && [ ! -L "$root/$item" ] || fail "Missing regular package file: $item"
done
if [ -n "$(find "$root" ! -type f ! -type d -print -quit)" ]; then fail 'Package links and special files are not supported.'; fi
version=$(cat "$root/VERSION")
target=$(cat "$root/TARGET")
revision=$(cat "$root/REVISION")
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]] || fail 'Invalid package version.'
[[ "$revision" =~ ^[a-f0-9]{40}$ ]] || fail 'Invalid framework revision.'
case "$(uname -s):$(uname -m)" in
  Darwin:arm64) host=aarch64-apple-darwin ;;
  Linux:x86_64) host=x86_64-unknown-linux-gnu ;;
  *) fail 'This package supports macOS Apple Silicon or Linux x86_64 only.' ;;
esac
[ "$target" = "$host" ] || fail "Package target $target does not match $host."
verify() {
  [ -f "$1/SHA256SUMS" ] && [ ! -L "$1/SHA256SUMS" ] || return 1
  [ -z "$(find "$1" ! -type f ! -type d -print -quit)" ] || return 1
  local line path
  while IFS= read -r line; do
    [[ "$line" =~ ^[a-f0-9]{64}\ \ [a-zA-Z0-9._/-]+$ ]] || return 1
    path=${line:66}
    case "$path" in bin/pm-agent|install.sh|VERSION|TARGET|REVISION|package.json|framework/*) ;; *) return 1 ;; esac
    case "/$path/" in *//*|*/../*|*/./*) return 1 ;; esac
    [ -f "$1/$path" ] || return 1
  done < "$1/SHA256SUMS"
  # Require complete, unique coverage, including the binary. Truncated manifests fail.
  cmp -s <(sed 's/^[a-f0-9]*  /\.\//' "$1/SHA256SUMS" | LC_ALL=C sort) \
    <(cd -- "$1" && find . -type f ! -path ./SHA256SUMS | LC_ALL=C sort) || return 1
  if command -v sha256sum >/dev/null 2>&1; then
    (cd -- "$1" && sha256sum -c SHA256SUMS >/dev/null)
  elif command -v shasum >/dev/null 2>&1; then
    (cd -- "$1" && shasum -a 256 -c SHA256SUMS >/dev/null)
  else
    fail 'Install sha256sum or shasum before installing this package.'
  fi
}
verify "$root" || fail 'Package checksum validation failed.'
id="$version-$revision-$target"
# Reject linked path components before creating or modifying the installation.
part=$prefix
while [ "$part" != / ]; do
  [ ! -L "$part" ] || fail "Installation path contains a symlink: $part"
  part=$(dirname -- "$part")
done
for part in "$prefix/bin" "$prefix/lib" "$prefix/lib/pm-agent"; do
  [ ! -L "$part" ] || fail "Installation path contains a symlink: $part"
done
umask 022
mkdir -p -- "$prefix/bin" "$prefix/lib/pm-agent"
base="$prefix/lib/pm-agent"
lock="$base/.install-lock"
mkdir -- "$lock" 2>/dev/null || fail 'Another installation is active; inspect a leftover lock before removing it.'
stage=
link_stage=
cleanup() {
  [ -z "$stage" ] || rm -rf -- "$stage"
  [ -z "$link_stage" ] || rm -rf -- "$link_stage"
  rmdir -- "$lock"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP
active="$prefix/bin/pm-agent"
if [ -L "$active" ]; then
  current=$(readlink -- "$active")
  [[ "$current" =~ ^\.\./lib/pm-agent/[a-zA-Z0-9._-]+/bin/pm-agent$ ]] || fail 'Existing command is not managed by this installer.'
  [ -f "$active" ] || fail 'Existing managed command is missing; repair it explicitly before installing.'
elif [ -e "$active" ]; then
  fail 'An unmanaged pm-agent already exists; choose another prefix.'
fi
destination="$base/$id"
if [ -e "$destination" ] || [ -L "$destination" ]; then
  [ -d "$destination" ] && [ ! -L "$destination" ] || fail 'Release destination is not a regular directory.'
  cmp -s "$root/SHA256SUMS" "$destination/SHA256SUMS" || fail 'Same release ID has different content; no files were replaced.'
  verify "$destination" || fail 'Existing release was modified; no files were replaced.'
else
  stage=$(mktemp -d "$base/.stage.XXXXXX")
  for item in bin framework install.sh VERSION TARGET REVISION SHA256SUMS package.json; do
    cp -R -- "$root/$item" "$stage/$item"
  done
  verify "$stage" || fail 'Staged installation checksum validation failed.'
  chmod 755 "$stage" "$stage/bin/pm-agent"
  mv -- "$stage" "$destination"
  stage=
fi
# A same-directory rename activates the verified release; older release directories remain.
link_stage=$(mktemp -d "$prefix/bin/.pm-agent-link.XXXXXX")
ln -s "../lib/pm-agent/$id/bin/pm-agent" "$link_stage/pm-agent"
mv -f -- "$link_stage/pm-agent" "$active"
printf 'Installed: %s\nRelease: %s\nStarter: %s/framework\n' "$active" "$destination" "$destination"
printf '%s\n' 'Add the prefix/bin directory to PATH yourself if needed. Existing Vaults and provider settings were not changed.'
