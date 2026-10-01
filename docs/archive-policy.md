# Personal archival policies

This is a product and engineering contract for future collection maintenance. Current commands can inspect sources and save source reviews, but cannot download or maintain a collection end to end. See [implementation status](implementation-status.md).

## The archive you actually want

An owner should be able to say: "Use up to 500 GB on this disk to retain publisher-original coding and vision models, plus selected modified variants. Prefer strong documented results, preserve earlier copies, and tell me when a new original will not fit."

The same policy scales to a 2 TB NAS. Disk capacity, the portion assigned to ModelPrepper, free-space reserve, transfer allowance, and replica targets are separate settings. A 2 TB device does not imply 2 TB of available archival space.

Explicit selections work without discovery, a leaderboard, or an agent. A collection remains understandable after the recommendation service disappears.

## Originals before derivatives

Default archival priority is the publisher's original release, including its actual tensor precision and required assets. FP16 and BF16 originals preserve information that a lossy quantization cannot restore. If the original is FP8 or mixed precision, retain that original rather than fabricating a larger "full precision" copy by upcasting it.

Store a quantization, fine-tune, merge, adapter, or abliterated release as a separate artifact. Record its declared base, exact base revision when established, conversion or training provenance when available, and missing evidence. A variant's existence does not prove that its base is preserved. A model described as uncensored has an attributed description, not a verified behavioral guarantee.

The planner offers three explicit representation policies:

| Policy | Behavior when the original does not fit |
| --- | --- |
| Original required | Defer the candidate and show the additional capacity required. |
| Original preferred | Offer a clearly labeled derivative as a separate decision; never substitute silently. |
| Selected derivative | Preserve the user's chosen artifact and report any missing original or base. |

Publisher-original status needs evidence. A filename, repository name, or model-card dtype claim alone is insufficient. Qualification must check bounded safetensors headers, dtype/shape declarations, shard closure, source identity, and captured publisher evidence without executing repository code. Current inspection does not verify tensor headers or establish that every selected file is original precision.

For scale, 40 billion two-byte parameters alone occupy about 80 GB before supporting files. A 500 GB budget cannot hold every frontier original. Mixture-of-experts storage follows all stored expert parameters, not just the number active during inference. Plans use actual file sizes, not parameter-count estimates, for admission.

## Categories and filters

Collections can target several independent dimensions:

| Dimension | Examples |
| --- | --- |
| Task | General text, coding, reasoning, image-to-text, image generation, video generation, speech, embeddings. |
| Release kind | Base, instruction-tuned, reasoning, fine-tune, merge, adapter, quantization. |
| Provenance | Publisher original, documented derivative, unknown lineage. |
| Owner preference | Explicit publishers, languages, architectures, card terms, declared uncensored or abliterated variants. |
| Evidence | Allowed license, complete dependency inventory, known precision, dated independent evaluation. |
| Resource bounds | Per-revision bytes, category ceiling, replica cost, transfer period, free-space reserve. |

Task metadata is a discovery hint. Each layout still requires its own preservation adapter and dependency closure. Video and diffusion families can depend on multiple repositories, text encoders, VAEs, processors, or custom pipelines. Until that closure is supported, show unsupported_layout rather than claiming a complete archive.

Owner-selected terms carry no centrally supplied political category list. Changes in access are events to record, not a reason to delete local copies or infer why a government or publisher acted.

## Ranking evidence

Model hosts, mirror recovery, and adapter qualification are covered separately in the [source catalog](model-sources.md). A discovery/ranking provider does not automatically become a trusted download source.

