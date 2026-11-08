# Product plan

## Promise

"Tell it what you want to keep. Give it a budget and a disk. Know what you have, whether it is intact, and how to use it without the host."

The first release delivers this without an assistant, GPU, hosted project account, or programming-language runtime.

## People and jobs

Detailed workflows, quality-of-life priorities, and persona acceptance drills are in [user-experience.md](user-experience.md). They cover first-time users, power users, archivists, and advocates of independent AI custody through one shared preservation engine.

The [personal archival policy](archive-policy.md) defines publisher-original priority, task/category filters, attributed ranking evidence, bounded NAS portfolios, revision retention, and optional agents without archive authority.

| Person | Job | Evidence of success |
| --- | --- | --- |
| Desktop user | Preserve a few useful models. | First plan and verified copy without editing JSON. |
| Developer or researcher | Preserve exact dependencies. | No silent revision change; recorded offline procedure. |
| NAS owner | Maintain a bounded collection. | Scheduled jobs resume and full disks stop work cleanly. |
| Privacy-conscious user | Avoid hidden service dependence. | Inventory, verification, and export work offline. |
| Collector | Preserve categories they select. | Discovery explains matches without changing rights rules. |

## First-run experience

1. Explain that the utility preserves files rather than serving inference.
2. Accept catalog and volume paths through prompts or command flags.
3. Validate permissions, filesystem limitations, space, and volume identity.
4. Create an explicit watchlist. A blank list downloads nothing.
5. Set a storage ceiling and weekly transfer budget. Manual approval is the default.
6. Explain license and file defaults, with inspectable exception rules.
7. Produce the first plan. A decision or matching approval rule starts work.
8. Show integrity, replica count, and offline test status separately.
9. Offer a schedule preview and second-copy procedure.

Use familiar labels: waiting for approval, downloading, interrupted, verified, disk unavailable, and needs attention. Stable reason codes support scripts.

## Selection modes

### Explicit watchlist

Accept a repository id or official hub URL. The source adapter normalizes it. A pinned commit preserves exactly that revision. Watching a branch proposes changed commits while retaining old copies.

Adding a watch does not approve every future revision. Approval rules constrain source, license evidence, representation, revision size, transfer budget, and total capacity. Changed conditions return to review.

### Collections

A collection names a user-maintained group, such as "offline coding" or "speech tools," and may set a replica target.

Shared collection definitions are inspectable data. `collection preview` lists those entries and does not resolve them or download them. `collection store` writes that parsed list into the catalog and `collection show` reads it back. `collection list` pages the stored names in name order and does not rank them. `collection remove` deletes that stored name and does not delete archived revisions. `collection custody` counts catalog rows with the same repository and revision text and does not hash or download them. A 40-character lowercase hex revision is labeled `commit`. Any other accepted revision is labeled `name`. The label is not a resolution. When every matching manifest records `size_bytes` and those totals agree, the report shows that sum. A missing manifest or disagreeing totals stay unmeasured. `collection custody` charges a written category ceiling only when every entry that names it has `measured_bytes`. The same measured total can count toward each category that entry names. An unmeasured entry is left out, a partial sum is not reported, and no size is invented. Preview, store, and show still leave `applied` false. `discover` searches the reviewed source catalog for an explicit role, status, or source id. Matches stay in catalog order. It does not rank them, contact a network, or approve a transfer. The `[discovery] enabled = false` sketch is not a command. `collection select` keeps stored entries whose category, representation, or evaluation text matches exactly. Stored order is kept. `evaluation_value` is not a score, and `ranked` stays false. `verify-sealed` hashes one custody plan with the existing file hash. Other plans are not read. `collection custody` does not become a rehash. Nothing is downloaded. `collection export` writes that stored list to a new schema 1 file and does not change the catalog. `collection diff` compares one schema 1 file with the stored list and does not change either. Storing does not apply the recorded fields or delete archived revisions. A representation name on an entry is recorded and not applied. Category names and a category ceiling are recorded and the ceiling is not charged. An evaluation attribution is recorded and not ranked. An evidence statement is recorded and not checked. A provenance statement is recorded and not verified. A declared base is recorded and is not treated as preserved. A repeated declared base is listed and not merged. Each installation still has to resolve current source and license evidence itself. `collection custody` lists the matching plan ids in plan id order. A replica row is not listed. Nothing is dropped or merged.

