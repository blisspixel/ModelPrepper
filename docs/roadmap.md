# Roadmap

Implementation follows evidence and dependencies, not calendar promises. Native Rust on Linux, macOS, and Windows is a requirement.

Current progress: M0 has strict contracts, canonical previews, and lint/test/coverage gates. M1 has pinned public-source inspection and a streaming segment experiment with checkpoint/range/rollback fault tests. M2 has bundled SQLite, kernel writer ownership, stable volume identities, recoverable committed initialization, and offline status. No milestone is fully qualified: persisted approvals, integrated payload jobs, manifests, full crash recovery, and complete platform drills remain outstanding. See [implementation-status.md](implementation-status.md), [contracts.md](contracts.md), and [transfer-spike.md](transfer-spike.md).

## Immediate execution order

1. Prove bounded staged byte transfer with local fixtures: Range correctness, retries, corrupted partials, interruption, URL refresh, and exact final hashes. Record actual workspace and bandwidth bounds before approval code depends on them.
2. Add catalog migrations and persisted immutable source plans. Bind policy, license evidence, selected/excluded identities, costs, and volume UUID to a separate trusted plan domain. Add watch and decision commands; stale decisions cannot authorize changed plans.
3. Implement reservations and one approved revision's durable publication. Exercise every transaction/file boundary, then add manifest reconstruction and verified replication.
4. Extend from one reliable revision to bounded scheduled watches. Add operating-system distribution and actual filesystem/offline recovery drills before calling the product a beta.

Initialization and inspection must remain useful, explicit operations while these steps are incomplete. Do not expose a pull command until the identity, resource, and publication gates can be enforced together.

## Release boundaries

| Release | Promise | Required milestones |
| --- | --- | --- |
| Developer preview | Resolve, plan, and preserve one exact model reliably. | M0 through M3. |
| Personal vault beta | Maintain watches, recover interrupted work, verify replicas, restore inventory. | M4 through M6. |
| Version 1 | A new user can install, schedule, maintain, and recover the utility on all three OSes. | M7 plus all earlier gates. |
| Later | Collections, discovery, agent access, broader artifacts. | M8 onward after version 1 stability. |

No release claims readiness solely because it can download a file.

## Quality of life across user types

The [user experience plan](user-experience.md) defines first-time, power-user, archivist, and independent-custody workflows, with milestone mapping and acceptance drills. Essential setup, explainable plans, actionable recovery, stable automation, and offline ownership are release requirements. Optional dashboards and discovery follow a reliable preservation core.

## M0: Contracts and quality foundation

Deliver versioned config, plan, manifest, job-state, reason-code, and CLI-output specifications. Agree on default rights and file profiles. Define supported architectures/layouts.

Set up the Rust workspace, pinned toolchain, lockfile, format/lint/test workflows, and at least 80% measured coverage gates. Current documentation lint/link CI is the preliminary foundation, not runtime CI.

Acceptance:

- Config validation explains invalid limits, unknown fields, and unsupported schema versions.
- Canonical plan identity has stable fixture examples.
- User workflow and fault cases map to test cases.
- CI exercises supported runner OSes without downloading real models.

## M1: Native transfer feasibility

Timebox a technical spike before building the application around an unproven client.

Evaluate the official Rust hub client and native Xet engine. Record exact versions, licenses, features, transitive dependencies, TLS behavior, release targets, and peak cache use.

Demonstrate listing, revision pinning, normal Git files, LFS, Xet, expired URL refresh, cancellation, and resume. Compare one opt-in live plan with reference hub behavior using development tooling only.

Acceptance:

- An ordinary native binary works on Windows, macOS, and Linux without Python or external downloaders.
- Interrupted downloads resume without mixing revisions or duplicating the entire payload.
- Workspace use and bandwidth overrun bounds are measured.
- A rejected client is replaced or adapted in Rust; a Python runtime is not a fallback.

Output is a recorded decision and fixture-backed adapter prototype. Keep this spike isolated from the future catalog.

## M2: Local vault and inspectable plans

Implement init, doctor, volume add/list, watch add/list/remove, plan, decide, and status.

Deliver local SQLite schema, kernel-backed writer ownership, versioned config, volume identity, source resolution, license evidence capture, and dependency-aware file selection.

Acceptance:

- Empty setup performs no source scan or download.
- A new user produces a plan for one explicit repository.
- Plans show included/excluded files, costs, evidence, blockers, and placement.
- Changes to approved identity or policy require another decision.
- Unknown license text and incomplete inventories cannot auto-approve.
- Status and doctor without source checks operate offline.

## M3: Seal and recover one revision

Implement staged transfer, exact hash interpretation, all-file SHA-256, manifest publication, reservation reconciliation, and interrupted-work inspection.

