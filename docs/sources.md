# Sources

Historical research from September 24, 2026. Implementation guidance is superseded by [the roadmap](roadmap.md) and current design documents. Claims below retain their original caveats and are not current product guarantees.

Checked while writing the design on 24 September 2026. Policy items move quickly. The engineering facts (API shape, dtype sizes, license tags sampled below) are what the code should test against, not the political forecasts.

This repository is MIT licensed ([LICENSE](../LICENSE)). That license covers ModelPrepper's code and docs. It does not relicense archived weights. The operating limits are in [notice.md](notice.md). The worry catalog is in [worries.md](worries.md).

## The video

- Devsplainers, "Open Models Might Get Banned. Download Yours Tonight," 23 September 2026. https://youtu.be/9eJvqI2MJts
- Review notes: [video-review.md](video-review.md). Caption download was rate-limited (HTTP 429). The chapter list and source list in the video description were used.

## Acquisition and hub

- Jensen Huang, "NVIDIA to Acquire Hugging Face," NVIDIA Blog, 3 September 2026. https://blogs.nvidia.com/blog/nvidia-to-acquire-hugging-face/
- "Nvidia Extends A.I. Spending Spree With $12.9 Billion Deal for Hugging Face," The New York Times, 3 September 2026. https://www.nytimes.com/2026/09/03/technology/nvidia-hugging-face.html
- "Nvidia bets $13 billion on open AI models with Hugging Face deal," Reuters, 3 September 2026. https://www.reuters.com/business/nvidia-buy-hugging-face-nearly-13-billion-big-bet-open-ai-models-2026-09-03/
- Hugging Face download guide (`snapshot_download`, `hf download`, dry-run, `hf_xet`). https://huggingface.co/docs/huggingface_hub/guides/download
- `HfApi.list_models`. https://huggingface.co/docs/huggingface_hub/package_reference/hf_api
- Xet storage overview. https://huggingface.co/docs/hub/xet/index

## Policy debate the video is about

- Axios, 20 July 2026, on a revived push aimed at Chinese open models. https://www.axios.com/2026/07/20/ai-us-china-open-source-kimi
- Rebecca Bellan, "As US weighs response to Chinese AI, industry urges against broad open-weight restrictions," TechCrunch, 24 July 2026. https://techcrunch.com/2026/07/24/as-us-weighs-response-to-chinese-ai-industry-urges-against-broad-open-weight-restrictions/
- Dario Amodei, "Our position on open-weights models," Anthropic, 27 July 2026. https://www.anthropic.com/news/position-open-weights-models
- Apache License 2.0, including the redistribution conditions in section 4 and the copyright grant in section 2. https://www.apache.org/licenses/LICENSE-2.0

## Disappearances the video cites