### Optional discovery

Discovery searches explicit constraints. Worries reorder results. A local language model may supply ranking hints; it never authorizes transfers.

Report incomplete pagination, deferred requests, missing metadata, and unknown licenses. Absence from a scan is not proof of deletion. A cloud ranker is outside the initial release.

## Defaults

| Setting | Initial policy |
| --- | --- |
| License allowlist | MIT and Apache-2.0 with captured matching evidence. |
| Source | Official public Hugging Face repositories. |
| Gated/private models | Disabled; future authorized opt-in. |
| Weights | Publisher safetensors at original precision, one selected representation. |
| Executable repository files | Excluded; preservation never executes them. |
| GGUF | Explicit opt-in, with preservation limitations displayed. |
| Approval | Manual; bounded automatic rules are opt-in. |
| Discovery | Off until configured. |
| Old revisions | Retained; no automatic pruning. |
| Network calls | Only commands that explicitly resolve or fetch sources. |
| Inference/GPU | Not required. |
| Schedule | Native OS jobs installed only after preview and user request. |

Excluded dependencies can make a model incomplete for offline use. Hash matching never implies runnability.

## Maintenance experience

Plans distinguish download bytes, final stored bytes, replica bytes, and temporary workspace. Status includes reservations and verification age.

Full drives give instructions to add storage. Missing drives show saved contents and replica locations. Failed transfers offer resume. Damaged revisions offer repair pinned to the original identity.

Text and JSON come first. A local web UI comes after the stable CLI. No remote dashboard or subscription is required.

## Scope

The first release supports local catalogs, mounted volumes, one writer, explicit watches, preservation bundles, verification, replicas, recovery, and native scheduling. Build order is the wave list in the [roadmap](roadmap.md). `watch` can fetch one granted revision on an operator-invoked pass. `schedule` can preview a weekly timer for that pass and install it only after the preview digest matches. A live scheduler was not enabled by the fixture.

`collection preview` lists an explicit shared file, and `collection store` writes that list into the catalog. `collection list` pages the stored names and does not rank them. `collection remove` deletes one stored name and does not delete archived revisions. `collection custody` counts catalog rows with the same repository and revision text and does not hash or download them. A 40-character lowercase hex revision is labeled `commit`. Any other accepted revision is labeled `name`. The label is not a resolution. When every matching manifest records `size_bytes` and those totals agree, the report shows that sum. A missing manifest or disagreeing totals stay unmeasured. `collection custody` charges a written category ceiling only when every entry that names it has `measured_bytes`. The same measured total can count toward each category that entry names. An unmeasured entry is left out, a partial sum is not reported, and no size is invented. Preview, store, and show still leave `applied` false. `discover` searches the reviewed source catalog for an explicit role, status, or source id. Matches stay in catalog order. It does not rank them, contact a network, or approve a transfer. The `[discovery] enabled = false` sketch is not a command. `collection select` keeps stored entries whose category, representation, or evaluation text matches exactly. Stored order is kept. `evaluation_value` is not a score, and `ranked` stays false. `verify-sealed` hashes one custody plan with the existing file hash. Other plans are not read. `collection custody` does not become a rehash. Nothing is downloaded. `collection export` writes that stored list to a new schema 1 file and does not change the catalog. `collection diff` compares one schema 1 file with the stored list and does not change either. Ranking, a hub model search, MCP, a local UI, other sources, datasets, software releases, and runtime kits remain later. `discover` searches only the reviewed source catalog. They consume the same contracts. `collection custody` lists the matching plan ids in plan id order. A replica row is not listed. Nothing is dropped or merged.

Public hosting, distributed write access, inference serving, and a mirror of the entire hub are outside the initial scope.

## Usability target

On a clean machine on each OS, a new user should create an understandable plan within ten minutes of obtaining the binary. This is a target, not a measurement; transfer duration is excluded.

Record confusing steps and improve defaults before expanding features.
