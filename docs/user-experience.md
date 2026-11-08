# User experience and quality-of-life plan

This is a delivery plan, not a list of shipped features. Current behavior is in [implementation-status.md](implementation-status.md). Milestones and release gates are in [roadmap.md](roadmap.md).

The [archival policy contract](archive-policy.md) develops the NAS-owner workflow: prefer publisher originals, choose categories and variants, inspect evaluation evidence, and maintain a bounded collection without losing earlier copies.

## Four people, one dependable engine

| Person | What they need | Successful outcome |
| --- | --- | --- |
| First-time user | Understand what to keep, where it goes, and what the limits mean. | An understandable plan and a verified first copy, without editing JSON or installing a model server. |
| Power user | Automation, precise control, fast inspection, and predictable failures. | Scripts safely maintain an explicit collection within resource bounds. |
| Archivist | Provenance, completeness, fixity, history, and reconstruction. | A future reader can inspect and recover the collection after the publisher and catalog disappear. |
| Independent-custody advocate | Possession without a vendor's continuing permission or service. | Ordinary copies remain inspectable and useful offline, with independent verification and a path to migrate away from the utility. |

The last group includes people who take a Second Amendment absolutist view of distributed AI capability. Serve that conviction through concrete ownership and resilience features. No mandatory account, subscription, remote activation, telemetry, centrally controlled collection, or vendor kill switch belongs in the core design. Publisher rights and applicable law remain distinct from the project's political position.

Profiles change presentation, priorities, and explicit configuration. They do not weaken hash verification, silently approve transfers, expand a user's rights, or hide missing dependencies.

## First-time user

Make the first useful task small: preserve one explicit model on one chosen disk.

- A guided setup explains preservation versus inference and offers equivalent noninteractive flags. An empty setup stays offline and downloads nothing.
- Show decimal and binary sizes with familiar units. Distinguish payload, temporary workspace, existing occupancy, reservations, replicas, and minimum free-space reserve.
- Before approval, show the exact commit, chosen representation, excluded alternatives, license evidence, required bytes, and destination disk.
- Explain a blocker in plain language, then provide a concrete next action. Missing license text must not become an unexplained permission error.
- Offer an offline practice fixture before a real download. Model suggestions or shared starter collections are optional data, independently resolved and reviewed.
- Download progress distinguishes network transfer, verification, and publication. An interrupted job says what was retained and how to resume.
- Finish with separate integrity, replica, and offline-load results. A green checksum must not imply that a laptop can run the model.
- Provide a second-copy walkthrough and a recovery exercise. Scheduling is offered after the user sees an approved job succeed.

Target: create an understandable plan within ten minutes of obtaining the binary, excluding transfer time. Measure this with new users on each OS. Do not report the target as an achieved result.

## Power user

Provide predictable primitives rather than a separate automation product.

- Stable JSON schemas, reason codes, and exit semantics. Keep progress on stderr and machine output on stdout.
- Batch watch import/export with preview, duplicate normalization, exact revision pinning, and bounded update rules.
- Filters by repository, collection, disk, integrity, verification age, availability, and job state. Status must not hash tensors or contact the hub by default.
- Explain policy and plan differences before another approval. Moving branch names never silently replace an archived commit.
- Explicit pause, resume, retry, cancel, and queued-job inspection. Report bytes already spent and reservations still held.
- Transfer windows, bandwidth limits, request backoff, and HDD-friendly bounded concurrency. Respect Retry-After without turning a scheduler run into an unbounded wait.
- Preview native OS schedules, including the command, config path, timezone, and expected working directory. Scheduled runs need no interactive prompt.
- Document portable exit codes and examples for shell automation on Windows, Linux, and macOS. MCP remains an optional adapter to the same engine.

Acceptance: a script distinguishes no work, waiting for approval, a missing disk, a temporary source failure, and damaged data without parsing prose. Repeated runs do not duplicate writes, approvals, or accounting.

## Archivist

Keep evidence that outlives the originating service and the application.

