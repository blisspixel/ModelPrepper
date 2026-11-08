# Roadmap

Implementation follows evidence and dependencies, not calendar promises. Native Rust on Linux, macOS, and Windows is a requirement.

Current progress: M0 has strict contracts, canonical previews, and lint/test/coverage gates. The prototype CLI now prints terminal text with one next step, explains unchecked and partial local coverage, and adopts one frozen file set when optional files are absent. M1 has pinned public-source inspection and a streaming segment experiment with checkpoint/range/rollback fault tests and verified uncommitted-tail recovery. An exclusive SQLite staging-session experiment now journals checkpoints, charges each segment allowance before the request, and reconciles restart tails without refunding that charge. Its checkpoint database and rollback journal are each capped at 1 MiB. A fixture refreshes one rejected payload location by requesting the original URL again, without requesting or storing that location again even when the refresh names it, and without changing transfer-file identity. A plaintext loopback fixture measured that the request and response bytes for that refreshed segment exceed the charged allowance. A read-only `readiness` command reports capability closure for one safetensors text profile and does not record a drill pass. Offline `plan --select`, `budget`, and `kit` can preview an explicitly named GGUF derivative and compare pinned sizes with the storage ceiling. They do not reserve bytes, approve a transfer, or complete M8. TLS input and output buffers, the rustls plaintext and send caps, and kernel socket buffers are recorded. `workspace_bytes` stays null. A grant can name one review's measured bounds, volume UUID, and policy digest, and it is dropped when those change. It does not authorize a transfer. `fetch` publishes that granted revision with a bundle manifest. `replica` copies it to another volume, `rebuild` restores the catalog from those bundles with the hub unreachable, and `import` records a local baseline without upstream proof. Status names the filesystem. A network catalog, FAT or exFAT, an unknown type, and a read-only mount are refused before a write. A live network share and HDD timing remain unmeasured. `schedule` can preview a weekly timer and install it only when the digest matches. CI does not enable a live timer. `collection preview` lists explicit entries from a shared file and does not resolve or download them. An entry may name a representation, kit slots, and a declared base. A collection may name a category ceiling. A repeated declared base is listed and not merged. The preview records those choices, an evaluation attribution, an evidence statement, and a provenance statement. It does not charge a ceiling, merge a base, preserve a base, rank by the attribution, check the evidence statement, or verify the provenance statement. `collection store` writes that list into the catalog and `collection show` reads it back. Storing does not apply those choices or delete archived revisions. `collection list` pages the stored names in name order and does not rank them. `collection remove` deletes that stored name and does not delete archived revisions. `collection custody` counts catalog rows with the same repository and revision text and does not hash or download them. A 40-character lowercase hex revision is labeled `commit`. Any other accepted revision is labeled `name`. The label is not a resolution. When every matching manifest records `size_bytes` and those totals agree, the report shows that sum. A missing manifest or disagreeing totals stay unmeasured. `collection custody` charges a written category ceiling only when every entry that names it has `measured_bytes`. The same measured total can count toward each category that entry names. An unmeasured entry is left out, a partial sum is not reported, and no size is invented. Preview, store, and show still leave `applied` false. `discover` searches the reviewed source catalog for an explicit role, status, or source id. Matches stay in catalog order. It does not rank them, contact a network, or approve a transfer. The `[discovery] enabled = false` sketch is not a command. `collection select` keeps stored entries whose category, representation, or evaluation text matches exactly. Stored order is kept. `evaluation_value` is not a score, and `ranked` stays false. `verify-sealed` hashes one custody plan with the existing file hash. Other plans are not read. `collection custody` does not become a rehash. Nothing is downloaded. `collection export` writes that stored list to a new schema 1 file and does not change the catalog. `collection diff` compares one schema 1 file with the stored list and does not change either. Clean-machine packaging drills remain open. M2 has bundled SQLite, transactional schema migration, writer ownership, stable volume identities, recoverable committed initialization, offline status, durable source reviews, and final rejection records. Installer scripts have offline fixtures. No milestone is fully qualified: transfer authorization, integrated payload jobs, manifests, full crash recovery, and complete platform drills remain outstanding. Remaining work follows the waves below. Milestone numbers match that order: restoring the sealed revision is M4. `watch` is M5: one pass creates a new plan for a new commit, keeps older revisions, and fetches only inside a current grant and the UTC-week byte ceiling. Scheduling is still M7. See [implementation-status.md](implementation-status.md), [contracts.md](contracts.md), and [transfer-spike.md](transfer-spike.md). `collection custody` lists the matching plan ids in plan id order. A replica row is not listed. Nothing is dropped or merged.

