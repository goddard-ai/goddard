//! The local voice engine: piper-rs voices text with a per-voice ONNX model
//! from the rhasspy/piper-voices dataset, fully offline — a selected Piper
//! voice downloads on first use into `piper_voices_directory()` and
//! synthesis runs on the background executor. Output is 16-bit mono WAV at
//! the model's sample rate, the same bytes `play_briefing_audio` already
//! decodes, so clips ride the briefing/speech cache unchanged.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context as _, anyhow, bail};
use futures::FutureExt;
use futures::future::{Either, select};
use futures::io::AsyncReadExt;

/// Voice files resolve out of the pinned piper-voices dataset revision.
const VOICES_BASE_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0";
/// Voice models run 20–100 MB — far past the gateway request timeout.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// One pickable Piper voice. `id` is the dataset's file stem; the label is
/// a product name and stays untranslated.
pub(super) struct PiperVoice {
    pub id: &'static str,
    pub label: &'static str,
}

/// The pickable voices — US English at the dataset's high quality tier.
/// `piper_voice_or_default` enforces the same boundary for a hand-edited
/// or stale `voice_briefing_piper_voice`: the model URL derives from any
/// qualifying id, and anything else falls back to the default.
pub(super) const PIPER_VOICES: &[PiperVoice] = &[
    PiperVoice {
        id: "en_US-lessac-high",
        label: "Lessac (US English)",
    },
    PiperVoice {
        id: "en_US-libritts-high",
        label: "LibriTTS (US English)",
    },
    PiperVoice {
        id: "en_US-ljspeech-high",
        label: "LJSpeech (US English)",
    },
    PiperVoice {
        id: "en_US-ryan-high",
        label: "Ryan (US English)",
    },
];

/// The catalog default — Lessac is Piper's reference voice.
const DEFAULT_PIPER_VOICE: &str = "en_US-lessac-high";

/// Whether a voice id stays inside the allowed set: `en_US-<name>-high`.
/// Broader than the picker's rows so a hand-edited setting that names
/// another qualifying voice still resolves.
fn piper_voice_allowed(voice: &str) -> bool {
    let Some(("en_US", rest)) = voice.split_once('-') else {
        return false;
    };
    let Some((name, "high")) = rest.rsplit_once('-') else {
        return false;
    };
    !name.is_empty()
}

/// The voice to synthesize with: the configured id when it qualifies, the
/// catalog default when it doesn't — a stale or out-of-catalog setting
/// never reaches the engine.
pub(super) fn piper_voice_or_default(voice: &str) -> &str {
    let voice = voice.trim();
    if piper_voice_allowed(voice) {
        voice
    } else {
        DEFAULT_PIPER_VOICE
    }
}

/// What the voice picker shows for the stored id — the catalog label, or
/// the raw id when it names a voice the catalog doesn't list.
pub(super) fn piper_voice_label(voice: &str) -> String {
    PIPER_VOICES
        .iter()
        .find(|entry| entry.id == voice)
        .map(|entry| entry.label.to_owned())
        .unwrap_or_else(|| voice.to_owned())
}

/// Speaker counts from the same pinned dataset revision as the model URLs.
/// LibriTTS is the catalog's only multi-speaker model (IDs 0 through 903).
pub(super) fn piper_speaker_count(voice: &str) -> u32 {
    match piper_voice_or_default(voice) {
        "en_US-libritts-high" => 904,
        _ => 1,
    }
}

pub(super) fn piper_speaker_or_default(voice: &str, speaker: u32) -> u32 {
    if speaker < piper_speaker_count(voice) {
        speaker
    } else {
        0
    }
}

/// Preserve existing cache keys for the default speaker and single-speaker models.
pub(super) fn piper_voice_key(voice: &str, speaker: u32) -> String {
    let voice = piper_voice_or_default(voice);
    let speaker = piper_speaker_or_default(voice, speaker);
    if speaker == 0 {
        format!("piper:{voice}")
    } else {
        format!("piper:{voice}:speaker:{speaker}")
    }
}

/// The loaded engine for the current voice, kept between calls so a speak
/// chain doesn't pay the ONNX load per fragment.
static PIPER_ENGINE: OnceLock<Mutex<Option<(String, piper_rs::Piper)>>> = OnceLock::new();

