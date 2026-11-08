# Roadmap

Implementation follows evidence and dependencies, not calendar promises. Native Rust on Linux, macOS, and Windows is a requirement.

Current progress: M0 has strict contracts, canonical previews, and lint/test/coverage gates. The prototype CLI now prints terminal text with one next step, explains unchecked and partial local coverage, and adopts one frozen file set when optional files are absent. M1 has pinned public-source inspection and a streaming segment experiment with checkpoint/range/rollback fault tests and verified uncommitted-tail recovery. An exclusive SQLite staging-session experiment now journals checkpoints, charges each segment allowance before the request, and reconciles restart tails without refunding that charge. Its checkpoint database and rollback journal are each capped at 1 MiB. A fixture refreshes one rejected payload location by requesting the original URL again, without requesting or storing that location again even when the refresh names it, and without changing transfer-file identity. A plaintext loopback fixture measured that the request and response bytes for that refreshed segment exceed the charged allowance. A read-only `readiness` command reports capability closure for one safetensors text profile and does not record a drill pass. Offline `plan --select`, `budget`, and `kit` can preview an explicitly named GGUF derivative and compare pinned sizes with the storage ceiling. They do not reserve bytes, approve a transfer, or complete M8. TLS buffers, kernel socket buffers, multi-file workspace, cross-platform qualification, and vault integration remain open. M2 has bundled SQLite, transactional schema migration, writer ownership, stable volume identities, recoverable committed initialization, offline status, durable source reviews, and final rejection records. Installer scripts have offline fixtures. No milestone is fully qualified: granted approvals, integrated payload jobs, manifests, full crash recovery, and complete platform drills remain outstanding. Remaining work follows the waves below. Milestone numbers match that order: restoring the sealed revision is M4, and unattended watches are M5. See [implementation-status.md](implementation-status.md), [contracts.md](contracts.md), and [transfer-spike.md](transfer-spike.md).

## Work waves

Do these in order. A later wave does not start because an earlier one is inconvenient. Initialization, inspection, adopt, plan, budget, kit, and readiness stay available while the waves are open. The transport constraints and the 7 November 2026 source check are in [transfer-spike.md](transfer-spike.md).

| Wave | Closes | Do | Why it is next |
| --- | --- | --- | --- |
| 1 | Honest previews | Done. A successful `selected-gguf-v1` selection carries root license and notice files. `examples/budget.json`, `examples/recipe.json`, and `examples/readiness` are what getting-started runs. | The frozen set is what a later seal copies. A GGUF plan that drops license text would store a derivative the rights gate does not allow. |
| 2 | One-file ceiling | Measure TLS buffers and kernel socket buffers and record the figures. Keep `workspace_bytes` null until those figures exist. Keep rejecting a ranged response that is not 206. | The charged segment allowance is not a bandwidth ceiling. A plaintext fixture already counted more request and response bytes than that charge. Approval cannot depend on an unmeasured number. |
| 3 | One revision, staged | Stage every file of one frozen set, including license and notice, under the same checkpoint rules as the single-file experiment. No seal and no fetch command. | One successful file does not prove a revision. The set is the set adopt already freezes. |
| 4 | One bound approval | Record a grant that names that plan's identity, measured bounds, volume UUID, and policy digest, and drop the grant when any of those change. | A grant that survives a changed file set can store bytes nobody reviewed. |
| 5 | M3, developer preview | Reservations around the [staging journal](staging-journal.md), all-file SHA-256, manifest, atomic publication, and crash recovery at each boundary. The folder is readable without the program. Then run the network-disabled empty-cache drill (issue 29) on that sealed test model. | Publishing the folder is the preview. The drill needs a seal to demonstrate, so it waits until this wave. |
| 6 | M4, offline restore | Replica and catalog rebuild of that one revision with the hub unreachable. Local-baseline import stays in this wave. | A cache directory or a movable tag is not a second copy. Further revisions wait until this one can be rebuilt. |
| 7 | M5, watches | Bounded watches: a new commit makes a new plan, older revisions stay, a repeated run is idempotent, and nothing fetches outside the grant. | This is the first time the vault multiplies revisions. |
| 8 | M6, disks | Removable-volume and filesystem drills. One bundle stays on one volume. A missing disk never receives the write. | M2 already refuses a missing or substituted disk. This wave proves that on NTFS, APFS, ext4, a full volume, and one network share. The beta claim waits for those drills. |
| 9 | M7, version 1 | Native packages, and OS schedulers installed only after a preview. | Distribution follows a recovery drill on each OS. |

