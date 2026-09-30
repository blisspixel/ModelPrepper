# Video review

Historical research from September 24, 2026. Implementation guidance is superseded by [the roadmap](roadmap.md) and current design documents. Claims below retain their original caveats and are not current product guarantees.

**Open Models Might Get Banned. Download Yours Tonight**
Devsplainers, 23 September 2026, about eight minutes.
https://youtu.be/9eJvqI2MJts

The caption track returned HTTP 429 during this review, so this note follows the publisher's chapters, description, and the sources linked under the video, plus the primary pages behind those links. Spoken asides that never made the description may be missing.

## What the video argues

Open-weight models people actually run mostly live on one website. Nvidia has agreed to buy that website. Separately, US officials have been reported as considering restrictions aimed at Chinese open-weight models. Either change can remove a download button that local tools depend on. The practical advice is to pick a few models, download the whole repository rather than a quantized file alone, and keep the files on ordinary hard drives.

That advice is sound. The title is ahead of the sources. The sources describe a proposed policy fight and a corporate acquisition that has not closed. They do not describe a ban that is already in force.

## Chapter by chapter

### One website, three million models

Nvidia's 3 September 2026 announcement says Hugging Face holds more than 3 million models, 500,000 datasets, and 1 million apps, used by more than 18 million people. The acquisition price in the post is $12,930,300,000. Reuters and the New York Times reported the same figure, with about $11.9 billion to investors and up to $1 billion in employee retention, and an expected close in the first half of 2027. Huang's post says the hub remains an open platform, that developers keep their choice of model, framework, cloud, and chip, and that Nvidia compute will not be required.

ModelPrepper treats that as a statement from the buyer, not as custody. A local pinned revision still runs if the account, the CDN, or the corporate owner changes its mind later.

### The ban on the table

Axios reported on 20 July 2026 that a push against Chinese open models was back in discussion. TechCrunch on 24 July covered an industry letter, signed by Hugging Face, Meta, Microsoft, Mistral, Nvidia, and others, asking policymakers to avoid broad restrictions on open-weight models. The letter separates ordinary distillation from misappropriation of closed models. Replit's CEO told TechCrunch that banning Chinese open models was effectively a ban on open models, because US open models are trained with that ecosystem in the loop.

Anthropic's 27 July post says the company has never advocated a ban on open weights as a category, and that a use-ban on US businesses would not address the risks it actually names (chip access, industrial distillation, and pre-release testing). The UK AI Security Institute passage Anthropic quotes is the real technical point for this project: once weights are public, the publisher cannot withdraw the copies people already have.

ModelPrepper does not try to predict which of those positions wins. It archives models whose license already allows you to keep a copy.

### What already disappeared

The video uses two different events. They need different responses.

**Publisher deleted the repo.** Runway deleted its Hugging Face account and `runwayml/stable-diffusion-v1-5` went with it. `huggingface/diffusers` issue 9322 is the public bug report from that day. People who had not downloaded it lost the canonical URL. Stable Diffusion 1.5 was released under CreativeML Open RAIL, which is not MIT or Apache-2.0, so it is outside the default allowlist even though the outage is a fair illustration of link rot.

**Copyright takedown.** On 21 March 2023 Meta sent GitHub a DMCA notice covering hundreds of repositories redistributing LLaMA weights. Those weights had been released under Meta's research terms, not under MIT or Apache-2.0. Keeping a copy you were not allowed to redistribute is a different problem from backing up Apache files. The license gate is how ModelPrepper stays on the lawful side of that line.

### What is worth the disk

The video's selection advice matches the funnel in the README. A model is worth a full-precision copy when you might still want to run or finetune it in a few years, and when a quantized file would be a lossy stand-in.

Size rule, checked against hub metadata: about 2 bytes per parameter at BF16 or FP16, so about 2 GB per billion parameters. `ibm-granite/granite-3.3-8b-instruct` is 8.17B BF16, on the order of 16 GB for the tensors. GPT-2 is MIT and small, but the safetensors file is FP32, 548 MB, which is 4 bytes per parameter. A tool that assumes every model is FP16 will mis-budget.

The video is right that the runnable set is more than the weight file: tokenizer, config, chat template, generation settings, and the license. ModelPrepper's file allowlist is that set, plus the safetensors shards and their index.

### Drives, optical discs, and a 9 KB recipe

The video compares archival Blu-ray (on the order of $70 to $100 per TB) with used server hard drives (on the order of $15 per TB) and tells people to use drives. For tens or hundreds of gigabytes per model, that cost comparison favors disks. Optical media still has a niche for a tiny artifact you want immune to a filesystem crash. It is a bad primary store for a 70B BF16 checkpoint (~140 GB).

The Heretic example in the video is a storage idea: a 9 KB recipe that can reconstruct a variant from a base model, instead of storing the variant in full. That belongs on the roadmap as "store a base revision, record a delta you already trust," and only when the delta is data (a LoRA, a tokenizer change) with its own license. It is not a feature for stripping safety training.

The video also points at Pirate Face, a third-party torrent index it describes as Apache and MIT models. ModelPrepper does not use it. Torrent metadata is not the hub's per-file SHA-256, and a model repo is a known place to hide a loader. In 2026 a repository dressed up as an official release reached the top of trending and was pulled onto a large number of machines with an infostealer beside the weights. The defense is boring: official client, pinned SHA, safetensors only, no Python from the repo, checksums before the revision is marked sealed.

### Software Heritage

The video's description says Software Heritage archived about 12 TB of code and skipped about 40 PB of weights. The cited ticket, [swh/meta#5099](https://gitlab.softwareheritage.org/swh/meta/-/work_items/5099), is a proposal: archive about 12 TiB of non-LFS Hugging Face files, and leave about 40 PiB of LFS weights out of scope. That is not a finished archive of 12 TB of code. The qualitative point still holds. Software Heritage's mission is source code, and that ticket treats model weights as out of scope. A personal vault is the thing that keeps a BF16 checkpoint. ModelPrepper is that vault for one disk budget, not a second Software Heritage.

## Worries, after the video

The video is mostly about one host and one policy conversation. A vault also has to survive a publisher that simply goes away. Runway deleting Stable Diffusion 1.5 is that case, and it is not a government. The worry list in [worries.md](worries.md) makes you order those fears. Category pressure, including models a publisher already shipped as uncensored or abliterated, is one entry you can put first. It is not hardcoded, and the tool does not create those weights. The cover-your-ass limits on that feature are in [notice.md](notice.md).

## What ModelPrepper takes from the video

- Pin a revision and store the whole runnable repo, not a lone GGUF.
- Budget disks with the 2-bytes-per-parameter rule, and check the dtype the repo actually published.
- Expect hosts and publishers to delete or gate things that are public today.
- Prefer two hard disks over a specialty optical format for the bytes.
- Keep the license file with the weights so the copy stays attributable.

## What it refuses to take

- A workflow whose source of bytes is a pirate index.
- An assumption that Llama, Gemma, or Open RAIL models are Apache or MIT.
- An automatic grab of every newly touched Apache tag. The hub's "latest Apache text-generation" page is mostly finetunes and repacks.
- A claim that open weights have already been banned. The sourced status, as of this review, is: acquisition announced and not closed; restrictions debated; Anthropic on record against a category ban.
