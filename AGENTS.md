# Contributor entry point

This file routes work to the canonical documents. It does not replace them.

## Orientation

Read [README](README.md) and [implementation status](docs/implementation-status.md) first, then [contracts](docs/contracts.md) and the matching gate in the [roadmap](docs/roadmap.md). `fetch`, grants, seals, replicas, rebuild, import, one bounded `watch` pass, `schedule` preview/install/remove, and `collection preview` are implemented. `decide` still cannot approve a transfer. The schedule CI uses an injected host runner and does not enable a live timer. A collection preview records a representation name and a category ceiling and does not apply either. It does not resolve, download, or store entries. Do not describe an approval, a live installed timer, or a maintained collection as current behavior.

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