One explicit fetch of one approved plan arrives in wave 5, when identity, measured bounds, and publication hold together. A scheduled or repeated fetch arrives in wave 7, after wave 6. There is no fetch command before wave 5.

The first fetched revision is a small public safetensors file over ranged HTTPS. Files the Hub bridge cannot return as one URL, including a full GET above the limit recorded in the transfer spike, are outside waves 2 through 5.

These are not waves, and a 7 November 2026 check of hub storage, local model managers, and archival practice did not move them forward:

- A native Xet client, including wrapping `hf-xet`, and adopting `hf-hub` 1.0.0 as the downloader. HTTPS through the LFS bridge still returns Xet-stored bytes. Wrap `hf-xet` only after wave 6, and only for a file the bridge cannot serve. Do not write a second Xet client.
- Cross-revision deduplication, a torrent or peer client, and a second hub. Those are locators for a revision that already has a hash. M10 and the deferred list below still own them.
- Collections, discovery, a local web UI, and MCP. They stay M8 and M9.
- The live Omarchy session checks in issue 19. The chip is already on main. It is not installed, and that session is not authorized.
- Vendor a runtime into the seal. Wave 5 records a runtime as named or retained with its own rights. Issue 29's drill uses a runtime; it does not bundle one.

## Release boundaries

| Release | Promise | Required milestones |
| --- | --- | --- |
| Developer preview | Resolve, plan, and preserve one exact model reliably. | M0 through M3. |
| Personal vault beta | Restore the sealed revision without the hub, run bounded watches, and qualify the disks those jobs write to. | M4 through M6. |
| Version 1 | A new user can install, schedule, maintain, and recover the utility on all three OSes. | M7 plus all earlier gates. |
| Later | Collections, discovery, agent access, broader artifacts. | M8 onward after version 1 stability. |

No release claims readiness solely because it can download a file.

## Quality of life across user types

The [user experience plan](user-experience.md) defines first-time, power-user, archivist, and independent-custody workflows, with milestone mapping and acceptance drills. Essential setup, explainable plans, actionable recovery, stable automation, and offline ownership are release requirements. Optional dashboards and discovery follow a reliable preservation core.

## M0: Contracts and quality foundation

Deliver versioned config, plan, manifest, job-state, reason-code, and CLI-output specifications. Agree on default rights and file profiles. Define supported architectures/layouts.

Maintain the Rust workspace, pinned toolchain, lockfile, format/lint/test checks, and at least 80% measured line and branch coverage gates. The single CI workflow checks native Rust and documentation on Windows, macOS, and Linux, with separate coverage and dependency checks feeding one aggregate result.

Acceptance:

- Config validation explains invalid limits, unknown fields, and unsupported schema versions.
- Canonical plan identity has stable fixture examples.
- User workflow and fault cases map to test cases.
- CI exercises supported runner OSes without downloading real models.

## M1: Native transfer feasibility

Timebox a technical spike before building the application around an unproven client.

Evaluate the official Rust hub client and native Xet engine. Record exact versions, licenses, features, transitive dependencies, TLS behavior, release targets, and peak cache use.

Demonstrate listing, revision pinning, normal Git files, LFS pointers, expired URL refresh, cancellation, and resume for ranged HTTPS. The first seal uses that path, including when the bytes are stored in Xet and the LFS bridge returns them. A native Xet client is not required to finish M1. Compare one opt-in live plan with reference hub behavior using development tooling only, after the fixture waves, on one small public safetensors file.

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
- A watch entry records intent. It does not resolve, fetch, or approve. Fetching a watch is M5.

