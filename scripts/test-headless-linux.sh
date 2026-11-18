#!/usr/bin/env bash
# Exercise the headless handoff with a few synthetic bytes. No host vault is used.
set -euo pipefail

if (( $# < 1 || $# > 2 )); then
    echo "usage: $0 MODEL_PREPPER_BINARY [ARIA2C_BINARY]" >&2
    exit 2
fi
binary=$(realpath "$1")
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

if (( $# == 2 )); then
    [[ -f "$2" && -x "$2" ]] || { echo "pass the path to an executable aria2c" >&2; exit 2; }
    client=$(realpath "$2")
    fixture=$(realpath "$(dirname "$0")/../tests/fixtures/torrent-webseed.py")
    "${isolated[@]}" --ro-bind "$client" /aria2c --ro-bind "$fixture" /fixture.py \
        --ro-bind-try /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt \
        --bind "$scratch/incoming" /incoming /usr/bin/python3 -I /fixture.py
else
    "${isolated[@]}" --bind "$scratch/incoming" /incoming /bin/sh -eu -c '
        test ! -e /primary
        test ! -e /state
        test ! -e /backup
        echo "synthetic intake" > /incoming/weights.txt
    '
fi

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
    /modelprepper catalog-backup --config config.toml --output catalog-backup.sqlite3 --json > snapshot.json
    test "$(jq -r ".sha256" snapshot.json)" = "$(sha256sum catalog-backup.sqlite3 | cut -d" " -f1)"
    test "$(sqlite3 -readonly catalog-backup.sqlite3 "PRAGMA integrity_check;")" = ok
'

revision=0000000000000000000000000000000000000001
"${isolated[@]}" --ro-bind "$scratch/primary/preserved/local/fixture/$revision" /model \
    /bin/sh -eu -c '
    test "$(cat /model/weights.txt)" = "synthetic intake"
    test ! -e /state
    test ! -e /incoming
    test ! -e /backup
    if touch /model/consumer-probe 2>/dev/null; then exit 1; fi
'

# This 4 MiB tmpfs is isolated from the developer's disk. A failed competing
# write fills it; the import must refuse the new payload and be safely retryable.
"${isolated[@]}" --ro-bind "$binary" /modelprepper --ro-bind "$scratch/incoming" /incoming \
    --size 4194304 --tmpfs /bounded /bin/sh -eu -c '
    cd /tmp
    /modelprepper config init --output tight.toml --catalog /tmp/catalog --volume /bounded/vault
    sed -i "s/^min_free_bytes = .*/min_free_bytes = 1048576/" tight.toml
    /modelprepper init --config tight.toml >/dev/null
    if dd if=/dev/zero of=/bounded/fill bs=65536 count=80 2>/dev/null; then exit 1; fi
    test "$(df -B1 --output=avail /bounded | tail -1 | tr -d " ")" = 0
    set -- /modelprepper import --config tight.toml --path /incoming --repo local/fixture \
        --revision 0000000000000000000000000000000000000001 --volume primary --json
    if "$@" >/dev/null 2>failure.json; then exit 1; fi
    jq -e ".error.code == \"insufficient_space\"" failure.json
    test ! -e /bounded/vault/preserved/local/fixture
    test "$(cat /incoming/weights.txt)" = "synthetic intake"
    rm /bounded/fill
    "$@" > retried.json
    jq -e ".provenance == \"local-baseline\"" retried.json
    # Recreate an interrupted two-file copy. One matching MiB is already staged.
    mkdir /tmp/pair
    dd if=/dev/zero of=/tmp/pair/first.bin bs=1048576 count=1 status=none
    cp /tmp/pair/first.bin /tmp/pair/second.bin
    /modelprepper config init --output reference.toml --catalog /tmp/reference-catalog --volume /tmp/reference
    sed -i "s/^min_free_bytes = .*/min_free_bytes = 262144/" reference.toml tight.toml
    /modelprepper init --config reference.toml >/dev/null
    set -- --path /tmp/pair --repo local/pair --revision 0000000000000000000000000000000000000002 --volume primary
    /modelprepper import --config reference.toml "$@" --json > pair.json
    id=$(jq -er ".plan_id" pair.json)
    staging=/bounded/vault/.modelprepper-adopt/$id
    mkdir -p "$staging"
    cp /tmp/pair/first.bin "$staging/first.bin"
    dd if=/dev/zero of=/bounded/fill bs=65536 count=24 status=none
    available=$(df -B1 --output=avail /bounded | tail -1)
    test "$available" -ge 1310720
    test "$available" -lt 2359296
    /modelprepper import --config tight.toml "$@" --json > resumed.json
    jq -e ".file_count == 2" resumed.json
    /modelprepper verify-sealed --config tight.toml --plan "$id" --json > checked.json
    jq -e ".matched" checked.json
'

# The original catalog, incoming tree and primary volume are absent here.
plan=$(jq -er '.plan_id' "$scratch/system/receipt.json")
"${isolated[@]}" --ro-bind "$binary" /modelprepper --bind "$scratch/recovery" /state \
    --ro-bind "$scratch/backup" /backup /bin/sh -eu -c '
    test ! -e /primary
    test ! -e /incoming
    cd /state
    /modelprepper config init --output config.toml --catalog /state/catalog --volume /backup
    sed -i "s/\"primary\"/\"backup\"/g" config.toml
    /modelprepper rebuild --config config.toml > rebuilt.json
    jq -e ".custody_count == 1 and .replica_count == 0 and .conflicts == [] and .damaged == [] and .approvals_transferred == false" rebuilt.json
    /modelprepper verify-sealed --config config.toml --plan "$1" > verified.json
    jq -e ".matched == true and .network_contact == false" verified.json
    test "$(sqlite3 -readonly catalog/catalog.sqlite3 "SELECT count(*) FROM grants;")" = 0
' sh "$plan"
echo "Offline baseline handoff, read-only consumer, catalog backup, full-filesystem retry and backup-only rebuild passed."
