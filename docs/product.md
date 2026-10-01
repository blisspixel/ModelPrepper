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

Shared collection definitions are inspectable data. Import previews entries and never starts downloads. Each installation resolves current source and license evidence itself.

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

The first release supports local catalogs, mounted volumes, one writer, explicit watches, preservation bundles, verification, replicas, recovery, and native scheduling.

Later candidates are discovery, collections, MCP, a local UI, other sources, datasets, software releases, and runtime kits. They consume the same contracts.

Public hosting, distributed write access, inference serving, and a mirror of the entire hub are outside the initial scope.

## Usability target

On a clean machine on each OS, a new user should create an understandable plan within ten minutes of obtaining the binary. This is a target, not a measurement; transfer duration is excluded.

Record confusing steps and improve defaults before expanding features.
