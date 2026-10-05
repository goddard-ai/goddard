# Whistle as the composer's default STT

**Recommendation: adopt when integration and product-quality gates are met; do not make it the default yet.** Whistle is a promising local transcription candidate: the published model is small, Apache-2.0 licensed, and targets Apple platforms. However, its current published interface is one-shot clips up to 30 seconds, the project is only newly released, and no in-app composer STT provider exists today to replace. First verify quality on Goddard dictation and settle a runtime/API integration path.

## Findings

### Model and claims

Whistle was announced October 2, 2026. It is a Cactus Compute speech-recognition model in a 16.9 MB `.cact` file, quantized at 2–4 bits. The model card/config describes a log-mel front end, convolutional stem, eight-block audio encoder, and Needle-derived laddered decoder with gated cross-attention. Config does not publish a parameter count; the file size is not a parameter count. Supported languages are English, German, French, Spanish, Italian, Dutch, and Polish. Inputs are 16 kHz mono, up to 30 seconds per call. [Release post](https://cactuscompute.com/blog/whistle) · [Hugging Face model card/config](https://huggingface.co/Cactus-Compute/whistle)

Hugging Face labels the model `apache-2.0`; I fetched the repository's `LICENSE`, which is Apache License 2.0. Its redistribution terms permit reproduction and distribution of source or object form, subject to providing the license and preserving notices (and marking modified files). This is a favorable starting point for bundling the weights, subject to normal legal review of the exact shipped artifacts and notices. [Model license metadata](https://huggingface.co/Cactus-Compute/whistle) · [Weight license file](https://huggingface.co/Cactus-Compute/whistle/blob/main/LICENSE) · [Runtime license](https://github.com/cactus-compute/needle/blob/main/LICENSE)

The vendor reports WER wins over multilingual Whisper base on LibriSpeech test-clean/test-other, SPGISpeech, Earnings-22, and FLEURS average; Whisper base wins on TED-LIUM, AMI, and MLS average. Their M4 Pro CPU comparison reports 11.1 ms time-to-first-token and 1,319 decode tokens/s for Whistle versus 73.2 ms and 266 tokens/s for Whisper base, on 10 seconds of audio. These are vendor-reported comparisons: Whistle was measured over 86,174 utterances while competitor numbers were taken from published results; the model card provides caveats, including AMI subset mismatch. This supports further evaluation, not a conclusion that Whistle is uniformly more accurate. [Benchmarks and methodology](https://huggingface.co/Cactus-Compute/whistle#benchmarks)

### Runtime, streaming, and resource profile

The weights use Cactus's C++ `needle3` runtime and `.cact` format. The model card says CPU-only, no GPU, and no dependencies. Vendor docs list prebuilt target folders for macOS arm64, iOS, Android, and other targets, plus C API entry points such as `needle_load` and `needle_transcribe`. The documented transcription operation accepts a clip; Whistle material does not document live partial-result streaming. The separate Cactus Engine streaming API I found currently names Whisper and Parakeet TDT as supported stream models, not Whistle. So the available evidence supports short-clip transcription, not streaming Whistle. [Whistle deployment docs](https://huggingface.co/Cactus-Compute/whistle#deploy) · [Cactus Engine streaming API](https://github.com/cactus-compute/cactus/blob/main/docs/cactus_engine.md#streaming-transcription)

The 16.9 MB figure is the compressed model artifact, not peak resident memory. The published benchmark is on an Apple M4 Pro CPU but does not report memory use, thermal behavior, or performance on older/lower-power devices. Runtime and weights can be fetched/cached for an offline flow, and the vendor's demo says audio stays on-device after download; first use requires obtaining the model. [Release post](https://cactuscompute.com/blog/whistle)

### Current Goddard composer path

The repository search found no Whistle, Whisper, or composer STT implementation under `src/`, `crates/`, or `apps/`. Desktop composer is a GPUI text composer; there is no app-level transcription pipeline/provider to displace. On mobile, [composer-text-input.native.tsx](../../apps/mobile/src/components/composer-text-input.native.tsx) renders React Native `TextInput` (lines 1–16), so dictation availability is supplied by the platform keyboard/OS, not a Goddard STT model.

The macOS Speech framework references in [src/platform.rs](../../src/platform.rs) are for a distinct voice-consent gate: `speech_recognition_access` checks authorization (lines 79–97), and `begin_consent` requires on-device recognition and partial results while listening for “go ahead” (lines 315–345). This is not composer dictation and must not be mistaken for the current composer backend. No cloud STT provider was found.

### Fit and integration shape

Whistle could sit behind a new local `ComposerTranscriber` interface: capture microphone PCM, resample to 16 kHz mono, run the Cactus/Whistle engine off the UI thread, then insert the returned text into the composer (with clear replace/append and cancel semantics). At 30 seconds maximum per call, a natural initial UX is push-to-talk or stop-to-transcribe. If live partial text is a requirement, either wait for a Whistle streaming API or evaluate another streaming backend; do not infer streaming support from the 11 ms first-token benchmark.

The main product benefit is a privacy-preserving, network-independent alternative to whichever OS keyboard dictation a user currently relies on, with the same model option possible on desktop and mobile. Main risks are seven-language coverage, max clip length, very recent release (two days old at research date), lack of independent Goddard-domain accuracy/robustness data, unknown peak memory on mobile, and no documented Whistle partial-stream API. Bundling requires shipping the model file plus Apache notices; downloading at first use means network availability is still needed once.

## Decision

**Adopt when:** (1) Cactus confirms redistribution of the exact model and runtime artifacts under the published Apache terms, (2) a small Goddard dictation bake-off validates accuracy on real composer-style prompts, names, accents, and noisy input, (3) representative Apple Silicon/iOS/Android measurements establish memory, latency, and package/download impact, and (4) product chooses clip-based dictation or Cactus documents streaming support for Whistle. Until then, keep OS-provided dictation as the current path and treat Whistle as the leading local-STT candidate, not the default.

## Sources read

- [Whistle release post (October 2, 2026)](https://cactuscompute.com/blog/whistle)
- [Hugging Face model card and config](https://huggingface.co/Cactus-Compute/whistle)
- [Hugging Face model `LICENSE`](https://huggingface.co/Cactus-Compute/whistle/blob/main/LICENSE)
- [Cactus Needle runtime repository/license](https://github.com/cactus-compute/needle)
- [Cactus Engine streaming transcription API](https://github.com/cactus-compute/cactus/blob/main/docs/cactus_engine.md#streaming-transcription)
