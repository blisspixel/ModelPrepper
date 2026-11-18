# A headless Linux vault

Use this profile for an Ubuntu or Arch guest with a small system disk and a separately attached bulk disk. The first usable workflow is supervised intake. The same native CLI works without a torrent client, inference server, GPU or desktop. The [installation guide](installation.md) covers building the experimental executable. Record its Git revision with your configuration backup.

## Place the files

| Example path | Writer | What to keep there |
| --- | --- | --- |
| `/var/lib/modelprepper/config.toml`, `/var/lib/modelprepper/catalog` | Vault operator | Config and local SQLite catalog on the system disk |
| `/var/lib/modelprepper/stage` | Vault operator | Local checkpoint and partial-file workspace if exercising granted HTTPS fetches. Size this filesystem separately. |
| `/srv/model-data/vault` | Vault operator only | Empty directory on the attached data filesystem for the primary volume |
| `/srv/model-data/incoming` | Intake account | External client's partial and completed files. Give the vault operator read access to completed files. |
| Separately mounted backup medium | Backup operator or vault writer during replication | Verified bundles, retained configuration and consistent catalog backup |

Mount the data filesystem by its recorded UUID. The administrator owns the underlying empty mountpoint and leaves it unwritable by either service identity. Create the writable child directories **on the mounted disk**. Before first initialization or starting an external client, check the exact mountpoint and expected filesystem UUID. A directory on the OS filesystem can exist at the same pathname after a mount failure. For example, in a shell with the expected UUID recorded independently:

```sh
test -n "$EXPECTED_DATA_UUID" &&
  test "$(findmnt -rn --mountpoint /srv/model-data -o UUID)" = "$EXPECTED_DATA_UUID" ||
  exit 1
findmnt --mountpoint /srv/model-data -o TARGET,SOURCE,FSTYPE,OPTIONS
df -B1 /var/lib/modelprepper /srv/model-data
```

Run these commands as the unprivileged vault operator after the mount check and directory setup. `config init` creates a new file.

```sh
cfg=/var/lib/modelprepper/config.toml
modelprepper config init --output "$cfg" --catalog /var/lib/modelprepper/catalog --volume /srv/model-data/vault
```

Before running `init`, set the measured storage limits in the config. To manage a second disk, verify its mount UUID as above and add its empty vault directory to the same config. For example:

```toml
[[volumes]]
label = "backup"
path = "/srv/model-backup/vault"
```

```sh
modelprepper config check --config "$cfg"
modelprepper init --config "$cfg"
modelprepper status --config "$cfg" --json
```

Record the vault and volume IDs separately from the filesystem UUID. On reboot, compare both layers of identity before intake. An offline, missing-marker or mismatched volume needs investigation. Keep the client stopped until the expected mount returns. Keep management private. Give the intake process access only to incoming files, and give consumers a read-only mount of the published files. Keep catalog, checkpoints, credentials, writable vault paths, host mounts and management sockets out of both. The CLI may need its writer lock even for inspection. A consumer can read the files directly.

## Budget the copy on both sides of the VM

Record free bytes on the system, data and backup filesystems before intake, at peak staging, after publication, and after any separately authorized cleanup. Use the host storage administrator's measurement of backing-pool free capacity, thin provisioning and snapshot growth alongside the guest's `df`. Two guest disks sharing a physical HDD share its failure domain.

For example, a completed 100 GB intake and a new 100 GB copy need room for both while they coexist. If the intake already occupies the data drive, its bytes are already reflected in current free space. Include any other partials, transfer caches, replicas on the same pool, filesystem overhead, and the operator's free reserve. The nominal 10 TB of a virtual disk alone gives no safe admission limit. `workspace_bytes` remains unknown. Measure copy and verification times on the actual spindle before setting a schedule.

## Hand off completed files