Acceptance:

- All crash points around reservation, file completion, manifest, rename, and catalog commit recover idempotently.
- Corruption, truncated bodies, ignored Range, source changes, and disk exhaustion never produce a seal.
- Concurrent workers cannot duplicate accounting or writes.
- A complete folder remains readable without the program.
- A compatible runtime loads the selected test model offline with an empty external cache.

Use a tiny local HTTP fixture in CI. Live weight downloads remain an opt-in developer drill.

## M4: Watches and bounded unattended jobs

Implement watch changes, bounded automatic approval rules, period accounting, rate limits, bandwidth limits, retries, pauses, and local reports.

Acceptance:

- Repeated runs are idempotent.
- Upstream updates create new plans and preserve older revisions.
- Source deletion leaves the local archive untouched.
- Partial scans and access failures are reported distinctly.
- Reservations survive reboot and period rollover without duplication.
- No watch expands outside its approval bounds.

## M5: Replicas, verification, and restoration

Implement full and quick checks, resumable verification sweeps, replica/export, import, repair, and catalog backup/reconstruction.

Acceptance:

- A corrupted file is found by a full sweep.
- Replica restoration succeeds with the hub unreachable.
- After simulated catalog loss, manifests rebuild sealed inventory and replica locations.
- Conflicting manifests and local-baseline imports retain accurate provenance.
- Interrupted replica writes never count as healthy.
- Backup restore does not silently transfer credentials or approval authority.

This milestone precedes discovery because preservation without recovery is an incomplete product.

## M6: Volumes and platform qualification

Exercise removable drives, large files, changing drive letters, missing mounts, read-only media, permission failures, and network-share interruptions.

Acceptance:

- Missing volume identity never redirects writes onto an unintended disk.
- A full volume accepts no new job exceeding available space.
- One complete bundle remains on one volume.
- NTFS, APFS, and ext4 drills pass; SMB/NFS payload support is tested with one writer.
- Unsupported filesystems, case collisions, and path escapes fail before payload writes.
- HDD transfer behavior is measured and bounded.

Define exactly which NAS CPU/OS combinations can run released binaries. "Linux" does not imply every appliance is supported.

## M7: Version 1 usability and distribution

Package Windows x64, macOS ARM64/x64, and Linux x64/ARM64 binaries. Linux libc baseline and any musl variant are explicit. Platform artifacts are smoke-tested on their architecture before being labeled supported.

Implement schedule preview/install/remove for Task Scheduler, launchd, and systemd timers. Quote paths and preserve the configured identity. Installation is user-requested; scans do not silently install jobs.

Acceptance:

- Clean-machine setup requires only the native binary and OS facilities.
- New-user setup achieves the product's ten-minute planning target.
- Upgrade preserves bundles and tests catalog migrations from every supported prior schema.
- Releases include checksums, dependency/license inventory, reproducible build instructions, and verified artifact provenance.
- Lint, tests, coverage, and release CI pass.
- A manual offline recovery drill is recorded on each OS.
- No telemetry, background update check, or account is required.

## M8: Collections and optional discovery

Deliver inspectable collection data, previewed imports, paginated discovery, deterministic worry ranking, and lineage-aware duplicate handling.

Acceptance: discovery cannot bypass the planner, rights gate, approval bounds, or dependency requirements. Explicitly selected variants remain preservable.

An optional local ranker comes only after deterministic selection is useful. No mandatory server or model dependency is introduced.

## M9: Optional interfaces

Add stdio MCP through the stable application layer. Queued work persists independently of chat sessions. Test malicious metadata and insufficient authorization.

A local web UI is optional and consumes the same operations. Remote HTTP control requires a separate authentication, origin, TLS, and deployment design; changing a bind address is insufficient.

## M10: Broader preservation

Evaluate another model source, selected datasets, software releases, and platform-specific runtime kits independently.

Each adapter must establish immutable identity, rights evidence, complete file inventory, bounded transfer, manifests, recovery, and verification. Ship one type at a time. The first release never waits for a universal archive abstraction.

## Deferred optimizations

Cross-revision deduplication, delta reconstruction, object storage, peer distribution, signatures for shared collections, and retention automation require separate decisions and acceptance tests.

Never sacrifice self-contained folders, recoverability, or user control to optimize throughput.

## Immediate implementation order

1. Finalize representative plan/manifest fixtures and supported model layouts.
2. Initialize the Rust quality foundation.
3. Complete the native transfer spike.
4. Deliver the explicit-watch-to-seal path.
5. Break that path deliberately and prove recovery.
6. Add replicas and native scheduling before discovery.

Dependencies, risks, and acceptance evidence are kept beside each milestone. A failed gate delays that milestone rather than being renamed success.
