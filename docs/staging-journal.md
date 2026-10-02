# Durable staging journal

The experimental `StagingSession` library owns persistent checkpoints for one pinned file. It adds no dependencies and does not expose a payload CLI, grant approval, reserve storage, enforce transfer budgets, or publish a sealed bundle.

## Ownership and lifecycle

Creation requires an absolute new directory under an existing parent. It rejects dot segments, symlink/reparse ancestors, existing directories, and unrelated entries. Windows UNC paths are rejected because the checkpoint database requires a local filesystem. Other network filesystems remain unqualified.

The directory contains an OS-locked `writer.lock`, `payload.part`, and `checkpoint.sqlite3`, with a temporary SQLite rollback journal during transactions. The lock is held for the lifetime of the session. Payload and database handles close before writer ownership is released. Reopening does not create missing files or infer ownership from a partial filename.

The caller supplies the same pinned source and expected file inventory when creating and reopening a session. The journal binds their canonical transfer-file identity. A changed commit, path, expected size, or hash cannot reuse the checkpoint. Full original inventory remains the future vault job's responsibility.

Interrupted initialization is retained for inspection. There is no automatic adoption or deletion of orphan directories. The existing vault catalog remains schema version 2; the staging database is a separate experiment, not a vault migration.

## Durable ordering

Creation syncs the empty payload and transactionally records its zero-byte checkpoint before any payload request. SQLite uses rollback journaling and `synchronous=FULL`. On Unix, creation also syncs the session directory and parent. Windows directory durability still needs filesystem and power-loss qualification.

For each segment:

1. Read and validate the last committed checkpoint.
2. Verify the committed prefix and reconcile any uncommitted tail using [staged recovery](staged-recovery.md).
3. Request a bounded range, write the payload, sync it, and verify a complete file against its upstream identity.
4. Transactionally commit the new length and prefix SHA-256.
5. Return success only after that checkpoint commit succeeds.

If the checkpoint transaction fails after payload sync, the operation returns an error. The old checkpoint remains authoritative. A later segment or reopened session verifies that old prefix and discards the extra bytes before resumption. Discarded bytes never imply a refund of network spending; reservations and durable accounting are the next integration item.

On reopen, schema/application identity and the single checkpoint row are checked before payload recovery. Progress uses decimal text for exact unsigned 64-bit lengths. Unknown versions, mismatched identities, corrupt hashes, impossible lengths, missing journals, and short/corrupt committed prefixes fail without truncating the payload. Journal files larger than 1 MiB are rejected. These checks detect local inconsistency, not a malicious rewrite of both the journal and its claimed baseline.

Completion is not a persisted authorization or seal. Even a complete checkpoint must pass full-file upstream verification before the library returns a verified result.

## Failure codes

These are library errors, not new CLI commands:

| Code | Meaning and recovery |
| --- | --- |
| writer_busy | Another process owns the session. Wait for it or investigate the running worker. |
| unsafe_staging_path | Supply an absolute path without dot segments. Existing vault path checks also reject links and reparse points. |
| unsupported_staging_filesystem | The Windows checkpoint directory is on a UNC share. Keep the experimental journal on a local filesystem. |
| staging_directory_not_owned | Unrelated entries exist. Retain and inspect the directory; do not adopt it automatically. |
| invalid_checkpoint_journal | Schema, ownership, shape, size, or progress is invalid. Retain the directory and original inventory for inspection. |
| checkpoint_journal_failed | SQLite failed. Do not count the segment as committed; reopening can reconcile a verified old prefix. |
| invalid_checkpoint | The payload and committed baseline disagree. Retain the partial file rather than guessing which bytes to keep. |
| staging_io_failed | A file operation failed. Restore access or capacity before retrying. |

## Evidence and next boundary

Nine new Windows-native tests exercise reopen/resume, competing ownership, empty files, exact 64-bit declarations, failed journal writes, corruption, foreign versions, missing files, nonregular entries, unrelated data, and oversized journals. A separate worker is forcibly terminated before checkpoint commit, inside an open SQLite transaction, and after commit. Reopening rolls back uncommitted journal progress, removes only uncommitted payload, and retains committed completion.

The local Windows suite passes 70 Rust tests. The single CI workflow runs the journal tests on native Windows, macOS, and Linux runners; consult the current main-branch run for hosted results. Existing transfer and installer drills are documented separately in [validation-drills.md](validation-drills.md).

Next work is durable storage and transfer reservations tied to approved vault plans. Until that exists, this library remains an experiment and approval stays blocked. Cancellation, startup orphan inspection, total workspace measurement, large-file performance, network filesystem qualification, manifests, and physical power-loss recovery remain open. Repeated prefix sweeps are bounded in memory but expensive in disk reads; this is not a large-model throughput claim.
