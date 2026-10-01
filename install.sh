#!/bin/sh
# Install an official native release, or an explicitly verified offline archive.
set -eu

fail() { printf '%s\n' "ModelPrepper installer: $*" >&2; exit 1; }
version=latest
install_dir="${HOME:?HOME is required}/.local/bin"
archive=
checksum=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version|--install-dir|--archive|--sha256)
            [ "$#" -ge 2 ] || fail "$1 requires a value"
            case "$1" in
                --version) version=$2 ;; --install-dir) install_dir=$2 ;;
                --archive) archive=$2 ;; --sha256) checksum=$2 ;;
            esac
            shift 2 ;;
        --help)
            printf '%s\n' 'Usage: sh install.sh [--version v0.1.0] [--install-dir DIR]' \
                'Offline: sh install.sh --version v0.1.0 --archive FILE --sha256 HEX' \
                'Installs a native binary only. No vault, models, or schedule are created.'
            exit 0 ;;
        *) fail "unknown option: $1" ;;
    esac
done
[ -n "$install_dir" ] || fail 'install directory cannot be empty'
case "$(uname -s):$(uname -m)" in
    Linux:x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-gnu ;;
    Darwin:x86_64) target=x86_64-apple-darwin ;;
    Darwin:arm64) target=aarch64-apple-darwin ;;
    *) fail 'supported platforms: Linux x64/ARM64 and macOS Intel/Apple Silicon' ;;
esac
if command -v sha256sum >/dev/null 2>&1; then
    hash_file() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
    hash_file() { shasum -a 256 "$1" | awk '{print $1}'; }
else
    fail 'sha256sum or shasum is required'
fi
work=$(mktemp -d "${TMPDIR:-/tmp}/modelprepper-install.XXXXXX")
candidate=
cleanup() { [ -z "$candidate" ] || rm -f "$candidate"; rm -rf "$work"; }
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
            --connect-timeout 10 --max-time 120 --retry 2 --output "$2" "$1"
    else
        fail 'curl is required for online installation; offline installation needs no network'
    fi
}
if [ -n "$archive" ]; then
    [ "$version" != latest ] || fail 'offline installation requires --version'
    [ -n "$checksum" ] || fail 'offline installation requires --sha256 from trusted release checksums'
else
    [ -z "$checksum" ] || fail '--sha256 is only supported with --archive'
    if [ "$version" = latest ]; then
        fetch https://api.github.com/repos/blisspixel/ModelPrepper/releases/latest "$work/release.json" || \
            fail 'no stable release could be resolved; build from source or specify a published version'
        version=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$work/release.json")
    fi
fi
printf '%s\n' "$version" | LC_ALL=C grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$' || fail 'version must be a release tag such as v0.1.0'
asset="modelprepper-$version-$target.tar.gz"
if [ -z "$archive" ]; then
    base="https://github.com/blisspixel/ModelPrepper/releases/download/$version"
    fetch "$base/SHA256SUMS" "$work/SHA256SUMS" || fail 'release checksums are unavailable'
    checksum=$(awk -v name="$asset" '$2==name { if(length($1)!=64 || $1 ~ /[^0-9a-f]/) exit 2; count++; hash=$1 } END { if(count!=1) exit 1; print hash }' "$work/SHA256SUMS") || fail 'release checksum entry is missing, invalid, or duplicated'
    archive="$work/$asset"
    fetch "$base/$asset" "$archive" || fail 'native release archive is unavailable for this platform'
fi
printf '%s\n' "$checksum" | LC_ALL=C grep -Eq '^[0-9a-f]{64}$' || fail 'SHA-256 must be 64 lowercase hexadecimal characters'
[ -f "$archive" ] || fail 'archive does not exist'
[ "$(hash_file "$archive")" = "$checksum" ] || fail 'archive checksum mismatch; existing installation was retained'
expected_entries=$(printf '%s\n' LICENSE modelprepper)
# Preserve tar's exit status instead of hiding corrupt archives behind a pipe.
tar -tzf "$archive" > "$work/entries" || fail 'archive listing failed'
[ "$(LC_ALL=C sort "$work/entries")" = "$expected_entries" ] || fail 'archive must contain exactly modelprepper and LICENSE'
tar -tvzf "$archive" > "$work/details" || fail 'archive inspection failed'
[ "$(cut -c1 "$work/details")" = "$(printf '%s\n' '-' '-')" ] || fail 'archive entries must be regular files'
# Cap each output before it reaches disk. Do not parse platform-specific tar
# listing columns or trust an archive's claimed decompressed size.
for entry in modelprepper LICENSE; do
    (
        if tar -xOzf "$archive" "$entry"; then
            printf 'ok' > "$work/extract-status"
        else
            printf 'failed' > "$work/extract-status"
        fi
    ) | head -c 134217729 > "$work/$entry"
    [ "$(wc -c < "$work/$entry")" -le 134217728 ] || fail 'archive entry exceeds the 128 MiB installer limit'
    [ "$(cat "$work/extract-status")" = ok ] || fail 'archive extraction failed'
done
[ -f "$work/modelprepper" ] && [ ! -L "$work/modelprepper" ] || fail 'invalid executable entry'
chmod 755 "$work/modelprepper"
[ "$("$work/modelprepper" --version)" = "modelprepper ${version#v}" ] || fail 'binary version does not match release tag'
case "$install_dir" in /*) ;; *) install_dir="$(pwd)/$install_dir" ;; esac
case "/$install_dir/" in */../*|*/./*) fail 'install directory cannot contain dot segments' ;; esac
walk=$install_dir
while [ "$walk" != / ]; do
    [ ! -L "$walk" ] || fail 'install directory cannot have symlink ancestors'
    walk=$(dirname "$walk")
done
[ ! -L "$install_dir/modelprepper" ] || fail 'existing executable cannot be a symlink'
[ ! -L "$install_dir/modelprepper.LICENSE" ] || fail 'existing license cannot be a symlink'
mkdir -p "$install_dir"
cp "$work/LICENSE" "$install_dir/modelprepper.LICENSE"
candidate=$(mktemp "$install_dir/.modelprepper.XXXXXX")
cp "$work/modelprepper" "$candidate"
chmod 755 "$candidate"
mv -f "$candidate" "$install_dir/modelprepper"
candidate=
printf '%s\n' "Installed ModelPrepper ${version#v} at $install_dir/modelprepper"
case ":${PATH:-}:" in *":$install_dir:"*) ;; *) printf '%s\n' "Add $install_dir to PATH, or run the executable by its full path." ;; esac
printf '%s\n' 'Next: modelprepper config init --output config.local.toml --catalog catalog --volume models'
