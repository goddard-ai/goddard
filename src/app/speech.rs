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

use super::piper::synthesize_piper;
use super::voice_briefing::{inference_credential, speech_parameters, synthesize};
use super::*;

/// The eval-decisions feature tag for clip-reuse judgments.
const SPEECH_FEATURE: &str = "boss-speech";
/// Library size at which clip expiry starts pruning — deliberately
/// generous: clips are tiny and every saved clip is a synthesis call
/// never made again. Under the threshold nothing is evicted; past it
/// expired clips drop first, then the least-recently-used.
const SPEECH_EXPIRY_THRESHOLD: usize = 256;
/// How long an unused clip stays live — every replay resets the clock,
/// so only fragments that fell out of rotation expire.
const SPEECH_CLIP_TTL_SECS: u64 = 30 * 24 * 60 * 60;
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
/// How many utterances may sit gated behind consent before the oldest
/// drop — an overflowing queue must not grow without bound.
const PENDING_BOSS_SPEECH_CAP: usize = 8;
/// How long "go ahead" stays armed after the latest gated utterance — the
/// mic never keeps listening on a prompt nobody is answering.
const VOICE_CONSENT_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
/// Restarts for a consent task that ended on its own before the feature
/// degrades to click-only consent.
const VOICE_CONSENT_RESTARTS: u8 = 3;
/// Cadence for re-polling the device set while the wanted input is
/// missing — the HAL device listener watches the device list and the
/// default-input property, neither of which reliably fires when a live
/// device gains input channels in place, the exact move a Bluetooth
/// headset makes flipping into HFP.
const VOICE_INPUT_RETRY: std::time::Duration = std::time::Duration::from_millis(500);
/// An input gap shorter than this never raises the "microphone
/// unavailable" row — a Bluetooth profile switch crosses it routinely.
const VOICE_INPUT_GRACE: std::time::Duration = std::time::Duration::from_millis(1_500);
/// Stop re-polling a missing input after this — a device gone that long
/// was removed rather than profile-switching, and any later HAL event
/// re-arms the poll.
const VOICE_INPUT_POLL_CAP: std::time::Duration = std::time::Duration::from_secs(60);

