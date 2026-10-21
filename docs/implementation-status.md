# Implementation status

This records executable behavior and evidence as of October 21, 2026. It does not mark the preservation roadmap complete.

## Executable features

| Operation | Current behavior | Remaining boundary |
| --- | --- | --- |
| config init/check | Exclusive config creation and strict validation. | No automatic configuration migration. |
| init | Bundled local SQLite, writer lock, stable vault/disk UUIDs, empty-directory adoption. | Full power-loss recovery and filesystem qualification remain open. |
| status | Offline catalog and mounted identity check, actual free bytes, and preserved local copies. | No downloaded bundles, jobs, reservations, replicas, or relocation yet. |
| resolve | Moving ref followed by pinned metadata; verified license/config/index bytes. GGUF-only repos report layout `gguf-v1`. | Public ungated models only; no payload download or approval. |
| verify-local / adopt | Read-only hash against a resolved commit, then copy or move the frozen matched set. Absent optional files are exclusions. Provenance is `upstream-verified (local source)`. | Not a download and not a local-baseline import. License evidence must already match. |
| local list | Read-only Ollama manifest inventory. Coverage is unchecked, none, partial, or fully recorded. | A catalog match is historical byte identity, not a fresh hash or an offline run. Does not import blobs or call the Ollama API. |
| propose / proposals / decide | Durable evidence/volume binding, paginated offline inspection, idempotent rejection, schema-1 migration. | Approval is blocked until transfer bounds and accounting qualify. |
| plan | Canonical preview from local inventory, file selection and blockers. | Always untrusted, never persisted or authorized. |
| Integrity library | Bounded-memory Git blob/LFS verification plus local SHA-256. | Transfer writer and durable manifest integration are next. |
| Transfer library experiment | Identity-bound prefix checkpoints, strict byte ranges, segment rollback, full-file upstream verification, offline uncommitted-tail recovery, exclusive SQLite checkpoint sessions. Each segment allowance is charged before the request and kept when a tail is discarded. The checkpoint database and its rollback journal are each capped at 1 MiB. A redirected 401 or 403 is refreshed once from the original URL; the rejected location is not retried or stored. A plaintext loopback fixture counts the request and response bytes for that attempt. | That cap is not `workspace_bytes`. Those plaintext counts are not TLS buffers or kernel socket buffers. Approval, vault reservations, and publication remain open. |

No command executes repository code. No automatic scan, account service, telemetry, deletion, or conversion exists.

An optional Omarchy presence chip in [plugins/omarchy-presence](../plugins/omarchy-presence/README.md) projects `status` into disk presence, free space, and historical custody rows. It is not installed, not published, and not part of the CLI. `qmllint`, click, Escape, disable, restart, and removal have not been run.

## Observed evidence

- Native Windows unit, CLI, contract, HTTP, integrity, and vault tests pass. Additional [validation drills](validation-drills.md) exercise abrupt process termination, the live NTFS test vault, and durable pinned-source decisions.
- HTTP fixtures exercise bounded reads, interruptions, redirect trust, signed-URL redaction, bad status/encoding, access restrictions, source changes, and metadata disagreement.
- Rights fixtures reject missing, changed, conflicting, and excessive evidence. Shard fixtures require exact index closure over selected weights.
- Vault fixtures exercise competing writer ownership, stable IDs, unrelated data rejection, all-volume preflight, interrupted marker publication, missing/substituted disks, schema/configuration drift, and Windows junctions.
- Source-review fixtures cover restart persistence, policy changes, exact decisions, offline disks, corruption, pagination, and identity-preserving migration. Native installer fixtures cover verification, version mismatch, archive path rejection, retained configuration, and upgrades.
- A Windows release builds with bundled SQLite and Rust TLS. Import inspection finds only OS DLLs.
- Offline fixtures for the optional Omarchy presence projection cover mounted, offline, missing, and replaced disks, writer contention, invalid and malformed status, missing executables, hostile paths, timeouts, stale snapshots, a changed config, a capped read that still opens a waiting inspection, and an inspection transcript that does not replace the snapshot. They check the launched arguments. They do not run `qmllint` or an Omarchy session.
- Live source inspection pins SmolLM2-135M-Instruct at 12fd25f77366fa6b3b4b768ec3050bf629380bac. Configuration bytes verify; missing license text remains unreviewed despite the Apache tag. No weights were fetched.
- A second release inspection pins Qwen2.5-0.5B-Instruct at 7ae557604adf67be50417f59c2c2f167def9a775. License/config/tokenizer configuration bytes verify, and the Apache template with its filled appendix copyright matches. Transfer authorization remains false; no weights were fetched.

Local Windows validation passes 108 Rust tests and 28 documentation-tool tests. Formatting and Clippy with warnings denied pass on Rust 1.99.1. Markdown lint and local links pass. The documentation link library measures 100% line and branch coverage. The CI coverage gate requires at least 80% of owned Rust lines and a real branch measurement; a new local percentage is not claimed until that job measures this tree. CLI captures are regenerated from the release executable. Dependency audit and license checks run in CI against the current lockfile. Offline installer fixtures run from the release binary. A release smoke test creates an empty vault and reports its mounted disk without contacting a publisher. Current artifact sizes are observations, not distribution guarantees.

Live source state can change. These checks are observations, not permanently pinned model recommendations. Use resolve to inspect a current source and retain the exact commit/evidence before later approval.

The single [CI workflow](https://github.com/blisspixel/ModelPrepper/actions/workflows/ci.yml) runs native Linux/macOS/Windows checks, coverage, documentation/capture checks, and dependency audits. Its aggregate Main checks job requires all jobs to succeed. Consult the actual main-branch run for hosted status; local results do not substitute for a hosted run. Native fixture/build results still do not qualify clean-machine, large-volume, power-loss, or offline model-loading behavior.

The [durable staging journal](staging-journal.md) adds exclusive single-file checkpoint sessions and crash-boundary tests. The single CI workflow exercises these tests on Windows, macOS, and Linux. Verify the main-branch run against the exact commit being used; a previous successful run does not validate newer changes.

## Next delivery gates

1. The single-file experiment already rejects ignored Range, changed identity, corrupt partials, truncated bodies, and exhausted segment allowances, and it resumes after process termination. It records the charged allowance and a 2 MiB side-file cap. A redirected payload location that returns 401 or 403 is requested again from the original URL once; that location is not repeated, stored, or allowed to replace the pinned identity, and the segment is charged once. A plaintext loopback fixture measured that the bytes read and written for that refreshed attempt exceed the charged allowance, so the charge is not a bandwidth ceiling. Still required before approval code depends on a workspace number: measure TLS buffers and kernel socket buffers, and cover every selected file. Preview `workspace_bytes` stays null.
2. Extend persisted source reviews and transactional migration into qualified transfer plans with measured resource bounds and granted approvals. Approval must bind that identity and expire on relevant change; original evidence, inventory, exclusions, volume UUID, and policy digest are already retained in source reviews.
3. Reservations, full-file verification, durable manifest, atomic publication, and reconciliation after every catalog/filesystem crash point. A renamed directory alone cannot count as sealed.
4. Replica copying and manifest-based reconstruction before unattended discovery. Prove recovery with the hub unreachable and the original catalog removed.
5. Actual native filesystem and offline-load drills on supported OSes, then installation/scheduling documentation and release packaging.

Initialized disks and successful inspection are useful foundations. They do not establish that a model has been preserved or can run offline.