## M3: Seal and recover one revision

Implement staged transfer, exact hash interpretation, all-file SHA-256, manifest publication, reservation reconciliation, and interrupted-work inspection.

Acceptance:

- All crash points around reservation, file completion, manifest, rename, and catalog commit recover idempotently.
- Corruption, truncated bodies, ignored Range, source changes, and disk exhaustion never produce a seal.
- Concurrent workers cannot duplicate accounting or writes.
- A complete folder remains readable without the program.
- A compatible runtime loads the selected test model offline with an empty external cache.

Use a tiny local HTTP fixture in CI. Live weight downloads remain an opt-in developer drill.

## M4: Replicas, verification, and restoration

Implement full and quick checks, resumable verification sweeps, replica/export, import, repair, and catalog backup/reconstruction for the revision M3 sealed.

Upstream-verified adoption of a local folder that matches a resolved commit is available before this milestone. Local-baseline import, which records a copy without that upstream proof, stays here.

Acceptance:

- A corrupted file is found by a full sweep.
- Replica restoration succeeds with the hub unreachable.
- After simulated catalog loss, manifests rebuild sealed inventory and replica locations.
- Conflicting manifests and local-baseline imports retain accurate provenance.
- Interrupted replica writes never count as healthy.
- Backup restore does not silently transfer credentials or approval authority.

This milestone precedes unattended watches and discovery. Preservation without a way to rebuild the sealed revision is an incomplete product. A cache, a branch name, or a second URL is not this milestone.

## M5: Watches and bounded unattended jobs

Start this milestone after M4 has restored the sealed revision with the hub unreachable.

Implement watch changes, bounded automatic approval rules, period accounting, rate limits, bandwidth limits, retries, pauses, and local reports. A watch entry that only records intent is M2 and still does not fetch.

Acceptance:

- Repeated runs are idempotent.
- Upstream updates create new plans and preserve older revisions.
- Source deletion leaves the local archive untouched.
- Partial scans and access failures are reported distinctly.
- Reservations survive reboot and period rollover without duplication.
- No watch expands outside its approval bounds.

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

Implement the [archival policy contract](archive-policy.md): original-required/preferred/selected-derivative modes, category coverage and byte budgets, attributed evaluation snapshots, stable portfolio selection, and capacity deferral. Unsupported modalities stay visible until their adapters establish complete dependencies. Source or ranking changes never silently replace or prune existing copies. The offline kit report lists slots and pinned sizes. It does not select a portfolio, reserve bytes, or satisfy this milestone.

Acceptance: discovery cannot bypass the planner, rights gate, approval bounds, or dependency requirements. Explicitly selected variants remain preservable.

An optional local ranker comes only after deterministic selection is useful. No mandatory server or model dependency is introduced.

## M9: Optional interfaces

Add stdio MCP through the stable application layer. Queued work persists independently of chat sessions. Test malicious metadata and insufficient authorization.

A local web UI is optional and consumes the same operations. Remote HTTP control requires a separate authentication, origin, TLS, and deployment design; changing a bind address is insufficient.

## M10: Broader preservation

Use the [model source catalog](model-sources.md) and its data-only registry to prioritize second-hub, official-release, NGC/Kaggle, modality-specific, and mirror-recovery investigations. Candidate entries do not enable network access or imply qualified adapters.

Evaluate another model source, selected datasets, software releases, and platform-specific runtime kits independently.

Each adapter must establish immutable identity, rights evidence, complete file inventory, bounded transfer, manifests, recovery, and verification. Ship one type at a time. The first release never waits for a universal archive abstraction.

## Deferred optimizations

Cross-revision deduplication, delta reconstruction, object storage, peer distribution, signatures for shared collections, and retention automation require separate decisions and acceptance tests.

Never sacrifice self-contained folders, recoverability, or user control to optimize throughput.

Dependencies, risks, and acceptance evidence are kept beside each milestone. A failed gate delays that milestone rather than being renamed success.