## Work waves

Do these in order. A later wave does not start because an earlier one is inconvenient. Initialization, inspection, adopt, plan, budget, kit, and readiness stay available while the waves are open. The transport constraints and the 7 November 2026 source check are in [transfer-spike.md](transfer-spike.md).

| Wave | Closes | Do | Why it is next |
| --- | --- | --- | --- |
| 1 | Honest previews | Done. A successful `selected-gguf-v1` selection carries root license and notice files. `examples/budget.json`, `examples/recipe.json`, and `examples/readiness` are what getting-started runs. | The frozen set is what a later seal copies. A GGUF plan that drops license text would store a derivative the rights gate does not allow. |
| 2 | One-file ceiling | Done. The agent records 128 KiB TLS input and output buffers. rustls 0.23.46 contributes 16 KiB of received plaintext and 64 KiB of outgoing buffer. The segment reader copies at most 64 KiB. A loopback socket measures kernel receive and send buffers. A ranged GET sends no `Accept-Encoding`, and a ranged response that is not 206 is not appended. A full GET above 50 GB stays outside this path. `workspace_bytes` stays null. | The charged segment allowance is not a bandwidth ceiling. Approval cannot depend on an unmeasured number, and the measured figures still do not cover every selected file. |
| 3 | One revision, staged | Done. One frozen set is staged as one checkpoint session per file, including license and notice. Part files stay part files. There is no seal and no fetch command. | One successful file does not prove a revision. The set is the set adopt already freezes. |
| 4 | One bound approval | Done. One grant names that plan, the measured connection bounds, the per-file staging bound, the volume UUID, and the policy digest. The grant is dropped when any of those change. `decide` still cannot approve a transfer, and there is no fetch command. | A grant that survives a changed file set can store bytes nobody reviewed. |
| 5 | M3, developer preview | Done. `fetch` stages one granted revision, reserves the volume path, hashes every selected file, writes `modelprepper-bundle.json`, and records custody after the rename. A directory without that file is not a seal. Stops before the rename and before the catalog record retry cleanly. `decide` stays blocked, `transfer_authorized` stays false, and `workspace_bytes` stays null. Readiness on the fixture reports `untested`, not a load. | The sealed folder is what a later restore copies. A drill that claims a runtime load would invent a result this wave does not run. |
| 6 | M4, offline restore | Done. `replica` copies one sealed revision onto another mounted volume and records it only after a full hash. An interrupted copy is not healthy. `rebuild` reads `modelprepper-bundle.json` back into custody and replica rows with no network, including after the catalog file is removed, and does not recreate grants or approvals. A bundle with no provenance stays `sealed bundle (provenance unrecorded)`. `import` records `local-baseline` and does not relabel matching bytes. A second folder on the sealing volume is refused. Failure domains stay unlabeled. | A cache directory or a movable tag is not a second copy. Further revisions wait until this one can be rebuilt. |
| 7 | M5, watches | Done. `watch` runs each configured watch once. A new commit is a new plan, older revisions stay, and a repeated run does not fetch again. A fetch happens only inside a current grant and `max_transfer_bytes_per_period`. Approval stays manual. The ceiling is that byte cap, not a measured rate. An interrupted seal resumes from its reservation and is charged once. | This is the first time the vault multiplies revisions. The OS timer is wave 9. |
| 8 | M6, disks | Done. Status names the filesystem. Catalog and checkpoint writes refuse a network share, FAT or exFAT, an unknown type, and a read-only mount before creating files. Payload writes allow the same local types plus NFS or SMB. | CI checks that the runner temporary directory is ext4, APFS, or NTFS. btrfs, xfs, tmpfs, and zfs stay usable. A live share was not mounted. HDD timing was not measured. A full volume still returns `insufficient_space`. A missing disk still receives no later write. |
| 9 | M7, version 1 | Done. `schedule preview` shows the weekly timer that runs one `watch` pass. `schedule install` writes it and enables that timer only when the preview digest matches. `schedule remove` deletes that timer only. systemd, launchd, and Task Scheduler text are tested. | CI injects the host runner and does not enable a live timer. Packages, clean-machine setup, and a recovery drill on each OS remain open. The timer does not approve a transfer. |
| 10 | M8, explicit collection | Done. `collection preview` lists the entries in a shared TOML file. A hub URL is normalized. The revision text is checked and not resolved. An unknown schema is reported and the file is left unchanged. | Nothing is downloaded, stored in the catalog, or reserved. Discovery and ranking stay open. |
| 11 | M8, representation name | Done. A collection entry may name `original-required`, `original-preferred`, or `selected-derivative`. An absent name stays null. Any other value is refused. | The preview does not defer a candidate, substitute a derivative, or reserve bytes. Discovery and ranking stay open. |
| 12 | M8, category ceiling | Done. A collection may name one byte ceiling for each kit slot. `bytes` may be zero. A repeated category is refused. | The preview records the number and does not charge entries, reserve bytes, or assign an entry to a slot. Discovery and ranking stay open. |
| 13 | M8, declared base | Done. An entry may name `base_repo` and `base_revision`. A hub URL is normalized. The revision text is checked and not resolved. A missing repository, or an entry naming itself, is refused. | `base_preserved` stays false. The preview does not fetch the base or treat the entry as proof that the base is preserved. Discovery and ranking stay open. |
| 14 | M8, repeated base | Done. Two or more entries that name the same established base are listed once with that entry count. A missing revision is not grouped. | `deduplicated` stays false. The preview does not drop an entry or claim a byte saving. Discovery and ranking stay open. |
| 15 | M8, category names | Done. An entry may name one or more kit slots. A repeated name is refused. An absent list stays empty. | The preview records the names and does not charge a ceiling or reserve bytes. Discovery and ranking stay open. |
| 16 | M8, evaluation attribution | Done. An entry may name `evaluation_source`, `evaluation_name`, and `evaluation_value` together. A partial attribution is refused. | `ranked` stays false. The text is not a score, and the preview does not order entries by it. Discovery and ranking stay open. |
| 17 | M8, evidence statement | Done. An entry may say `evidence` is `present` or `missing`. An absent statement stays null. | `evidence_checked` stays false. The preview does not open files to confirm the statement. Discovery and ranking stay open. |
| 18 | M8, provenance statement | Done. An entry may name `provenance` as conversion or training text. An absent statement stays null. | `provenance_verified` stays false. The preview does not check the statement. Discovery and ranking stay open. |
| 19 | M8, stored list | Done. `collection store` writes a schema 1 list into the catalog. `collection show` reads that name back. An unknown schema is not stored. The same name replaces only that list. | Catalog schema 7 holds the rows. A failed write leaves the previous list. Custody and archived revisions stay. The stored fields are still not applied. Discovery and ranking stay open. |
| 20 | M8, stored names | Done. `collection list` pages stored collection names in name order. `--limit` is 1 to 100. `--after` continues after the last name on the page. | `ranked` stays false. The page does not order by evaluation text, entry count, or replica. It does not download. Discovery stays open. |
| 21 | M8, remove one list | Done. `collection remove` deletes one stored name. The report is the list that was deleted. | Custody and archived revisions stay. `replaced` stays false. The command does not download. Discovery and ranking stay open. |
| 22 | M8, custody text match | Done. `collection custody` counts custody rows with the same repository and revision text. Distinct provenance strings are listed in plan id order. | A branch does not match a commit. Files are not hashed. `resolved` stays false. Discovery and ranking stay open. |
| 23 | M8, export one list | Done. `collection export` writes one stored list to a new schema 1 file. Optional fields that were absent stay absent. | The catalog is not changed. An existing output path is left unchanged. The file is not resolved or downloaded. Discovery and ranking stay open. |
| 24 | M8, compare one list | Done. `collection diff` compares one schema 1 file with the stored list of the same name. Comments are ignored. `matches` is true only when the parsed fields are equal. | The catalog and the file are not changed. An unknown schema does not open the catalog. Discovery and ranking stay open. |
| 25 | M8, name revision shape | Done. `collection custody` labels a 40-character lowercase hex revision `commit` and any other accepted revision `name`. | The label is not a resolution and not a hash. A branch such as `main` stays `name` and `custody not recorded`. Discovery and ranking stay open. |
| 26 | M8, report sealed bytes | Done. When every matching custody manifest records `size_bytes` and those totals agree, `measured_bytes` is that sum. | A missing manifest or disagreeing totals stay unmeasured. The ceiling is not charged. No size is invented. Discovery and ranking stay open. |
| 27 | M8, charge a measured ceiling | Done. `collection custody` charges a written category ceiling only when every entry that names it has `measured_bytes`. | An unmeasured entry is left out. A partial sum is not reported. No size is invented. The catalog is not changed. Discovery and ranking stay open. |
| 28 | M8, search explicit constraints | Done. `discover` searches the reviewed source catalog for an explicit role, status, or source id. | Matches stay in catalog order. The command does not rank, download, or approve. The `[discovery]` sketch is not a command. Ranking stays open. |
| 29 | M8, exact text selection | Done. `collection select` keeps stored entries whose category, representation, or evaluation text matches exactly. | Stored order is kept. `evaluation_value` is not a score. `ranked` stays false. The catalog is not changed. |
| 30 | M8, hash one plan | Done. `verify-sealed` hashes one custody plan with the existing file hash. | Other plans are not read. `collection custody` does not become a rehash. Nothing is downloaded. |
| 31 | M8, name matching plans | Done. `collection custody` lists the matching plan ids in plan id order. | A replica row is not listed. Nothing is dropped or merged. The catalog is not changed. |

