#!/usr/bin/env bash
# Exercise the headless handoff with a few synthetic bytes. No host vault is used.
set -euo pipefail

binary=$(realpath "${1:?pass the release modelprepper executable}")
[[ -x "$binary" ]]
for tool in bwrap jq sqlite3; do command -v "$tool" >/dev/null; done
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
mkdir "$scratch"/{system,incoming,primary,backup,recovery}

# The only writable host paths passed below are children of the fresh tempdir.
isolated=(bwrap --unshare-all --die-with-parent --cap-drop ALL --clearenv
    --ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib
    --ro-bind-try /lib64 /lib64 --proc /proc --dev /dev --tmpfs /tmp
    --setenv PATH /usr/bin:/bin --setenv HOME /tmp)

"${isolated[@]}" --bind "$scratch/incoming" /incoming /bin/sh -eu -c '
    test ! -e /primary && test ! -e /state && test ! -e /backup
    echo "synthetic intake" > /incoming/weights.txt
'

"${isolated[@]}" --ro-bind "$binary" /modelprepper --ro-bind "$scratch/incoming" /incoming \
    --bind "$scratch/system" /state --bind "$scratch/primary" /primary \
    --bind "$scratch/backup" /backup /bin/sh -eu -c '
    cd /state
    /modelprepper config init --output config.toml --catalog /state/catalog --volume /primary
    # A tiny fixture must fit on a test tmpfs as well as a disk-backed tempdir.
    sed -i "s/^min_free_bytes = .*/min_free_bytes = 1048576/" config.toml
    printf "\n[[volumes]]\nlabel = \"backup\"\npath = \"/backup\"\n" >> config.toml
    /modelprepper config check --config config.toml
    /modelprepper init --config config.toml
    rev=0000000000000000000000000000000000000001
    /modelprepper import --config config.toml --path /incoming --repo local/fixture \
        --revision "$rev" --volume primary > receipt.json
    plan=$(jq -er ".plan_id" receipt.json)
    jq -e ".provenance == \"local-baseline\"" receipt.json
    /modelprepper replica --config config.toml --plan "$plan" --volume backup
    sqlite3 -readonly catalog/catalog.sqlite3 ".backup catalog-backup.sqlite3"
    test "$(sqlite3 -readonly catalog-backup.sqlite3 "PRAGMA integrity_check;")" = ok
'

revision=0000000000000000000000000000000000000001
"${isolated[@]}" --ro-bind "$scratch/primary/preserved/local/fixture/$revision" /model \
    /bin/sh -eu -c '
    test "$(cat /model/weights.txt)" = "synthetic intake"
    test ! -e /state && test ! -e /incoming && test ! -e /backup
    if touch /model/consumer-probe 2>/dev/null; then exit 1; fi
'

# The original catalog, incoming tree and primary volume are absent here.
plan=$(jq -er '.plan_id' "$scratch/system/receipt.json")
"${isolated[@]}" --ro-bind "$binary" /modelprepper --bind "$scratch/recovery" /state \
    --ro-bind "$scratch/backup" /backup /bin/sh -eu -c '
    test ! -e /primary && test ! -e /incoming
    cd /state
    /modelprepper config init --output config.toml --catalog /state/catalog --volume /backup
    sed -i "s/\"primary\"/\"backup\"/g" config.toml
    /modelprepper rebuild --config config.toml > rebuilt.json
    jq -e ".custody_count == 1 and .replica_count == 0 and .conflicts == [] and .damaged == [] and .approvals_transferred == false" rebuilt.json
    /modelprepper verify-sealed --config config.toml --plan "$1" > verified.json
    jq -e ".matched == true and .network_contact == false" verified.json
    test "$(sqlite3 -readonly catalog/catalog.sqlite3 "SELECT count(*) FROM grants;")" = 0
' sh "$plan"
echo "Offline baseline handoff, read-only consumer, catalog backup and backup-only rebuild passed."
