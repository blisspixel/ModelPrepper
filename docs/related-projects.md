# Related projects and research

Reviewed September 30, 2026. Observations come from primary documentation, not executed benchmarks or source-code audits.

## Nearby tools

| Project | Documented overlap | Lesson |
| --- | --- | --- |
| [model-mirror](https://github.com/xlr8harder/model-mirror) | Pinned mirrors, hashes, verification, repair, explicit updates. | Separate repair from revision updates. |
| [hftools](https://github.com/ziozzang/hftools) | Native binaries, download/check/repair, cache import/export, offline serving. | Import existing copies and support native deployment. |
| [Hub client](https://huggingface.co/docs/huggingface_hub/main/guides/download) | Snapshot selection, filtering, local folders, dry runs. | Compare source resolution with reference behavior. |
| [restic checking](https://restic.readthedocs.io/en/stable/045_working_with_repos.html) | Snapshot integrity and payload-reading checks. | Reading catalog metadata differs from reading stored bytes. |
| [restic restore](https://restic.readthedocs.io/en/stable/050_restore.html) | Restoring snapshots to a target directory. | Demonstrate recovery. |
| [rclone copy](https://rclone.org/commands/rclone_copy/) | Copying without deleting destination files. | Avoid destructive synchronization semantics. |
| [ArchiveBox](https://archivebox.io/) | Self-hosted web preservation into local artifacts. | Ownership and inspectable exports belong in the user experience. |

Model-mirror is the closest direct precedent found. Its README also documents experimental torrent recovery and additional repository types. These are research leads, not initial requirements.

Hftools documents Go and six native release targets. This corrects the earlier implication that native distribution requires Rust. Rust is our chosen implementation, with transfer feasibility proven before a large build.

## Proposed emphasis

Explicit watches, bounded unattended maintenance, user-defined priorities, removable volumes, catalog reconstruction, verified replicas, and honest offline readiness.

This is not a claim that no other tool implements those features. Before building a component, inspect upstream code and licenses and record whether reuse is feasible.

## Patterns to borrow

- Resolve branches to immutable source identities.
- Write temporary files and publish only after verification.
- Repair the archived revision rather than following latest.
- Separate metadata checks, full verification, and actual loading.
- Expose interrupted work and explicit cleanup.
- Support import/export and recovery drills.

Review licenses before copying code. Referencing a project does not select it as a dependency.

## Proof required here

- Rust client supports required listing, revision, ordinary-file, LFS, and Xet paths.
- Peak disk workspace is measured and bounded.
- Resume cannot mix versions.
- Filesystem behavior works on all three operating systems.
- Replica recovery works without the hub.
- Catalog loss does not destroy file inventory or provenance.

Evaluate the [official Rust hub client](https://github.com/huggingface/hf-hub). Take exact crate names and APIs from the evaluated version.

## Policy context

The [September 29 Anthropic article](https://www.anthropic.com/research/glm-5-3-and-the-spread-of-advanced-cyber-capabilities) discusses cyber capability, removable safeguards, vetted access, and evaluations. Its [July 27 position](https://www.anthropic.com/news/position-open-weights-models) rejects a category ban and supports mandatory testing.

Our concern is structural: requirements for revocability and centralized permission may conflict with individual custody. This motivation needs neither an IPO valuation nor a claim about hidden motives.

## Remaining research

1. Inspect model-mirror and hftools transfer and interruption implementations.
2. Prototype the Rust client with deterministic local fixtures and an opt-in live revision.
3. Sample license and metadata edge cases across publishers.
4. Distinguish Git blob hashes, LFS SHA-256, and Xet identifiers.
5. Measure disk, memory, and resume behavior on HDD and NAS storage.
6. Evaluate runtime kit redistribution and platform portability separately.

This pass is not an exhaustive project inventory or a reliability certification.
