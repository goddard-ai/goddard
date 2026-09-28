//! Voice briefing (experimental): landing on a task whose latest reply is
//! long speaks a short "what happened / what you decide" summary aloud.
//! A reply that settles off screen gets its clip built immediately — the
//! selected provider's chat model writes a ~45-second plain-speech transcript
//! and the chosen TTS model voices it — so opening the task plays
//! instantly instead of waiting on both calls. Sessions that settled
//! before the clip cache existed still generate on arrival. Everything
//! degrades quietly — no key, no model, a short reply, or a failed call
//! all leave the transcript as the only surface.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use futures::future::{Either, select};
use futures::io::AsyncReadExt;
use futures::{FutureExt, pin_mut};
use serde_json::{Value, json};
use uuid::Uuid;

use waku_protocol::inference::InferenceProvider;

use super::status_markers::tail_chars;
use super::*;

/// Replies shorter than this read faster than their briefing would.
const MIN_RESPONSE_CHARS: usize = 300;
/// Only the tail of a long reply reaches the summarizer — conclusions and
/// asks live at the end, and the cap keeps the request inside a small
/// model's latency budget.
const RESPONSE_INPUT_CHARS: usize = 24_000;
/// Roughly 45 seconds of speech at a normal pace; the prompt asks for this
/// and the result is trimmed to it as a backstop.
const TRANSCRIPT_WORD_CAP: usize = 110;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CHAT_COMPLETIONS_URL: &str = "https://ai-gateway.vercel.sh/v1/chat/completions";
const SPEECH_URL: &str = "https://ai-gateway.vercel.sh/v4/ai/speech-model";
const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_SPEECH_URL: &str = "https://openrouter.ai/api/v1/audio/speech";
/// The example voice ID used by the Fish Audio gateway models.
const FISH_AUDIO_VOICE: &str = "933563129e564b19a115bedd57b7406a";
/// Ready clips are a small cache, not a library — a ~45s audio clip is a few
/// MB, so eight covers an unread sweep without holding the heap.
const BRIEFING_CLIPS_CAP: usize = 8;
/// Pipelines in flight at once; past this a settle simply misses its
/// prefetch and generates on arrival instead.
const BRIEFING_PENDING_CAP: usize = 4;
/// The dedupe set is a bound, not a history: past this it clears and a
/// revisit can brief again.
const BRIEFED_MESSAGES_CAP: usize = 256;

impl Waku {
    /// The settle-side half: a reply that finishes off screen gets its
    /// clip built now, so landing on the task plays instantly.
    pub(super) fn prefetch_voice_brief(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.state.selected_session == Some(session_id) {
            return;
        }
        let Some((message_id, response)) = self.voice_briefing_candidate(session_id) else {
            return;
        };
        if self.briefed_messages.contains(&message_id)
            || self.briefing_clips.contains_key(&message_id)
            || self.briefing_pending.contains_key(&message_id)
        {
            return;
        }
        self.start_voice_briefing(message_id, response, false, cx);
    }

    /// The activation-side half: play the clip if it is ready, ride a
    /// prefetch already in flight, or build it on arrival.
    pub(super) fn maybe_voice_brief(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some((message_id, response)) = self.voice_briefing_candidate(session_id) else {
            return;
        };
        if self.briefed_messages.contains(&message_id) {
            return;
        }
        if let Some(bytes) = self.briefing_clips.get(&message_id) {
            if crate::platform::play_briefing_audio(bytes, self.state.completion_sound_volume) {
                self.mark_briefed(message_id);
            } else {
                self.show_toast(tr!("errors.voice_briefing_playback"));
            }
            return;
        }
        if let Some(play) = self.briefing_pending.get_mut(&message_id) {
            // A prefetch for this reply is already running — flag it to
            // play the moment it lands rather than starting a second.
            *play = true;
            return;
        }
        self.start_voice_briefing(message_id, response, true, cx);
    }

