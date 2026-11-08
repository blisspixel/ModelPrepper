# Getting started with the prototype

Read [current status](implementation-status.md) before setup. This version initializes and inspects a vault. `fetch` can seal one granted revision, and `replica`, `rebuild`, and `import` can copy or record local bytes. `watch` can fetch a new commit only when that plan already has a grant. These examples do not download weights and do not run a watch.

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

Use a local catalog filesystem and separate storage directories. ext4, APFS, and NTFS are the baseline. btrfs, xfs, tmpfs, and zfs also work locally. A network share cannot hold the catalog. FAT and exFAT are refused. First initialization adopts empty directories. `import` records an existing local folder as a baseline. The identity marker connects each storage directory to this vault. `status` names the filesystem of a mounted volume.

Storage/transfer limits are configuration, not preallocated space. Init stays offline even with configured watches. Status reports free bytes for a matching mounted volume. `budget` and `kit` compare pinned sizes and do not reserve space or account for jobs.

## Inspect a source

```text
cargo run --locked --release -- resolve --repo Qwen/Qwen2.5-0.5B-Instruct --revision main
```

Only public, ungated official Hugging Face model repositories are supported. A branch is resolved to an exact commit before evidence is read. Output includes the pinned inventory, file roles, license evidence, layout assessment, and transfer_authorized=false.

Matched evidence meets the narrow current text matcher. Missing/conflicting evidence needs review. Configuration checks do not establish runtime compatibility. This example demonstrates inspection, not a workload or hardware recommendation.

A terminal shows a short text verdict. Redirects, pipes, and `--json` stay on one compact JSON object unless you pass `--pretty`. A repository Hugging Face answers with HTTP 401 is `repository_not_found_or_private`. A missing branch, tag, or commit is `revision_not_found`.

## Verify a copy you already have

```text
cargo run --locked --release -- verify-local --repo Qwen/Qwen2.5-0.5B-Instruct --revision main --path ~/Downloads/qwen
cargo run --locked --release -- adopt --config config.local.toml --plan <plan_id> --path ~/Downloads/qwen --copy
```

`verify-local` only reads the local folder. `--repo` still resolves the commit from the publisher. With `--plan` and `--config` it uses the saved review and does not use the network. Exit status is non-zero when a required file is missing or does not match, or when an optional file is present but damaged. The message names that file. An optional file that is absent is reported as excluded and is left out of the copy. `adopt` requires a pending review whose license evidence matched, copies or moves only that frozen matched set through a staging journal, re-checks the destination, and records `upstream-verified (local source)`. A successful adopt can print a preservation receipt. A mismatch does not. That is not a local-baseline import. `import` records bytes without this upstream proof and keeps the provenance `local-baseline`. Interrupted adoption resumes; a failed hash removes the staging copy and does not publish a partial tree.

`local list` reads Ollama manifests from `OLLAMA_MODELS`, `~/.ollama/models`, and `/usr/share/ollama/.ollama/models`. It does not contact Ollama and does not change the store. Without `--config` every tag is `not_checked`. With `--config` the command compares manifest digests with digests already recorded in the catalog and names missing roles when the match is partial. A recorded match survives an unplugged disk because it is catalog history, not a live hash of the volume. A missing store is explained in the output instead of failing with only an error code.

## Preview local metadata

```text
cargo run --locked --release -- plan --config examples/config.toml --inventory tests/fixtures/inventory.json --available-bytes 500000000000
```

This inventory is synthetic. Every local preview contains local_inventory_untrusted and transfer_authorized=false. Available bytes are a supplied observation, not a reservation. A GGUF derivative preview requires profile `selected-gguf-v1` and one `--select` path per file. `budget` and `kit` compare pinned sizes with the storage ceiling. Neither command contacts a publisher, reserves bytes, or authorizes a transfer. See [contracts](contracts.md).

```text
cargo run --locked --release -- budget --config examples/config.toml --request examples/budget.json
cargo run --locked --release -- kit --config examples/config.toml --kit examples/kit.toml
```

