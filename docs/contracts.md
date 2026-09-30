# Version-one contracts

This document distinguishes implemented contracts from future preservation operations. Unknown fields and unsupported schema versions are rejected. Fixture changes that alter canonical identity require explicit review.

## Implemented configuration

The executable accepts the subset in [examples/config.toml](../examples/config.toml). Required top-level fields are schema_version, catalog_path, licenses, storage, transfer, approval, files, and volumes. watches defaults to an empty list.

Nested structures reject unknown fields. Budgets must be positive. Parallel file count is bounded to 1 through 8. Volume labels and configured paths must be unique, and the default volume must exist.

Only manual approval, UTC weeks, MIT/Apache-2.0, public Hugging Face model sources, propose updates, and publisher-safetensors-v1 are implemented. Discovery, automatic rules, repository code, GGUF, and extended licenses remain unsupported. Do not add the future discovery/worries sections to a config passed to the current validator.

Offline previews record catalog and volume paths as provided. Vault commands resolve relative paths against the canonical configuration file's directory. Init creates bundled SQLite and volume identities; status measures free space only for a matching mounted volume. Neither command mounts disks.

## Implemented inventory

[tests/fixtures/inventory.json](../tests/fixtures/inventory.json) is synthetic, not captured publisher evidence.

Inventories record source endpoint, repository id/type, requested and resolved revisions, complete-list status, access flags, layout, license evidence, and file records.

The initial resolved commit is exactly 40 lowercase hexadecimal characters. Upstream file hashes distinguish raw SHA-256 from Git blob SHA-1. Xet hashes are not silently reinterpreted as either algorithm.

Each file has path, size, role, required flag, and optional upstream identity. Files must have unique portable paths, including case-insensitive collision and file-as-directory checks. Initial paths are ASCII and at most 240 bytes; Unicode and wider path support require a later explicit policy.

The current planner trusts no input file. Claimed complete layout or matched license status is a preview assertion, not validated publisher provenance. Every local preview carries local_inventory_untrusted and transfer_authorized=false.

## Selection and blockers

Select files using explicit roles and supported extensions. Unsafe tensor formats and repository code are excluded regardless of claimed role. Excluding a required file creates required_file_excluded.

The offline planner checks asserted weight, configuration, tokenizer, and license roles. It does not validate locally asserted layout claims. The online resolver separately checks config.json, tokenizer_config.json when present, and exact closure of a safetensors shard index. Remote-code mappings or explicit trust_remote_code requirements make the layout unsupported. None of these checks proves that a runtime can load the model.

Blockers are stable snake_case identifiers:

| Code | Meaning |
| --- | --- |
| local_inventory_untrusted | Local metadata cannot authorize a source transfer. |
| incomplete_inventory | Full upstream listing was not asserted. |
| unsupported_layout | Inventory uses a layout outside the initial profile. |
| access_restricted | Gated or private source is outside current policy. |
| license_needs_review | Evidence is unreviewed, conflicting, or absent. |
| license_not_allowed | Claimed license is outside the configured allowlist. |
| missing_required_role | A required model component category is absent. |
| required_file_excluded | File policy excludes a component asserted required. |
| missing_upstream_hash | Selected file lacks upstream content identity. |
| revision_too_large | Payload exceeds the configured total storage ceiling. |
| transfer_budget_exceeded | Payload exceeds the standalone period transfer allowance. |
| insufficient_space | Payload plus minimum reserve exceeds the supplied free-space observation. |

Byte arithmetic is checked for overflow. The current budget checks are standalone lower bounds. They do not account for existing archive occupancy, reservations, transfer history, or workspace. workspace_bytes is null until a backend bound is established.

## Canonical preview identity

Policy identity is SHA-256 of compact JSON serialized from the version-one Config structure in declaration order. Sort licenses, volumes by label, and normalized watches by repository id/revision first.

Preview identity hashes a compact JSON object whose declaration order is domain, policy_digest, source, inventory. The domain is modelprepper.offline-plan.v1. Normalize official repository URLs to ids, sort inventory files by path, and sort license evidence paths.

Source identity and the complete file inventory, including exclusions and license assertions, are bound to the preview. File size, hash, required flag, evidence, configured placement, or policy changes alter identity. Current free-space observations are not identity.

This format is a project-specific canonical encoding, not a claim of compliance with a separate JSON canonicalization standard. It is frozen by the regression vector below and tests of order invariance.

For the committed config and inventory fixtures:

```text
policy_digest:
3a30ff35811c93095815ea8a4a21fcda725b5c0f9201bf2d33d968d2145a60dc

plan_id:
1cf5f58c1b7819a7f5104edbdf7de51168ca25ca37797e8e48e1f4a9a14d2795

payload_bytes:
5504
```

The future trusted transfer plan gets a distinct identity domain. An offline preview is never upgraded into an authorized plan by editing its output JSON.

## CLI output and errors

Successful commands emit a single JSON object on stdout. Structured application failures emit schema_version plus error.code and error.message on stderr and exit 1.

Clap argument errors exit 2. Help and version are ordinary text. Successful offline preview exits 0 even when blockers are present: it completed a preview rather than a transfer.

