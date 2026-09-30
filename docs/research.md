# Research pass

Historical research from September 24, 2026. Implementation guidance is superseded by [the roadmap](roadmap.md) and current design documents. Claims below retain their original caveats and are not current product guarantees.

A cross-checked research run on 24 September 2026 reviewed the Devsplainers video, the public hub APIs, and how a weekly local archive should be built. Status of that run: **partial**. The corrections that changed the design are already in the architecture, the video review, and the sources list. This note is what the run could not pin down, so later work does not treat those lines as measured.

## Folded into the design

- The Software Heritage figure in the video is a proposal ([swh/meta#5099](https://gitlab.softwareheritage.org/swh/meta/-/work_items/5099)): about 12 TiB of non-LFS Hugging Face files, about 40 PiB of LFS weights left out of scope. Not a completed 12 TB code archive.
- Hugging Face list calls have no file-size filter and no last-modified date range. Size and the weekly window happen after the page comes back.
- Rate limits quoted from the Hub's September 2025 table: 5-minute windows, anonymous 500 API / 3,000 resolver requests, free account 1,000 / 5,000. The page says the anonymous and free numbers can change. Current `huggingface_hub` waits out HTTP 429.
- License cards are author YAML, including non-SPDX values (`llama4`, `gemma`, `openrail`, `other`). The LICENSE file stays the gate.
- `downloads` is 30 days, not all-time. GGUF repos inflate it. Budget safetensors shard bytes, not `usedStorage`.
- Skip finetunes, quants, adapters, and merges when `base_model` is already sealed.
- `isVerified` is not quality. Do not require eval scores.
- On Windows without symlinks, the Hub cache copies files and does not dedup. Default remains a `local_dir` folder you can point a loader at.
- `hf_transfer` does not hash the body. A finished transfer is not a seal. `IncompleteSnapshotError` leaves the revision unsealed for the next run.
- Ollama and GitHub releases are not discovery catalogs. ModelScope's list API is a later hub, with mixed license strings and a 3,000-row page cap.

The run's recommendation matches the architecture: an OS-scheduled CLI, official snapshot client, license-file gate, disk budget on FP16/BF16 shards. A language model is not required for those structured fields. It stays in the design only for the free-text worry paragraph, and only over rows the code already kept.

## Not verified

- The spoken transcript. YouTube returned HTTP 429 for captions here too. Claims follow the creator's description and chapter list.
- The description's "three policy options" for Chinese open models. The Axios page did not open past a bot check during the research run. Search extracts mention several instruments (an Entity List, a security advisory, hosting liability, Commerce draft rules). Those may not be a list of three.
- Pirate Face's claimed index of 669,000 Apache and MIT models, and the "9 KB" Heretic recipe. Neither was checked. The tool still does not use a torrent index.
- Whether Software Heritage later stored any Hugging Face weights beyond that planning ticket.
- A numeric rate limit on ModelScope `GET /models`. A "100 requests per minute" example in the OpenAPI spec is attached to other paths.
- Any Unraid or TrueNAS app, or any Internet Archive item, that documents checksums, manifests, and resume for an open-weight mirror.
- hf-mirror.com's object layout. The public page tells clients to set `HF_ENDPOINT` and use a resume flag. It does not describe checksum verification. ModelPrepper's default endpoint stays the official hub.
- Exact current Hub quotas. The September 2025 table was not re-measured.
- That a Grok or Claude plugin is forbidden from downloading. The reason to keep downloads in the CLI is task lifetime and determinism, not a vendor rule found in their docs.

## Excluded claims

The run dropped a few candidate sentences because a verifier could not see the video page, even when the underlying document was real. Those documents were checked separately while writing the video review: Nvidia's 3 September 2026 post, the 21 March 2023 Meta DMCA notice (403 repositories; the notice does not itself say they all disappeared in one day), and Anthropic's 27 July 2026 post. The video description lists those URLs. The "one day" wording stays the video's.