/// The dataset's relative path for a voice id: `en_US-lessac-medium` lives
/// at `en/en_US/lessac/medium/en_US-lessac-medium`. Voice ids are always
/// `<locale>-<name>-<quality>` where the name itself may carry underscores.
fn voice_dataset_path(voice: &str) -> anyhow::Result<String> {
    let (locale, rest) = voice
        .split_once('-')
        .ok_or_else(|| anyhow!("piper voice id `{voice}` is not <locale>-<name>-<quality>"))?;
    let (name, quality) = rest
        .rsplit_once('-')
        .ok_or_else(|| anyhow!("piper voice id `{voice}` is not <locale>-<name>-<quality>"))?;
    let language = locale
        .split('_')
        .next()
        .filter(|language| !language.is_empty())
        .ok_or_else(|| anyhow!("piper voice id `{voice}` has no language"))?;
    Ok(format!("{language}/{locale}/{name}/{quality}/{voice}"))
}

/// The model and config files for `voice`, downloading both from the
/// dataset on first use. A half-download never leaves a live file — each
/// side lands through a temp rename.
async fn ensure_voice(
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    voice: &str,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let dir = waku_client::persistence::piper_voices_directory();
    let stem = voice_dataset_path(voice)?;
    let model_path = dir.join(format!("{voice}.onnx"));
    let config_path = dir.join(format!("{voice}.onnx.json"));
    if model_path.is_file() && config_path.is_file() {
        return Ok((model_path, config_path));
    }
    std::fs::create_dir_all(&dir).context("creating the voices directory")?;
    for (suffix, target) in [
        (".onnx", model_path.clone()),
        (".onnx.json", config_path.clone()),
    ] {
        if target.is_file() {
            continue;
        }
        let bytes = fetch(http, executor, &format!("{VOICES_BASE_URL}/{stem}{suffix}"))
            .await
            .with_context(|| format!("downloading piper voice {voice}"))?;
        let staged = target.with_extension("part");
        std::fs::File::create(&staged)
            .and_then(|mut file| file.write_all(&bytes))
            .and_then(|_| std::fs::rename(&staged, &target))
            .with_context(|| format!("writing {}", target.display()))?;
    }
    Ok((model_path, config_path))
}

/// GET a URL's bytes with a timeout — the sibling `post` helper is JSON-only.
async fn fetch(
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    url: &str,
) -> anyhow::Result<Vec<u8>> {
    let request =
        gpui::http_client::Request::get(url).body(gpui::http_client::AsyncBody::empty())?;
    let exchange = async {
        let mut response = http.send(request).await?;
        let status = response.status();
        let mut bytes = Vec::new();
        response.body_mut().read_to_end(&mut bytes).await?;
        anyhow::Ok((status, bytes))
    };
    futures::pin_mut!(exchange);
    let (status, bytes) = match select(exchange, executor.timer(DOWNLOAD_TIMEOUT).fuse()).await {
        Either::Left((result, _)) => result?,
        Either::Right(_) => bail!("the voice download timed out"),
    };
    if !status.is_success() {
        bail!("the voice download answered HTTP {status} for {url}");
    }
    Ok(bytes)
}

/// espeak-rs locates `espeak-ng-data` under PIPER_ESPEAKNG_DATA_DIRECTORY,
/// the working directory, or the executable's directory. The bundler ships
/// the tables in `Contents/Resources` — `Contents/MacOS` may only carry
/// executable code, so data there breaks codesigning — and the build script
/// drops them beside the target binary, but other launch shapes — a `cargo
/// test` binary under `deps/`, a nested lane — still need the env var.
/// Point it at the first directory near the executable that carries the data.
fn ensure_espeak_data() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("PIPER_ESPEAKNG_DATA_DIRECTORY").is_some() {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        if let Some(dir) = espeak_data_directory(&exe) {
            // SAFETY: runs once before the first espeak call, and nothing
            // else in the process mutates the environment.
            unsafe { std::env::set_var("PIPER_ESPEAKNG_DATA_DIRECTORY", dir) };
        }
    });
}

/// The directory containing `espeak-ng-data` nearest the executable: a
/// sibling for `cargo run`/`cargo test` layouts, `Contents/Resources` for a
/// packaged macOS app.
fn espeak_data_directory(executable: &std::path::Path) -> Option<PathBuf> {
    let mut dir = executable.parent();
    std::iter::from_fn(|| {
        let current = dir?;
        dir = current.parent();
        Some(current)
    })
    .take(4)
    .find_map(|dir| {
        if dir.join("espeak-ng-data").is_dir() {
            return Some(dir.to_path_buf());
        }
        let resources = dir.join("Resources");
        resources
            .join("espeak-ng-data")
            .is_dir()
            .then_some(resources)
    })
}

