# Model source catalog and adapter plan

Reviewed September 30, 2026 against primary project documentation. This is a research catalog, not a claim that every API, download, license, or mirror was tested. ModelPrepper currently inspects only official public, ungated Hugging Face model repositories. None of the other entries is enabled by adding a URL to configuration.

The companion [data-only registry](../resources/model-sources.json) gives future discovery tools a curated starting point. It is not a trusted network allowlist, runtime plugin registry, or authority to download. Source changes require another documented review.

## Four separate roles

| Role | What it establishes |
| --- | --- |
| Publisher | Who released the artifact and supplied its original evidence. |
| Host | Where a particular revision's files can be obtained. |
| Mirror or recovery transport | Another route to bytes with an independently recorded identity. |
| Discovery or evaluation source | Why a candidate was suggested; it does not establish artifact identity. |

One service can play several roles. Several publisher accounts on Hugging Face are still one hosting dependency. A matching model name on two hubs does not establish matching bytes, precision, license, or required dependencies.

## Practical sources

| Source | Useful role | Archival identity and qualification needed | Reference |
| --- | --- | --- | --- |
| Hugging Face Hub | Broad publisher/community hosting; current inspection adapter. | Pin a full commit, retain complete inventory and original evidence, distinguish Git blob SHA-1 from LFS SHA-256. | [Download documentation](https://huggingface.co/docs/huggingface_hub/guides/download) |
| ModelScope, including modelscope.cn and modelscope.ai | Another model hub and a candidate for alternate-host recovery. | Its official SDK documents revision selection, resumable downloads, and SHA-256 checks. Prove exact revision/hash semantics and compare any claimed HF counterpart file by file. | [Official Hub client](https://github.com/modelscope/modelscope_hub) |
| NVIDIA NGC | Versioned models, resources, and GPU-oriented artifacts. | Bind organization/team/model/version plus files and evidence. Public guest downloads and entitled content have different access paths. Some models have signing support; verify the specific signature and trust basis rather than assuming every model is signed. | [NGC catalog guide](https://docs.nvidia.com/ngc/latest/ngc-catalog-user-guide.html) |
| Kaggle Models | Publisher/community models, framework-specific variations, and versions. | Bind owner/model/framework/variation/version. The documented download API can return an archive; exact inventory, extraction bounds, license consent, and hashes need qualification. | [Kaggle model documentation](https://www.kaggle.com/docs/models) |
| Civitai | Image-model checkpoints, LoRAs, variants, and related discovery. | Bind a model version and chosen file, retain licenses and base dependencies, and distinguish original checkpoints from derivatives. Historical API documentation includes file hashes; current endpoints need renewed verification. | [Current documentation pointer](https://github.com/civitai/civitai/wiki/REST-API-Reference), [historical API contract](https://github.com/civitai/civitai/wiki/REST-API-Reference/dff336bf9450cb11e80fb5a42327221ce3f09b45) |
| Official publisher downloads, including Meta Llama | Direct original releases independent of a hub download path. | Meta documents license acceptance and approved signed download URLs. Retain model identity and evidence, refresh expired links without changing identity, and never store signed URLs as durable provenance. This requires a future authorized-access profile. | [Meta model access](https://dev.meta.ai/llama/docs/getting-the-models/meta) |
| Official GitHub repositories and release assets | Research checkpoints, supporting assets, release manifests, and provenance. | A source-code tag is not a weight identity. Bind release/asset identifiers and actual digests; capture original license and inventory. GitHub documents asset download redirects and digest metadata. | [Release asset API](https://docs.github.com/en/rest/releases/assets) |
| Ollama library and existing local stores | Packaged runnable variants and a future local-import path. | Retain manifests, referenced blobs, templates, license, and actual format/quantization. Its API exposes local model digests and details. Do not call a GGUF derivative a publisher-original FP16 copy. | [Local model metadata](https://docs.ollama.com/api/tags), [blob/manifest API](https://github.com/ollama/ollama/blob/main/docs/api.md) |
| Zenodo | Research deposits, exact record versions, and scientific model artifacts. | Bind the specific record/version and files rather than a latest link. Its developer docs include MD5 file checksums; MD5 is not strong authenticity evidence. Add local SHA-256 and preserve the recorded trust basis. | [Developer documentation](https://developers.zenodo.org/), [example model-weight deposit](https://zenodo.org/records/19263943) |
| Internet Archive | Secondary archival copies and recovery research. | Item metadata and downloadable files do not automatically establish original publisher provenance. Compare against independently retained identities and evidence; distinguish original files from generated derivatives. | [Developer portal](https://archive.org/developers/) |
| PirateFace | HF-linked torrent discovery and potential recovery transport. | Its documentation describes pinned-source submissions and comparison with recorded HF SHA-256. Qualify actual manifests, metadata, peer availability, file completeness, and transport behavior before integration. | [How it works](https://pirateface.co/how-it-works), [site FAQ](https://pirateface.co/) |

These are source families, not blanket endorsements or statements that every hosted artifact meets the current MIT/Apache-2.0 policy. A provider's terms, an artifact's license, and rights for dependent components are separate evidence.

TensorFlow Hub is not another independent modern host to count alongside Kaggle: its [official repository](https://github.com/tensorflow/hub) documents migration to Kaggle Models and deletion of unmigrated assets. Track legacy identifiers as aliases with recorded migration evidence.

Publisher indexes such as NVIDIA's Hugging Face organization are useful discovery entry points, but still rely on Hugging Face hosting. Similarly, an API-only inference endpoint is not a downloadable model archive. Confirm that weights are actually offered.

## PirateFace findings

The site describes community magnets, source checksums, and recovery after upstream removal while complete peers remain available. Its HF-compatible drop-in endpoint and direct publishing of models never hosted on HF are marked as planned. Do not implement an endpoint substitution based on the illustrated examples alone. See its [FAQ](https://pirateface.co/).

A magnet is a locator, not a permanence guarantee. An infohash or torrent piece hash does not establish the original publisher's identity or replace whole-file checks against independently recorded source evidence. A creator-account badge also does not verify every tensor file. These are preservation design conclusions, not results from auditing PirateFace's implementation.

The first useful recovery path can be manual media/client download followed by verified import, once import is implemented. Native torrent support is a later, isolated Rust transport experiment. Do not make aria2, a torrent daemon, or a Python SDK a mandatory runtime dependency of the archive utility.

Seeding is an explicit opt-in operation with independent upload limits, time windows, peer-network settings, and rights review. Downloading an artifact must not silently authorize indefinite upload. Tracker, DHT, and peer discovery require their own network/privacy design and fault tests.

## Mirror admission and failover

Keep canonical artifact identity separate from retrieval locations. A bundle retains its original publisher/commit, evidence, selected representation, and hashes when recovered elsewhere. Record mirror endpoint, observation time, retrieval route, and independently verified file results separately.

Before admitting a mirror:

1. Match required paths, sizes, and recorded content identities for the exact archived representation.
2. Verify complete dependency closure and retained license/notice evidence; a mirror's license tag cannot replace originals.
3. Fetch only plan-selected files through a qualified redirect/credential policy. No arbitrary endpoint becomes trusted because it appears in a card.
4. Hash downloaded bytes independently and retain all-file local SHA-256. Do not silently upgrade unknown provenance.
5. If original evidence is unavailable and no independently retained identity exists, label the import as a local baseline or uncertain recovery, not upstream verified.

An original source failure creates a visible event and possible recovery proposal. It never switches to another similarly named model, an unapproved quantization, or a newer commit. Credentials remain source-scoped; URLs containing secrets stay out of catalogs and manifests.

## Adapter delivery order

This is an engineering recommendation based on documented fit, not a reliability benchmark:

1. Finish the existing HF preservation and recovery path, including manifests and offline import.
2. Qualify ModelScope as a second hub, and a narrowly scoped official-release/HTTPS adapter with captured inventories and digests. Avoid a universal downloader that trusts arbitrary links.
3. Qualify Kaggle and NGC independently, especially archive extraction, licenses, entitlement, format dependencies, and signatures where offered.
4. Add Civitai alongside a supported image-model dependency profile; add other modality adapters only after their closure gates pass.
5. Add local Ollama/import adapters for explicitly selected runnable derivatives.
6. Research PirateFace/torrent and Internet Archive recovery without weakening provenance or introducing mandatory daemons.
7. Add research-deposit adapters such as Zenodo when exact record/version and hash trust limitations are handled.

Discovery entries can appear before a transfer adapter exists, but must say unsupported_source or unsupported_layout. Rankings remain the separate evidence pipeline in [archival policies](archive-policy.md); provider popularity cannot authorize a transfer.

Each adapter must prove stable identity, complete pagination, exact hash semantics, captured rights, bounded transfer/workspace, interruption recovery, credential isolation, portable paths, and independent restoration. Keep implementation compiled and reviewable rather than executing arbitrary source plugins. See [architecture](architecture.md) and [roadmap](roadmap.md).
