# Getting started with the prototype

Read [current status](implementation-status.md) before setup. This version initializes and inspects a vault; it does not preserve a model end to end.

## Build and run

Install Rust and let rust-toolchain.toml select the tested toolchain. Run cargo build --locked --release from the project directory.

The binary is target/release/modelprepper on Linux/macOS and target/release/modelprepper.exe on Windows. Invoke it directly or use cargo run --locked --release -- followed by a command. Node is only needed for contributor documentation checks.

## Configure and adopt storage

```text
cargo run --locked --release -- config init --output config.local.toml --catalog catalog --volume models
cargo run --locked --release -- config check --config config.local.toml
cargo run --locked --release -- init --config config.local.toml
cargo run --locked --release -- status --config config.local.toml
```

The generator refuses to overwrite its output. Catalog and volume paths resolve relative to the config file, even from another working directory. Absolute paths are supported.

On macOS, some system paths such as /var and /tmp are symlink aliases. Relative paths resolve against the canonical config location. For explicit absolute paths, use the real filesystem location; ownership checks reject symlink ancestors.

Use a local catalog filesystem and separate storage directories. First initialization adopts empty directories; existing collection import is a future explicit operation. The identity marker connects each storage directory to this vault.

Storage/transfer limits are configuration, not preallocated space. Init stays offline even with configured watches. Status reports free bytes for a matching mounted volume; future job accounting is not implemented yet.

## Inspect a source

```text
cargo run --locked --release -- resolve --repo Qwen/Qwen2.5-0.5B-Instruct --revision main
```

Only public, ungated official Hugging Face model repositories are supported. A branch is resolved to an exact commit before evidence is read. Output includes the pinned inventory, file roles, license evidence, layout assessment, and transfer_authorized=false.

Matched evidence meets the narrow current text matcher. Missing/conflicting evidence needs review. Configuration checks do not establish runtime compatibility. This example demonstrates inspection, not a workload or hardware recommendation.

## Preview local metadata

```text
cargo run --locked --release -- plan --config examples/config.toml --inventory tests/fixtures/inventory.json --available-bytes 500000000000
```

This inventory is synthetic. Every local preview contains local_inventory_untrusted and transfer_authorized=false. Available bytes are a supplied observation, not a reservation. See [contracts](contracts.md).

## Common outcomes

| Result | What to do |
| --- | --- |
| directory_not_empty | Choose an empty directory. Collection import is a future operation. |
| writer_busy | Let the active process finish. A filename alone does not establish an active kernel lock. |
| volume_configuration_changed | Restore the configured label/path. Relocation is not implemented yet. |
| offline or identity_missing | Reconnect the expected disk. Init never recreates a ready vault's missing volume. |
| identity_mismatch | Check the actual disk and marker. Do not copy a different disk's identity to bypass the check. |
| access_restricted | Choose an authorized public source supported by this prototype. |
| unreviewed or conflicting license | Inspect the original evidence. A tag alone cannot clear it. |
| invalid_range or invalid_checkpoint | The library experiment rejected inconsistent bytes/identity. No CLI download workflow uses it yet. |

Commands emit JSON; application errors emit JSON on stderr and exit 1. Argument errors exit 2. Help/version are text. A preview can succeed while reporting blockers.

The [roadmap](roadmap.md) prioritizes transfer, persisted decisions, publication, and recovery. The [user experience plan](user-experience.md) covers all four user types. Read [responsibilities](notice.md) before relying on an archive.
