//! Canned speech for the boss: `boss speak` broadcasts land here through
//! `bossSpeechRequested`, and each fragment resolves to a clip in this
//! client's speech library or a fresh TTS synthesis. Generated clips persist
//! under `speech_clips_directory()`, so a fragment is voiced once and replayed
//! instantly after — exact text matches skip the eval entirely, and Jev only
//! arbitrates near misses (a saved clip whose wording differs but voices the
//! same thing). Every step degrades quietly: feature off, no credential, a
//! failed eval, or a failed synthesis just skips or generates the fragment.
//!
//! Playback reuses the briefing audio slot, so speech never overlaps a
//! voice briefing — queued clips chain through the playback tick.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use waku_protocol::eval::{EvalAnswer, EvalQuestion};
use waku_protocol::inference::InferenceProvider;

use super::voice_briefing::{speech_parameters, synthesize};
use super::*;

/// The eval-decisions feature tag for clip-reuse judgments.
const SPEECH_FEATURE: &str = "boss-speech";
/// Bound on the persisted library — enough stock phrases and names that the
/// eval candidates stay cheap, small enough that `clips/` stays tidy.
const SPEECH_LIBRARY_CAP: usize = 64;
/// How many candidate clips one fragment's reuse question may offer.
const SPEECH_JEV_CANDIDATES: usize = 16;
/// Request ids retained for dedupe — a broadcast arriving twice must not
/// voice twice.
const SPEECH_SEEN_CAP: usize = 64;
/// A clip is reused only when Jev is sure: `choice` confidence plus the
/// picked option's share of the probability mass. Wrong reuse voices the
/// wrong words, so the bar sits above a coin flip.
const REUSE_PROBABILITY: f64 = 0.7;
const REUSE_CONFIDENCE: f64 = 0.5;
/// The `Choice` option meaning "no saved clip fits — synthesize one".
/// Deliberately a sentence, not an id: option names are what the model sees.
const NEW_CLIP_OPTION: &str = "synthesize a new clip";

/// The persisted clip library: `index.json` beside the `clips/` audio files
/// it names. Entries append oldest-first so position implies recency.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct SpeechLibrary {
    clips: Vec<SpeechClip>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SpeechClip {
    id: Uuid,
    /// The fragment this clip voices — the reuse match runs on this text.
    text: String,
    /// File name inside `clips/` — never a path: the reader takes only the
    /// file-name component when loading.
    file: String,
}

