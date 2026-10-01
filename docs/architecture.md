# Architecture

Target architecture for the Rust implementation. Strict config/inventory validation, preview planning, bounded public-source resolution, local catalog initialization, disk identities, and offline status are implemented. Persisted transfer plans, jobs, payload storage, and sealing remain planned. Current behavior is specified in [contracts.md](contracts.md) and [implementation-status.md](implementation-status.md).

## Boundaries

One user-owned vault, one local catalog, one writer process, and multiple mounted or offline volumes. The catalog must live on a local filesystem of the writer host. Bulk volumes may be mounted NAS exports.

CLI, scheduler, and optional MCP call one application layer. Only source adapters make source network requests. Verification, inventory, catalog reconstruction, and replica restoration remain offline.

```text
watchlist / optional discovery
             |
             v
resolve -> rights + file selection -> immutable plan -> approval
                                                        |
                                                        v
reserve -> stage -> download -> verify -> publish -> catalog
                                         |
                                         v
                              replica / offline test
```

## Components

| Component | Responsibility |
| --- | --- |
| config | Versioned configuration, validation, bounded approval rules. |
| source | Resolve revisions, list files, capture rights evidence, fetch pinned bytes. |
| selection | Representation and dependency closure, inclusion/exclusion reasons. |
| planner | Canonical plan identity, exact costs, source and policy digests. |
| catalog | Watches, plans, jobs, reservations, volumes, replicas, audit events. |
| transfer | Resume, throttling, retries, staging, independent integrity checks. |
| preservation | Manifests, publication, verification, repair, reconstruction. |
| application | Shared command behavior and authorization scope. |
| cli | Human output, JSON, exit codes, prompts. |
| mcp | Optional adapter to the same application commands. |

Avoid generic plugin execution in the core. Initial source adapters are compiled Rust modules. Later artifact types must satisfy the same preservation contract.

## Source adapter contract

The [source catalog](model-sources.md) identifies research candidates and qualification gaps. Canonical artifact identity and original evidence remain separate from retrieval locations; hash-verified mirror recovery must not change the archived revision or promote unknown provenance.

Resolve a normalized source reference and requested revision to an immutable identity. Return a complete paginated inventory, sizes, available authoritative hashes with named algorithms, captured metadata, license evidence, and access status.

Reject unknown or incomplete inventory as unsuitable for automatic transfer. Record retrieval time, source endpoint, requested reference, and resolved commit. Cache source responses separately from archived payload.

Fetch accepts only a plan-selected path at the recorded identity. Redirects are restricted to the source's validated resolver/CDN policy. Never forward credentials to unrelated hosts. Tokens, cookies, and signed query strings do not enter manifests or logs.

The transfer spike must prove ordinary Git files, LFS, Xet, cancellation, and re-resolution of expired signed URLs without changing identity. If upstream cannot supply trustworthy content identity for a required file, record that limitation and require review.

## Rights and file selection

License tags are discovery hints. Capture license and notice files at the pinned revision. Check recognized text and modifications; a header alone is insufficient. Unknown, missing, conflicting, or mixed evidence blocks automatic approval.

Explicit selection still passes the same rights gate. Record rights for included components and any base dependencies. Preserve original files, not a generated paraphrase.

Select one weight representation. Follow shard indexes and tokenizer/configuration references to produce a complete required file set. Never download every alternate tensor representation by a broad wildcard.

Default file policy excludes executable code, pickle, and unsafe paths. Missing dependencies remain visible. A structurally complete data bundle may still need an unsupported runtime.

## Immutable plans and approval

A plan stores source identity, file list, authoritative hashes, selected representation, rights evidence digest, policy version/digest, expected costs, and selected volume.

The plan id hashes a canonical versioned encoding. Approval applies to that identity, not a repository name or mutable branch. Changed file list, revision, rights evidence, policy, representation, or placement creates a new plan requiring a new decision.

Before transfer, recheck access, pinned inventory, evidence, capacity, and reservations. A moving main branch alone does not invalidate an already approved pinned revision. Unexpected differences at that same commit block work.

User approval is manual by default. Automatic rules apply only inside explicit bounds and are logged with the matching rule id.

