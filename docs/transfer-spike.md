# Native transfer investigation

Status: package inspection and a bounded blocking HTTP/source adapter, October 1, 2026. Landscape re-check 7 November 2026, below. M1 is not complete. Live metadata inspection passed on Windows; no live weight transfer, live resume drill, peak-space measurement, or full platform runtime qualification has been performed.

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

The adapter proves moving-ref resolution followed by pinned metadata and hash-verified config/license/index bytes. Local fixtures cover redirects, rejected origins, bad status, truncated bodies, unexpected encoding, limits, source changes, access restrictions, and shard closure. A redirected location that returns 401 or 403 is abandoned. The original URL is requested once more, without credentials, and the signed query is not stored or repeated. A second rejection stops. This does not change the pinned transfer identity. `fetch` requires a current grant, refuses to open a payload socket without one, and does not set `transfer_authorized`. `watch` can fetch a granted revision on one explicit pass and does not set `transfer_authorized` either. `replica`, `rebuild`, and `import` do not open a socket. `schedule` can run `watch` weekly after a matching preview. The CI fixture does not enable a live timer. `collection preview` reads a local file, records a representation name, category names, a category ceiling, a declared base, an evaluation attribution, an evidence statement, and a provenance statement without applying them, lists a repeated base without merging it, and does not open a socket. `collection store` writes that list into the catalog and does not open a socket either. `collection list` reads the stored names and does not open a socket. `collection remove` deletes that stored name and does not open a socket or delete archived revisions. `collection custody` reads matching catalog rows and does not open a socket or hash files.

The new transfer library experiment streams bounded segments into a caller-owned staging file using a 64 KiB application buffer. Its checkpoint binds the source/file identity, exact prefix length, prefix SHA-256, and the cumulative segment allowance charged before each request. That charge is not a socket total: a rejected body still owes the full allowance, and a plaintext loopback fixture measured more request and response bytes than the allowance for a refreshed segment. The 64 KiB buffer is not included in the 2 MiB side-file cap. The fixture counts are HTTP message sizes. The recorded TLS figures are separate: ureq keeps 128 KiB on each side of the rustls stream, rustls 0.23.46 caps received plaintext at 16 KiB and outgoing bytes at 64 KiB, and the segment reader copies at most 64 KiB. Kernel receive and send buffers are read from a connected loopback socket and are not a copied constant. Existing bytes are checked before any resumed request. Range responses must match the requested start/end and full pinned size; ignored Range during resumption is rejected before appending. Truncated, excessive, or wrong-hash bodies roll back the current segment. A complete file is verified against its upstream hash, including Git object-header semantics.

Fixture tests prove those invariants, full-body 200 acceptance only for a fresh complete request, and no-request handling of already complete/empty files. The new [staging-session experiment](staging-journal.md) owns exclusive access and SQLite checkpoint journaling, with failed-commit and process-termination fixtures. It is not the integrated vault transfer engine: approvals, budgets, and cancellation remain open. Publication of one granted revision is `fetch`. Explicit [staged recovery](staged-recovery.md) now verifies the committed prefix before discarding and syncing an unjournaled tail. Reopen, idempotency, corruption, and read-only fixtures cover this operation. Separate native worker termination now covers interruption during the body read and after segment sync. The staging journal's allowance charge is not vault job accounting. Power-loss reconciliation remains open. Prefix sweeps on each restart are correct but require a persistent incremental session before repeated segments can be efficient on large models. The plaintext fixture measures HTTP message sizes only. TLS buffer sizes and a loopback kernel-socket measurement are recorded and are still not a plan workspace number. The 64 KiB application buffer is not a total-memory claim. A full GET above 50 GB (50,000,000,000 bytes) stays outside this path.

The live SmolLM2 inspection resolved commit 12fd25f77366fa6b3b4b768ec3050bf629380bac. Its config matched the Git blob identity. The repository had an Apache-2.0 tag but no recognized license file, so evidence remained unreviewed. No weights were downloaded. This verifies source inspection only.

A second release inspection of Qwen2.5-0.5B-Instruct at 7ae557604adf67be50417f59c2c2f167def9a775 verified license, model configuration, and tokenizer configuration bytes. Apache evidence matched with its publisher-filled appendix copyright. Both inspections retained transfer_authorized=false and fetched no weights.

The next transfer experiment uses public pinned hub resolve URLs and validates their byte streams. Native Xet optimization remains a separate candidate; no custom Xet protocol is proposed. HTTPS redirect origins are restricted as specified in [contracts.md](contracts.md).

## Integration boundary

Keep source resolution behind a narrow adapter. Own file-path validation, staged publication, exact identity checks, full-file hashing, and accounting in ModelPrepper.

Evaluate lower-level pinned byte streams for ordinary/LFS payload and the native Xet path separately. Reuse protocol implementations where sound; retain the preservation contract at the application boundary.