## Still in this order

Do these after wave 31. Do not skip an item because a later one looks more useful. An item without a wave number is not a wave yet.

1. Resolving every entry in a collection is a network batch. One `resolve` already exists. Leave the batch alone.
2. Dropping or merging copies, capacity deferral, and stable portfolio selection still wait for an inspectable retirement plan. Naming plan ids does not drop a copy and does not complete the M8 acceptance list by itself.

These stay open and are not collection waves:

- Version 1 acceptance is still open: clean-machine setup, the ten-minute planning drill, catalog migration from every supported prior schema, published checksums and provenance, and a recovery drill on each OS. No release tag until those gates and transfer approval hold. `decide` still cannot approve. `workspace_bytes` stays null.
- Issue 29 stays open. `readiness` is on main. The network-disabled, empty-cache task and restore drill has not been run. Fixture evidence is not that drill.
- Issue 19 stays open. The presence chip is on main and is not installed. Live Omarchy session checks are not authorized.

One explicit fetch of one granted plan is `fetch`, added in wave 5, when identity, measured bounds, and publication hold together. `watch` repeats that fetch for configured revisions, one operator-invoked pass at a time, and only inside a grant that is already recorded. `schedule` installs that weekly timer only after a matching preview. There is no fetch command before wave 5.

The first fetched revision is a small public safetensors file over ranged HTTPS. Files the Hub bridge cannot return as one URL, including a full GET above the limit recorded in the transfer spike, are outside waves 2 through 5.

