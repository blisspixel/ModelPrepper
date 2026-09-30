# Quality and release gates

## Current repository

This repository contains the Rust vault foundation, public-source inspection, offline preview planning, and design documents. Tests cover catalog ownership, disk substitution, initialization recovery, pinned metadata, rights evidence, shard closure, HTTP failures, byte integrity, and CLI behavior. Downloads, persisted approvals, sealed manifests, and readiness tests are not implemented.

Development Node tooling is not a dependency of the planned native product.

Run the current checks with Node 22.19 or newer:

```text
npm ci --ignore-scripts
npm run check
```

The single CI workflow runs documentation and native checks on Linux, macOS, and Windows, with separate coverage and dependency jobs. Its aggregate Main checks job passes only when every required job succeeds. Node tests cover local-link validation and Rust coverage-gate behavior. Capture fingerprints and image/output hashes are checked without regenerating screenshots. External URLs and Markdown anchors are excluded from automatic link checking; reference-style links are not currently parsed. Online research links are reviewed during research rather than fetched on every CI run.

Rust checks:

```text
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
cargo audit
cargo deny check
```

Line and branch coverage use a separate pinned nightly toolchain because cargo-llvm-cov branch instrumentation is currently unstable. Release code remains pinned to Rust 1.96.0.

```text
cargo +nightly-2026-07-26 llvm-cov --locked --branch --json --output-path target/coverage.json --ignore-filename-regex 'tests[\\/]' --fail-under-lines 80
node scripts/check-rust-coverage.mjs target/coverage.json
```

Install cargo-llvm-cov 0.9.1 and the nightly llvm-tools-preview component for that development-only check. The gate requires real line and branch measurements of at least 80%; it rejects an empty branch report. Tests are excluded from the coverage denominator, while the entire owned src tree, including the CLI, remains included.

Dependency checks use cargo-audit 0.22.2 and cargo-deny 0.19.8 as development tools. [deny.toml](../deny.toml) gates permissive dependency licenses, registry sources, advisories, and duplicate versions. These tools and their advisory database are not runtime dependencies.

## Required Rust CI

M0 establishes these checks on every change:

- cargo fmt --check.
- cargo clippy --all-targets with warnings denied for supported feature combinations.
- Unit and integration tests on Windows, macOS, and Linux.
- Measured line and branch coverage of at least 80% for owned application code.
- Critical invariant and failure tests regardless of aggregate coverage.
- Dependency/license review and reproducible offline fixture tests.
- Versioned config, plan, manifest, and output compatibility checks.
- Documentation lint and local-link validation.

Select a coverage tool that measures both metrics accurately. Generated bindings and vendored upstream code may be excluded with explicit documented reasons. Do not exclude difficult transfer or recovery logic to reach the threshold.

A green aggregate percentage does not substitute for fault coverage. Release CI builds and smoke-tests every labeled architecture. No weights or private credentials are required by ordinary CI.

## Fault and behavior matrix

| Area | Required cases |
| --- | --- |
| Source | Pagination, 429/Retry-After, timeout, auth failure, expired URLs, changed pinned metadata. |
| Rights | Missing files, mismatched tag, modified text, multiple licenses, unknown terms. |
| Files | Missing shard, LFS pointer instead of body, wrong hash algorithm, truncated content. |
| Resume | Range ignored, changed ETag/identity, corrupted partial, process termination. |
| Storage | Full disk mid-write, competing external writes, read-only volume, changed mount id. |
| Paths | Traversal, drive paths, symlinks/reparse points, reserved names, case collisions. |
| Accounting | Retries, period rollover, cancellation, orphan reservations, replica bytes. |
| Publication | Crash before/after each durable boundary, existing conflicting destination. |
| Recovery | Lost catalog, invalid manifest, unknown schema, offline replica, corrupt replica. |
| User control | No automatic pruning, unchanged older revisions, bounded auto-approval. |
| Privacy | Secret redaction, no status network call, credential-safe exports. |
| Optional interfaces | Same policy decisions as CLI; chat disconnect does not lose job state. |

Use a local deterministic HTTP server and tiny generated artifacts. Real models are unnecessary for automated failure coverage.

## Performance targets

Measure before publishing claims. Establish bounded memory, transfer workspace, concurrency, and bandwidth overrun in M1.

Status should depend on catalog size rather than reading every tensor. Full verification supports a byte/time budget and reports remaining work.

Benchmark SSD, HDD, and supported mounted NAS volumes. Correctness remains a release gate when throughput optimizations change the write path.

## Offline release drill

On each supported OS, install the release artifact on a clean machine, preserve a supported model, verify, replicate, simulate catalog loss, reconstruct, restore, and load offline with an empty external cache.

Record versions, filesystem, runtime, hardware, failure evidence, and results. These manual/live drills are separate from hermetic CI.

## CI truthfulness

Local checks establish that the workflow's commands pass locally. Hosted CI can only be reported passing after observing an actual remote run.

A design-only checkout without a Git remote cannot produce a hosted run. Document that limitation rather than invent a CI result.
