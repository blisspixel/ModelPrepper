# Durable source reviews

Source reviews connect public inspection to the local catalog. They do not download model payloads or grant transfer approval yet.

## Workflow

Initialize the vault first, then inspect and save a public source:

```text
modelprepper propose --config config.local.toml --repo Qwen/Qwen2.5-0.5B-Instruct --revision main
modelprepper proposals --config config.local.toml list --limit 20
modelprepper proposals --config config.local.toml show --plan EXACT_PLAN_ID
modelprepper decide --config config.local.toml --plan EXACT_PLAN_ID --decision reject
```

Replace EXACT_PLAN_ID with the full returned plan_id. Propose makes explicit publisher requests without downloading weights. List, show, and decide work offline. Approval attempts fail with transfer_backend_unqualified and leave the review pending until durable transfer, workspace, reservations, and accounting qualify.

Rejection retains evidence and records a final decision for that exact review. Decisions cannot reset to pending or change to another final decision. Repeated rejection retains the original decision time.

## Identity and evidence

Reviews contain the pinned commit and complete inventory, original evidence and hashes, selected/excluded files, payload cost, minimum reserve, policy digest, vault UUID, destination UUID/label, and blockers. Creation time and observed free bytes are stored separately. Times use local-clock Unix seconds, not an external timestamp authority.

The document uses modelprepper.source-review.v1 and compact declaration-order JSON hashed with SHA-256. Sort inventory files, license paths, and evidence records first. Policy, source, evidence, destination, selection, costs, and blockers bind identity. Observation time, observed free bytes, and decision do not. A change in space sufficiency can change the captured insufficient_space blocker and produce another review.

Repeated identical inspection retains the first observation and decision. Changed policy produces a new identity; older reviews report policy_current=false, and attempted approval fails with stale_policy. The full configuration is conservatively bound, including watch changes.

Check disk identity before and after source requests. Release the writer lock during networking, then reacquire it before saving. Missing/substituted disks block proposal creation. Saved evidence remains inspectable and rejectable while the disk is offline. Branch movement never changes a pinned review.

## Trust and catalog bounds

Only the internal resolver can create a persisted review. There is no JSON-import route. Feeding resolve output to local plan retains local_inventory_untrusted. Reviews are neither imported approvals nor sealed manifests.

Reads check document hash, strict schema, domain, vault/disk ownership, and decision consistency. This detects accidental corruption relative to the id, not a malicious writer replacing both ids and documents. Transfer authorization is independently disabled even if stored fields claim otherwise.

Catalog schema 2 adds proposals through a transaction under the writer lock. Schema 1 upgrades without changing ownership UUIDs. Unknown versions fail closed. Status can trigger migration and is not read-only. Downgrading to schema-1 software after migration is unsupported; retain independent backups before upgrades.

Documents are capped at 8 MiB. List pages contain 1 through 100 summaries in ascending id order, default 20. next_after supplies the following page's cursor. Evidence is parsed one record at a time instead of retained in the list. Separate invocations do not form a snapshot; repeat a scan if proposals change during pagination.

The future executable plan needs measured bounds and a distinct qualified identity. Editing blockers cannot upgrade a review. See [contracts](contracts.md), [status](implementation-status.md), and [roadmap](roadmap.md).
