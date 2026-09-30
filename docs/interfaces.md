# Interfaces

The executable implements config init/check, init with an existing config, offline status, resolve for public models, and plan against an untrusted local inventory. Resolve fetches bounded metadata and license evidence; it never downloads weights. Init and status currently cover catalog and disk ownership only. See [contracts.md](contracts.md). The table below describes the target interface; other commands and broader behavior remain proposed.

## CLI

| Command | Behavior | Source network? |
| --- | --- | --- |
| init | Create validated config, local catalog, and adopt volume. | No |
| doctor | Check local paths, permissions, schemas, filesystem support. | Only explicit source check |
| watch add/list/remove | Manage preservation targets; removal retains copies. | No, resolution happens in plan |
| plan | Resolve watches and produce immutable plans. | Yes |
| decide | Approve or reject an existing plan id. | No |
| pull | Run approved queued transfers or resume jobs. | Yes |
| status | Inventory, jobs, space, replicas, readiness. | Only explicit source check |
| verify | Quick metadata or full payload check. | No |
| repair | Plan recovery from replicas or pinned source. | Only explicit source recovery |
| replica | Copy, rehash, and record a second volume copy. | No hub access |
| export/import | Portable bundles and inspected adoption. | No |
| catalog backup/recover | Consistent snapshot or new catalog from manifests. | No |
| volume add/list | Adopt storage and show availability. | No |
| schedule preview/install/remove | Manage requested native OS jobs. | No |
| mcp | Optional local adapter to application commands. | Only delegated source operations |

Every mutating operation supports an inspectable plan where meaningful. JSON output is a versioned contract. Progress goes to stderr so stdout remains parseable. No interactive prompts occur in scheduled/noninteractive commands; ambiguous decisions stay pending.

Document stable reason codes and exit semantics in M0. Expected no-work conditions are successful outcomes with reason codes; damaged files and invalid configuration are nonzero failures.

## Configuration sketch

Illustrative future configuration. The current validator accepts only the subset in [examples/config.toml](../examples/config.toml); discovery and worries below are not implemented:

```toml
schema_version = 1
catalog_path = "C:/ModelVault/catalog"

[licenses]
allow = ["mit", "apache-2.0"]

[storage]
default_volume = "home-disk"
max_stored_bytes = 500_000_000_000
min_free_bytes = 20_000_000_000

[transfer]
max_transfer_bytes_per_period = 80_000_000_000
period = "utc-week"
max_parallel_files = 2

[approval]
mode = "manual"

[files]
profile = "publisher-safetensors-v1"
include_repo_code = false
include_gguf = false

[[volumes]]
label = "home-disk"
path = "E:/Models"

[[watches]]
source = "huggingface"
repo_type = "model"
repo_id = "HuggingFaceTB/SmolLM2-1.7B-Instruct"
revision = "main"
update_policy = "propose"

[discovery]
enabled = false

[worries]
order = ["category_pressure", "publisher_gone", "host_dependency"]
narrative = "Keep useful published models that may become harder to obtain."

[worries.signals.category_pressure]
card_terms = []
```

Replace paths for your OS. Tokens come from explicitly selected environment or OS credential sources, not config, command-line arguments, or manifests.

The volume label is user-facing. volume.json UUID is the identity. Automatic rules require a separate explicit rule with source, license, profile, and size bounds; a size threshold alone is insufficient.

## Scheduling

Native OS jobs execute the binary with an explicit config path. A worker holds the writer lock, drains eligible jobs within budget, and exits. The scheduler does not encode quota policy.

Plans can be generated independently of transfer. Schedule installers show commands, paths, triggers, and removal instructions before changing the OS.

## MCP

Implement only after the CLI and recovery contracts pass. stdio is the first transport. Confirm the supported [MCP specification](https://modelcontextprotocol.io/specification/latest) and [Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) at that milestone.

Read tools expose status, plans, volumes, and jobs. Mutation tools decide an existing plan or queue approved work. An agent cannot invent download URLs or bypass the planner. It may add a watch only through the same validation as CLI, when authorized.

MCP queues persistent work; the CLI worker or OS scheduler owns transfers. Disconnecting a client does not delete queued jobs or mark unfinished work complete. Do not spawn untracked background children to simulate durability.

Remote HTTP access is deferred until an explicit authentication/TLS/origin design exists. Localhost binding is not a remote-access solution.

## Agent package

An optional package may provide instructions and MCP configuration following the [Agent Plugins specification](https://agent-plugins.org/specification). Validate the exact supported format when implemented.

Instructions require reading status and the concrete plan before decisions. Prior user authorization may establish approval scope; do not repeatedly request permission already granted. Vault data is user-owned storage, never disposable plugin state.
