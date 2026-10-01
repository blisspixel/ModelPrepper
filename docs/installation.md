# Native installation

Installer scripts are implemented for release binaries and have offline fixture tests. No downloadable release has been published yet. Online commands below are prepared for a future qualified release and cannot install a binary today. Build from source using [getting started](getting-started.md) until qualified artifacts are available.

## One-command release installation

After the first qualified release, Windows PowerShell will use:

```powershell
irm https://raw.githubusercontent.com/blisspixel/ModelPrepper/main/install.ps1 | iex
```

Linux and macOS will use:

```sh
curl -fsSL https://raw.githubusercontent.com/blisspixel/ModelPrepper/main/install.sh | sh
```

These follow the native bootstrap pattern used by [Codex CLI](https://github.com/openai/codex) and [Claude Code](https://code.claude.com/docs/en/setup). ModelPrepper selects a platform binary, resolves a stable release, verifies its archive SHA-256, validates archive contents, checks the executable version, and replaces an installation only after those checks succeed. No Rust, Python, Node, model server, or GPU is needed to install or run it.

The bootstrap and checksums come from project-controlled GitHub hosting. Checksums detect mismatched bytes; they are not an independent signature or protection against compromise of that hosting account. For stricter custody, review and retain the script, pin its commit, and obtain trusted checksums separately. Signed release provenance is a separate release gate.

## Paths and platform boundaries

| Platform | Default executable | Installer archive target |
| --- | --- | --- |
| Windows x64 | %LOCALAPPDATA%\ModelPrepper\bin\modelprepper.exe | x86_64-pc-windows-msvc |
| Linux x64 / ARM64 | ~/.local/bin/modelprepper | x86_64 / aarch64-unknown-linux-gnu |
| macOS Intel / Apple Silicon | ~/.local/bin/modelprepper | x86_64 / aarch64-apple-darwin |

This table identifies archive formats, not qualified production releases. Linux glibc baseline, supported OS versions, and each architecture need actual release testing. Musl, Windows ARM64, and arbitrary NAS appliances are not implied support. Unsupported architectures fail with an explanation.

Windows updates current-process and user PATH unless -NoPath is supplied. Open a new terminal for other processes to see the change. Linux/macOS print an instruction if the directory is outside PATH and do not edit shell startup files. Installers do not request administrator privileges, create a vault, download a model, schedule jobs, or change collections.

## Pin a version or directory

Download and review the script before passing options. These examples assume a published v0.1.0 and reviewed local scripts:

```powershell
.\install.ps1 -Version v0.1.0 -InstallDir C:\Tools\ModelPrepper -NoPath
```

```sh
sh install.sh --version v0.1.0 --install-dir "$HOME/tools/modelprepper"
```

Prerelease tags can be selected explicitly. The default latest endpoint selects stable releases. There is no source-build fallback or background upgrade. Rerun with a chosen version to upgrade or downgrade. Model data remains separate from the executable.

Linked installation ancestors or executable destinations are rejected. Checksum or version failure retains the existing binary. Windows replacement may fail if the executable is in use; stop it and retry. Concurrent installations into one directory are not qualified.

## Offline installation

Keep the reviewed installer, platform archive, trusted SHA256SUMS, license, dependency notices, and provenance on independent media. Pass the archive's exact trusted digest:

```powershell
.\install.ps1 -Version v0.1.0 -ArchivePath .\modelprepper-v0.1.0-x86_64-pc-windows-msvc.zip -Sha256 YOUR_DIGEST -NoPath
```

```sh
sh install.sh --version v0.1.0 --archive ./modelprepper-v0.1.0-x86_64-unknown-linux-gnu.tar.gz --sha256 YOUR_DIGEST
```

Replace YOUR_DIGEST with 64 lowercase hexadecimal characters. Offline installation makes no network requests and still checks archive contents and executable version. A trusted local binary can also be copied and run directly.

## Packaging and verification

scripts/package-native-release.py uses Python's standard library, verifies a native binary's version, creates a zip/tar.gz containing exactly the executable and MIT LICENSE with fixed metadata, and records SHA256SUMS. Installers retain modelprepper.LICENSE alongside the executable. Existing assets cannot be overwritten. Deterministic packaging does not claim reproducible compiler output.

```text
cargo build --locked --release
python scripts/package-native-release.py --binary target/release/modelprepper.exe --version v0.1.0 --output-dir target/release-assets
python scripts/test-installers.py --binary target/release/modelprepper.exe
```

Use target/release/modelprepper on Linux/macOS. The single CI workflow runs offline installer fixtures after native builds. Python is development-only. Dependency notices, provenance, architecture smoke tests, clean-machine setup, and the other [release gates](release.md) remain required before publishing downloads.