    /// Shared gate: experiment on, key and model set, the session settled
    /// enough that its latest reply is final, and that reply long enough
    /// to be worth hearing. Returns the message id and the tail excerpt
    /// the summarizer sees.
    fn voice_briefing_candidate(&self, session_id: Uuid) -> Option<(Uuid, String)> {
        if !self.state.voice_briefing_enabled {
            return None;
        }
        let provider = self.state.voice_briefing_provider;
        if !self
            .state
            .inference
            .get(&provider)
            .is_some_and(|entry| entry.credential_configured)
            || self.state.voice_briefing_summary_model.trim().is_empty()
            || (self.state.voice_briefing_tts_model == VoiceBriefingTtsModel::Custom
                && self.state.voice_briefing_tts_custom_model.trim().is_empty())
        {
            return None;
        }
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;
        // Connecting, working, and parked-with-detached-work all mean the
        // reply is still moving; waiting-for-input is exactly the moment a
        // briefing helps.
        if matches!(
            session.status,
            SessionStatus::Connecting | SessionStatus::Working | SessionStatus::Background
        ) {
            return None;
        }
        let message = session
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::Assistant && !message.streaming)?;
        if message.visible_content().chars().count() < MIN_RESPONSE_CHARS {
            return None;
        }
        Some((
            message.id,
            tail_chars(message.visible_content(), RESPONSE_INPUT_CHARS),
        ))
    }

    /// Stamp a reply as played. The set is a bound, not a history — past
    /// the cap it clears and an old revisit can brief again.
    fn mark_briefed(&mut self, message_id: Uuid) {
        if self.briefed_messages.len() >= BRIEFED_MESSAGES_CAP {
            self.briefed_messages.clear();
        }
        self.briefed_messages.insert(message_id);
    }

    /// Run the summarize → speak pipeline for one reply. `play` decides
    /// whether the finished clip sounds on arrival — prefetch runs with it
    /// off and only fills the cache.
    fn start_voice_briefing(
        &mut self,
        message_id: Uuid,
        response: String,
        play: bool,
        cx: &mut Context<Self>,
    ) {
        if self.briefing_pending.len() >= BRIEFING_PENDING_CAP {
            return;
        }
        self.briefing_pending.insert(message_id, play);
        let provider = self.state.voice_briefing_provider;
        let summary_model = self.state.voice_briefing_summary_model.trim().to_owned();
        let tts_model = self.state.voice_briefing_tts_model;
        let tts_model_id = match tts_model {
            VoiceBriefingTtsModel::Custom => {
                self.state.voice_briefing_tts_custom_model.trim().to_owned()
            }
            _ => tts_model
                .model_id_for(provider)
                .unwrap_or_default()
                .to_owned(),
        };
        let http = cx.http_client();
        let daemon = self.daemon.client();
        let executor = cx.background_executor().clone();
        let work = executor.spawn({
            let executor = executor.clone();
            async move {
                // The credential lives in the daemon's secret store — the
                // app's settings mirror only carries the configured flag.
                let key = daemon
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
                        anyhow!("{} has no configured credential", provider.display_name())
                    })?;
                let transcript =
                    summarize(&http, &executor, provider, &key, &summary_model, &response)
                        .await
                        .context("summary generation")?;
                synthesize(&http, &executor, provider, &key, &tts_model_id, &transcript)
                    .await
                    .context("speech generation")
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, _| {
                let play = this.briefing_pending.remove(&message_id).unwrap_or(false);
                match result {
                    Ok(bytes) => {
                        this.briefing_clips.insert(message_id, bytes);
                        this.briefing_clip_order.push_back(message_id);
                        while this.briefing_clip_order.len() > BRIEFING_CLIPS_CAP {
                            if let Some(oldest) = this.briefing_clip_order.pop_front() {
                                this.briefing_clips.remove(&oldest);
                            }
                        }
                        if play && !this.briefed_messages.contains(&message_id) {
                            // AVAudioPlayer must start on the UI thread, so
                            // the bytes ride the spawn back rather than
                            // playing from the executor.
                            let started =
                                this.briefing_clips.get(&message_id).is_some_and(|bytes| {
                                    crate::platform::play_briefing_audio(
                                        bytes,
                                        this.state.completion_sound_volume,
                                    )
                                });
                            if started {
                                this.mark_briefed(message_id);
                            } else {
                                this.show_toast(tr!("errors.voice_briefing_playback"));
                            }
                        }
                    }
                    // Backend error bodies can echo the prompt — the toast
                    // stays generic and the detail only hits stderr.
                    Err(error) => {
                        eprintln!("Goddard: voice briefing failed: {error:#}");
                        if play {
                            this.show_toast(tr!("errors.voice_briefing"));
                        }
                    }
                }
            });
        })
        .detach();
    }
}

/// Ask the provider's chat model for the spoken transcript: what the reply
/// did and what it needs from the user, in plain sentences bounded to the
/// word cap. Both endpoints answer the OpenAI chat-completions envelope.
async fn summarize(
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    provider: InferenceProvider,
    key: &str,
    model: &str,
    response: &str,
) -> anyhow::Result<String> {
    let body = json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": format!(
                    "You write a short spoken briefing for a user returning to an agent \
                     coding session. Given the agent's latest reply, say what it did and \
                     how it ended, then state plainly any decision or action the user \
                     needs to take. Plain spoken sentences only — no markdown, lists, or \
                     code. At most {TRANSCRIPT_WORD_CAP} words. Output only the transcript."
                ),
            },
            {"role": "user", "content": response},
        ],
    });
    let parsed = match provider {
        InferenceProvider::VercelGateway => {
            post_json(
                http,
                executor,
                CHAT_COMPLETIONS_URL,
                key,
                provider,
                None,
                &body,
            )
            .await
        }
        InferenceProvider::OpenRouter => {
            post_json(
                http,
                executor,
                OPENROUTER_CHAT_URL,
                key,
                provider,
                None,
                &body,
            )
            .await
        }
        other => bail!(
            "{} cannot write a briefing transcript",
            other.display_name()
        ),
    }
    .with_context(|| format!("summary gateway request for model {model}"))?;
    let transcript = parsed
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow!("the summary model returned no text"))?;
    Ok(transcript
        .split_whitespace()
        .take(TRANSCRIPT_WORD_CAP)
        .collect::<Vec<_>>()
        .join(" "))
}