/// The persisted clip library: `index.json` beside the `clips/` audio files
/// it names. Entries append oldest-first; past `SPEECH_EXPIRY_THRESHOLD`
/// each clip's `expires_at` — refreshed on every use — decides what prunes.
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
    /// Unix seconds at which this clip expires — every use resets it to
    /// `now + SPEECH_CLIP_TTL_SECS`. Absent in libraries written before
    /// expiry existed, so those clips read as already expired once the
    /// library grows past the threshold and pruning engages.
    #[serde(default)]
    expires_at: u64,
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

    /// Reset a clip's expiry after a use — replays keep a clip live, so
    /// the least-recently-used clips are always next to prune.
    fn touch(&mut self, id: Uuid, now: u64) {
        if let Some(clip) = self.clips.iter_mut().find(|clip| clip.id == id) {
            clip.expires_at = now + SPEECH_CLIP_TTL_SECS;
        }
    }

    /// Append a synthesized clip and prune — over the threshold, expired
    /// entries drop first, then the least-recently-used.
    fn push(&mut self, dir: &Path, clip: SpeechClip, now: u64) {
        self.clips.push(clip);
        self.prune(dir, now);
    }

    /// Enforce expiry once the library is large. Under the threshold
    /// nothing prunes — disk is cheap and each clip is a skipped
    /// synthesis. Past it, expired clips go (deleting their files), then
    /// the least-recently-used until the library fits again.
    fn prune(&mut self, dir: &Path, now: u64) -> bool {
        if self.clips.len() <= SPEECH_EXPIRY_THRESHOLD {
            return false;
        }
        let mut index = 0;
        while index < self.clips.len() {
            if self.clips[index].expires_at <= now {
                self.evict_at(dir, index);
            } else {
                index += 1;
            }
        }
        while self.clips.len() > SPEECH_EXPIRY_THRESHOLD {
            let Some(oldest) = self
                .clips
                .iter()
                .enumerate()
                .min_by_key(|(_, clip)| clip.expires_at)
                .map(|(index, _)| index)
            else {
                break;
            };
            self.evict_at(dir, oldest);
        }
        true
    }

    /// Drop one entry and its audio file, keeping the index honest about
    /// what `clips/` still holds.
    fn evict_at(&mut self, dir: &Path, index: usize) {
        let evicted = self.clips.remove(index);
        if let Some(path) = self.clip_path(dir, &evicted) {
            let _ = std::fs::remove_file(path);
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

/// Where a fragment's audio comes from: the briefing provider's speech
/// endpoint with its stored credential, or a local Piper voice model that
/// needs neither.
enum SpeechEngine {
    Gateway {
        provider: InferenceProvider,
        credential: String,
        model_id: String,
        voice: String,
    },
    Piper {
        voice: String,
        speaker: u32,
    },
}

/// Resolve each fragment to audio bytes in order: exact clip hit, Jev-picked
/// clip, or a fresh synthesis appended to the library. A failed synthesis
/// fails the utterance — a sentence missing its middle is worse than none.
async fn resolve_speech_clips(
    client: &waku_client::DaemonClient,
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    eval_ready: bool,
    engine: &SpeechEngine,
    parts: &[String],
) -> anyhow::Result<Vec<Vec<u8>>> {
    let dir = waku_client::persistence::speech_clips_directory();
    let mut library = SpeechLibrary::load(&dir);
    let now = unix_time();
    let mut library_changed = library.prune(&dir, now);
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

    // Every reuse resets the clip's expiry — a clip in rotation never
    // prunes; only fragments nobody voices age out.
    if resolved.iter().any(Option::is_some) {
        for clip in resolved.iter().flatten() {
            library.touch(clip.id, now);
        }
        library_changed = true;
    }

    // Whatever stayed unresolved is generated once and banked for reuse.
    let extension = match engine {
        SpeechEngine::Gateway {
            provider: InferenceProvider::OpenRouter,
            ..
        } => "mp3",
        SpeechEngine::Gateway { model_id, .. } => speech_parameters(model_id).1,
        SpeechEngine::Piper { .. } => "wav",
    };
    for (index, part) in parts.iter().enumerate() {
        if resolved[index].is_some() {
            continue;
        }
        let bytes = match engine {
            SpeechEngine::Gateway {
                provider,
                credential,
                model_id,
                voice,
            } => synthesize(http, executor, *provider, credential, model_id, voice, part).await?,
            SpeechEngine::Piper { voice, speaker } => {
                synthesize_piper(http, executor, voice, *speaker, part).await?
            }
        };
        let id = Uuid::new_v4();
        let clip = SpeechClip {
            id,
            text: part.clone(),
            file: format!("{id}.{extension}"),
            expires_at: now + SPEECH_CLIP_TTL_SECS,
        };
        std::fs::create_dir_all(dir.join("clips"))?;
        if let Some(path) = library.clip_path(&dir, &clip) {
            std::fs::write(path, bytes)?;
        }
        library.push(&dir, clip.clone(), now);
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
    pub(super) fn toggle_dictation(&mut self, cx: &mut Context<Self>) {
        match &self.dictation_state {
            super::DictationState::Recording => self.finish_dictation(cx),
            super::DictationState::Transcribing | super::DictationState::ModelDownloading => {}
            _ => self.prepare_dictation(cx),
        }
    }

    fn prepare_dictation(&mut self, cx: &mut Context<Self>) {
        self.dictation_state = super::DictationState::ModelDownloading;
        cx.notify();
        let client = self.daemon.client();
        let work = cx.background_executor().spawn(async move {
            let status = client.request(
                Uuid::nil(),
                Uuid::nil(),
                waku_client::Command::GetWhistleStatus,
            )?;
            let (available, downloaded) = match status {
                waku_client::ResponsePayload::WhistleStatus {
                    available,
                    downloaded,
                } => (available, downloaded),
                _ => anyhow::bail!("daemon returned an unexpected Whistle status"),
            };
            anyhow::ensure!(available, "Whistle dictation is unavailable on this Mac");
            if downloaded {
                return Ok::<_, anyhow::Error>(());
            }
            match client.request(
                Uuid::nil(),
                Uuid::nil(),
                waku_client::Command::DownloadWhistleModel,
            )? {
                waku_client::ResponsePayload::WhistleStatus {
                    downloaded: true, ..
                } => Ok(()),
                _ => anyhow::bail!("Whistle model download did not complete"),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.whistle_model_downloaded = true;
                    this.start_dictation_permission_or_capture(cx);
                }
                Err(error) => {
                    this.dictation_state = super::DictationState::Error(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn start_dictation_permission_or_capture(&mut self, cx: &mut Context<Self>) {
        match crate::platform::microphone_access() {
            crate::platform::CaptureAccess::Granted => self.begin_dictation_capture(cx),
            crate::platform::CaptureAccess::Undetermined => {
                self.dictation_pending_permission = true;
                self.request_voice_mic_access();
            }
            crate::platform::CaptureAccess::Denied => {
                self.dictation_state =
                    super::DictationState::Error(tr!("composer.dictation_mic_denied"));
                cx.notify();
            }
        }
    }

    fn begin_dictation_capture(&mut self, cx: &mut Context<Self>) {
        crate::platform::end_consent_recognition();
        self.dictation_state = if crate::platform::begin_dictation_capture() {
            super::DictationState::Recording
        } else {
            super::DictationState::Error(tr!("composer.dictation_capture_failed"))
        };
        cx.notify();
        if matches!(&self.dictation_state, super::DictationState::Recording) {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if matches!(&this.dictation_state, super::DictationState::Recording) {
                        this.finish_dictation(cx);
                    }
                });
            })
            .detach();
        }
    }

    fn finish_dictation(&mut self, cx: &mut Context<Self>) {
        let Some(pcm) = crate::platform::finish_dictation_capture() else {
            self.dictation_state = super::DictationState::Error(tr!("composer.dictation_no_audio"));
            self.maybe_stop_voice_listener();
            cx.notify();
            return;
        };
        self.maybe_stop_voice_listener();
        self.dictation_state = super::DictationState::Transcribing;
        cx.notify();
        let client = self.daemon.client();
        let work = cx.background_executor().spawn(async move {
            client.request(
                Uuid::nil(),
                Uuid::nil(),
                waku_client::Command::Transcribe {
                    pcm,
                    language: None,
                    keywords: None,
                },
            )
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(waku_client::ResponsePayload::Transcription { text, .. }) => {
                    if !text.trim().is_empty() {
                        this.composer
                            .update(cx, |input, cx| input.insert_text(&text, cx));
                        this.dictation_state = super::DictationState::Idle;
                    } else {
                        this.dictation_state =
                            super::DictationState::Error(tr!("composer.dictation_not_recognized"));
                    }
                    cx.notify();
                }
                Ok(_) => {
                    this.dictation_state =
                        super::DictationState::Error(tr!("composer.dictation_unexpected_response"));
                    cx.notify();
                }
                Err(error) => {
                    this.dictation_state = super::DictationState::Error(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

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
            // Keep the spoken text in the selected conversation at receipt
            // time, before clip resolution can reorder the audible playback.
            if let Some(session) = self
                .state
                .selected_session
                .and_then(|id| self.state.session_mut(id))
            {
                let item = ActivityItem::new(
                    None,
                    ActivityKind::Tool,
                    "Spoken aloud",
                    Some(parts.join(" ")),
                    true,
                );
                session.transcript_blocks.push(TranscriptBlock {
                    after_message: session.messages.len(),
                    turn_id: session.active_turn_id(),
                    activities: vec![item],
                });
                session.updated_at = unix_time();
                self.stream_state_dirty = true;
            }
            self.start_speech_request(key, parts, cx);
            changed = true;
        }
        changed
    }

    /// Fire one utterance through the gate and then the library → eval →
    /// synthesis pipeline, queueing resolved clips behind whatever is
    /// already playing. Runs only while the voice briefing experiment is on
    /// and its provider has a credential — speak borrows the briefing voice
    /// wholesale.
    fn start_speech_request(
        &mut self,
        key: waku_client::DaemonKey,
        parts: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if parts.is_empty() || !self.state.voice_briefing_enabled {
            return;
        }
        // Outside the boss's own chat, speech waits for a deliberate consent:
        // the attention toast, entering the boss chat, or a spoken "go
        // ahead". A denied mic can't listen for any of that, so the request
        // plays as it always has.
        let mic = crate::platform::microphone_access();
        if self.boss_chat_key() != Some(key) && mic != crate::platform::CaptureAccess::Denied {
            if self.pending_boss_speech.len() == PENDING_BOSS_SPEECH_CAP {
                self.pending_boss_speech.pop_front();
            }
            if self.pending_boss_speech.is_empty() {
                self.show_toast_for(
                    tr!("boss.voice_waiting"),
                    super::ToastTone::Notice,
                    Some(super::ToastAction {
                        label: tr!("boss.voice_play").into(),
                        kind: super::ToastActionKind::BossSpeech,
                    }),
                    std::time::Duration::from_secs(10),
                );
            }
            self.pending_boss_speech.push_back((key, parts));
            if mic == crate::platform::CaptureAccess::Undetermined {
                self.request_voice_mic_access();
            } else {
                self.arm_voice_consent(cx);
            }
            return;
        }
        self.dispatch_speech_request(key, parts, mic, cx);
    }

    /// The resolve-and-play half of a speech request. The attention gate
    /// lives in `start_speech_request`; consented replays enter here
    /// directly so a spoken "go ahead" plays where the user already is.
    fn dispatch_speech_request(
        &mut self,
        key: waku_client::DaemonKey,
        parts: Vec<String>,
        mic: crate::platform::CaptureAccess,
        cx: &mut Context<Self>,
    ) {
        let provider = self.state.voice_briefing_provider;
        let tts_model = self.state.voice_briefing_tts_model;
        let Some(daemon) = self.daemons.supervisor(key) else {
            return;
        };
        // Credentials live on the daemon the speak came from — a remote
        // boss's request resolves against its host's store, not this app's
        // mirror of the local document. Piper voices entirely offline, so
        // only the gateway engine needs a credential at all.
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
        if !tts_model.is_piper() && !credential_configured {
            return;
        }
        // Ambient detection runs only once macOS has granted the mic; the
        // undetermined case asks now and the answer lands as a pump event.
        // A missing or silent detector always fails open to playback.
        match mic {
            crate::platform::CaptureAccess::Granted => {
                crate::platform::start_voice_listener();
            }
            crate::platform::CaptureAccess::Undetermined => self.request_voice_mic_access(),
            crate::platform::CaptureAccess::Denied => {}
        }
        let request_id = Uuid::new_v4();
        self.last_speech_key = Some(key);
        self.last_speech_request = Some(request_id);
        self.last_speech_clips.clear();
        cx.notify();
        let model_id = match tts_model {
            VoiceBriefingTtsModel::Custom => {
                self.state.voice_briefing_tts_custom_model.trim().to_owned()
            }
            _ => tts_model
                .model_id_for(provider)
                .unwrap_or_default()
                .to_owned(),
        };
        let gateway_voice = self.voice_briefing_gateway_voice().to_owned();
        let piper_speaker = self.state.voice_briefing_piper_speaker;
        let piper_voice =
            super::piper::piper_voice_or_default(&self.state.voice_briefing_piper_voice).to_owned();
        let http = cx.http_client();
        let client = daemon.client();
        let executor = cx.background_executor().clone();
        let work = executor.spawn({
            let executor = executor.clone();
            async move {
                let engine = if tts_model.is_piper() {
                    SpeechEngine::Piper {
                        voice: piper_voice,
                        speaker: piper_speaker,
                    }
                } else {
                    let credential = inference_credential(&client, provider)?;
                    SpeechEngine::Gateway {
                        provider,
                        credential,
                        model_id,
                        voice: gateway_voice,
                    }
                };
                resolve_speech_clips(&client, &http, &executor, eval_ready, &engine, &parts).await
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
                    this.maybe_stop_voice_listener();
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
        if self.last_speech_key != Some(key) || self.last_speech_clips.is_empty() {
            return;
        }
        // Clips already waiting behind the current playback voice on their
        // own — requeueing the retained utterance would sound it twice.
        if self
            .speech_clip_queue
            .iter()
            .any(|(queued, _)| *queued == key)
        {
            return;
        }
        for clip in self.last_speech_clips.iter().rev() {
            self.speech_clip_queue.push_front((key, clip.clone()));
        }
        self.pump_speech_queue(cx);
    }

    /// The toast action's consent: opening the chat is itself the ask, so
    /// select it before replaying.
    pub(super) fn accept_pending_boss_speech(&mut self, cx: &mut Context<Self>) {
        self.consent_pending_boss_speech(true, cx);
    }

    /// Flush the gated queue: drop the offer, end "go ahead" listening, and
    /// replay each utterance through the pipeline. A spoken consent just
    /// plays where the user already is; the toast also selects the chat.
    fn consent_pending_boss_speech(&mut self, open_chat: bool, cx: &mut Context<Self>) {
        self.hide_boss_speech_toast();
        if self.pending_boss_speech.is_empty() {
            return;
        }
        if open_chat {
            let session_id = self.pending_boss_speech.front().and_then(|(key, _)| {
                self.boss_ui
                    .states
                    .get(key)
                    .and_then(|state| state.session_id)
            });
            if let Some(session_id) = session_id {
                self.select_session(session_id, cx);
            }
        }
        crate::platform::end_consent_recognition();
        self.replay_pending_boss_speech(cx);
    }

    /// Entering a boss's chat is itself consent for its waiting queue —
    /// replay only that boss's gated utterances and leave other daemons'.
    pub(super) fn flush_pending_boss_speech_for(
        &mut self,
        key: waku_client::DaemonKey,
        cx: &mut Context<Self>,
    ) {
        if self.pending_boss_speech.is_empty() {
            return;
        }
        let (ready, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_boss_speech)
            .into_iter()
            .partition(|(owner, _)| *owner == key);
        self.pending_boss_speech = kept.into_iter().collect();
        if self.pending_boss_speech.is_empty() {
            crate::platform::end_consent_recognition();
            self.hide_boss_speech_toast();
        }
        for (key, parts) in ready {
            self.dispatch_speech_request(key, parts, crate::platform::microphone_access(), cx);
        }
    }

    /// Dismiss the attention toast only when the one showing is the boss
    /// voice offer — consent arriving late must not eat an unrelated toast.
    fn hide_boss_speech_toast(&mut self) {
        if matches!(
            self.toast
                .as_ref()
                .and_then(|toast| toast.action.as_ref())
                .map(|action| &action.kind),
            Some(super::ToastActionKind::BossSpeech)
        ) {
            self.hide_toast();
        }
    }

    fn replay_pending_boss_speech(&mut self, cx: &mut Context<Self>) {
        let pending = std::mem::take(&mut self.pending_boss_speech);
        for (key, parts) in pending {
            self.dispatch_speech_request(key, parts, crate::platform::microphone_access(), cx);
        }
    }

    /// Arm the mic-side consent path for gated utterances: the "go ahead"
    /// recognizer plus the bounded window that keeps the mic from staying
    /// hot on a prompt nobody is answering.
    fn arm_voice_consent(&mut self, cx: &mut Context<Self>) {
        self.voice_consent_timer_gen = self.voice_consent_timer_gen.wrapping_add(1);
        self.voice_consent_restarts = 0;
        let generation = self.voice_consent_timer_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VOICE_CONSENT_WINDOW).await;
            let _ = this.update(cx, |this, _| this.expire_voice_consent(generation));
        })
        .detach();
        if crate::platform::microphone_access() != crate::platform::CaptureAccess::Granted {
            return;
        }
        if crate::platform::start_voice_listener() {
            self.start_consent_recognition();
        }
    }

    /// The consent window lapsed: stop listening. Gated items still flush
    /// on a click or on entering the boss chat.
    fn expire_voice_consent(&mut self, generation: u64) {
        if generation != self.voice_consent_timer_gen {
            return;
        }
        crate::platform::end_consent_recognition();
        self.maybe_stop_voice_listener();
    }

    /// Start the on-device consent recognizer, asking for speech-recognition
    /// permission first when macOS hasn't decided yet. Denial degrades the
    /// feature to click-only consent — the toast keeps working.
    fn start_consent_recognition(&mut self) {
        match crate::platform::speech_recognition_access() {
            crate::platform::CaptureAccess::Granted => {
                let tx = self.boss_voice_gate_tx.clone();
                let wake = self.event_wake_tx.clone();
                crate::platform::begin_consent_recognition(Box::new(move |signal| {
                    let event = match signal {
                        crate::platform::ConsentSignal::Heard => VoiceGateEvent::ConsentHeard,
                        crate::platform::ConsentSignal::Ended => VoiceGateEvent::ConsentTaskEnded,
                    };
                    if tx.try_send(event).is_ok() {
                        signal_event_pump(&wake);
                    }
                }));
            }
            crate::platform::CaptureAccess::Undetermined => self.request_speech_auth(),
            crate::platform::CaptureAccess::Denied => {}
        }
    }

    /// Ask macOS for the mic once; the answer lands on the pump as
    /// `VoiceGateEvent::MicAccess`.
    fn request_voice_mic_access(&mut self) {
        if std::mem::replace(&mut self.voice_mic_requested, true) {
            return;
        }
        let tx = self.boss_voice_gate_tx.clone();
        let wake = self.event_wake_tx.clone();
        crate::platform::request_microphone_access(Box::new(move |granted| {
            if tx.try_send(VoiceGateEvent::MicAccess(granted)).is_ok() {
                signal_event_pump(&wake);
            }
        }));
    }

    /// Ask macOS for speech recognition once; the answer lands on the pump
    /// as `VoiceGateEvent::SpeechAuth`.
    fn request_speech_auth(&mut self) {
        if std::mem::replace(&mut self.speech_auth_requested, true) {
            return;
        }
        let tx = self.boss_voice_gate_tx.clone();
        let wake = self.event_wake_tx.clone();
        crate::platform::request_speech_recognition_access(Box::new(move |granted| {
            if tx.try_send(VoiceGateEvent::SpeechAuth(granted)).is_ok() {
                signal_event_pump(&wake);
            }
        }));
    }

    /// Consent and permission answers from the voice listener's threads.
    pub(super) fn drain_voice_gate_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok(event) = self.boss_voice_gate_events.try_recv() {
            changed = true;
            match event {
                VoiceGateEvent::ConsentHeard => self.consent_pending_boss_speech(false, cx),
                VoiceGateEvent::ConsentTaskEnded => {
                    if !self.pending_boss_speech.is_empty()
                        && self.voice_consent_restarts < VOICE_CONSENT_RESTARTS
                    {
                        self.voice_consent_restarts += 1;
                        crate::platform::end_consent_recognition();
                        self.start_consent_recognition();
                    }
                }
                VoiceGateEvent::MicAccess(true) => {
                    crate::platform::start_voice_listener();
                    if self.dictation_pending_permission {
                        self.dictation_pending_permission = false;
                        self.begin_dictation_capture(cx);
                    }
                    if !self.pending_boss_speech.is_empty() {
                        self.arm_voice_consent(cx);
                    }
                }
                VoiceGateEvent::MicAccess(false) => {
                    if self.dictation_pending_permission {
                        self.dictation_pending_permission = false;
                        self.dictation_state =
                            super::DictationState::Error(tr!("composer.dictation_mic_denied"));
                    }
                    // No mic means nothing to gate with — speak as today.
                    self.replay_pending_boss_speech(cx);
                }
                VoiceGateEvent::SpeechAuth(true) => {
                    if !self.pending_boss_speech.is_empty() {
                        self.start_consent_recognition();
                    }
                }
                VoiceGateEvent::SpeechAuth(false) => {}
                VoiceGateEvent::InputDevicesChanged => {
                    // The platform rebinds (or parks) the engine here, on the
                    // main thread that owns it; scratchpads mirror the
                    // availability into their status — behind the grace, so
                    // a brief Bluetooth profile switch never flashes it.
                    let available = crate::platform::voice_input_devices_changed();
                    self.sync_voice_input(available, cx);
                }
            }
        }
        changed
    }

    /// Anything still holding the mic open — playback or a queued clip,
    /// a consent session, a composer capture, or a scratchpad sink;
    /// gated items alone never hold it.
    fn voice_listener_wanted(&self) -> bool {
        !self.speech_clip_queue.is_empty()
            || self.voice_briefing_playback.is_some()
            || crate::platform::consent_recognition_active()
            || crate::platform::dictation_capture_active()
            || crate::platform::voice_audio_sink_active()
    }

    /// The mic goes off when nothing wants it any longer.
    pub(super) fn maybe_stop_voice_listener(&mut self) {
        if !self.voice_listener_wanted() {
            crate::platform::stop_voice_listener();
        }
    }

    /// Fold a fresh device-set verdict into the scratchpad status. An
    /// input that can actually capture clears the "microphone
    /// unavailable" row at once; a missing one only raises the row after
    /// the grace, and arms the retry poll — the HAL does not reliably
    /// report a device gaining input channels in place, so events alone
    /// can leave a returning headset waiting.
    pub(super) fn sync_voice_input(&mut self, available: bool, cx: &mut Context<Self>) {
        let ready = available && crate::platform::voice_listener_running();
        if ready {
            self.voice_input_down_since = None;
            self.set_voice_input_unavailable(false);
            return;
        }
        if !self.voice_listener_wanted() {
            // Nobody owns the tap — mirror the device set as before but
            // run no clock; the next start re-checks on its own.
            self.voice_input_down_since = None;
            self.set_voice_input_unavailable(!available);
            return;
        }
        match self.voice_input_down_since {
            None => self.voice_input_down_since = Some(Instant::now()),
            Some(since) if since.elapsed() >= VOICE_INPUT_GRACE => {
                self.set_voice_input_unavailable(true);
            }
            _ => {}
        }
        self.schedule_voice_input_retry(cx);
    }

    /// Keep re-polling the device set while the wanted input is missing —
    /// covers the device-list notifications CoreAudio never sends. One
    /// task at a time; it stops when the engine is live again, when the
    /// mic is no longer wanted, or when the outage outlives the cap.
    fn schedule_voice_input_retry(&mut self, cx: &mut Context<Self>) {
        if self.voice_input_retry_scheduled {
            return;
        }
        self.voice_input_retry_scheduled = true;
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(VOICE_INPUT_RETRY).await;
                let keep_polling = weak
                    .update(cx, |this, cx| {
                        let available = crate::platform::voice_input_devices_changed();
                        this.sync_voice_input(available, cx);
                        cx.notify();
                        this.voice_input_down_since
                            .is_some_and(|since| since.elapsed() < VOICE_INPUT_POLL_CAP)
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
            let _ = weak.update(cx, |this, _| {
                this.voice_input_retry_scheduled = false;
            });
        })
        .detach();
    }

    /// Sound the next queued speech clip when nothing is playing — and only
    /// once the mic says the room is quiet, so the boss never talks over the
    /// user. Clip completion hands off through the playback tick, so a
    /// queued chain keeps voicing until the queue runs dry; undecodable
    /// clips drop. A missing or silent detector fails open to playback.
    pub(super) fn pump_speech_queue(&mut self, cx: &mut Context<Self>) {
        if self.voice_briefing_playback.is_some() {
            return;
        }
        if self.speech_clip_queue.is_empty() {
            self.maybe_stop_voice_listener();
            return;
        }
        if crate::platform::ambient_speech_active() {
            if self.speech_waiting_for_ambient {
                return;
            }
            self.speech_waiting_for_ambient = true;
            let weak = cx.weak_entity();
            cx.spawn(async move |_, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                    let should_continue = weak
                        .update(cx, |this, cx| {
                            if !crate::platform::ambient_speech_active() {
                                this.speech_waiting_for_ambient = false;
                                this.pump_speech_queue(cx);
                                false
                            } else {
                                true
                            }
                        })
                        .unwrap_or(false);
                    if !should_continue {
                        break;
                    }
                }
            })
            .detach();
            return;
        }
        while let Some((key, bytes)) = self.speech_clip_queue.pop_front() {
            if let Some(duration) =
                crate::platform::play_briefing_audio(&bytes, self.state.completion_sound_volume)
            {
                self.speech_playback_key = Some(key);
                self.track_voice_briefing_playback(duration, None, false, cx);
                return;
            }
        }
        self.speech_playback_key = None;
        self.maybe_stop_voice_listener();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(text: &str) -> SpeechClip {
        clip_expiring(text, u64::MAX)
    }

    fn clip_expiring(text: &str, expires_at: u64) -> SpeechClip {
        SpeechClip {
            id: Uuid::new_v4(),
            text: text.to_owned(),
            file: format!("{}.mp3", Uuid::new_v4()),
            expires_at,
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
    fn eviction_removes_least_recently_used_clips() {
        let dir = std::env::temp_dir().join(format!("speech-test-{}", Uuid::new_v4()));
        let now = unix_time();
        let mut library = SpeechLibrary::default();
        for index in 0..SPEECH_EXPIRY_THRESHOLD + 5 {
            library.push(
                &dir,
                clip_expiring(&format!("phrase {index}"), now + index as u64),
                now,
            );
        }
        assert_eq!(library.clips.len(), SPEECH_EXPIRY_THRESHOLD);
        assert_eq!(library.clips[0].text, "phrase 5");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_used_clip_leaves_the_eviction_tail() {
        let dir = std::env::temp_dir().join(format!("speech-test-{}", Uuid::new_v4()));
        let now = 1_000;
        let mut library = SpeechLibrary::default();
        for index in 0..SPEECH_EXPIRY_THRESHOLD {
            library.push(
                &dir,
                clip_expiring(&format!("phrase {index}"), now + index as u64 + 1),
                now,
            );
        }
        // "phrase 0" sits on the tail; replaying it resets its expiry, so
        // the next push evicts "phrase 1" instead.
        library.touch(library.clips[0].id, now + 10_000);
        library.push(&dir, clip_expiring("fresh", now + 20_000), now);
        assert!(library.clips.iter().any(|clip| clip.text == "phrase 0"));
        assert!(!library.clips.iter().any(|clip| clip.text == "phrase 1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn expiry_prunes_only_past_the_threshold_and_drops_files() {
        let dir = std::env::temp_dir().join(format!("speech-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        let now = 1_000;
        let stale = clip_expiring("stale", now - 1);
        let stale_path = dir.join("clips").join(&stale.file);
        std::fs::write(&stale_path, b"clip").unwrap();
        // Under the threshold even an expired clip and its file stay put.
        let mut library = SpeechLibrary::default();
        library.clips.push(stale);
        assert!(!library.prune(&dir, now));
        assert!(stale_path.exists());
        // Crossing the threshold prunes the expired entry and its file
        // while every live clip survives.
        for index in 0..SPEECH_EXPIRY_THRESHOLD {
            library.push(
                &dir,
                clip_expiring(&format!("live {index}"), now + 100),
                now,
            );
        }
        assert_eq!(library.clips.len(), SPEECH_EXPIRY_THRESHOLD);
        assert!(!library.clips.iter().any(|clip| clip.text == "stale"));
        assert!(!stale_path.exists());
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