Hugging Face model cards support task metadata and structured evaluation results, including self-reported results. Treat these as attributed claims and preserve their source. See the [Hub model-card documentation](https://huggingface.co/docs/hub/en/model-cards).

Evaluation sources are optional, replaceable adapters. For each observation retain provider, retrieval time, benchmark name/version, score direction, evaluated model identity, precision, prompting/harness settings when available, and limitations. If a score cannot be connected to an exact downloadable revision, display that uncertainty instead of transferring the score to a similarly named release.

The [EleutherAI evaluation harness](https://github.com/EleutherAI/lm-evaluation-harness) is one source of reproducible language-model evaluation methods. [Artificial Analysis documents its intelligence methodology](https://artificialanalysis.ai/methodology/intelligence-benchmarking) and a separate [openness methodology](https://artificialanalysis.ai/methodology/openness-index). These are reference sources, not required dependencies, endorsed model lists, or permission to scrape their services. Verify each adapter's access and data terms before implementing it.

Compare like measurements. Text scores do not rank video models. Results for a hosted model, a different quantization, or a different harness do not establish the preserved artifact's score. Popularity and download counts are separately labeled signals, not substitutes for capability, openness, or rights.

Keep a dated ranking snapshot with each selection explanation. If the source fails, retain the last snapshot with a stale label and continue explicit watches. Incomplete scans never imply disappearance. A changed ranking never evicts an archived revision automatically.

## Bounded portfolio selection

Use a deterministic, inspectable selection algorithm before an optional agent. It should:

1. Reserve space for explicitly pinned, protected bundles and required in-flight operations.
2. Deduplicate identical candidate identities across categories without inventing physical deduplication savings.
3. Apply access, rights, layout, dependency, and per-revision gates.
4. Include original representations and dependencies in the candidate's true cost.
5. Allocate owner-defined category ceilings or minimum coverage goals, then apply a documented stable order within each category.
6. Prefer missing capabilities, confirmed originals, and independent replicas according to explicit priorities; use immutable identity as a final tie-breaker.
7. Report selected, deferred, unsupported, and awaiting-review candidates with reasons and the unused budget.

Category membership may overlap. Reports distinguish category attribution from physical bytes so one model is not charged twice to the same disk. Replica copies, staging, and independently stored derivatives do consume additional physical capacity. NAS RAID is not an independent second copy.

Recompute after every completed reservation or admission change. Concurrent jobs must not each spend the same free bytes. Report policy ceilings separately from a filesystem quota: current configuration is not an OS-enforced quota, and payload accounting is not implemented yet.

## Latest without losing history

"Latest" means a tracked source reference last successfully resolved at a stated time. Preserve the exact commit. A changed branch creates a new proposal; it never mutates the previous bundle.

Retain old sealed revisions by default. A full archive pauses new admission and explains the choices: add capacity, change priorities, explicitly select a derivative, or review a pruning plan. New data must be verified and durably published before any separately authorized retirement. Never delete the only verified copy to make room for an unverified replacement.

Future retention rules need protected pins, minimum verified-copy counts, explicit grace periods, byte savings, and a preview of affected identities. Pruning is a separate operation with its own approval, recovery design, and tests. Ranking updates cannot authorize it.

## Optional agents

An agent may translate a request into a draft policy or suggest candidates from bounded saved metadata. Show the filters and ranking sources it selected before saving the policy. Its output is untrusted data subject to the same strict parser and deterministic planner.

The agent has no credentials, arbitrary URL fetcher, shell, filesystem writer, approval authority, or ability to prune. Repository descriptions cannot expand its tools or override owner settings. Preserve the draft, explanation, model/version when used, and final owner-reviewed policy so decisions remain reproducible without the agent.

## Preparedness acceptance drills

- Resolve a watched branch, preserve it, then simulate source removal. Existing bundles remain intact and independently inspectable.
- Disconnect the hub and ranking services. Local inventory, full verification, replicas, and reconstruction still work.
- Fill a test allocation. A new original is deferred with an exact explanation; no silent downgrade or deletion occurs.
- Lose the catalog. Rebuild from documented manifests and expose conflicting or incomplete evidence.
- Corrupt a replica. Verification detects the damage, and repair uses a verified identity rather than a moving branch.
- Unplug a NAS or removable disk. No empty mount directory is adopted, and reservations retain the expected disk identity.
- Inject hostile card text and inconsistent leaderboard names. They cannot authorize a transfer, run code, or misattribute evaluation results.

These are release gates for maintenance and discovery, not claims about the current prototype. Priority and persona mapping are in [user experience](user-experience.md) and the [roadmap](roadmap.md).
