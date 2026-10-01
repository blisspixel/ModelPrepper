# Staged transfer recovery

The Rust transfer experiment includes explicit, offline recovery of an uncommitted file tail. This is a library operation, not a payload CLI or a complete durable job engine. It adds no dependencies.

## Failure being handled

A worker writes a segment and syncs its staging file. The process stops before the corresponding checkpoint is committed to the journal. After restart, the file is longer than the last committed checkpoint. Ordinary resumption rejects that mismatch rather than guessing which bytes to trust.

For example, the committed checkpoint covers `abc`, but the file contains `abcdef`. Recovery verifies the checkpoint's identity and the SHA-256 of `abc`, truncates the file back to `abc`, and syncs the truncation. The next segment requests the remaining bytes from the same pinned source. Bytes beyond the committed boundary are discarded even if they happen to be correct.

## Implemented invariants

`recover_stage` takes an exclusively owned open file, the pinned source/file inventory, and the caller's last durably committed checkpoint. It performs no HTTP request.

- Source restrictions and file identity validation are shared with ordinary staging.
- Checkpoint schema, source/file identity, committed length, and committed prefix hash must match before any truncation.
- A short file, changed revision, or corrupt committed prefix fails without changing the file bytes.
- An extra tail can exceed the expected payload size. It is still uncommitted data and can be discarded after the committed prefix verifies.
- Successful recovery syncs the file and positions the cursor at the committed boundary. Repeated recovery discards zero additional bytes and syncs again, including after a previous sync failure.
- A read-only file produces an I/O error when truncation is needed. Failure never counts as successful recovery.
- A recovered prefix does not establish final integrity. Completion still requires the pinned upstream hash and a full-file local SHA-256.

The checkpoint is a local integrity baseline. It cannot authenticate a journal that someone has deliberately rewritten together with its hashes. No checkpoint grants approval, bypasses rights checks, or constitutes a sealed archive.

## Required integration protocol

The following order is the contract for the future job engine, not functionality already shipped:

1. Acquire writer ownership, verify the mounted volume identity, and validate the approved immutable transfer plan.
2. Durably establish the staging file and its initial zero-byte checkpoint before requesting payload. If a nonempty file has no committed checkpoint, report it as unowned recovery data; do not invent an identity or truncate it automatically.
3. Reserve storage and transfer allowance before each segment. Network spending and verified file progress are separate records.
4. Write and sync the segment, then transactionally commit its new checkpoint. Never commit a checkpoint ahead of the file sync.
5. On restart, load the last committed checkpoint and call recovery before ordinary resumption. Failures leave the job incomplete and expose an actionable reason.
6. Re-download discarded bytes only with remaining transfer allowance. Discarding local bytes does not refund network spending. A crash during a request needs conservative accounting for its reserved allowance until reconciled.
7. Verify every complete file before the separate manifest and publication protocol. Retain the previous committed recovery evidence until its replacement is durable.

Creating directory entries, checkpoint persistence, transactional accounting, cancellation, and publication still require their own crash-boundary tests. File synchronization alone does not prove directory durability or a sealed bundle on every filesystem.

## Evidence and remaining work

Native fixtures close and reopen a staged file with a synced but unjournaled tail, recover it twice, and resume through a local HTTP server to the exact upstream hash. Additional fixtures cover empty committed prefixes, invalid schemas and identities, short files, corrupt prefixes, and read-only truncation failure. Separate [process-termination drills](validation-drills.md) now forcibly stop a real worker during an HTTP body read and after segment sync, then recover and resume. None of these drills simulate physical power loss.

Repeated prefix scans use bounded memory but cost disk reads proportional to the committed prefix. A qualified large-model implementation still needs an efficient session strategy, durable accounting, integrated job termination drills, and filesystem qualification on Windows, macOS, and Linux. See [transfer-spike.md](transfer-spike.md), [contracts.md](contracts.md), and [roadmap.md](roadmap.md).
