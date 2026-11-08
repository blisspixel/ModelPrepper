# Contributor entry point

This file routes work to the canonical documents. It does not replace them.

## Orientation

Read [README](README.md) and [implementation status](docs/implementation-status.md) first, then [contracts](docs/contracts.md) and the matching gate in the [roadmap](docs/roadmap.md). `fetch`, grants, seals, replicas, rebuild, import, one bounded `watch` pass, `schedule` preview/install/remove, and `collection preview` are implemented. `decide` still cannot approve a transfer. The schedule CI uses an injected host runner and does not enable a live timer. A collection preview records a representation name, category names, a category ceiling, a declared base, an evaluation attribution, and an evidence statement. It does not apply the name, charge the ceiling, preserve the base, rank by the attribution, check an evidence statement, or verify a provenance statement. A repeated declared base is listed and not merged. `collection store` writes that parsed list into the catalog and `collection show` reads it back. `collection list` pages the stored names in name order and does not rank them. `collection remove` deletes that stored name and does not delete archived revisions. `collection custody` counts catalog rows with the same repository and revision text and does not hash or download them. A 40-character lowercase hex revision is labeled `commit`. Any other accepted revision is labeled `name`. The label is not a resolution. When every matching manifest records `size_bytes` and those totals agree, the report shows that sum. A missing manifest or disagreeing totals stay unmeasured. `collection custody` charges a written category ceiling only when every entry that names it has `measured_bytes`. The same measured total can count toward each category that entry names. An unmeasured entry is left out, a partial sum is not reported, and no size is invented. Preview, store, and show still leave `applied` false. `discover` searches the reviewed source catalog for an explicit role, status, or source id. Matches stay in catalog order. It does not rank them, contact a network, or approve a transfer. The `[discovery] enabled = false` sketch is not a command. `collection select` keeps stored entries whose category, representation, or evaluation text matches exactly. Stored order is kept. `evaluation_value` is not a score, and `ranked` stays false. `verify-sealed` hashes one custody plan with the existing file hash. Other plans are not read. `collection custody` does not become a rehash. Nothing is downloaded. `collection export` writes that stored list to a new schema 1 file and does not change the catalog. `collection diff` compares one schema 1 file with the stored list and does not change either. Storing does not resolve, download, charge, rank, check, or verify the recorded fields. Do not describe an approval, a live installed timer, or a maintained collection as current behavior. `collection custody` lists the matching plan ids in plan id order. A replica row is not listed. Nothing is dropped or merged. Each listed plan names its stored volume and its own manifest total when that manifest parses. Disagreeing plans stay listed. Nothing is retired. Each listed plan counts replica rows with that plan id. Other entries in the same stored list that name this repository and revision as a declared base are counted as dependents. A replica row is not a custody plan. Counting them does not retire a copy. Each entry names why it is not retired. No sealed copy, the last sealed copy, and declared dependents are named when they apply. Retirement is not approved, and nothing is retired.

## Start tidy, stop tidy

Work in this repository may come from several agent tools, and a session can end partway through a task.

- Check first. Before starting new work, run `git fetch --prune` and check `git worktree list`, local and remote branches, and open pull requests. Finish, merge, or park leftover work instead of starting a parallel attempt. Squash merges hide merged work from ancestry, so check pull request state, not only `git branch --merged`.
- Checkpoint and hand off. Commit and push at meaningful checkpoints. Before stopping, leave a short handoff note in the pull request or commit message saying what is done and what is next.
- Keep the tree clean. Test fixtures, scratch scripts, logs, and generated output go only in the designated test or temp locations, or in the gitignored `.agents/` directory. Do not leave throwaway files elsewhere in the tree. Remove scratch artifacts before finishing.
- Keep branches short-lived. Merge promptly and delete branches after merge. Keep GitHub current: push finished work, update or close pull requests, and close finished issues.
- Goal state. One `main` with CI passing. Do not leave `main` red, and do not leave work stranded on a side branch.
- Remove only with evidence. Remove a worktree or branch only once its work is merged, contained in `main`, or pushed to the remote. Use a normal remove, never `--force`. Report anything uncertain instead of deleting it. Run `git worktree prune` for entries whose directories are gone.
- Ask when unclear. If it is unclear whether leftover work should be finished, kept, or removed, or what the intended state is, stop and ask instead of guessing.

## Canonical code

- Bounded publisher requests live in [src/http.rs](src/http.rs).
- Byte identities live in [src/integrity.rs](src/integrity.rs) and [src/verify.rs](src/verify.rs).
- Catalog ownership, the writer lock, adoption, persistence, and recovery live under [src/vault/](src/vault/).
- Deeper design is in [architecture](docs/architecture.md) and [contracts](docs/contracts.md). Link those instead of copying them.
- The optional Omarchy presence projection lives in [plugins/omarchy-presence/Model.js](plugins/omarchy-presence/Model.js). The QML shell only displays it. Do not add a second status verdict or a background service.

## Invariants

Keep these states distinct: an untrusted preview, a source review, transfer authorization, an upstream-verified local adoption, a downloaded seal, and demonstrated offline usability. Adoption provenance `upstream-verified (local source)` is not a downloaded seal and does not prove a model runs offline. `readiness` reports a local recipe. It does not run a model, and a drill note in that recipe is prior evidence, not a new demonstration.

Keep the source directory outside the catalog and every vault volume. A changed source must not be deleted. Move and retry checks are documented in [contracts](docs/contracts.md); they are not an atomic multi-file transaction.

`verify-local --repo` asks the publisher for the pinned inventory. `verify-local --config --plan` reads the saved review and stays offline. Only the frozen matched set is copied, recorded, retried, and removed.

## Verification

Follow [CONTRIBUTING](CONTRIBUTING.md) and [quality](docs/quality.md). Use the existing tiny offline fixtures for ordinary checks. Do not run a live model resolution, download, adoption, or move just to validate documentation.

## Evidence

Report the checks you actually ran and the exact revision. Local results, hosted CI, and native platform qualification are different claims. When behavior changes, update the canonical document that states it.
