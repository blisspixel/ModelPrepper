# Contributor entry point

This file routes work to the canonical documents. It does not replace them.

## Orientation

Read [README](README.md) and [implementation status](docs/implementation-status.md) first, then [contracts](docs/contracts.md) and the matching gate in the [roadmap](docs/roadmap.md). Planned downloads, granted approvals, sealed bundles, replicas, and unattended maintenance are not implemented. Do not describe them as current behavior.

## Canonical code

- Bounded publisher requests live in [src/http.rs](src/http.rs).
- Byte identities live in [src/integrity.rs](src/integrity.rs) and [src/verify.rs](src/verify.rs).
- Catalog ownership, the writer lock, adoption, persistence, and recovery live under [src/vault/](src/vault/).
- Deeper design is in [architecture](docs/architecture.md) and [contracts](docs/contracts.md). Link those instead of copying them.

## Invariants

Keep these states distinct: an untrusted preview, a source review, transfer authorization, an upstream-verified local adoption, a downloaded seal, and demonstrated offline usability. Adoption provenance `upstream-verified (local source)` is not a downloaded seal and does not prove a model runs offline.

Keep the source directory outside the catalog and every vault volume. A changed source must not be deleted. Move and retry checks are documented in [contracts](docs/contracts.md); they are not an atomic multi-file transaction.

`verify-local --repo` asks the publisher for the pinned inventory. `verify-local --config --plan` reads the saved review and stays offline. Only the frozen matched set is copied, recorded, retried, and removed.

## Verification

Follow [CONTRIBUTING](CONTRIBUTING.md) and [quality](docs/quality.md). Use the existing tiny offline fixtures for ordinary checks. Do not run a live model resolution, download, adoption, or move just to validate documentation.

## Evidence

Report the checks you actually ran and the exact revision. Local results, hosted CI, and native platform qualification are different claims. When behavior changes, update the canonical document that states it.