## Check offline readiness

```text
cargo run --locked --release -- readiness --recipe examples/recipe.json --path examples/readiness
```

The recipe is local JSON. The directory is the retained files. A missing directory is reported and is not created. The report keeps integrity, capability closure, rights, and readiness separate. Matching small files still leave readiness `untested` until a real network-disabled drill is recorded. A package name or URL is not a retained runtime. A retained runtime larger than 2 MiB is `unverified`, and a present runtime that does not match its declared bytes is `damaged`. Neither is called missing. See [preservation](preservation.md).

## Shared collection

`collection preview --file <file>` lists the repositories in a shared TOML file. An entry may name `original-required`, `original-preferred`, or `selected-derivative`, one or more kit slots, and a declared base. The collection may name a byte ceiling for a kit slot. An entry may also name an evaluation attribution. The preview does not apply that name, charge that ceiling, preserve the base, rank by the attribution, check an evidence statement, or verify a provenance statement. A repeated declared base is listed and not merged. It does not resolve those revisions or download them. `collection store --config <file> --file <file>` writes that parsed list into the catalog. `collection show --config <file> --name <name>` reads it back. `collection list --config <file>` pages the stored names in name order and does not rank them. Replacing a stored name does not delete archived revisions, and the stored fields are still not applied. See [examples/collection.toml](../examples/collection.toml). Preview does not import a collection.

## Weekly timer

`schedule preview` shows the weekly timer that would run one `watch` pass, including the command, the config path, the stage path, and a digest. `schedule install` writes that timer only when the digest matches. `schedule remove` deletes that timer and no other job. This guide does not install one. The CI fixture injects the host command and does not enable a live scheduler. Approval stays manual.

## Common outcomes

To retain pinned source evidence and record rejection, use the [source review workflow](source-reviews.md). For category portfolios, original precision, and NAS budget planning, read [archival policies](archive-policy.md). Native release installers and their publication limits are in [installation](installation.md).

| Result | What to do |
| --- | --- |
| directory_not_empty | Choose an empty directory. Collection import is a future operation. |
| writer_busy | Let the active process finish. A filename alone does not establish an active kernel lock. |
| volume_configuration_changed | Restore the configured label/path. Relocation is not implemented yet. |
| offline or identity_missing | Reconnect the expected disk. Init never recreates a ready vault's missing volume. |
| identity_mismatch | Check the actual disk and marker. Do not copy a different disk's identity to bypass the check. |
| repository_not_found_or_private | The hub returned HTTP 401. The repository is missing or private. |
| revision_not_found | The branch, tag, or commit was not found. |
| overlapping_vault_paths | Choose separate catalog and volume directories. config check rejects this before init. |
| license_needs_review | Adoption will not store a commit whose license evidence did not match. |
| access_restricted | Choose an authorized public source supported by this prototype. |
| unreviewed or conflicting license | Inspect the original evidence. A tag alone cannot clear it. |
| notice_present | The license text matched and a NOTICE file was captured. Read that file. It is not a match and not a conflict. |
| invalid_range or invalid_checkpoint | The library experiment rejected inconsistent bytes/identity. No CLI download workflow uses it yet. |
| transfer_backend_unqualified | A saved source review cannot be approved yet; durable transfer/accounting gates remain open. |
| stale_policy | Propose a new review using the current configuration. |
| decision_conflict | Inspect the saved final decision; it cannot be changed or reset silently. |

A terminal prints text, including `error:` and `next:` for failures. Pipes and `--json` print JSON on stdout or, for failures, on stderr. Application errors exit 1. Argument errors exit 2. Help and version are text. A preview can succeed while reporting blockers.

The [roadmap](roadmap.md) prioritizes transfer, persisted decisions, publication, and recovery. The [user experience plan](user-experience.md) covers all four user types. Read [responsibilities](notice.md) before relying on an archive.
