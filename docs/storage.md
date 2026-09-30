# Storage

## Paths and authority

The active SQLite catalog lives on a local filesystem of the single writer host. A NAS can run the binary with its catalog on local storage and its bulk dataset as a volume. A desktop can keep its catalog local and write weights to SMB/NFS.

Do not place the active WAL database on a mounted network share. [SQLite documents this limitation](https://www.sqlite.org/wal.html). Use its [backup API](https://www.sqlite.org/backup.html) for consistent database backups rather than copying only a live database file.

```text
catalog/
  config.toml
  catalog.sqlite
  manifest.jsonl
  events.jsonl
  logs/

volume/
  volume.json
  staging/<job-id>/
  revisions/<safe-repo-id>/<commit>/<profile-id>/
    payload/
    modelprepper.json
    source.json
    README.offline.txt
```

The model loader is pointed at payload/. Archival metadata never overwrites files from the source repository.

SQLite is authority for live plans, approvals, jobs, and reservations. Per-bundle manifests are recovery authority for sealed files. manifest.jsonl is a regenerable inventory snapshot, atomically replaced after commits; it is not an append-only ledger. Audit events have separate ids and tolerate an incomplete final export line.

## Volumes

Each volume has a UUID, schema version, optional label, and supported filesystem properties. Adoption must not format a disk or silently claim an unrelated nonempty directory.

Lifecycle is writable, read-only, or retired. Availability is mounted or offline. "Insufficient space" is a current condition rather than a permanent lifecycle state: a drive unable to fit a large bundle may fit a small one.

The default destination is user-selected. Each complete revision resides on one volume. A replica also resides on one volume and must be self-contained.

A missing path never falls back to a plain directory on the system disk. Match volume.json and reject identity mismatch. Reattaching a disk under a new letter binds by volume UUID through an explicit mount/adopt operation.

## Space accounting

Track these separately:

- Final logical payload bytes across all preserved bundles.
- Known occupied bytes, including replicas, staging, and identified runtime caches.
- Pending reserved final bytes.
- Peak additional workspace needed by the selected transfer backend.
- Filesystem free bytes and configured minimum reserve.
- Transfer bytes spent and reserved in the configured UTC period.

Do not assume logical size equals physical allocation. Compression, reflinks, sparse files, and external writes can differ. Actual free-space checks remain mandatory.

For a resumed job, admission checks require:

```text
free_now >= remaining_final_bytes + peak_additional_workspace + min_free_bytes
```

Already downloaded partial files are reflected in free_now; do not count them twice. Plans show selected payload and backend workspace bounds. A universal 1.2 multiplier cannot replace measuring Xet/cache behavior.

Use transactional reservations to prevent overcommitting final capacity. Reconcile reservations with disk state after interruption. Recheck free space during downloads because unrelated processes can consume it.

## Transfer budget

max_transfer_bytes_per_period counts accepted HTTP response body bytes, including retries and metadata fetches; it is not a promise about total wire overhead. A separate logical admission limit bounds planned new payload.

Reserve estimated remaining payload before starting. Runtime counting stops transfers at the cap, subject to a documented bounded buffer allowance. Persist counters/checkpoints and recover conservatively after a crash. Protocol overhead is excluded and must be stated.

The default period is a UTC calendar week. Configuration specifies the boundary. Reset a period's transfer allowance without forgetting unfinished work or its storage reservation.

Cancellation does not erase partial-byte accounting. Cleanup is explicit. Replica copies consume storage but not a network budget when made locally.

## Concurrency and locks

One writer process per catalog and managed volume. Use kernel-backed host locks, with owner metadata for diagnostics; a leftover lock file alone is not evidence of a live writer.

A local catalog does not protect against another host using a different catalog on the same NAS volume. The supported contract forbids multiple writer hosts. Do not advertise distributed coordination until it is separately implemented and tested.

No database transaction is held while downloading or hashing.

## Recovery

Inspect on-volume manifests into a new catalog. Deduplicate identical bundle identities, identify replica locations, and flag conflicts without changing source files.

Manifests restore sealed inventory and provenance. They do not reconstruct lost watchlists, unexported approvals, transfer history, or all job intent. Back up config and catalog as well.

Staging contains job checkpoints sufficient to offer inspection or adoption after catalog loss. Unknown staging never becomes sealed merely because its directory name resembles a commit.

## Replica and export

Replica operations copy a sealed bundle, rehash the destination, publish its manifest, and only then record a healthy replica. A interrupted replica is incomplete.

Offline export includes the selected bundles, volume identity, sanitized inventory, and use notes. It excludes credentials and machine-specific approval authority.

Imported bytes with no captured source evidence remain local-baseline imports. Matching a known manifest can establish consistency with that captured identity.

A second copy on the same physical disk does not satisfy a separate-media target. Ask the user to label failure domains; filesystem paths alone cannot establish that two disks are independent.

## Repair and deletion

Prefer a verified replica when repairing corruption. Source repair remains pinned to the original revision. If both are absent, retain the damaged record and report unrecoverable files.

No automatic pruning in version one. Later deletion requires an inspectable plan naming exact bundle ids, volumes, bytes, dependency impact, and remaining replicas. Parent directories outside the adopted volume are never targets.

Migration defaults to verified copy. Source deletion is a separate explicit step. Never combine "move" and deletion before the target is verified.

## Filesystem support

Baseline: NTFS on Windows, APFS on macOS, and ext4 on Linux, with tested alternatives documented. Mounted SMB/NFS are supported only for payload with a single writer. exFAT needs explicit tests; FAT32 cannot hold files over its per-file limit.

Reject unsupported path collisions before transfer. All bundles must be readable without symlinks or platform-specific deduplication.

Measure cache and staging behavior on HDD as well as SSD. Deduplication is a later optimization and must not undermine portable self-contained copies.
