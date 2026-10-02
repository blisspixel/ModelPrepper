# Implementation status

This records executable behavior and evidence as of October 1, 2026. It does not mark the preservation roadmap complete.

## Executable features

| Operation | Current behavior | Remaining boundary |
| --- | --- | --- |
| config init/check | Exclusive config creation and strict validation. | No automatic configuration migration. |
| init | Bundled local SQLite, writer lock, stable vault/disk UUIDs, empty-directory adoption. | Full power-loss recovery and filesystem qualification remain open. |
| status | Offline catalog and mounted identity check, actual free bytes. | No archived bundles, jobs, reservations, replicas, or relocation yet. |
| resolve | Moving ref followed by pinned metadata; verified license/config/index bytes. | Public ungated models only; no payload download or approval. |
| propose / proposals / decide | Durable evidence/volume binding, paginated offline inspection, idempotent rejection, schema-1 migration. | Approval is blocked until transfer bounds and accounting qualify. |
| plan | Canonical preview from local inventory, file selection and blockers. | Always untrusted, never persisted or authorized. |
| Integrity library | Bounded-memory Git blob/LFS verification plus local SHA-256. | Transfer writer and durable manifest integration are next. |
| Transfer library experiment | Identity-bound prefix checkpoints, strict byte ranges, segment rollback, full-file upstream verification, offline uncommitted-tail recovery, exclusive SQLite checkpoint sessions. | The session experiment owns locking/journaling; approval, reservations, accounting, vault integration, and publication remain open. |

No command executes repository code. No automatic scan, account service, telemetry, deletion, or conversion exists.

## Observed evidence

- Native Windows unit, CLI, contract, HTTP, integrity, and vault tests pass. Additional [validation drills](validation-drills.md) exercise abrupt process termination, the live NTFS test vault, and durable pinned-source decisions.
- HTTP fixtures exercise bounded reads, interruptions, redirect trust, signed-URL redaction, bad status/encoding, access restrictions, source changes, and metadata disagreement.
- Rights fixtures reject missing, changed, conflicting, and excessive evidence. Shard fixtures require exact index closure over selected weights.
- Vault fixtures exercise competing writer ownership, stable IDs, unrelated data rejection, all-volume preflight, interrupted marker publication, missing/substituted disks, schema/configuration drift, and Windows junctions.
- Source-review fixtures cover restart persistence, policy changes, exact decisions, offline disks, corruption, pagination, and identity-preserving migration. Native installer fixtures cover verification, version mismatch, archive path rejection, retained configuration, and upgrades.
- A Windows release builds with bundled SQLite and Rust TLS. Import inspection finds only OS DLLs.
- Live source inspection pins SmolLM2-135M-Instruct at 12fd25f77366fa6b3b4b768ec3050bf629380bac. Configuration bytes verify; missing license text remains unreviewed despite the Apache tag. No weights were fetched.
- A second release inspection pins Qwen2.5-0.5B-Instruct at 7ae557604adf67be50417f59c2c2f167def9a775. License/config/tokenizer configuration bytes verify, and the Apache template with its filled appendix copyright matches. Transfer authorization remains false; no weights were fetched.

Local Windows validation passes 70 Rust tests, ten offline installer/packaging tests, and six documentation-tool tests. Measured owned Rust coverage is 96.76% of lines and 92.71% of branches; the documentation link library measures 100% for both. Rust formatting, Clippy with warnings denied, Markdown lint, local links, and Python tooling lint pass. CLI captures are regenerated from the release binary and checked for freshness. Dependency source/license/duplicate checks and the vulnerability audit passed for the unchanged lockfile; no known vulnerabilities were reported. A release smoke test creates an empty vault and reports its mounted disk without contacting a publisher. Current artifact sizes are observations, not distribution guarantees.

Live source state can change. These checks are observations, not permanently pinned model recommendations. Use resolve to inspect a current source and retain the exact commit/evidence before later approval.

The single [CI workflow](https://github.com/blisspixel/ModelPrepper/actions/workflows/ci.yml) runs native Linux/macOS/Windows checks, coverage, documentation/capture checks, and dependency audits. Its aggregate Main checks job requires all jobs to succeed. Consult the actual main-branch run for hosted status; local results do not substitute for a hosted run. Native fixture/build results still do not qualify clean-machine, large-volume, power-loss, or offline model-loading behavior.

The [durable staging journal](staging-journal.md) adds exclusive single-file checkpoint sessions and crash-boundary tests. The single CI workflow exercises these tests on Windows, macOS, and Linux. Verify the main-branch run against the exact commit being used; a previous successful run does not validate newer changes.

## Next delivery gates

1. Streaming transfer fixture with exact Range validation, bounded workspace, incremental accounting, and verified resume after process termination. Reject ignored Range, changed identity, corrupt partials, truncated responses, and exhausted storage.
2. Extend persisted source reviews and transactional migration into qualified transfer plans with measured resource bounds and granted approvals. Approval must bind that identity and expire on relevant change; original evidence, inventory, exclusions, volume UUID, and policy digest are already retained in source reviews.
3. Reservations, full-file verification, durable manifest, atomic publication, and reconciliation after every catalog/filesystem crash point. A renamed directory alone cannot count as sealed.
4. Replica copying and manifest-based reconstruction before unattended discovery. Prove recovery with the hub unreachable and the original catalog removed.
5. Actual native filesystem and offline-load drills on supported OSes, then installation/scheduling documentation and release packaging.

Initialized disks and successful inspection are useful foundations. They do not establish that a model has been preserved or can run offline.
