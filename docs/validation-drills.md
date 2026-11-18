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

A dedicated empty test directory on a fixed NTFS H: volume was initialized with a 500,000,000,000-byte storage quota and a 50,000,000,000-byte free-space reserve. Its local configuration and catalog are excluded from Git; the catalog is outside the build directory. These notes are not the wave 8 filesystem qualification. That wave classifies the runner's own temporary directory and does not treat this renamed directory as a network share, a filled disk, or an HDD timing measurement.

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

## Offline task and restore

This is the procedure for [issue 29](https://github.com/blisspixel/ModelPrepper/issues/29). It requires a retained, permitted model and a separately identified runtime. The small files in `examples/readiness` exercise preflight only.

1. Pick one exact task and a checkable success criterion before the run. Record model commit, representation, selected shard hashes, template and processor, recipe digest, runtime origin/build/hash, OS, architecture, drivers and hardware. Retain licenses and notices for the model and runtime separately. List every dependency that is only named or remote. Keep secrets and private paths out of shared evidence.
2. On a clean destination, restore the files and the applicable runtime packages from retained media. Disconnect the guest's network interfaces or enforce a verified outbound block for the entire runtime and its child processes. Record the isolation method, routes, and the result of a connectivity check. Start with a new empty home and empty runtime, model and package caches. Record paths and contents before launch. Disable shared host caches. A library's offline environment flag alone does not establish isolation.
3. Run `modelprepper readiness --recipe "$recipe" --path "$restored" --json` and save its output. When the runtime is restored separately, pass `--runtime-root "$runtime_kit"` to check its declared relative artifact there. Review rights, dependencies, unverified large files and current disk state. Use `--full-hash` when ready to read every declared retained file, including the runtime. Run the reviewed local runtime manually with the exact retained files and remote repository code disabled. Capture command, versions, exit status, task input and output, a UTC timestamp, and outbound attempts (or “not measured”). Loading and performing the selected task are separate observations.
4. Repeat the task on a freshly restored destination with the original catalog unavailable, publisher unreachable and caches empty again. For a bundle-bearing copy use retained config and markers, rebuild to a fresh catalog, inspect conflicts/damage/offline volumes and hash the recovered plan. A verified local adoption can be restored from its portable replica. If no replica was created, recover the primary payload and its consistent catalog backup together. Identify the medium used on each pass.
5. Record both results. A missing shard, tokenizer, runtime library, failed isolation check, unplanned cache hit, failed load or unusable task output needs the failing stage and diagnostic. Record incomplete controls as inconclusive. Keep corrections and reruns as new observations. Describe which exact model files, runtime version, environment and task the result covers. Keep retained kit contents, checksum list, missing prerequisites and plain-text restore steps beside the evidence.

Use the [version 2 drill observation](preservation.md#recording-a-prior-task) for the date, runtime source and license, task, isolation method, outbound observations and log hash. Retain the detailed log separately. After copying or restoring it, pass its path to `readiness --evidence-log` to compare it with the version 2 digest. Record the check separately for each run. A subsequent readiness report may quote an applicable prior result. Each new capability or environment needs its own observed run. No completed live task or clean-cache restore is claimed here.
