# Durable staging journal

The experimental `StagingSession` library owns persistent checkpoints for one pinned file. It adds no dependencies and does not expose a payload CLI, grant approval, reserve vault storage, or publish a sealed bundle. It does charge each segment allowance in the checkpoint journal before requesting those bytes.

## Ownership and lifecycle

Creation requires an absolute new directory under an existing parent. It rejects dot segments, symlink/reparse ancestors, existing directories, and unrelated entries. Windows UNC paths are rejected because the checkpoint database requires a local filesystem. Other network filesystems remain unqualified.

The directory contains an OS-locked `writer.lock`, `payload.part`, and `checkpoint.sqlite3`, with a temporary SQLite rollback journal during transactions. The lock is held for the lifetime of the session. Payload and database handles close before writer ownership is explicitly released, including when a concurrent process spawn briefly inherits the Unix file description. Reopening does not create missing files or infer ownership from a partial filename.

The caller supplies the same pinned source and expected file inventory when creating and reopening a session. The journal binds their canonical transfer-file identity. A changed commit, path, expected size, or hash cannot reuse the checkpoint. Full original inventory remains the future vault job's responsibility.

Interrupted initialization is retained for inspection. There is no automatic adoption or deletion of orphan directories. The existing vault catalog remains schema version 2; the staging database is a separate experiment, not a vault migration.

## Durable ordering

Creation syncs the empty payload and transactionally records its zero-byte checkpoint before any payload request. SQLite uses rollback journaling and `synchronous=FULL`. On Unix, creation also syncs the session directory and parent. Windows directory durability still needs filesystem and power-loss qualification.

For each segment:

1. Read and validate the last committed checkpoint and its cumulative charge.
2. Verify the committed prefix and reconcile any uncommitted tail using [staged recovery](staged-recovery.md). Discarding that tail does not reduce the charge.
3. Add this segment's allowance to the journaled charge and commit that update before any payload request. A journal that cannot record the charge makes no request.
4. Request a bounded range from the pinned resolve URL. If a redirect location returns 401 or 403, abandon that location and request the same original URL once more. A second rejection fails without writing a payload or disclosing the location. The allowance charged in step 3 covers the whole attempt, including that refresh. Write the accepted payload, sync it, and verify a complete file against its upstream identity. A refreshed body that does not match the pinned hash is rolled back and does not change the transfer-file identity.
5. Transactionally commit the new length and prefix SHA-256. This update does not change the charge.
6. Return success only after that checkpoint commit succeeds.

If the length checkpoint fails after payload sync, the operation returns an error. The old length remains authoritative, and the allowance charged in step 3 remains spent. A later segment or reopened session verifies that old prefix and discards the extra bytes before resumption. The retry is charged again. This in-journal charge is not a vault reservation and is not a measured bandwidth total: a rejected or interrupted body still owes the full allowance. Vault reservations and period accounting remain the next integration item.

On reopen, schema/application identity and the single checkpoint row, including the decimal charge, are checked before payload recovery. Progress and the charge use decimal text for exact unsigned 64-bit lengths. Unknown versions, mismatched identities, corrupt hashes, impossible lengths, a corrupt charge, missing journals, and short/corrupt committed prefixes fail without truncating the payload. The checkpoint database and `checkpoint.sqlite3-journal` are each rejected above 1 MiB. These checks detect local inconsistency, not a malicious rewrite of both the journal and its claimed baseline. `staging_workspace_bound` is those two caps added together. It does not include `payload.part`, process memory, or TLS buffers.

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
| payload_location_rejected | The original URL or a redirected location returned 401 or 403. The location is not stored or repeated. The charge for that attempt remains spent. Retry only with the same pinned identity. |

## Evidence and next boundary

Windows-native tests exercise reopen/resume, competing ownership, empty files, exact 64-bit declarations, failed journal writes, corruption, foreign versions, missing files, nonregular entries, unrelated data, oversized database and rollback-journal files, a charge that survives a rejected body and a killed worker, and a retry that is charged again after the tail is discarded. They also refresh one rejected redirect location without a second charge or an identity change, and they keep the charge when that refresh is rejected or returns different bytes. A separate worker is forcibly terminated before checkpoint commit, inside an open SQLite transaction, and after commit. Reopening rolls back uncommitted journal progress, removes only uncommitted payload, and retains committed completion.

The local Windows suite passes 108 Rust tests. The single CI workflow runs the journal tests on native Windows, macOS, and Linux runners; consult the current main-branch run for hosted results. Existing transfer and installer drills are documented separately in [validation-drills.md](validation-drills.md).

The expired-location fixture requests the original URL once more and does not reuse the rejected location. Next integration is still durable vault storage and transfer reservations tied to approved plans. Until a workspace number also covers every selected file, TLS and socket buffers, and measured bandwidth overrun, this library remains an experiment and approval stays blocked. The recorded side-file cap is not the multi-file workspace number. Cancellation, startup orphan inspection, large-file performance, network filesystem qualification, manifests, and physical power-loss recovery remain open. Repeated prefix sweeps are bounded in memory but expensive in disk reads; this is not a large-model throughput claim.
