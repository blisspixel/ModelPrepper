# Native transfer investigation

Status: package inspection and a bounded blocking HTTP/source adapter, October 1, 2026. M1 is not complete. Live metadata inspection passed on Windows; no live weight transfer, live resume drill, peak-space measurement, or full platform runtime qualification has been performed.

## Candidate inspected

[Official hf-hub](https://github.com/huggingface/hf-hub), published crate version 1.0.0, Apache-2.0, Rust 2024 edition. Inspection used the package returned by cargo info, not an unpinned main-branch checkout.

The package includes async repository operations, snapshot APIs, explicit revision arguments, stream/range operations, and native Xet support. Its Cargo manifest directly includes hf-xet, tokio, reqwest, futures, and supporting libraries. Xet is not an optional feature in this package.

The blocking feature enables the runtime needed by synchronous entry points. The rustls-tls feature enables reqwest's Rust TLS feature; full transitive TLS behavior still requires graph inspection and compilation.

It has not been added to the application dependency graph. Actual dependency decisions are recorded in [stack.md](stack.md).

## Concrete concern found

In the inspected 1.0.0 package, the ordinary-file local-directory path obtains a full GET and delegates to stream_response_to_file_with_progress. That helper uses File::create for the destination. Its ordinary cached-file path also sends a full GET into an incomplete file before rename.

These convenience paths should not be assumed to resume interrupted ordinary-file downloads: the inspected helper truncates its output rather than demonstrating an append checkpoint. This is a narrow observation about those paths, not a claim that no lower-level API or Xet path can support resume.

The public streaming/range interface is a candidate for an application-owned staging and validation layer. Do not substitute a filename ending in .incomplete for proof of safe resumption.

Reference source: the crate's src/repository/download.rs. These observations must be revisited if the evaluated version changes.

## Current adapter decision

Use ureq with Rust TLS for a blocking, bounded HTTP adapter. No async runtime, Python, external downloader, compression, or native Xet engine is required by source inspection. URL parsing and redirect origin validation use the maintained url crate.

The adapter proves moving-ref resolution followed by pinned metadata and hash-verified config/license/index bytes. Local fixtures cover redirects, rejected origins, bad status, truncated bodies, unexpected encoding, limits, source changes, access restrictions, and shard closure.

The new transfer library experiment streams bounded segments into a caller-owned staging file using a 64 KiB application buffer. Its checkpoint binds the source/file identity, exact prefix length, and prefix SHA-256. Existing bytes are checked before any resumed request. Range responses must match the requested start/end and full pinned size; ignored Range during resumption is rejected before appending. Truncated, excessive, or wrong-hash bodies roll back the current segment. A complete file is verified against its upstream hash, including Git object-header semantics.

Fixture tests prove those invariants, full-body 200 acceptance only for a fresh complete request, and no-request handling of already complete/empty files. This is not the durable transfer engine: the caller still owns exclusive access, checkpoint journaling, approvals, budgets, cancellation, and publication. Explicit [staged recovery](staged-recovery.md) now verifies the committed prefix before discarding and syncing an unjournaled tail. Reopen, idempotency, corruption, and read-only fixtures cover this operation. Integrated process-termination and power-loss reconciliation remain open. Prefix sweeps on each restart are correct but require a persistent incremental session before repeated segments can be efficient on large models. TLS/HTTP/socket buffers and network overrun are not yet measured; the 64 KiB application buffer is not a total-memory claim. No CLI payload command is exposed.

The live SmolLM2 inspection resolved commit 12fd25f77366fa6b3b4b768ec3050bf629380bac. Its config matched the Git blob identity. The repository had an Apache-2.0 tag but no recognized license file, so evidence remained unreviewed. No weights were downloaded. This verifies source inspection only.

A second release inspection of Qwen2.5-0.5B-Instruct at 7ae557604adf67be50417f59c2c2f167def9a775 verified license, model configuration, and tokenizer configuration bytes. Apache evidence matched with its publisher-filled appendix copyright. Both inspections retained transfer_authorized=false and fetched no weights.

The next transfer experiment uses public pinned hub resolve URLs and validates their byte streams. Native Xet optimization remains a separate candidate; no custom Xet protocol is proposed. HTTPS redirect origins are restricted as specified in [contracts.md](contracts.md).

## Integration boundary

Keep source resolution behind a narrow adapter. Own file-path validation, staged publication, exact identity checks, full-file hashing, and accounting in ModelPrepper.

Evaluate lower-level pinned byte streams for ordinary/LFS payload and the native Xet path separately. Reuse protocol implementations where sound; retain the preservation contract at the application boundary.

Do not add Python, external downloaders, or a mandatory service to work around a client gap. Do not write a custom Xet implementation without demonstrating that a maintained native integration cannot satisfy the requirements.

## Required next experiments

| Experiment | Evidence required |
| --- | --- |
| Build graph | Exact locked transitive versions, licenses, native libraries, enabled TLS features. |
| Local HTTP fixture | Revision resolution, valid full and partial responses, ignored Range, malformed ranges, truncated bodies. |
| Interrupted ordinary download | Checkpoint bound to pinned identity; resume without loss or mixed bytes. |
| Xet download | Cancellation/resume and measured workspace under controlled limits. |
| Expired source URL | Refresh location without changing content identity or leaking credentials. |
| Resource limits | Memory, temporary cache, concurrency, and bounded transfer-budget overrun. |
| Cross-platform execution | Actual Windows/macOS/Linux binaries, not only cross-target type checks. |
| Opt-in live comparison | One pinned small repository compared with the reference hub client's dry run. |

Until transfer experiments pass, workspace stays unknown and no command can authorize or perform a model transfer. The current CLI can initialize a vault and inspect sources independently.
