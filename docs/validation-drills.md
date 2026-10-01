# Validation drills

Recorded October 1, 2026. These observations supplement automated coverage. They do not qualify production downloads, every filesystem, or hardware power-loss recovery.

## Abrupt transfer termination

The native Rust suite starts a separate worker process with an exclusively locked staging file and a synced checkpoint. The parent forcibly terminates it at two boundaries:

| Boundary | Observed result |
| --- | --- |
| During a real local HTTP body read, after one uncommitted byte is appended | The OS releases the worker's file lock. Recovery discards that byte and retains the committed prefix. |
| After the next complete segment is synced, before its checkpoint is saved | Recovery discards the unjournaled segment even though its bytes were correct. |

Both cases resume through a local HTTP fixture and verify the exact upstream hash. Worker readiness is explicitly signaled, file length is observed before termination, timeouts are bounded, and a cleanup guard kills and reaps workers on failure. The child helper does no work during ordinary test execution unless explicitly invoked with the test's private environment.

Run this drill with:

```text
cargo test --locked terminated_transfer_process -- --nocapture
```

Native CI repeats it on Windows, macOS, and Linux. These tests qualify the transfer primitive's process-interruption boundary. They do not test transactional job accounting, directory durability, hard disk cache loss, or hardware power failure.

## Installer failure expansion

Offline native installer fixtures now cover truncated archives, an install-directory collision with unrelated user data, and an entry larger than 128 MiB, in addition to checksum, version, path, link, extra-entry, upgrade, and exclusive packaging cases.

The oversized-entry drill exposed a shell-installer gap: the Windows installer rejected it, but the shell installer accepted it. A local Git shell fixture reproduced acceptance using the previously committed script and rejection using the fixed script. This is shell behavior evidence, not native Linux qualification.

The shell installer now limits each extracted entry to 128 MiB plus one detection byte before rejecting oversized output. It preserves tar listing and extraction failures, rejects the archive before executing its binary or writing the installation, and uses no new product runtime. Streaming output avoids relying on platform-specific archive listing columns. Native Linux and macOS installer fixtures exercise the fix in CI.

## Local NTFS vault

A dedicated empty test directory on a fixed NTFS H: volume was initialized with a 500,000,000,000-byte storage quota and a 50,000,000,000-byte free-space reserve. Its local configuration and catalog are excluded from Git; the catalog is outside the build directory.

Observed live checks:

- Three separate native initialization processes preserve vault and volume identity.
- Temporarily renaming the empty test directory makes status and repeated initialization report the volume offline with no available-byte value. Initialization does not recreate the missing directory.
- Source proposal rejects that offline destination before source inspection.
- Temporarily substituting the marker's volume UUID makes status and initialization report an identity mismatch. Source proposal rejects the destination.
- Restoring the exact marker bytes restores mounted status and the original identity.
- Holding a competing OS file lock makes the native CLI reject the writer. Releasing the lock restores access.
- SQLite integrity and foreign-key checks pass; the catalog retains schema version 2.

The mount simulation renames a directory, not the drive. This is not a physical disconnect, network-share, filled-disk, or 500 GB payload drill. Quota configuration is not evidence that future integrated transfer accounting enforces the quota.

## Live pinned source review

The native CLI inspected Qwen/Qwen2.5-0.5B-Instruct at commit 7ae557604adf67be50417f59c2c2f167def9a775 against the mounted test vault. License evidence matched; the selected inventory reported 999,602,607 payload bytes. Only metadata and bounded evidence files were fetched.

Approval was rejected by the unqualified-transfer gate and left the review pending. An explicit rejection persisted across native process restarts. Repeating resolution retained the same review identity and final rejection. Transfer authorization remained false throughout, and no model payload was downloaded.

This checks the live metadata adapter and durable decision boundary. It does not establish runnable model completeness or offline loading. See [implementation-status.md](implementation-status.md), [staged-recovery.md](staged-recovery.md), and [quality.md](quality.md).