- `huggingface/diffusers` issue 9322, Stable Diffusion 1.5 unavailable after the Runway account was removed. https://github.com/huggingface/diffusers/issues/9322
- GitHub DMCA notice, Meta, 21 March 2023, LLaMA weight redistribution. https://github.com/github/dmca/blob/master/2023/03/2023-03-21-meta.md
- Software Heritage ticket "Archive Hugging Face (ML models)" (#5099). A proposal to archive about 12 TiB of non-LFS Hugging Face files and to leave about 40 PiB of LFS weights out of scope. https://gitlab.softwareheritage.org/swh/meta/-/work_items/5099

## License tags read from the hub API

`GET https://huggingface.co/api/models/<repo>` on 24 September 2026:

| Repo | `cardData.license` | Gated | Safetensors dtype |
| --- | --- | --- | --- |
| `Qwen/Qwen3-8B` | apache-2.0 | false | BF16 8.19B |
| `Qwen/Qwen2.5-7B-Instruct` | apache-2.0 | false | BF16 7.62B |
| `mistralai/Mistral-7B-Instruct-v0.3` | apache-2.0 | false | BF16 7.25B |
| `mistralai/Mistral-7B-v0.1` | apache-2.0 | false | BF16 7.24B |
| `ibm-granite/granite-3.3-8b-instruct` | apache-2.0 | false | BF16 8.17B |
| `allenai/OLMo-2-1124-7B-Instruct` | apache-2.0 | false | BF16 7.30B |
| `HuggingFaceTB/SmolLM2-1.7B-Instruct` | apache-2.0 | false | BF16 1.71B |
| `microsoft/Phi-3-mini-4k-instruct` | mit | false | BF16 3.82B |
| `openai-community/gpt2` | mit | false | FP32 137M |
| `deepseek-ai/DeepSeek-R1` | mit | false | mostly F8_E4M3 ~681B, plus a small BF16 slice |
| `meta-llama/Llama-3.1-8B-Instruct` | llama3.1 | manual | BF16 8.03B |
| `google/gemma-2-9b-it` | gemma | manual | BF16 9.24B |

The same day's "recently modified, `license:apache-2.0`" listing was dominated by low-signal finetunes and GGUF repacks. That listing is why scan results go through a ranker instead of a download.

## Research pass (24 September 2026)

A cross-checked research pass confirmed the hub mechanics below and corrected the Software Heritage line above. Notes on what that pass could not verify are in [research.md](research.md).

Measured safetensors byte sizes, paired with a bartowski Q4_K_M of the same parameter count. Budget the BF16 column. Q4_K_M landed at about 30% of those bytes. "7B" is not one size: `meta-llama/Llama-2-7b-hf` is FP16, 6.74B parameters, 13.48 GB of safetensors, smaller than Qwen2.5-7B.

| Checkpoint | BF16 safetensors | bartowski Q4_K_M |
| --- | --- | --- |
| Qwen2.5-7B | 15.23 GB | 4.68 GB |
| Qwen2.5-14B | 29.54 GB | 8.99 GB |
| Qwen2.5-32B-Instruct | 65.53 GB | 19.85 GB |
| Llama-3.1-70B | 141.11 GB | 42.52 GB |

`usedStorage` on a repo is the wrong budget number when the same weights are stored twice. Llama-2-7b-hf reported about 54 GB of used storage while its safetensors shards were about 13.5 GB, with PyTorch shards of similar size beside them.

Other hub facts used in the architecture:

- List filters that work: `license:<card-id>`, `pipeline_tag`, `num_parameters=min:1B,max:3B`, `gated=false`, `sort=lastModified`. No server-side file-size filter and no last-modified date range. Bytes and the calendar window are applied after the list. https://huggingface.co/docs/huggingface_hub/package_reference/hf_api
- Hub rate-limit table dated September 2025, 5-minute windows: anonymous 500 API requests and 3,000 resolver requests; free user 1,000 and 5,000. The page says anonymous and free quotas can change. `huggingface_hub` 1.2.0 and later waits out HTTP 429 using the `RateLimit` header. https://huggingface.co/docs/hub/rate-limits
- License card values are author-written YAML. The vocabulary includes `apache-2.0` and `mit` and also `llama4`, `gemma`, `openrail`, `unknown`, and `other`. `license: other` stores the real name in `license_name`. https://huggingface.co/docs/hub/repositories-licenses
- `downloads` is a rolling 30-day count. `downloadsAllTime` is separate. A GGUF repo counts every GGUF file, so clones inflate the number. https://huggingface.co/docs/hub/models-download-stats
- Redundancy keys off `base_model` plus a relation of adapter, merge, quantized, or finetune. https://huggingface.co/docs/hub/model-cards
- `isVerified` is not a quality score. The organization overview for Qwen has shown an unverified team plan. https://huggingface.co/api/organizations/Qwen/overview
- Hub cache dedup (etag blobs, Xet symlinks, `trees/<commit>.json`) applies when symlinks work. On Windows without symlink support the cache copies files into snapshots and does not dedup. https://huggingface.co/docs/huggingface_hub/guides/manage-cache
- ModelScope's list API (OpenAPI 1.1.0) can filter by task and license and sort by `last_modified`, returns `file_size` and `gated`, and caps paging at 3,000 rows. License strings in the spec mix SPDX ids and display names. Bearer auth. Not a phase-1 hub. https://modelscope.cn/.well-known/openapi.json
- Ollama's HTTP API lists local models and pulls a name. It is not a filterable public catalog.
- GitHub releases are per repository. `GET /repos/{owner}/{repo}/license` runs Licensee on a LICENSE file and can return an SPDX id. It is not a model index.

## Prior art to learn from, not wrap blindly

- `hf download <repo> --dry-run` already answers "what would land, and how big is it." Phase 2 should shell out to the library behind that command rather than reimplement LFS.
- Ollama, LM Studio, and llama.cpp are loaders. They solve "run a local file," not "decide what is worth keeping, pin the license, and resume a 16 GB pull onto a NAS."
- Personal `snapshot_download` scripts and DataHoarder-style copy jobs are the real predecessors. They usually lack the license-file check and the queue. That gap is the product.

## Stack, ranker, and front doors (24 September 2026)

- `hf-xet` 1.6.0, Apache-2.0, the Rust client for Hugging Face Xet storage, used inside `huggingface_hub`. https://crates.io/crates/hf-xet and https://github.com/huggingface/xet-core
- `huggingface_hub_rust`, experimental async Rust Hub client with an optional `xet` feature. APIs may change without notice. https://github.com/huggingface/huggingface_hub_rust
- MCP specification `2026-07-28` (current as of September 2026): stateless requests, Streamable HTTP, `Mcp-Method` and `Mcp-Name` headers. https://modelcontextprotocol.io/specification/2026-07-28 and the changelog https://modelcontextprotocol.io/specification/2026-07-28/changelog
- Official Rust MCP SDK `rmcp` 3.4.0, implements `2026-07-28` and remains compatible with `2025-11-25`. https://github.com/modelcontextprotocol/rust-sdk
- Agent Plugins 1.0.0: portable `plugin.json`, `skills/`, and `mcp.json` only. https://agent-plugins.org/specification
- Agent Skills format used by that package. https://agentskills.io/specification
- `Qwen/Qwen3.5-4B` and `Qwen/Qwen3.5-9B`, both `apache-2.0`, ungated, BF16, read from `https://huggingface.co/api/models/<id>` on 24 September 2026. Q4_K_M file sizes around 2.7 to 3.0 GB are from public GGUF repos of the 4B (`unsloth/Qwen3.5-4B-GGUF`, `lmstudio-community/Qwen3.5-4B-GGUF`), not from the publisher's BF16 tree.
- llama.cpp `llama-server`, Ollama, and LM Studio expose OpenAI-compatible chat endpoints used as the ranker transport. Comparison of their concurrency defaults as of 20 September 2026: https://cohorte.co/blog/local-llm-openai-compatible-server