1. For public Hugging Face artifacts supported by the current profile, save the source evidence using `modelprepper propose --config "$cfg" --repo "$repo" --revision "$commit" --json`. Use the full `plan_id` from the result. A changed commit needs its own review.
2. Manual media or an optional external client may supply bytes. PirateFace is the torrent locator discussed in the [source catalog](model-sources.md#pirateface-findings). Review redistribution rights and the client's upload, stop and peer-discovery settings **before joining a swarm**. Peers can see the client IP and pieces may upload during download. Leave peer networking off where rights or upload scope are unresolved. Keep private artifacts, credentials and signed access URLs out of peer intake.
3. Quiesce the completed directory. Keep the exact paths, required shards, selected representation, license and notices together. Run the following against the saved review. A mismatched path, hash, required shard or incomplete source is a reason to stop. Only the matched set is adopted.

```sh
modelprepper verify-local --config "$cfg" --plan "$plan" --path "$completed" &&
  modelprepper adopt --config "$cfg" --plan "$plan" --path "$completed" --copy
```

Keep the source and the receipt. This records `upstream-verified (local source)`. Local adoption keeps its custody manifest and saved review in the catalog. While the catalog and adopted files are available, run `modelprepper replica --config "$cfg" --plan "$plan" --volume "$backup_label"` to create a portable backup. Choose a second volume that was configured and initialized with the vault. The backup retains the publisher hashes and local-source provenance. For intake with no supported upstream proof, explicitly review the folder and use `import --config ... --repo ... --revision ... --volume ... --path ...`. Its 40-hex revision is an operator label. It records `local-baseline`, including when the same bytes have been seen elsewhere. A peer infohash cannot establish the publisher identity.

## Recover before depending on the archive

Back up config, catalog and payload from the same quiescent state. Stop writers and complete a consistent SQLite backup, including the state in the WAL, before copying the result to independent media. For example, using the optional SQLite CLI from an empty private backup directory, `sqlite3 -readonly /var/lib/modelprepper/catalog/catalog.sqlite3 ".backup catalog-backup.sqlite3"` writes a consistent copy. Check it with `sqlite3 -readonly catalog-backup.sqlite3 "PRAGMA integrity_check;"` and expect `ok`. A catalog backup contains private reviews and historical authority. Keep the recovery guest offline with scheduling disabled during inspection. Store the executable revision and file checksums with the backup. Keep grants and credentials private. A VM snapshot can roll back the guest but shares the backing storage's failure modes.

For an existing seal or an imported baseline, run `replica --config ... --plan ... --volume ...` against the separately identified medium. Rehearse recovery on a fresh catalog path using retained configuration, original volume markers and bundles. Keep the original catalog unavailable and publisher access disabled. Run `rebuild --config ...`, inspect damaged, conflicting and offline entries, then `verify-sealed --config ... --plan ...` for the recovered custody copy. For a replica-only rehearsal, use a separate config with a fresh catalog path. List only the available backup volume, keep its original label, and make that label the default. Fresh-catalog recovery needs a valid marker for every volume listed in that config. Record which medium supplied the verified bytes. Rebuild does not recreate grants. Retain the consistent catalog backup for reviews, adoptions without replicas, and other catalog records. Follow the [offline task drill](validation-drills.md#offline-task-and-restore) for actual runtime evidence.

| Failure to rehearse | Repository evidence | What the operator must observe on the real deployment |
| --- | --- | --- |
| Removed or replaced data disk | Tiny volume-identity and watch fixtures | Boot with data disk unavailable. Check the bare OS mountpoint for new files and the intake client for peer traffic. |
| Interrupted intake or adoption | Interrupted copy, changed source and corrupt-byte fixtures | Restart with a partial external download. Only hand off a completed directory. Retry a stopped adoption and compare custody and source. |
| Guest or host capacity exhausted | Reservation failures use bounded fixtures | Use a disposable guest allocation and isolated host test pool. Record actual I/O errors and verify source and previously published data survive. |
| Lost catalog or disk | Replica-only recovery with a fresh catalog, renamed primary, retained backup marker and a full hash in the fixture | Restore with the original unavailable on genuinely separate media. Record volume IDs, checksums, elapsed time and every missing item. |

On Linux, run `scripts/test-headless-linux.sh target/release/modelprepper` to rehearse the handoff with 17 synthetic bytes in a fresh temporary directory. This developer drill uses Bubblewrap with unprivileged namespaces, jq and the SQLite CLI. The intake process sees only the writable incoming tree, the consumer sees a read-only published revision, and the recovery process sees only the backup volume and a fresh catalog. Each process has its own network namespace. The fixture checks the catalog backup, hashes the recovered bytes, and verifies the new catalog contains zero grants. It needs no VM, torrent client, weights or inference runtime. Its two temporary directories share one underlying filesystem.

An always-available VM can run explicit commands now. Current scheduling is one weekly watch pass. Transfer approval, autonomous admission, provider-assisted curation, unattended recovery and release qualification still have open gates. See [issue 80](https://github.com/blisspixel/ModelPrepper/issues/80) for the operational evidence.