/// Synthesize `text` with `voice` into WAV bytes. Blocking CPU work — the
/// caller's pipeline already runs on the background executor.
pub(super) async fn synthesize_piper(
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    voice: &str,
    speaker: u32,
    text: &str,
) -> anyhow::Result<Vec<u8>> {
    let voice = piper_voice_or_default(voice);
    let speaker = piper_speaker_or_default(voice, speaker);
    let (model_path, config_path) = ensure_voice(http, executor, voice).await?;
    ensure_espeak_data();
    let engine = PIPER_ENGINE.get_or_init(|| Mutex::new(None));
    let mut guard = engine
        .lock()
        .map_err(|_| anyhow!("the piper engine lock is poisoned"))?;
    if !matches!(guard.as_ref(), Some((loaded, _)) if loaded == voice) {
        let piper = piper_rs::Piper::new(&model_path, &config_path)
            .map_err(|error| anyhow!("loading piper voice {voice}: {error}"))?;
        *guard = Some((voice.to_owned(), piper));
    }
    let piper = &mut guard.as_mut().expect("just loaded").1;
    let mut samples = Vec::new();
    let mut sample_rate = 0;
    for sentence in briefing_sentences(text) {
        let (sentence_samples, rate) = piper
            .create(sentence, false, Some(i64::from(speaker)), None, None, None)
            .map_err(|error| anyhow!("piper synthesis failed: {error}"))?;
        if sentence_samples.is_empty() {
            bail!("piper returned no audio for a sentence");
        }
        if !samples.is_empty() {
            if rate != sample_rate {
                bail!("piper changed sample rate between sentences");
            }
            // A phrasing pause, only between sentences, in the model's sample rate.
            let silence_samples = (u64::from(rate) * 180 / 1000) as usize;
            samples.resize(samples.len() + silence_samples, 0.0);
        }
        sample_rate = rate;
        samples.extend(sentence_samples);
    }
    drop(guard);
    if samples.is_empty() {
        bail!("piper returned no audio");
    }
    Ok(wav_bytes(&samples, sample_rate))
}

/// Keep punctuation with its sentence; don't cut decimals, dotted abbreviations,
/// or common titles. Briefing summaries are plain sentences, not arbitrary prose.
fn briefing_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if index < start {
            continue;
        }
        let mut end = index + ch.len_utf8();
        if ch != '\n' && ch != '\r' {
            if !matches!(ch, '.' | '!' | '?' | '…') {
                continue;
            }
            // Include repeated punctuation and closing quotes/brackets.
            for next in text[end..].chars() {
                if !matches!(
                    next,
                    '.' | '!' | '?' | '…' | '"' | '\'' | '”' | '’' | ')' | ']'
                ) {
                    break;
                }
                end += next.len_utf8();
            }
            if text[end..]
                .chars()
                .next()
                .is_some_and(|next| !next.is_whitespace())
            {
                continue;
            }
            if ch == '.' && end == index + 1 && !text[end..].trim().is_empty() {
                let word = text[start..end]
                    .split_whitespace()
                    .next_back()
                    .unwrap_or("");
                let word = word
                    .trim_start_matches(['"', '\'', '(', '['])
                    .to_ascii_lowercase();
                if matches!(
                    word.as_str(),
                    "e.g."
                        | "i.e."
                        | "mr."
                        | "mrs."
                        | "ms."
                        | "dr."
                        | "prof."
                        | "sr."
                        | "jr."
                        | "vs."
                ) || (word.len() == 2 && word.as_bytes()[0].is_ascii_alphabetic())
                    || (word[..word.len().saturating_sub(1)].contains('.')
                        && word.trim_end_matches('.').split('.').all(|part| {
                            part.len() == 1 && part.as_bytes()[0].is_ascii_alphabetic()
                        }))
                {
                    continue;
                }
            }
        }
        if end <= start {
            continue;
        }
        let sentence = text[start..end].trim();
        if !sentence.is_empty() {
            sentences.push(sentence);
        }
        start = end;
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

