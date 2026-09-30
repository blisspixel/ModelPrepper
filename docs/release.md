# Public repository and release preparation

The project name is ModelPrepper. Its code and documentation use the [MIT license](../LICENSE). Publishing the repository does not imply that a preservation release is complete.

## Repository checklist

- A concise README explains current capabilities, installation/build steps, limitations, responsibilities, and links to detailed docs.
- One CI workflow covers native tests/builds, documentation, line/branch coverage, and dependency checks. A final aggregate job represents the required check on main.
- A workflow badge references the actual repository and main branch. A passing claim requires an observed successful run for the current commit.
- CLI captures come from the current executable and are described accurately. Proposed screens are never presented as implemented UI.
- Contributions use the existing MIT terms. Model payloads, live vaults, credentials, dependency caches, and build artifacts are excluded from Git.

## Release gate

Before publishing downloadable release artifacts, qualify the supported OS/architecture combinations, native dependencies, filesystem behavior, workspace bounds, crash recovery, manifest reconstruction, replicas, and offline-load drills described in [quality.md](quality.md).

Publish artifact checksums and the source commit. Preserve applicable third-party dependency notices. Signing/provenance should be documented before claiming authenticated releases; a checksum alone is not a signature.

Do not tag a stable release while core approvals, accounting, transfer publication, or recovery remain experiments. A public source repository can openly host that work with a clear prototype status.