## States

Decision state and transfer state are independent. A rejected plan never becomes a transfer job.

| Job state | Meaning | Recovery |
| --- | --- | --- |
| queued | Approved, not reserved. | Recheck plan and capacity. |
| reserved | Capacity allocated transactionally. | Acquire writer ownership before staging. |
| downloading | Partial bytes exist. | Validate checkpoints and resume. |
| paused | User pause or unavailable volume. | Preserve reservation and progress. |
| retry_wait | Transient source failure. | Bounded retry after recorded deadline. |
| verifying | Files received, full checks pending. | Rehash before publication. |
| publishing | Manifest and folder publication in progress. | Reconcile folder and transaction record. |
| sealed | All selected files verified and published. | Periodic verification and replication. |
| blocked | Access/evidence changed or intervention needed. | Explain reason and required action. |
| damaged | Previously sealed bytes failed verification. | Repair from a replica or pinned source. |
| cancelled | User ended work. | Explicit staging cleanup releases storage. |

Offline volume status is separate from file integrity. Readiness is also separate from job state.

## Reservation and transfer

Acquire the OS writer lock, reconcile outstanding jobs, and reserve capacity in a short SQLite transaction. Do not hold a database transaction during network I/O.

Reservation includes remaining final payload, bounded workspace, and free-space reserve. Partial bytes remain accounted for even after cancellation. Full accounting is in [storage.md](storage.md).

Default to one revision transfer at a time and bounded per-file concurrency. Support rate and bandwidth limits, cancellation, timeouts, and retry with jitter. Authentication failure is not retried indefinitely. Respect Retry-After and surface deferred scans.

Resume checkpoints bind path, revision, expected size, hash identity, and verified progress. Range resume must validate response status and content range; a full response cannot be blindly appended. Completed files are rechecked before reuse. A partial file never counts as sealed.

## Publication and crash consistency

1. Stage under the selected volume, on the same filesystem as its final directory.
2. Verify required files and authoritative identity, computing local SHA-256 for every file.
3. Write a versioned manifest, flush payload and manifest, and record publishing intent.
4. Rename the staged bundle into its unique final location; sync directory metadata where supported.
5. Commit the seal and accounting in SQLite.
6. Regenerate the catalog JSONL export from committed state.

The filesystem and SQLite do not share a transaction. Recovery handles a published bundle without a catalog row and a publishing intent without a completed folder. A conflicting existing destination is quarantined for review, never overwritten.

Use idempotent operation ids. Crash injection is required around every durable boundary. Filesystem flush guarantees on network mounts must be documented and tested; do not promise stronger durability than the storage provides.

## Local file safety

Reject traversal, absolute paths, drive prefixes, alternate data streams, reserved Windows names, case/Unicode collisions, and symlink or reparse-point escapes. Validate paths before making any writes.

Escape terminal controls from model cards. Treat metadata as untrusted data. Hashing or preserving a model never executes its repository code. License evidence is not a security certification.

## Reconstruction and repair

Versioned on-volume manifests contain enough information to reconstruct revisions and replicas without the hub. Recovery imports into a fresh catalog and does not overwrite a damaged database in place.

Locally imported files without captured upstream identity retain a distinct provenance status. A local hash is a baseline, not proof of publisher authenticity.

Repair creates an inspectable operation pinned to the original identity. Prefer verified replicas. Preserve failure evidence until explicit cleanup. Never repair by silently upgrading to latest.

## Optional ranker and MCP

The ranker sees saved cards and user priorities, has no tools or credentials, and cannot change the approved file list or rights decision. Deterministic ranking remains available.

MCP records queued jobs through the application layer. A foreground CLI or native scheduled worker owns long transfers. Ending a chat does not own or delete a job. Protocol-specific behavior belongs in [interfaces.md](interfaces.md).

## Observability

JSON status includes source reachability, incomplete scans, current work, transfer counters, reservations, last verification, replica health, and readiness.

No network request is made by status unless explicitly requested. Logs are bounded and redact credentials. Audit events record decisions and state changes; they are distinct from regenerable inventory exports.