/// 16-bit mono PCM WAV — the container AVAudioPlayer decodes on every
/// platform the voice feature runs on.
fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    let mut push = |bytes: &[u8]| out.extend_from_slice(bytes);
    push(b"RIFF");
    push(&(36 + data_len).to_le_bytes());
    push(b"WAVE");
    push(b"fmt ");
    push(&16u32.to_le_bytes());
    push(&1u16.to_le_bytes());
    push(&1u16.to_le_bytes());
    push(&sample_rate.to_le_bytes());
    push(&(sample_rate * 2).to_le_bytes());
    push(&2u16.to_le_bytes());
    push(&16u16.to_le_bytes());
    push(b"data");
    push(&data_len.to_le_bytes());
    for &sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn briefing_sentences_preserve_words_and_punctuation() {
        for (text, expected) in [
            (
                "First sentence. Second! Third? Last…",
                vec!["First sentence.", "Second!", "Third?", "Last…"],
            ),
            (
                "Dr. Lee tested v1.2, e.g. the retry. It works.",
                vec!["Dr. Lee tested v1.2, e.g. the retry.", "It works."],
            ),
            (
                "Use U.S. voices, i.e. English. Done.",
                vec!["Use U.S. voices, i.e. English.", "Done."],
            ),
            ("Updated to v1.2. Done.", vec!["Updated to v1.2.", "Done."]),
            (
                "Ready?! \"Done.\" Next... Final",
                vec!["Ready?!", "\"Done.\"", "Next...", "Final"],
            ),
            (
                "One line\n\nTwo lines\r\nLast.",
                vec!["One line", "Two lines", "Last."],
            ),
            ("  \n ", vec![]),
        ] {
            assert_eq!(briefing_sentences(text), expected, "{text:?}");
        }
    }

    #[test]
    fn voice_dataset_path_derives_the_dataset_layout() {
        assert_eq!(
            voice_dataset_path("en_US-lessac-medium").unwrap(),
            "en/en_US/lessac/medium/en_US-lessac-medium"
        );
        assert_eq!(
            voice_dataset_path("en_GB-northern_english_male-medium").unwrap(),
            "en/en_GB/northern_english_male/medium/en_GB-northern_english_male-medium"
        );
        assert!(voice_dataset_path("lessac").is_err());
    }

    #[test]
    fn piper_voice_or_default_enforces_the_catalog_boundary() {
        assert_eq!(piper_voice_or_default("en_US-ryan-high"), "en_US-ryan-high");
        assert_eq!(
            piper_voice_or_default(" en_US-ljspeech-high "),
            "en_US-ljspeech-high"
        );
        // Other locales, lower tiers, and malformed ids fall back.
        for stale in [
            "en_US-lessac-medium",
            "en_GB-alan-high",
            "en_US--high",
            "lessac",
            "",
        ] {
            assert_eq!(piper_voice_or_default(stale), DEFAULT_PIPER_VOICE);
        }
    }

    #[test]
    fn speaker_selection_is_bounded_and_changes_the_briefing_cache_key() {
        let voice = "en_US-libritts-high";
        assert_eq!(piper_speaker_or_default(voice, 903), 903);
        assert_eq!(piper_speaker_or_default(voice, 904), 0);
        assert_eq!(piper_speaker_or_default(voice, u32::MAX), 0);
        assert_ne!(piper_voice_key(voice, 0), piper_voice_key(voice, 42));
        assert_eq!(piper_voice_key(voice, 904), piper_voice_key(voice, 0));
        for voice in [
            "en_US-lessac-high",
            "en_US-ljspeech-high",
            "en_US-ryan-high",
            "",
        ] {
            assert_eq!(piper_speaker_count(voice), 1);
            assert_eq!(piper_voice_key(voice, 42), piper_voice_key(voice, 0));
        }
    }

    #[test]
    fn wav_bytes_writes_a_decodable_header() {
        let wav = wav_bytes(&[0.0, 0.5, -0.5, 1.0, -1.0], 22050);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 5 * 2);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 22050);
    }

    #[test]
    fn espeak_data_directory_follows_each_launch_layout() {
        let root = std::env::temp_dir().join(format!("goddard-espeak-{}", uuid::Uuid::new_v4()));
        let bundled = root.join("Goddard.app/Contents");
        let dev = root.join("target/debug");
        for dir in [
            bundled.join("MacOS"),
            bundled.join("Resources/espeak-ng-data"),
            dev.join("espeak-ng-data"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(bundled.join("MacOS/Goddard"), []).unwrap();
        std::fs::write(dev.join("goddard"), []).unwrap();

        assert_eq!(
            espeak_data_directory(&bundled.join("MacOS/Goddard")),
            Some(bundled.join("Resources"))
        );
        assert_eq!(
            espeak_data_directory(&dev.join("goddard")),
            Some(dev.clone())
        );
        // A test binary under deps/ still finds the profile dir's copy.
        let deps = dev.join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        std::fs::write(deps.join("waku-abcdef"), []).unwrap();
        assert_eq!(
            espeak_data_directory(&deps.join("waku-abcdef")),
            Some(dev.clone())
        );

        std::fs::remove_dir_all(&root).unwrap();
    }
}
