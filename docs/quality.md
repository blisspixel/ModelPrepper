# Quality and release gates

## Current repository

This repository contains the Rust vault foundation, public-source inspection, persisted source reviews/rejection, offline preview planning, installer scripts, and design documents. Tests cover ownership/migration, disk substitution, initialization recovery, source-review persistence/pagination, pinned metadata, rights evidence, shard closure, HTTP failures, byte integrity, CLI behavior, offline installer failures/upgrades, offline readiness preflight, ranged seals, replicas, catalog rebuild, and one bounded watch pass. `decide` still cannot approve a transfer. `schedule` preview, install, and remove are covered with an injected host runner. A live timer is not enabled. `collection preview` reads a local file, records a representation name, category names, a category ceiling, a declared base, an evaluation attribution, an evidence statement, and a provenance statement without applying them, lists a repeated base without merging it, and does not contact a publisher. `collection store` writes that list into catalog schema 7 and `collection show` reads it back. `collection list` pages the stored names in name order and does not rank them. `collection remove` deletes that stored name and does not delete archived revisions. `collection custody` counts catalog rows with the same repository and revision text and does not hash or download them. A 40-character lowercase hex revision is labeled `commit`. Any other accepted revision is labeled `name`. The label is not a resolution. When every matching manifest records `size_bytes` and those totals agree, the report shows that sum. A missing manifest or disagreeing totals stay unmeasured. `collection custody` charges a written category ceiling only when every entry that names it has `measured_bytes`. The same measured total can count toward each category that entry names. An unmeasured entry is left out, a partial sum is not reported, and no size is invented. Preview, store, and show still leave `applied` false. `discover` searches the reviewed source catalog for an explicit role, status, or source id. Matches stay in catalog order. It does not rank them, contact a network, or approve a transfer. The `[discovery] enabled = false` sketch is not a command. `collection select` keeps stored entries whose category, representation, or evaluation text matches exactly. Stored order is kept. `evaluation_value` is not a score, and `ranked` stays false. `verify-sealed` hashes one custody plan with the existing file hash. Other plans are not read. `collection custody` does not become a rehash. Nothing is downloaded. `collection export` writes that stored list to a new schema 1 file and does not change the catalog. `collection diff` compares one schema 1 file with the stored list and does not change either. A failed replace leaves the previous list in place. `collection custody` lists the matching plan ids in plan id order. A replica row is not listed. Nothing is dropped or merged. Each listed plan names its stored volume and its own manifest total when that manifest parses. Disagreeing plans stay listed. Nothing is retired. Each listed plan counts replica rows with that plan id. Other entries in the same stored list that name this repository and revision as a declared base are counted as dependents. A replica row is not a custody plan. Counting them does not retire a copy.

Development Node tooling is not a dependency of the planned native product.

Run the current checks with Node 24 or newer:

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

Line and branch coverage use a separate pinned nightly toolchain because cargo-llvm-cov branch instrumentation is currently unstable. Release code remains pinned to Rust 1.99.1.

```text
cargo +nightly-2026-10-08 llvm-cov --locked --branch --json --output-path target/coverage.json --ignore-filename-regex 'tests[\\/]' --fail-under-lines 80
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

A green aggregate percentage does not substitute for fault coverage. Recorded [validation drills](validation-drills.md) document actual process termination, installer regressions, and live-vault checks with their limits. Release CI builds and smoke-tests every labeled architecture. No weights or private credentials are required by ordinary CI.

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