These are not waves, and a 7 November 2026 check of hub storage, local model managers, and archival practice did not move them forward:

- A native Xet client, including wrapping `hf-xet`, and adopting `hf-hub` 1.0.0 as the downloader. HTTPS through the LFS bridge still returns Xet-stored bytes. Wrap `hf-xet` only after wave 6, and only for a file the bridge cannot serve. Do not write a second Xet client.
- Cross-revision deduplication, a torrent or peer client, and a second hub. Those are locators for a revision that already has a hash. M10 and the deferred list below still own them.
- A hub model search, a local web UI, and MCP. They stay M8 and M9. A collection preview is wave 10. Wave 19 stores that list. Wave 20 lists the stored names and does not discover or rank them. Wave 21 removes one stored name and does not delete archived revisions. Wave 22 counts matching custody rows and does not hash or resolve them. Wave 23 writes one stored list back to schema 1 and does not change the catalog. Wave 24 compares one schema 1 file with the stored list and does not change either. Wave 25 labels a 40-character lowercase hex revision `commit` and any other accepted revision `name`, and does not resolve or hash it. Wave 26 reports an agreed manifest byte total and does not charge a ceiling. Wave 27 charges a category ceiling only when every entry that names it has `measured_bytes`, and does not charge an unmeasured entry. Wave 28 searches the reviewed source catalog with an explicit constraint and does not rank, download, or approve. Wave 29 selects stored entries by exact text and does not rank them. Wave 30 hashes one custody plan and does not rehash a collection. Wave 31 lists matching custody plan ids and does not drop or merge them.
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

