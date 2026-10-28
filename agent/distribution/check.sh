#!/usr/bin/env bash
# Exercise the packaged executable and starter without a compiler, model, or configured Vault.
set -euo pipefail
[ "$#" -eq 1 ] || { printf '%s\n' 'Usage: bash check.sh /absolute/package.zip' >&2; exit 2; }
archive=$1
case "$archive" in /*) ;; *) printf '%s\n' 'Archive path must be absolute.' >&2; exit 2 ;; esac
parent=$(dirname -- "$archive")
name=$(basename -- "$archive")
if command -v sha256sum >/dev/null 2>&1; then
  (cd -- "$parent" && sha256sum -c "$name.sha256")
else
  (cd -- "$parent" && shasum -a 256 -c "$name.sha256")
fi
scratch=$(mktemp -d)
scratch=$(CDPATH= cd -- "$scratch" && pwd -P)
trap 'rm -rf -- "$scratch"' EXIT
unzip -q "$archive" -d "$scratch/unpacked"
package="$scratch/unpacked/${name%.zip}"
prefix="$scratch/install with spaces"
bash "$package/install.sh" --prefix "$prefix"
bash "$package/install.sh" --prefix "$prefix"
binary="$prefix/bin/pm-agent"
test "$("$binary" --version)" = "pm-agent $(cat "$package/VERSION")"
cp -R "$package/framework" "$scratch/vault"
test ! -e "$scratch/vault/.pm-agent"
"$binary" --vault "$scratch/vault" storage inspect
test ! -e "$scratch/vault/.pm-agent"
"$binary" --vault "$scratch/vault" pm validate
"$binary" --vault "$scratch/vault" skills
"$binary" --vault "$scratch/vault" sessions
"$binary" --vault "$scratch/vault" storage inspect
"$binary" --vault "$scratch/vault" backup create --output "$scratch/recovery.pmbak" --include-sessions
"$binary" backup check "$scratch/recovery.pmbak"
mkdir "$scratch/restored"
"$binary" backup restore "$scratch/recovery.pmbak" --parent "$scratch/restored" --include-sessions
test ! -e "$scratch/vault/.pm-agent/config.toml"
printf '%s\n' 'Package checks passed: checksum, install/reinstall, native CLI, clean starter, Skills, storage, backup and restore.'
