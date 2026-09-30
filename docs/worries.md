# Watchlists and preservation priorities

The first question is "what do you want to keep?" The second is "what else should we look for?" Explicit watches do not require a narrative or model ranker.

## Watchlists

Add exact repositories or pinned commits. Organize them into collections later. A watched branch proposes a new revision when it changes and retains old sealed copies.

A watch does not grant unlimited approval. License evidence, selected files, representation, capacity, and transfer budget still apply.

## Optional worries

| Id | Goal | Observable hint |
| --- | --- | --- |
| publisher_gone | Preserve work if its publisher disappears. | User's durability list and publisher history. |
| host_dependency | Reduce dependence on one download site. | No verified local copy exists. |
| category_pressure | Preserve a category the user believes faces restrictions. | User-selected terms in names, tags, or cards. |
| relicense | Preserve an allowed revision while accessible. | Captured rights evidence differs across revisions. |
| format_rot | Retain information for future conversion or study. | Missing publisher-precision representation. |
| application_dependency | Keep models needed by a local workflow. | Explicit dependency/collection declaration. |
| hardware_fit | Prioritize usable capabilities within a hardware goal. | Size and documented requirements, with uncertainty shown. |

Hints are not forecasts. An unfamiliar organization is not proof of imminent failure. A category match is not a legal or technical assessment.

category_pressure ships with no keywords. Users can choose uncensored or abliterated releases, speech models, niche research models, or any category that matters to them. Preservation copies existing releases; it does not manufacture variants.

## Ranking

1. Explicit requested revisions are reported separately from discovered candidates.
2. Within discovery, earlier worries have priority.
3. Prefer candidates that fit the remaining budgets and dependency requirements.
4. Explain matching terms, provenance gaps, redundancy, and unsupported layouts.
5. If a worry matches nothing, say so and consider the next.

Deterministic ordering remains available. An optional local ranker reads saved cards, not live arbitrary URLs, and has no tools, token, or approval authority. Its interpretation cannot expand a rights allowlist or transfer plan.

Repo names and card prose are untrusted. Escape terminal controls and bound input sizes. Ranking does not execute code from a card.

The utility does not substitute a provider's content preferences for the user's collection choices. License/access rules and file-integrity protections remain independent of viewpoint.

## Example

```toml
[worries]
order = ["category_pressure", "application_dependency", "publisher_gone"]
narrative = """
I want a local reserve of models I use and published releases that may
become harder to obtain. Preserve exact versions and retain older copies.
"""

[worries.signals.category_pressure]
card_terms = ["uncensored", "abliterated"]
```

This is an opt-in example, not a shipped hunt. The complete config sketch is in [interfaces.md](interfaces.md).

## Reports

Show accepted, awaiting review, excluded, and deferred rows with reason codes. Include byte costs and whether dependencies and replicas are satisfied.

A scan deferred by rate limiting remains incomplete. A previously public repository that cannot be reached may be deleted, private, unavailable, or inaccessible to the current account; do not infer a policy event from one failed request.