`watch` is that fetching pass. It does not call `decide`, it does not set `transfer_authorized`, and `workspace_bytes` stays null. The period cap is `max_transfer_bytes_per_period` for the UTC week, not a measured bandwidth rate. A pause refuses the fetch when that ceiling would be exceeded. A retry is the existing fetch reservation. The operator invokes one pass. The weekly OS timer is `schedule`, and it is not part of this watch pass.

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

Wave 8 records the filesystem on `status` and refuses an unsupported or read-only filesystem before a catalog, checkpoint, or payload write. ext4, APFS, and NTFS are the baseline. btrfs, xfs, tmpfs, and zfs remain usable local filesystems, so a development machine and the temporary-directory suite are not blocked. NFS and SMB are payload-only in that policy. The allowance and the catalog refusal are tested with an injected type, because this tree did not mount a share. FAT and exFAT are refused. Case collisions and path escapes already fail in validation before a write. A full volume still returns `insufficient_space`. One bundle still stays on one volume, and a missing or substituted disk still receives no later initialization. HDD cache timing was not measured, so that acceptance line stays open. The personal-vault beta does not claim qualified disks.

Define exactly which NAS CPU/OS combinations can run released binaries. "Linux" does not imply every appliance is supported.

## M7: Version 1 usability and distribution

Package Windows x64, macOS ARM64/x64, and Linux x64/ARM64 binaries. Linux libc baseline and any musl variant are explicit. Platform artifacts are smoke-tested on their architecture before being labeled supported.

Implement schedule preview/install/remove for Task Scheduler, launchd, and systemd timers. Quote paths and preserve the configured identity. Installation is user-requested; scans do not silently install jobs.

`schedule preview` renders that weekly user timer. Its command is one `watch` pass with the absolute config path and the absolute stage path. `schedule install` writes the unit and enables that timer only when the operator passes the preview digest. `schedule remove` disables and deletes that timer only. Paths with spaces, quotes, and XML metacharacters are quoted in the unit. One timer name is used per user, so a second install replaces it. systemd and Task Scheduler fire Monday 00:05 UTC. launchd fires Monday 00:05 in the machine's local time, and the byte ceiling remains the UTC week. systemd `TimeoutStartSec` and the task execution limit are 12 hours. launchd sets no process limit. CI checks the rendered text and an injected host runner. It does not call `systemctl`, `launchctl`, or `schtasks`, and it does not enable a live timer. `decide` stays blocked, `transfer_authorized` stays false, and `workspace_bytes` stays null.

Clean-machine setup, the ten-minute planning drill, upgrade migration from every supported schema, published checksums and provenance, and a manual offline recovery drill on each OS remain open. The existing installer scripts still do not schedule a job.

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

`collection preview` lists schema 1 entries from a shared file. It normalizes a hub URL, checks the revision text, and refuses an unknown field or a repeated entry. It does not resolve the revision, contact a publisher, write the catalog, or reserve bytes. An entry may name `original-required`, `original-preferred`, or `selected-derivative`. The preview keeps an absent name null and does not defer, substitute, or rank. A collection may name one byte ceiling for each kit slot. The preview records that number and leaves `applied` false. An entry may name one or more of those slots. The preview records the names and does not charge the ceiling. An entry may name an evaluation attribution. The preview leaves `ranked` false and does not order entries by that text. An entry may state that evidence is present or missing. The preview leaves `evidence_checked` false and does not open files to confirm it. An entry may name a provenance statement. The preview leaves `provenance_verified` false and does not check it. An entry may name a declared base. The preview leaves `base_preserved` false and does not fetch that base. A repeated established base is listed and `deduplicated` stays false. A hub model search, ranking, and dropping or merging copies remain open. `discover` searches the reviewed source catalog and does not rank or download. `collection store` writes the parsed list into catalog schema 7 and `collection show` reads it back. Replacing a name does not delete archived revisions. The stored fields stay unapplied. `collection list` pages stored names in name order and leaves `ranked` false. `collection remove` deletes one stored name and does not delete archived revisions. `collection custody` counts exact custody matches, labels a 40-character lowercase hex revision `commit` and any other accepted revision `name`, and does not hash files. `collection export` writes one stored list back to schema 1 and does not change the catalog. `collection diff` compares one schema 1 file with the stored list and does not change either. These commands do not satisfy the acceptance list above. The remaining order is the still-in-this-order list above.

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