impl SpeechLibrary {
    fn load(dir: &Path) -> Self {
        std::fs::read(dir.join("index.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir.join("clips"))?;
        let path = dir.join("index.json");
        let staged = dir.join("index.json.tmp");
        std::fs::write(&staged, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(staged, path)
    }

    fn clip_path(&self, dir: &Path, clip: &SpeechClip) -> Option<PathBuf> {
        Path::new(&clip.file)
            .file_name()
            .map(|name| dir.join("clips").join(name))
    }

    /// The clip whose spoken text matches `text` after whitespace and case
    /// normalization — the deterministic reuse path that costs no eval.
    fn exact_match<'a>(&'a self, text: &str) -> Option<&'a SpeechClip> {
        let key = clip_key(text);
        self.clips.iter().find(|clip| clip_key(&clip.text) == key)
    }

    /// Candidates worth spending a reuse question on: clips sharing the most
    /// tokens with the fragment, newest first as the tie-break.
    fn candidates(&self, text: &str) -> Vec<SpeechClip> {
        let tokens = token_set(text);
        let mut scored: Vec<(usize, usize, &SpeechClip)> = self
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| clip.text != NEW_CLIP_OPTION)
            .map(|(index, clip)| {
                let shared = token_set(&clip.text).intersection(&tokens).count();
                (shared, index, clip)
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        scored
            .into_iter()
            .take(SPEECH_JEV_CANDIDATES)
            .map(|(_, _, clip)| clip.clone())
            .collect()
    }

    /// Append a synthesized clip, evicting the oldest entries (and their
    /// files) past the cap.
    fn push(&mut self, dir: &Path, clip: SpeechClip) {
        self.clips.push(clip);
        while self.clips.len() > SPEECH_LIBRARY_CAP {
            let evicted = self.clips.remove(0);
            if let Some(path) = self.clip_path(dir, &evicted) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Whitespace- and case-insensitive match key — "Your build" and "your
///  build" are the same utterance.
fn clip_key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn token_set(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Apply Jev's reuse answer for one fragment: the picked clip only when it
/// clears the reuse bar, otherwise `None` and the fragment synthesizes.
fn picked_clip<'a>(
    answer: Option<&EvalAnswer>,
    candidates: &'a [SpeechClip],
) -> Option<&'a SpeechClip> {
    let Some(EvalAnswer::Choice {
        choice,
        confidence,
        probabilities,
    }) = answer
    else {
        return None;
    };
    if choice == NEW_CLIP_OPTION
        || confidence.unwrap_or(0.0) < REUSE_CONFIDENCE
        || probabilities.get(choice).copied().unwrap_or(0.0) < REUSE_PROBABILITY
    {
        return None;
    }
    candidates.iter().find(|clip| &clip.text == choice)
}

/// Resolve each fragment to audio bytes in order: exact clip hit, Jev-picked
/// clip, or a fresh synthesis appended to the library. A failed synthesis
/// fails the utterance — a sentence missing its middle is worse than none.
async fn resolve_speech_clips(
    client: &waku_client::DaemonClient,
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    eval_ready: bool,
    provider: InferenceProvider,
    credential: &str,
    model_id: &str,
    parts: &[String],
) -> anyhow::Result<Vec<Vec<u8>>> {
    let dir = waku_client::persistence::speech_clips_directory();
    let mut library = SpeechLibrary::load(&dir);
    let mut library_changed = false;
    let mut resolved: Vec<Option<SpeechClip>> = vec![None; parts.len()];
    for (index, part) in parts.iter().enumerate() {
        resolved[index] = library.exact_match(part).cloned();
    }

    // Jev arbitrates only fragments with no exact clip and only when the
    // daemon can answer — an unconfigured or failed eval generates instead.
    let open: Vec<usize> = resolved
        .iter()
        .enumerate()
        .filter_map(|(index, clip)| clip.is_none().then_some(index))
        .collect();
    if eval_ready && !open.is_empty() && !library.clips.is_empty() {
        let candidate_sets: Vec<Vec<SpeechClip>> = open
            .iter()
            .map(|&index| library.candidates(&parts[index]))
            .collect();
        let mut questions = BTreeMap::new();
        for (&index, candidates) in open.iter().zip(candidate_sets.iter()) {
            if candidates.is_empty() {
                continue;
            }
            let part = &parts[index];
            let mut criteria: BTreeMap<String, Option<String>> = candidates
                .iter()
                .map(|clip| (clip.text.clone(), None))
                .collect();
            criteria.insert(
                NEW_CLIP_OPTION.to_owned(),
                Some("No saved clip voices this fragment — synthesize a new one".to_owned()),
            );
            questions.insert(
                format!("part-{index}"),
                EvalQuestion::Choice {
                    instructions: format!(
                        "The user's assistant wants to voice this fragment aloud: \"{part}\". \
                         Which saved clip's spoken text voices it faithfully — the same words \
                         or a natural spoken equivalent? Pick a clip only when it is a suitable \
                         stand-in; otherwise pick \"{NEW_CLIP_OPTION}\"."
                    ),
                    criteria,
                },
            );
        }
        if !questions.is_empty() {
            let state = json!({
                "utterance": parts.join(""),
                "task": "Pick which saved voice clips, if any, voice each fragment so the audio can be reused instead of regenerated.",
            });
            if let Ok(waku_client::ResponsePayload::Evaluation { evaluation }) = client.request(
                Uuid::nil(),
                Uuid::nil(),
                waku_client::Command::Evaluate {
                    state,
                    questions,
                    feature: Some(SPEECH_FEATURE.to_owned()),
                    timeout_secs: None,
                },
            ) {
                for (&index, candidates) in open.iter().zip(candidate_sets.iter()) {
                    let answer = evaluation.answers.get(&format!("part-{index}"));
                    resolved[index] = picked_clip(answer, candidates).cloned();
                }
            }
        }
    }

    // Whatever stayed unresolved is generated once and banked for reuse.
    let extension = match provider {
        InferenceProvider::OpenRouter => "mp3",
        _ => speech_parameters(model_id).1,
    };
    for (index, part) in parts.iter().enumerate() {
        if resolved[index].is_some() {
            continue;
        }
        let bytes = synthesize(http, executor, provider, credential, model_id, part).await?;
        let id = Uuid::new_v4();
        let clip = SpeechClip {
            id,
            text: part.clone(),
            file: format!("{id}.{extension}"),
        };
        std::fs::create_dir_all(dir.join("clips"))?;
        if let Some(path) = library.clip_path(&dir, &clip) {
            std::fs::write(path, bytes)?;
        }
        library.push(&dir, clip.clone());
        resolved[index] = Some(clip);
        library_changed = true;
    }
    if library_changed {
        library.save(&dir)?;
    }

    resolved
        .iter()
        .map(|entry| {
            let clip = entry.as_ref().expect("every part resolved");
            let path = library
                .clip_path(&dir, clip)
                .ok_or_else(|| anyhow::anyhow!("speech clip has no file name"))?;
            std::fs::read(&path).with_context(|| format!("reading speech clip {}", clip.id))
        })
        .collect()
}

impl Waku {
    /// `bossSpeechRequested` broadcasts — each becomes a resolve-and-play
    /// pipeline when this client's voice feature can voice it.
    pub(super) fn drain_speech_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok((key, request_id, parts)) = self.speech_events.try_recv() {
            if self.speech_requests_seen.contains(&request_id) {
                continue;
            }
            self.speech_requests_seen.push_back(request_id);
            while self.speech_requests_seen.len() > SPEECH_SEEN_CAP {
                self.speech_requests_seen.pop_front();
            }
            self.start_speech_request(key, parts, cx);
            changed = true;
        }
        changed
    }

    /// Fire one utterance through the library → eval → synthesis pipeline,
    /// then queue the resolved clips behind whatever is already playing.
    /// Runs only while the voice briefing experiment is on and its provider
    /// has a credential — speak borrows the briefing voice wholesale.
    fn start_speech_request(
        &mut self,
        key: waku_client::DaemonKey,
        parts: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if parts.is_empty() || !self.state.voice_briefing_enabled {
            return;
        }
        let provider = self.state.voice_briefing_provider;
        let Some(daemon) = self.daemons.supervisor(key) else {
            return;
        };
        // Credentials live on the daemon the speak came from — a remote
        // boss's request resolves against its host's store, not this app's
        // mirror of the local document.
        let (credential_configured, eval_ready) = {
            let settings = daemon.settings();
            (
                settings
                    .inference
                    .get(&provider)
                    .is_some_and(|entry| entry.credential_configured),
                settings
                    .eval
                    .as_ref()
                    .is_some_and(|eval| eval.ready(&settings.inference)),
            )
        };
        if !credential_configured {
            return;
        }
        let request_id = Uuid::new_v4();
        self.last_speech_key = Some(key);
        self.last_speech_request = Some(request_id);
        self.last_speech_clips.clear();
        cx.notify();
        let tts_model = self.state.voice_briefing_tts_model;
        let model_id = match tts_model {
            VoiceBriefingTtsModel::Custom => {
                self.state.voice_briefing_tts_custom_model.trim().to_owned()
            }
            _ => tts_model
                .model_id_for(provider)
                .unwrap_or_default()
                .to_owned(),
        };
        let http = cx.http_client();
        let client = daemon.client();
        let executor = cx.background_executor().clone();
        let work = executor.spawn({
            let executor = executor.clone();
            async move {
                let credential = client
                    .request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::GetInferenceCredential { provider },
                    )
                    .ok()
                    .and_then(|payload| match payload {
                        waku_client::ResponsePayload::InferenceCredential { credential } => {
                            credential
                        }
                        _ => None,
                    })
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("{} has no configured credential", provider.display_name())
                    })?;
                resolve_speech_clips(
                    &client,
                    &http,
                    &executor,
                    eval_ready,
                    provider,
                    &credential,
                    &model_id,
                    &parts,
                )
                .await
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(clips) => {
                    if this.last_speech_request == Some(request_id) {
                        this.last_speech_clips = clips.clone();
                    }
                    this.speech_clip_queue
                        .extend(clips.into_iter().map(|clip| (key, clip)));
                    this.pump_speech_queue(cx);
                    cx.notify();
                }
                Err(error) => {
                    eprintln!("boss speech pipeline failed: {error:#}");
                    if this.last_speech_request == Some(request_id) {
                        this.last_speech_key = None;
                        this.last_speech_clips.clear();
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn toggle_boss_speech_transport(
        &mut self,
        key: waku_client::DaemonKey,
        cx: &mut Context<Self>,
    ) {
        if self.speech_playback_key == Some(key) && self.voice_briefing_playback.is_some() {
            self.toggle_voice_briefing_playback(cx);
            return;
        }
        if self.last_speech_key == Some(key) && !self.last_speech_clips.is_empty() {
            for clip in self.last_speech_clips.iter().rev() {
                self.speech_clip_queue.push_front((key, clip.clone()));
            }
            self.pump_speech_queue(cx);
        }
    }

    /// Sound the next queued speech clip when nothing is playing. Clip
    /// completion hands off through the playback tick, so a queued chain
    /// keeps voicing until the queue runs dry; undecodable clips drop.
    pub(super) fn pump_speech_queue(&mut self, cx: &mut Context<Self>) {
        if self.voice_briefing_playback.is_some() {
            return;
        }
        while let Some((key, bytes)) = self.speech_clip_queue.pop_front() {
            if let Some(duration) =
                crate::platform::play_briefing_audio(&bytes, self.state.completion_sound_volume)
            {
                self.speech_playback_key = Some(key);
                self.track_voice_briefing_playback(duration, cx);
                return;
            }
        }
        self.speech_playback_key = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(text: &str) -> SpeechClip {
        SpeechClip {
            id: Uuid::new_v4(),
            text: text.to_owned(),
            file: format!("{}.mp3", Uuid::new_v4()),
        }
    }

    #[test]
    fn exact_match_ignores_whitespace_and_case() {
        let mut library = SpeechLibrary::default();
        library.clips.push(clip("Your build  on"));
        assert!(library.exact_match(" your BUILD on ").is_some());
        assert!(library.exact_match("your build off").is_none());
    }

    #[test]
    fn candidates_rank_by_shared_tokens_newest_first() {
        let mut library = SpeechLibrary::default();
        library.clips.push(clip("unrelated words"));
        library.clips.push(clip("your build on"));
        library.clips.push(clip("the build finished"));
        let candidates = library.candidates("build finished today");
        assert_eq!(candidates[0].text, "the build finished");
        assert_eq!(candidates[1].text, "your build on");
        assert!(candidates.len() <= SPEECH_JEV_CANDIDATES);
    }

    #[test]
    fn eviction_removes_oldest_clips() {
        let dir = std::env::temp_dir().join(format!("speech-test-{}", Uuid::new_v4()));
        let mut library = SpeechLibrary::default();
        for index in 0..SPEECH_LIBRARY_CAP + 5 {
            library.push(&dir, clip(&format!("phrase {index}")));
        }
        assert_eq!(library.clips.len(), SPEECH_LIBRARY_CAP);
        assert_eq!(library.clips[0].text, "phrase 5");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn picked_clip_respects_the_reuse_bar() {
        let candidates = vec![clip("Goddard"), clip("Waku")];
        let accepted = EvalAnswer::Choice {
            choice: "Goddard".to_owned(),
            confidence: Some(0.9),
            probabilities: BTreeMap::from([
                ("Goddard".to_owned(), 0.8),
                (NEW_CLIP_OPTION.to_owned(), 0.2),
            ]),
        };
        assert_eq!(
            picked_clip(Some(&accepted), &candidates).unwrap().text,
            "Goddard"
        );

        let uncertain = EvalAnswer::Choice {
            choice: "Goddard".to_owned(),
            confidence: Some(0.9),
            probabilities: BTreeMap::from([
                ("Goddard".to_owned(), 0.6),
                (NEW_CLIP_OPTION.to_owned(), 0.4),
            ]),
        };
        assert!(picked_clip(Some(&uncertain), &candidates).is_none());

        let generate = EvalAnswer::Choice {
            choice: NEW_CLIP_OPTION.to_owned(),
            confidence: Some(0.95),
            probabilities: BTreeMap::from([(NEW_CLIP_OPTION.to_owned(), 0.95)]),
        };
        assert!(picked_clip(Some(&generate), &candidates).is_none());
        assert!(picked_clip(None, &candidates).is_none());
    }
}
