# Preservation contract

A saved weight file is not automatically a recoverable model. This contract defines what the utility promises and what it has actually proved.

## Identity

A preserved bundle is identified by source endpoint, repository type/id, resolved commit, selected representation, and file-selection profile digest.

The same commit can have several bundles, for example publisher tensors and a GGUF representation. These have distinct bundle ids. A branch name is a watch target, never archival identity.

## Bundle contents

| Item | Required evidence |
| --- | --- |
| Weight representation | Exact files, dtypes, sizes, shard index closure. |
| Tokenizer and processors | Referenced vocabulary, merges, configuration, preprocessing assets. |
| Model configuration | Architecture and generation settings as published. |
| Templates | Referenced chat or preprocessing templates. |
| Rights | Original license/notice files and captured matching evidence. |
| Source record | Requested reference, resolved identity, sanitized metadata and retrieval time. |
| Manifest | Schema version, file hashes, exclusions, dependencies, profile, operation id. |
| Use notes | Runtime requirements and available offline test results. |

Required items vary by architecture. Unsupported dependency closure is a visible limitation, not a successful readiness result.

The initial supported profile is a small set of well-understood safetensors model layouts. Broad task labels are not a guarantee of support for every image, audio, or custom model.

## Representation policy

Preserve the original publisher precision. BF16 and FP16 are common, but FP8 and mixed dtypes remain valid. File sizes come from the selected inventory, including supporting assets.

Do not reconstruct full precision from a lossy quant or claim that conversion recovers information. A quant-only bundle is allowed by explicit policy and accurately labeled. A converted local artifact records input identity, tool version, parameters, and output hashes separately.

Variants and adapters require pinned base dependencies. Unknown base identity prevents a complete dependency claim. Preserve a variant when the user explicitly selects it; default discovery may deprioritize redundant representations.

## Manifest sketch

Illustrative schema, to be finalized during the format milestone:

```json
{
  "schema_version": 1,
  "bundle_id": "canonical-plan-digest",
  "source": {
    "adapter": "huggingface",
    "repo_type": "model",
    "repo_id": "example/model",
    "requested_revision": "main",
    "resolved_revision": "full-commit-id"
  },
  "profile": "publisher-safetensors-v1",
  "rights": {
    "status": "allowlisted",
    "license_ids": ["apache-2.0"],
    "evidence_digest": "digest-of-captured-evidence"
  },
  "files": [
    {
      "path": "model.safetensors",
      "size_bytes": 123456,
      "sha256": "local-sha256",
      "upstream_hash": {
        "algorithm": "sha256",
        "value": "publisher-file-hash"
      }
    }
  ],
  "excluded": [],
  "dependencies": [],
  "sealed_at": "UTC-timestamp"
}
```

Local SHA-256 is recorded for all files. Ordinary Git blob ids use Git object hashing, not raw-file SHA-256. Xet metadata requires the client's documented interpretation. Never compare unlike algorithms.

A hash match establishes identity relative to captured metadata. It does not make an unsigned publisher cryptographically authenticated beyond the source trust used when capturing that metadata.

## Independent status dimensions

| Dimension | Example states |
| --- | --- |
| Integrity | Never checked, verified at a time, damaged. |
| Structural completeness | Required assets present, excluded dependency, unsupported layout. |
| Provenance | Captured upstream identity verified, imported local baseline, conflict. |
| Availability | Mounted, offline disk, missing replica. |
| Offline readiness | Untested, load passed, task passed, failed. |

Reports do not collapse these into one green status. Cached file size/mtime checks are labeled quick checks, not full verification.

## Offline drill

1. Choose a sealed bundle and a runtime compatible with its actual architecture and representation.
2. Record runtime version/build, operating system, hardware, driver requirements, and dependency versions.
3. Disable source network access and use an empty external model cache.
4. Load tokenizer and model from the local bundle.
5. Run a small task appropriate to its modality and record success or failure.
6. Save the procedure and result with the catalog's verification history.

The core utility does not execute arbitrary runtime commands from manifests. Early drills are documented manual procedures. A later isolated helper may automate supported runtimes with explicit consent.

A load test establishes that the specific environment worked at that time, not general model quality. Re-run after a major runtime or platform change. `readiness` never records that result. A drill note stored in a recipe is evidence from a previous procedure. It applies only when the recipe digest, runtime version, OS, architecture, disabled network, and empty cache still match, and the selected capability's retained files still hash. Any other recorded pass is stale. Fixture evidence is not a completed drill.

## Runtime recipe

`readiness --recipe <file> --path <dir>` reads one versioned JSON recipe and the local directory. Schema version 1 is the only accepted recipe. Unknown fields and unknown schema versions are rejected. The only profile is `publisher-safetensors-text-v1`. For the `text-generation` capability it declares required config, tokenizer, weight, and chat-template files. Another selected capability, such as a transcription model, is closed on its own files. A missing file blocks that capability. An unknown need or an adapter whose base commit is absent stays unknown. It is not reported as complete.

Each file and the runtime artifact says whether it is `retained`, `named`, or `remote`. A retained file has a size and SHA-256. Ordinary preflight hashes a retained file only when its declared size is at most 2 MiB. A larger file can match in length and still be unverified. The report says `retained` for a runtime only when its bytes match, `unverified` when that retained file is larger than 2 MiB, `damaged` when the present file does not match, and `missing` when it is absent. A package name or an HTTPS URL is not a retained artifact and cannot make the closure complete. Rights are echoed from the recipe as a declaration. This command does not re-check the publisher, so provenance stays `declared_only`.

The report keeps integrity, capability closure, rights, disk availability, environment, runtime retention, and offline readiness separate. There is no single ready badge. A missing directory is `unavailable` and is not created. A symlink or Windows reparse point used as the directory is `unsafe_path` and is not followed. The command does not contact a network, execute a model or runtime, or write a vault.

The manual drill remains:

1. Restore the selected files and, when one was retained, the runtime artifact to a clean directory.
2. Disable network access for the runtime and start with an empty model and package cache.
3. Assemble only from retained files. Record host drivers and libraries the recipe cannot supply.
4. Load the model and run one small task for the selected capability. Record the recipe digest, runtime version, OS, architecture, hardware, and the result.
5. Keep that record with the recipe. A later `readiness` reading can show it only while it still applies. It does not run the task again.

No catalog-independent restore is claimed. That remains the restoration milestone.

## Runtime kits

A later optional kit may preserve compatible runtime binaries, build recipes, dependency lockfiles, and redistributable packages. Keep platform-specific kits distinct from model bundles. The recipe above can name those pieces. Naming them does not package them.

A container image alone does not guarantee an offline GPU driver, compatible kernel, or portability across all OSes. A source checkout alone does not preserve all build dependencies. A lockfile alone is not a package cache.

Runtime kits are opt-in and never a runtime dependency of ModelPrepper itself.

## Long-term custody

Keep a second verified copy on separate media. Verify on a configurable byte/time budget; report unfinished sweeps explicitly. Reconstruct a catalog and restore a replica during release drills.

Schema upgrades preserve old manifests or create new versioned records. Unknown newer schemas can be listed as unsupported without modifying their payload.

Ordinary bundle files remain accessible if the program is discontinued.
