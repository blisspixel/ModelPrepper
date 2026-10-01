# Contributing

ModelPrepper is an MIT-licensed Rust preservation utility under active development. Read the [current status](docs/implementation-status.md), [contracts](docs/contracts.md), [roadmap](docs/roadmap.md), and [user experience plan](docs/user-experience.md) before proposing broader features.

## Development

Install Rust using the pinned rust-toolchain.toml. Node 22.19 or newer is used only for documentation checks.

Native installer fixture tests and packaging use Python's standard library. Install Ruff 0.15.16 for those scripts' lint/format checks. Screenshot rendering separately uses optional Pillow. None is a product runtime dependency.

```text
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
python scripts/test-installers.py --binary target/release/modelprepper.exe
ruff check scripts/package-native-release.py scripts/test-installers.py
ruff format --check scripts/package-native-release.py scripts/test-installers.py
npm ci --ignore-scripts
npm run check
```

Coverage and dependency checks are documented in [quality.md](docs/quality.md). Owned Rust code must measure at least 80% line and branch coverage. Fault tests matter beyond the percentage. Ordinary tests require no credentials or real model weights.

Use target/release/modelprepper for installer tests on Linux/macOS. Test installers use temporary workspace directories, offline archives, and no PATH updates.

## Change expectations

Keep runtime dependencies small and justified. Reuse reviewed TLS, database, hash, and platform implementations. Avoid mandatory services, language runtimes, or inference servers.

Changes to identities, source selection, rights evidence, accounting, or publication need focused invariant and failure tests. Preserve versioned contracts or document an explicit migration. Keep current behavior separate from proposed behavior.

Update relevant docs and CLI captures when public behavior changes. Do not label a workflow, release artifact, platform, or recovery path as qualified without observed evidence. Keep commits free of credentials, model payloads, private paths, and generated build directories.

Explain the problem, resulting behavior, validation, and material limitations in a pull request. Small coherent changes are easier to review than unrelated cleanup.

## Licensing

Contributions to the project's code and documentation are offered under the existing [MIT license](LICENSE). Do not contribute third-party artifacts without their required rights and notices. Model licenses remain separate from the software license.