/// Voice the transcript through the provider's speech endpoint. Vercel
/// answers a JSON envelope whose `audio` field is base64 audio; OpenRouter's
/// OpenAI-compatible `/audio/speech` answers the raw byte stream.
async fn synthesize(
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    provider: InferenceProvider,
    key: &str,
    model_id: &str,
    text: &str,
) -> anyhow::Result<Vec<u8>> {
    match provider {
        InferenceProvider::VercelGateway => {
            let (voice, output_format) = speech_parameters(model_id);
            let body = json!({
                "text": text,
                "voice": voice,
                "outputFormat": output_format,
            });
            let parsed = post_json(
                http,
                executor,
                SPEECH_URL,
                key,
                provider,
                Some(model_id),
                &body,
            )
            .await
            .with_context(|| format!("speech gateway request for model {model_id}"))?;
            let audio = parsed
                .get("audio")
                .and_then(Value::as_str)
                .filter(|audio| !audio.is_empty())
                .ok_or_else(|| anyhow!("the speech model returned no audio"))?;
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(audio)
                .context("the speech model returned invalid audio")
        }
        InferenceProvider::OpenRouter => {
            let body = json!({
                "model": model_id,
                "input": text,
                "voice": speech_parameters(model_id).0,
                "response_format": "mp3",
            });
            post(
                http,
                executor,
                OPENROUTER_SPEECH_URL,
                key,
                provider,
                None,
                &body,
            )
            .await
            .with_context(|| format!("speech gateway request for model {model_id}"))
        }
        other => bail!("{} cannot voice a briefing", other.display_name()),
    }
}

fn speech_parameters(model_id: &str) -> (&'static str, &'static str) {
    match model_id {
        "openai/tts-1" | "openai/tts-1-hd" => ("alloy", "mp3"),
        // Grok voices its own names — "eve" on both spellings (Vercel's
        // `spacexai/*` and OpenRouter's `x-ai/*`).
        "spacexai/grok-tts" | "x-ai/grok-voice-tts-1.0" => ("eve", "wav"),
        "fish-audio/s1" | "fish-audio/s2-pro" | "fish-audio/s2.1-pro" => (FISH_AUDIO_VOICE, "mp3"),
        "fish-audio/s2.1-pro-free" | "fish-audio/s2.1-pro-free:free" => (FISH_AUDIO_VOICE, "mp3"),
        _ => ("Kore", "wav"),
    }
}

/// POST a JSON body with the provider bearer and return the raw answer.
/// `model_header` carries the Vercel speech endpoint's model and protocol
/// headers — the AI-SDK surface (`/v4/ai/*`) rejects calls without
/// `ai-gateway-protocol-version`, so it rides along on every Vercel request
/// while chat completions names its model in the body instead. OpenRouter
/// takes the plain OpenAI envelope. Non-2xx statuses fail with the code
/// alone — error bodies can echo the request.
async fn post(
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    url: &str,
    key: &str,
    provider: InferenceProvider,
    model_header: Option<&str>,
    body: &Value,
) -> anyhow::Result<Vec<u8>> {
    let mut request = gpui::http_client::Request::post(url)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json");
    if provider == InferenceProvider::VercelGateway {
        request = request.header("ai-gateway-protocol-version", "0.0.1");
        if let Some(model) = model_header {
            request = request
                .header("ai-speech-model-specification-version", "4")
                .header("ai-model-id", model);
        }
    }
    let request = request.body(gpui::http_client::AsyncBody::from(serde_json::to_vec(
        body,
    )?))?;
    let exchange = async {
        let mut response = http.send(request).await?;
        let status = response.status();
        let mut bytes = Vec::new();
        response.body_mut().read_to_end(&mut bytes).await?;
        anyhow::Ok((status, bytes))
    };
    pin_mut!(exchange);
    let (status, bytes) = match select(exchange, executor.timer(REQUEST_TIMEOUT).fuse()).await {
        Either::Left((result, _)) => result?,
        Either::Right(_) => bail!("the gateway request timed out"),
    };
    if !status.is_success() {
        bail!("the gateway answered HTTP {status} for {url}");
    }
    Ok(bytes)
}

/// The JSON half of `post` — every endpoint but OpenRouter speech answers a
/// JSON envelope.
async fn post_json(
    http: &Arc<dyn gpui::http_client::HttpClient>,
    executor: &gpui::BackgroundExecutor,
    url: &str,
    key: &str,
    provider: InferenceProvider,
    model_header: Option<&str>,
    body: &Value,
) -> anyhow::Result<Value> {
    let bytes = post(http, executor, url, key, provider, model_header, body).await?;
    serde_json::from_slice(&bytes).context("the gateway returned invalid JSON")
}