Do not add Python, external downloaders, or a mandatory service to work around a client gap. Do not write a custom Xet implementation. `hf-xet` 1.8.0 (Apache-2.0; crates.io page stamped 29 October 2026) is the maintained library to wrap later, and only for a file the bridge cannot serve.

## Landscape check, 7 November 2026

Checked against Hugging Face's own docs and the notes under `.agents/research_notes/`. No live weight was downloaded for this check. The [LFS bridge page](https://huggingface.co/docs/hub/xet/legacy-git-lfs) was opened that day.

Xet is the storage default for new Hub users since the 23 May 2025 changelog, and the 15 July 2025 migration post says existing repositories move without a hard cut-over. A repository may mix Xet and LFS files. A client that does not speak Xet still receives one URL from the bridge, which reconstructs the file. The bridge page names `curl` and the web UI as clients of that URL. `resolve/<commit>/<path>` remains the byte URL. `hf_transfer` is deprecated in the current download guide. `HF_HUB_DISABLE_XET` still selects ordinary HTTP.

That path is what `fetch` uses for one granted revision. CI exercises it with a local fixture. A live weight download stays an opt-in developer drill. The first seal is a small public safetensors file, pinned by the full commit SHA, hashed by this program against the LFS SHA-256. A Hub engineer wrote on 14 April 2025, in huggingface_hub pull request 2991, that a full GET of a file over 50 GB returns 400 from the CDN, while a Range GET can proceed. Forum reports in 2026 say the bridge itself fails on single files of several hundred GB. Those files are outside the first seal.

A Range request that also sends `Accept-Encoding` can be answered with HTTP 200 by CloudFront. huggingface_hub 0.36.2 (6 February 2026, pull request 3778) treats that 200 as a failed resume and starts over. This experiment rejects an ignored Range, and a ranged GET does not send `Accept-Encoding`. A partial range answered with HTTP 200 is not appended.

`hf-hub` 1.0.0 stays off the dependency graph. Its ordinary download uses `File::create` and a full GET, and the crate requires reqwest, tokio, and hf-xet. Wrapping `hf-xet` early would add a second HTTP stack beside ureq and a cache this program does not bound. Neither crate documents a workspace quota.

Local managers checked the same day store a runnable file or a movable tag. Ollama content-addresses blobs and can keep license text, and a pull can drop unreferenced blobs. LM Studio, llama.cpp's cache, Jan, and vLLM's default directory do not pin a commit. The Hub cache does pin `snapshots/<commit>`, and huggingface_hub builds from 30 July 2026 until 8 October 2026 could treat a short snapshot as complete. A seal is a folder of bytes, hashes this program computed, and the LICENSE and NOTICE text that were retrieved. It is not a cache directory and not an Ollama blob name.

BagIt, git-annex, Software Heritage, ORAS, restic, and Borg treat a URL as a locator and a hash as identity. Nothing in that check argued for chunk deduplication, a torrent client, or a second hub before one revision restores with the origin unreachable. The [roadmap](roadmap.md) waves follow that order.

## Required next experiments

| Experiment | State | Evidence still required |
| --- | --- | --- |
| Build graph | `hf-hub` 1.0.0 inspected and rejected as the downloader. | Re-inspect before any later decision to wrap `hf-xet`. |
| Local HTTP fixture | Range, truncation, bad status, and rollback are in the transfer library. A ranged GET omits `Accept-Encoding`. A 200 that answers a partial range is not appended. | Live hub expiry stays out of the fixture waves. |
| Interrupted ordinary download | One frozen set, including license and notice, uses the single-file checkpoint rules. Part files stay part files. `fetch` publishes one granted revision after the staged set is complete. | A renamed directory without `modelprepper-bundle.json` is not a seal. |
| Expired source URL | One fixture refresh from the original URL. | Live hub expiry and credentialed refresh stay out of the fixture waves. |
| Resource limits | Plaintext HTTP sizes for one refreshed segment exceed the charged allowance. TLS input, output, received-plaintext, and send buffers are recorded. Kernel receive and send buffers are measured on a loopback socket. A grant records those figures and the per-file staging bound. | `workspace_bytes` stays null. The grant is not a transfer authorization. |
| Xet download | Not started. Not next. | After one HTTPS revision restores with the hub unreachable, and only for a file the bridge cannot serve. Wrap `hf-xet`. Do not write a client. |
| Cross-platform execution | CI runs the fixtures on Windows, macOS, and Linux, and checks the runner temporary directory is ext4, APFS, or NTFS. | Clean-machine qualification stays in M7. A live network share and HDD timing were not measured in M6. |
| Opt-in live comparison | Not started. | One small public safetensors repository, outside CI. The fixture `fetch` is not that drill. |

`workspace_bytes` stays null on plans, budgets, and kits. `fetch` performs one granted transfer and does not authorize it: `decide` still blocks, and `transfer_authorized` stays false. A grant can record measured bounds for one review and is dropped when those bounds, the volume, or the policy change. The current CLI can initialize a vault, inspect sources, and seal one granted revision from a fixture or a ranged HTTPS source.
