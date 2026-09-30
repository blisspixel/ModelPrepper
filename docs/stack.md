# Stack and dependency policy

## Runtime requirement

Ship one native Rust binary for Linux, macOS, and Windows. No mandatory Python, Node, Java, Docker, external downloader, inference server, or GPU.

Rust is a deliberate implementation choice, not a claim that other languages cannot ship native utilities. Minimize dependencies without replacing mature TLS, SQLite, or transfer code with fragile custom implementations.

## Adopted components

The prototype locks eleven direct normal dependencies. Each now has an implemented purpose. tempfile is development-only. Cargo.lock is the source of exact versions; disabled default features avoid optional stacks where practical.

| Component | Implemented purpose | Enabled scope |
| --- | --- | --- |
| clap | Native CLI parsing and help. | Derive, standard library, help, usage, error context. |
| serde, serde_json, toml | Strict configuration and JSON contracts. | TOML parse/display; no alternate config framework. |
| sha2 | Policy/preview identities and local file SHA-256. | SHA-256 implementation. |
| sha1 | Verify Git blob identities, including object headers. | Integrity compatibility, not new signatures or trust roots. |
| ureq | Bounded blocking source HTTP. | Rust TLS; compression and automatic redirects disabled. |
| url | Parse and constrain HTTP redirect origins. | Standard URL parser including its IDNA dependencies. |
| rusqlite | Local persistent vault/volume catalog. | Bundled SQLite; default extras disabled. |
| fs4 | Measure actual free bytes on matching volumes. | Default extras disabled; locking uses the Rust standard library. |
| uuid | Durable vault and disk identities. | OS-random version 4 identifiers. |

Direct package metadata permits MIT or Apache-2.0 for these libraries; rusqlite is MIT. Transitive dependencies also include permissive certificate/data licenses. [deny.toml](../deny.toml) checks dependencies across the five supported native targets. Ring and UUID require different getrandom APIs; this one documented duplicate-version exception avoids patching TLS internals for cosmetic deduplication. cargo-audit checks the full lockfile, including unsupported-target entries.

The Windows release was built and inspected locally after adding HTTP and SQLite. It is approximately 5.6 MB and imports only Windows operating-system DLLs. Neither an external database library nor a separately installed Visual C++ runtime appears in the import table. This is artifact evidence on the current machine, not a substitute for clean-machine qualification.

No async executor, Python, Node, OpenSSL installation, external downloader, or mandatory Xet runtime is included. Bundled SQLite and Rust TLS add build-time native compilation requirements for contributors; users receive the finished executable.

## Remaining candidates

| Need | Candidate | Constraint |
| --- | --- | --- |
| CLI | clap | Keep enabled features limited to actual usage. |
| Configuration and manifests | serde plus TOML/JSON parsers | Strict schema validation and bounded inputs. |
| Catalog | rusqlite with bundled SQLite | Avoid system SQLite installation; local database only. |
| HTTP/TLS | Official client stack, Rust TLS where feasible | Avoid two competing HTTP runtimes. |
| Transfer | Official [hf-hub](https://github.com/huggingface/hf-hub), native Xet | Prove ordinary files, LFS, Xet, resume, and workspace limits. |
| Integrity | Maintained SHA-256 implementation | Record algorithm semantics explicitly. |
| Async execution | Shared runtime needed by the transfer client | Bound concurrency; do not add a second executor. |
| Locking | OS-backed locking through a small maintained abstraction | Cross-platform ownership; no distributed-lock claim. |
| Optional MCP | Official [Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) | Feature-gated after core reliability. |

The table describes future evaluation, not additional installed packages. Avoid adding a second HTTP/TLS stack merely to obtain an optional transfer optimization.

## Dependency admission

Record purpose, license, maintenance status, supported targets, features, native libraries, and replacement cost for each dependency.

Prefer a maintained library when it materially reduces protocol or integrity risk. Reject dependencies added only for minor convenience when the standard library is adequate.

Commit Cargo.lock and a pinned tested toolchain. Review updates with changelogs and fault tests. cargo-audit and cargo-deny check vulnerabilities, licenses, sources, and duplicate versions in development/CI. Avoid exact SDK version claims in prose that drift independently of the lockfile.

A bundled library is a build dependency, not a required separate user installation. OS APIs and documented platform libc requirements remain part of the support contract.

## Distribution

Required targets:

- Windows x86-64 MSVC.
- macOS ARM64 and x86-64.
- Linux x86-64 and ARM64 with a documented libc baseline.

Build and smoke-test actual artifacts. Cross-compilation alone does not prove they run. A musl Linux build is evaluated for appliance compatibility; it is not universally promised.

No mandatory installer, administrator rights, service daemon, or container. Per-user OS scheduling is enough for the initial product.

Windows MSVC builds enable static CRT linkage through .cargo/config.toml so the intended executable does not require a separately installed Visual C++ runtime. Linux and macOS still use their documented operating-system libraries. Actual clean-machine qualification remains a release gate.

## Optional programs

A local ranking model and its server are entirely optional later additions. The utility remains useful without them.

Inference runtimes are tools for offline readiness drills, not linked requirements for preservation. Runtime kits are optional artifacts with their own rights and platform constraints.

## Development tools

Rust toolchain, formatting, lint, test, coverage, and documentation tools may be required for contributors and CI. They are not installed beside the shipped binary.

The current documentation uses Node-based lint and link checks. No Node component belongs in the product runtime.

## Decisions that require evidence

- Exact Rust client and native transfer backend.
- Cross-platform Xet cache and workspace bounds.
- TLS implementation and system certificate-store behavior.
- SQLite build options and durability settings.
- Dependency feature flags and artifact size.
- Platform signing/provenance and offline verification instructions.

M1 resolves transfer feasibility before committing the whole application to an experimental API.