Local metadata reads are capped at 2 MiB and require UTF-8, with optional leading BOM. Parser errors avoid echoing a full TOML source excerpt. Only resolve makes publisher requests today.

config init creates only a config file using exclusive creation. It does not initialize a vault and never overwrites an existing output. Failure during writing is reported; if storage failure leaves a partial config, the user inspects it before retrying.

## Implemented source resolution

resolve accepts a repository id or official hub URL and a bounded revision. It reads the moving revision, then reads its exact 40-character commit. Repository identity, pin, access flags, file paths, sizes, and upstream identities are checked before evidence is fetched. Only public, ungated model repositories are supported.

The complete pinned inventory remains in the output, including excluded alternatives. Root publisher safetensors, conventional configuration/tokenizer files, templates, README, licenses, and notices receive explicit roles. Nested weight variants, adapters, repository code, and pickle weights are not selected by this profile.

License and notice bytes are fetched at the pinned commit and verified against their Git blob or LFS identity. Exact text and local SHA-256 remain in the result. Git blob SHA-1 includes the object header; it is never raw file SHA-1.

Matching currently accepts whitespace-normalized Apache-2.0 template text with an optional single filled copyright line in its appendix, or the unchanged MIT permission/warranty text with a narrow heading and copyright prefix. Unknown text, added terms, missing evidence, mismatched tags, or any NOTICE remains unreviewed or conflicting. Legitimate variants can therefore require later human review. This is intentionally narrower than a general license classifier. Embedded templates in resources/licenses originate from the [SPDX license data](https://github.com/spdx/license-list-data/tree/main/text).

Metadata responses and individual configuration/index files are limited to 2 MiB. License/notice evidence is limited to 16 files and 2 MiB combined. Requests use identity encoding, a 60-second per-request timeout, and at most five redirects. Redirects require HTTPS on huggingface.co or hf.co and their subdomains, without credentials, nonstandard ports, or fragments. Transport errors omit URLs and response bodies to avoid exposing signed locations. Rust TLS uses bundled Mozilla roots; custom OS certificate authorities are not supported yet.

Resolution returns mode=resolved_inventory and transfer_authorized=false. Output JSON is an inspection result, not a persisted approval or trusted transfer plan. Feeding its inventory into the local preview retains local_inventory_untrusted.

## Implemented vault foundation

init --config creates a catalog directory, catalog.sqlite3, writer.lock, and a .modelprepper-volume.json identity on each configured empty volume. Catalog schema 1 records a vault UUID, initialization state, and each volume's UUID, label, and absolute path. SQLite uses WAL, synchronous=FULL, and foreign keys. The catalog must be on a local filesystem. Explicit Windows UNC catalog paths are rejected before filesystem access; mapped drives and Unix mount types are not classified yet, so the operator must choose a local catalog directory.

Catalog and volumes cannot overlap. Dot segments, symlink ancestors, reparse points, and unrelated files during adoption are rejected. Ownership checks also cover SQLite sidecars. A kernel-held exclusive file lock covers the database operation; a contending writer receives writer_busy. These checks assume other programs do not maliciously replace directories during the operation; handle-relative filesystem operations remain a later hardening gate.

Initialization commits disk identities to the catalog before publishing volume markers. A valid pending identity can be renamed and initialization resumed with the same UUIDs. Recovery preflights all configured volumes before writing any markers. Malformed pending identities or an incomplete schema fail closed for inspection. Recovery from every power-loss boundary is not qualified yet.

Once initialization reaches ready, init never recreates a missing volume or adopts a replacement. Existing user payload files are retained. Status reports mounted, offline, identity_missing, or identity_mismatch per volume. Invalid marker text is an application error. The top-level ready state means catalog initialization completed; availability is reported separately for each disk.

status is offline, acquires the writer lock, opens only an existing catalog, and checks identities before measuring free bytes. It can update SQLite journal bookkeeping, so it is not a read-only media command. Changing a configured disk path or label fails with volume_configuration_changed. Relocating a volume requires a future explicit operation.

## Future manifest contract

The durable manifest in [preservation.md](preservation.md) will contain schema version, bundle identity, source identity, original rights evidence, file hashes/sizes, selected profile, excluded dependencies, seal timestamp, and operation identity.

A trusted seal is created only after downloaded bytes have been fully verified. Imported local-baseline bytes cannot acquire upstream provenance merely by resembling a known layout.

Finalize executable manifest parsing and crash-consistency tests before M3; no current command produces a sealed manifest.

## Future job-state contract

Decision states: pending, approved, rejected. Approval binds a trusted immutable plan identity.

Transfer states: queued, reserved, downloading, paused, retry_wait, verifying, publishing, sealed, blocked, damaged, cancelled.

Only approved plans may enter queued. Verification and durable publication precede sealed. Loss of a mounted drive affects availability, not integrity. Repair creates an operation pinned to the archived identity. Resume does not change that identity.

Detailed recovery behavior is in [architecture.md](architecture.md). Persistent jobs and transitions are not implemented in the offline preview.