- Capture the exact source commit, full inventory, original license/notice text, metadata evidence, selected profile, omissions, and all-file SHA-256.
- Preserve publisher bytes. Record conversions or derived artifacts as separate objects with lineage; never replace an original silently.
- Distinguish an upstream-verified copy, a local-baseline import, a verified replica, and a recorded offline-load test.
- Track verification age and support resumable fixity sweeps. Offer byte/time limits so HDD maintenance remains practical.
- Preserve older revisions and record an upstream disappearance as an event. Absence from an incomplete scan is not evidence of deletion.
- Support collections, annotations, descriptive metadata, and exportable inventories without making them part of byte integrity identity unless necessary.
- Reconstruct inventory from manifests after catalog loss. Invalid, conflicting, and unknown-version manifests must be visible rather than silently accepted.
- Treat cold storage as normal: record a disk's contents while it is unplugged, request the correct disk by identity, and never write to an empty mount path as a substitute.
- Offer replica health and failure-domain information. Two folders on one physical disk are not equivalent to two independent storage devices.
- Consider [BagIt-compatible exports](https://www.rfc-editor.org/rfc/rfc8493) and content-addressed references after native manifests stabilize. Adopt interoperable standards where they improve independent inspection; do not add a required archival framework to the runtime.

Acceptance: another person can inspect a bundle with ordinary filesystem tools, verify its checksums, identify omissions, and reconstruct a collection with the hub unreachable and the original catalog removed.

## Independent custody and preparedness

The project should remain useful even if its own maintainers, hosting account, or preferred model hub disappear.

- Ordinary self-contained folders and documented manifests. Export carries bytes and evidence, without inheriting the original installation's approval authority.
- No remote revocation mechanism for existing local copies. Source removal or access changes never trigger local deletion.
- Explicit watches and local operation work without an AI ranker or inference server. No cloud intelligence service is required to maintain the collection.
- Manual media transfer, verified replicas, local-baseline adoption, and catalog reconstruction come before unattended watches, discovery, or dashboards.
- A disconnected operations guide covers inventory, verification, restoration, and runtime readiness. Preserve optional runtime/toolchain kits separately with their own rights and platform evidence.
- Repository source, locked dependencies, build instructions, and release checksums make independent builds possible. Document an optional vendored/offline build kit after platform qualification.
- User-controlled source adapters and admission profiles are an extensibility goal. The application must not acquire a centrally managed opinion filter or a provider-controlled authorization service.
- Optional encryption needs explicit key ownership and recovery instructions. It must not introduce a vendor key service, and the project should reuse reviewed formats rather than invent cryptography.

Acceptance: disabling internet access does not prevent local status, verification, replication, or reconstruction. Archived bytes remain usable without ModelPrepper. The utility's disappearance does not strand the collection.

## Delivery order

| Priority | Milestones | Deliverables |
| --- | --- | --- |
| Foundation | M1 through M3 | Honest plans, exact byte identities, measured bounds, resumable transfer, and one published revision readable without the program. |
| Protect that revision | M4 | Replica, fixity, catalog rebuild with the hub unreachable, and local-baseline import. |
| Bounded automation | M5 | `watch` creates a new plan for a new commit, keeps old revisions, and fetches only inside a current grant and the weekly byte ceiling. |
| Disk qualification | M6 | Status names the filesystem. Unsupported and read-only mounts fail before a write. A live share and HDD timing are still unmeasured. |
| Release usability | M7 | `schedule` previews a weekly timer and installs it only when the digest matches. CI injects the host runner. Clean-machine setup and persona drills remain open. |
| Later conveniences | M8 onward | `collection preview` lists an explicit shared file, records a representation name, category names, a category ceiling, a declared base, an evaluation attribution, an evidence statement, and a provenance statement without applying them, lists a repeated base without merging it, and does not download it. `collection store` writes that list into the catalog and does not apply it. `collection list` pages the stored names and does not rank them. `collection remove` deletes one stored name and does not delete archived revisions. `collection custody` counts catalog rows with the same repository and revision text and does not hash or download them. A 40-character lowercase hex revision is labeled `commit`. Any other accepted revision is labeled `name`. The label is not a resolution. When every matching manifest records `size_bytes` and those totals agree, the report shows that sum. A missing manifest or disagreeing totals stay unmeasured. `collection custody` charges a written category ceiling only when every entry that names it has `measured_bytes`. The same measured total can count toward each category that entry names. An unmeasured entry is left out, a partial sum is not reported, and no size is invented. Preview, store, and show still leave `applied` false. `discover` searches the reviewed source catalog for an explicit role, status, or source id. Matches stay in catalog order. It does not rank them, contact a network, or approve a transfer. The `[discovery] enabled = false` sketch is not a command. `collection select` keeps stored entries whose category, representation, or evaluation text matches exactly. Stored order is kept. `evaluation_value` is not a score, and `ranked` stays false. `collection export` writes that stored list to a new schema 1 file and does not change the catalog. `collection diff` compares one schema 1 file with the stored list and does not change either. Ranking, a hub model search, a local web UI, optional MCP, interoperable archival exports, and runtime kits remain open. `discover` searches only the reviewed source catalog. |

Setup, readable output, and explainable plans are M2 and stay available while later milestones are open. A watch entry that only records intent is M2. `watch` fetches that entry only inside a current grant. `schedule` previews the weekly timer and installs it only when the digest matches. A live scheduler was not enabled. The [roadmap](roadmap.md) wave list is the same order.

The first release should be small enough to trust. Do not hold core recovery hostage to a dashboard or make every convenience part of version one.

## Persona drills

| Drill | Required observations |
| --- | --- |
| New user on an empty machine | Chooses a disk, understands a blocker, reviews a plan, and distinguishes verification from runnability. |
| Power user running a scheduled job twice | Idempotent outcomes, bounded runtime/resources, useful JSON, and no unexpected prompt. |
| Archivist rebuilding after catalog loss | Accurate provenance and omissions, restored inventory, visible conflicts, and matching checksums. |
| Custody advocate with internet blocked | Local inspection, verification and recovery succeed without login or project-hosted services. |
| Any user unplugging the destination disk | No writes to a replacement path; retained jobs identify the required volume. |

Capture friction and failures as evidence. User preferences about autonomy must be explicit, exportable configuration, not hidden behavior inferred from a persona label.
