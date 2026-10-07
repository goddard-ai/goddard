//! Voice briefing: automatic generation follows the visible idle chat.
//! One ready clip may wait behind the current playback; a newer successful
//! generation replaces it. Manual replay bypasses autoplay and the Jev gate.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use futures::future::{Either, select};
use futures::io::AsyncReadExt;
use futures::{FutureExt, pin_mut};
use serde_json::{Value, json};
use uuid::Uuid;

use waku_protocol::eval::{EvalAnswer, EvalQuestion};
use waku_protocol::inference::InferenceProvider;

use super::piper::{piper_voice_or_default, synthesize_piper};
use super::status_markers::tail_chars;
use super::*;
use crate::ui::ActivationExt;

/// Replies shorter than this read faster than their briefing would.
const MIN_RESPONSE_CHARS: usize = 300;
/// Only the tail of a long reply reaches the summarizer — conclusions and
/// asks live at the end, and the cap keeps the request inside a small
/// model's latency budget.
const RESPONSE_INPUT_CHARS: usize = 24_000;
/// Roughly 45 seconds of speech at a normal pace; caps transcripts even
/// when custom instructions ask for a longer briefing.
const TRANSCRIPT_WORD_CAP: usize = 110;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const CHAT_COMPLETIONS_URL: &str = "https://ai-gateway.vercel.sh/v1/chat/completions";
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
/// The automatic path's Jev gate: one Noul on the settled turn decides
/// whether the briefing is worth generating.
const GATE_FEATURE: &str = "voice-briefing-gate";
const GATE_QUESTION: &str = "brief";
const GATE_THRESHOLD: f64 = 0.5;

/// A rendered briefing clip: the transcript the summary model wrote and
/// the audio `voice` gave it. `voice` keys the clip to the engine and
/// voice that rendered it, so a settings change marks the clip stale
/// without touching its words.
pub(super) struct BriefingClip {
    audio: Vec<u8>,
    transcript: String,
    voice: String,
}

pub(super) fn default_voice_briefing_summary_instructions() -> String {
    "Summarize the agent's latest turn into a single, plain-spoken sentence under 20 words that states only the current status. \
     Do not include questions, details, next steps, suggestions, or greetings. \
     Write exclusively in plain text suitable for TTS, with no punctuation except a final period. Output only the sentence."
        .to_owned()
}

pub(super) fn effective_voice_briefing_summary_instructions(
    stored: &str,
    is_full_prompt: bool,
) -> String {
    if is_full_prompt {
        return stored.to_owned();
    }
    let mut instructions = default_voice_briefing_summary_instructions();
    if !stored.trim().is_empty() {
        instructions.push_str("\n\n");
        instructions.push_str(stored.trim());
    }
    instructions
}

/// Identity prevents a cancelled async result from consuming a later request
/// for the same message. Manual activation can claim an existing pipeline.
pub(super) struct PendingBriefing {
    generation: u64,
    play: bool,
    manual: bool,
}

impl PendingBriefing {
    fn take_current(
        pending: &mut HashMap<Uuid, Self>,
        message_id: Uuid,
        generation: u64,
    ) -> Option<Self> {
        if pending.get(&message_id)?.generation != generation {
            return None;
        }
        pending.remove(&message_id)
    }
}

/// Ready automatic clips have one waiting slot. Older async completions
/// cannot displace a newer clip, even after that clip has started playing.
#[derive(Default)]
pub(super) struct BriefingQueue {
    sequence: u64,
    accepted: u64,
    waiting: Option<Uuid>,
}

impl BriefingQueue {
    fn issue(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }

    fn accept(&mut self, sequence: u64, message_id: Uuid) -> bool {
        if sequence <= self.accepted {
            return false;
        }
        self.accepted = sequence;
        self.waiting = Some(message_id);
        true
    }
}

fn automatic_briefing_allowed(
    viewed: Option<Uuid>,
    session_id: Uuid,
    status: SessionStatus,
) -> bool {
    viewed == Some(session_id)
        && !matches!(
            status,
            SessionStatus::Connecting | SessionStatus::Working | SessionStatus::Background
        )
}

impl Waku {
    /// The settle-side half: a reply that finishes off screen gets its
    /// clip built now, so landing on the task plays instantly. Runs only
    /// under automatic playback — manual mode leaves generation to the
    /// footer's on-demand button.
    fn viewed_briefing_session(&self) -> Option<Uuid> {
        if self.settings_page.is_some() || self.selected_terminal.is_some() {
            return None;
        }
        match self.navigation_location() {
            Some(NavigationLocation::Task(id)) => Some(id),
            _ => None,
        }
    }

    pub(super) fn sync_voice_briefing_navigation(&mut self) -> bool {
        let viewed = self.viewed_briefing_session();
        if viewed == self.briefing_viewed_session {
            return false;
        }
        self.briefing_viewed_session = viewed;
        // Pausing is terminal for the chrome. A manual replay remains possible.
        if self
            .voice_briefing_playback
            .is_some_and(|p| p.message_id.is_some())
        {
            crate::platform::pause_briefing_audio();
            self.voice_briefing_playback = None;
            self.voice_briefing_playback_generation =
                self.voice_briefing_playback_generation.wrapping_add(1);
        }
        self.briefing_queue.waiting = None;
        self.briefing_pending.clear();
        self.briefing_gate_pending.clear();
        true
    }

    /// A completed turn can brief only while its chat is visible.
    pub(super) fn prefetch_voice_brief(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.maybe_voice_brief(session_id, cx);
    }

    /// On arrival, consider only the last completed reply of an idle chat —
    /// play the clip if it is ready, ride a prefetch already in flight,
    /// revoice a stale clip in place, or build it.
    pub(super) fn maybe_voice_brief(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if !self.state.voice_briefing_autoplay || self.viewed_briefing_session() != Some(session_id)
        {
            return;
        }
        let Some((message_id, turn_id, response)) = self.voice_briefing_candidate(session_id)
        else {
            return;
        };

        if let Some(play) = self
            .briefing_pending
            .get_mut(&message_id)
            .or(self.briefing_gate_pending.get_mut(&message_id))
        {
            // A pipeline for this reply is already running — flag it to
            // play the moment it lands rather than starting a second.
            play.play = true;
            return;
        }
        if let Some(clip) = self.briefing_clips.get(&message_id) {
            if clip.voice != self.voice_briefing_voice_key() {
                // The voice changed since the latest turn's clip rendered
                // — revoice its cached transcript on arrival. Earlier
                // turns keep their rendered voice: they're history.
                let transcript = clip.transcript.clone();
                self.revoice_voice_briefing(message_id, transcript, cx);
                return;
            }
            if self.briefed_messages.contains(&message_id) {
                return;
            }
            let sequence = self.briefing_queue.issue();
            self.briefing_queue.accept(sequence, message_id);
            self.pump_briefing_queue(cx);
            return;
        }
        if self.briefed_messages.contains(&message_id) {
            return;
        }
        self.queue_voice_briefing(session_id, message_id, turn_id, response, true, cx);
    }

    /// What would voice a briefing rendered now: `piper:<voice>` for the
    /// local engine, `provider:model` for a gateway voice. A cached clip
    /// whose key differs was rendered under an older setting.
    fn voice_briefing_voice_key(&self) -> String {
        let provider = self.state.voice_briefing_provider;
        let tts_model = self.state.voice_briefing_tts_model;
        if tts_model.is_piper() {
            return super::piper::piper_voice_key(
                &self.state.voice_briefing_piper_voice,
                self.state.voice_briefing_piper_speaker,
            );
        }
        let model_id = if tts_model.is_custom() {
            self.state.voice_briefing_tts_custom_model.trim()
        } else {
            tts_model.model_id_for(provider).unwrap_or_default()
        };
        format!("{}:{model_id}", provider.id())
    }

    /// Shared gate: experiment on, key and model set, the session settled
    /// enough that its latest reply is final, and that reply long enough
    /// to be worth hearing. Returns the message id, its turn id for the
    /// Jev gate's state, and the tail excerpt the summarizer sees.
    fn voice_briefing_candidate(&self, session_id: Uuid) -> Option<(Uuid, Option<Uuid>, String)> {
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
        if !automatic_briefing_allowed(self.viewed_briefing_session(), session_id, session.status) {
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
            message.turn_id,
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

    /// The automatic path's Jev gate: when enabled and evaluable, a `Noul`
    /// on the turn decides whether the reply is worth a spoken briefing
    /// before either gateway call runs. Anything that keeps the gate from
    /// answering — unconfigured backend, a turn the state builder can't
    /// see, a failed or missing answer — fails open and the briefing
    /// generates; only a confident "no" suppresses it.
    fn queue_voice_briefing(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        turn_id: Option<Uuid>,
        response: String,
        play: bool,
        cx: &mut Context<Self>,
    ) {
        let sequence = self.briefing_queue.issue();
        if self.state.voice_briefing_gate_enabled
            && self.briefing_gate_pending.len() < BRIEFING_PENDING_CAP
            && let Some((daemon, state)) = self.voice_briefing_gate_request(session_id, turn_id)
        {
            let custom = self
                .state
                .voice_briefing_gate_instructions
                .trim()
                .to_owned();
            let instructions = if custom.is_empty() {
                "Should the user proactively hear a short spoken briefing when they return \
                 to this task? Answer true when the turn's reply warrants the interruption \
                 — a decision only the user can make, a failure or surprise worth flagging \
                 — and false when it is routine or self-explanatory."
                    .to_owned()
            } else {
                format!(
                    "Should the user proactively hear a short spoken briefing when they \
                     return to this task? Apply these criteria from the user: {custom}"
                )
            };
            let questions = BTreeMap::from([(
                GATE_QUESTION.to_owned(),
                EvalQuestion::Noul {
                    instructions,
                    criteria: None,
                },
            )]);
            self.briefing_gate_pending.insert(
                message_id,
                PendingBriefing {
                    generation: sequence,
                    play,
                    manual: false,
                },
            );
            cx.notify();
            let work = cx.background_executor().spawn(async move {
                daemon
                    .client()
                    .request(
                        Uuid::nil(),
                        session_id,
                        waku_client::Command::Evaluate {
                            state,
                            questions,
                            feature: Some(GATE_FEATURE.to_owned()),
                            timeout_secs: None,
                        },
                    )
                    .ok()
                    .and_then(|payload| match payload {
                        waku_client::ResponsePayload::Evaluation { evaluation } => evaluation
                            .answers
                            .get(GATE_QUESTION)
                            .and_then(|answer| match answer {
                                EvalAnswer::Noul { noul } => Some(*noul >= GATE_THRESHOLD),
                                _ => None,
                            }),
                        _ => None,
                    })
                    .unwrap_or(true)
            });
            cx.spawn(async move |this, cx| {
                let approved = work.await;
                let _ = this.update(cx, |this, cx| {
                    // A cancel that landed mid-eval drops the entry — the
                    // answer, whatever it was, goes nowhere.
                    let Some(PendingBriefing { play, .. }) = PendingBriefing::take_current(
                        &mut this.briefing_gate_pending,
                        message_id,
                        sequence,
                    ) else {
                        return;
                    };
                    if this.viewed_briefing_session() != Some(session_id) {
                        return;
                    }
                    if approved {
                        this.start_voice_briefing(message_id, response, play, Some(sequence), cx);
                    } else {
                        // The gate said no — treat the reply as settled so
                        // arrivals don't re-ask the same question.
                        this.mark_briefed(message_id);
                        cx.notify();
                    }
                });
            })
            .detach();
            return;
        }
        self.start_voice_briefing(message_id, response, play, Some(sequence), cx);
    }

    /// The daemon and turn state the gate needs, or `None` when eval cannot
    /// run — the caller fails open and briefs without asking.
    fn voice_briefing_gate_request(
        &self,
        session_id: Uuid,
        turn_id: Option<Uuid>,
    ) -> Option<(waku_client::DaemonSupervisor, Value)> {
        let daemon = self.daemon_for_session(session_id)?;
        if !daemon.settings().eval_ready() {
            return None;
        }
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;
        let state = status_markers::turn_eval_state(session, turn_id?, None);
        Some((daemon, state))
    }

    /// The footer's headphones button and the palette command: generate —
    /// or replay — one reply's briefing on demand. Autoplay, the Jev gate,
    /// and the length floor don't apply; an explicit click is its own
    /// judgment. The experiment flag still gates the feature.
    pub(super) fn request_voice_briefing(&mut self, message_id: Uuid, cx: &mut Context<Self>) {
        if !self.state.voice_briefing_enabled {
            return;
        }
        self.sync_voice_briefing_navigation();
        // A gate eval in flight loses to the click — generate directly.
        self.briefing_gate_pending.remove(&message_id);
        if let Some(play) = self.briefing_pending.get_mut(&message_id) {
            play.play = true;
            play.manual = true;
            return;
        }
        if self.briefing_clips.contains_key(&message_id) {
            if !self.play_voice_briefing_clip(message_id, cx) {
                self.show_toast(tr!("errors.voice_briefing_playback"));
            }
            return;
        }
        let Some(response) = self
            .state
            .sessions
            .iter()
            .flat_map(|session| session.messages.iter())
            .find(|message| {
                message.id == message_id
                    && message.role == MessageRole::Assistant
                    && !message.streaming
            })
            .map(|message| tail_chars(message.visible_content(), RESPONSE_INPUT_CHARS))
            .filter(|response| !response.is_empty())
        else {
            return;
        };
        self.start_voice_briefing(message_id, response, true, None, cx);
    }

    /// Drop a briefing in flight — gate eval or generation — and treat the
    /// reply as heard so the automatic path does not re-arm it.
    pub(super) fn cancel_voice_briefing(&mut self, message_id: Uuid, cx: &mut Context<Self>) {
        let removed = self.briefing_pending.remove(&message_id).is_some()
            | self.briefing_gate_pending.remove(&message_id).is_some();
        if removed {
            self.mark_briefed(message_id);
            cx.notify();
        }
    }

    /// Whether a reply's clip is being decided or generated — the footer
    /// reads this to show the cancellable indicator.
    pub(super) fn voice_briefing_in_flight(&self, message_id: Uuid) -> bool {
        self.briefing_pending.contains_key(&message_id)
            || self.briefing_gate_pending.contains_key(&message_id)
    }

    /// The selected task's latest settled reply, for the palette's
    /// "last turn" generate — any assistant message qualifies regardless
    /// of length.
    pub(super) fn voice_briefing_last_reply(&self, session_id: Uuid) -> Option<Uuid> {
        self.state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::Assistant && !message.streaming)
            .map(|message| message.id)
    }

    /// Manual replay is available independently of automatic playback.
    pub(super) fn message_voice_briefing_footer(
        &self,
        message_id: Uuid,
    ) -> Option<VoiceBriefingFooter> {
        if self.voice_briefing_in_flight(message_id) {
            return Some(VoiceBriefingFooter::Generating);
        }
        self.state.voice_briefing_enabled.then(|| {
            if self.briefing_clips.contains_key(&message_id) {
                VoiceBriefingFooter::Generated
            } else {
                VoiceBriefingFooter::Generate
            }
        })
    }

    /// Briefing pipelines — decided or generating — that belong to this
    /// session's transcript, for the palette's cancel command.
    pub(super) fn session_briefing_in_flight(&self, session_id: Uuid) -> Vec<Uuid> {
        self.state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .into_iter()
            .flat_map(|session| session.messages.iter())
            .map(|message| message.id)
            .filter(|message_id| self.voice_briefing_in_flight(*message_id))
            .collect()
    }

    pub(super) fn voice_briefing_playback_status(&self) -> Option<super::VoiceBriefingPlayback> {
        self.voice_briefing_playback
    }

    pub(super) fn toggle_voice_briefing_playback(&mut self, cx: &mut Context<Self>) {
        let Some(playback) = self.voice_briefing_playback else {
            return;
        };
        self.voice_briefing_playback_generation =
            self.voice_briefing_playback_generation.wrapping_add(1);
        let remaining = if playback.playing {
            crate::platform::pause_briefing_audio()
        } else {
            crate::platform::resume_briefing_audio()
        };
        let Some(remaining) = remaining else {
            crate::platform::stop_briefing_audio();
            self.voice_briefing_playback = None;
            self.pump_briefing_queue(cx);
            self.pump_speech_queue(cx);
            cx.notify();
            return;
        };
        if remaining.is_zero() {
            crate::platform::stop_briefing_audio();
            self.voice_briefing_playback = None;
            self.pump_briefing_queue(cx);
            self.pump_speech_queue(cx);
            cx.notify();
            return;
        }

        let playing = !playback.playing;
        self.voice_briefing_playback = Some(super::VoiceBriefingPlayback {
            playing,
            remaining,
            message_id: playback.message_id,
        });
        if playing {
            self.schedule_voice_briefing_playback_tick(cx);
        }
        cx.notify();
    }

    /// Compact playback controls beside the context gauge.
    pub(super) fn voice_briefing_playback_controls(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let playback = self.voice_briefing_playback_status();
        let can_skip = playback.is_some()
            || self.briefing_queue.waiting.is_some()
            || !self.speech_clip_queue.is_empty();
        if !self.state.voice_briefing_enabled && !can_skip {
            return None;
        }
        let playing = playback.is_some_and(|playback| playback.playing);
        let button = |id, path, label, enabled| {
            div()
                .id(id)
                .when(enabled, |button| button.tab_index(0))
                .size(px(24.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.raised)
                .cursor_default()
                .when(enabled, |button| {
                    button
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .hover(|style| style.bg(theme.raised.blend(theme.overlay_strong)))
                })
                .when(!enabled, |button| button.opacity(0.4))
                .tooltip(Tooltip::text(label))
                .child(icon(path, 12.0, theme.text_secondary))
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(
                    button(
                        "voice-briefing-restart",
                        "icons/rotate-ccw.svg",
                        "Restart briefing",
                        playback.is_some(),
                    )
                    .when(playback.is_some(), |button| {
                        button.on_activation(cx, |this, _, cx| {
                            if let Some(remaining) = crate::platform::restart_briefing_audio() {
                                let message_id =
                                    this.voice_briefing_playback.and_then(|p| p.message_id);
                                this.track_voice_briefing_playback(remaining, message_id, cx);
                            }
                        })
                    }),
                )
                .child(
                    button(
                        "voice-briefing-toggle",
                        if playing {
                            "icons/pause.svg"
                        } else {
                            "icons/play.svg"
                        },
                        if playing { "Pause" } else { "Resume" },
                        playback.is_some(),
                    )
                    .when(playback.is_some(), |button| {
                        button.on_activation(cx, |this, _, cx| {
                            this.toggle_voice_briefing_playback(cx)
                        })
                    }),
                )
                .child(
                    button(
                        "voice-briefing-skip",
                        "icons/fast-forward.svg",
                        "Skip briefing",
                        can_skip,
                    )
                    .when(can_skip, |button| {
                        button.on_activation(cx, |this, _, cx| this.skip_voice_briefing(cx))
                    }),
                )
                .when_some(playback, |controls, playback| {
                    let seconds = playback.remaining.as_secs();
                    controls.child(
                        div()
                            .px(px(5.0))
                            .text_size(sp(11.0))
                            .text_color(theme.text_tertiary)
                            .child(format!("{:02}:{:02}", seconds / 60, seconds % 60)),
                    )
                }),
        )
    }

    fn skip_voice_briefing(&mut self, cx: &mut Context<Self>) {
        crate::platform::stop_briefing_audio();
        self.voice_briefing_playback = None;
        self.voice_briefing_playback_generation =
            self.voice_briefing_playback_generation.wrapping_add(1);
        self.pump_briefing_queue(cx);
        self.pump_speech_queue(cx);
        cx.notify();
    }

    fn pump_briefing_queue(&mut self, cx: &mut Context<Self>) {
        if self.voice_briefing_playback.is_some() || self.viewed_briefing_session().is_none() {
            return;
        }
        if let Some(message_id) = self.briefing_queue.waiting.take() {
            if !self.play_voice_briefing_clip(message_id, cx) {
                self.show_toast(tr!("errors.voice_briefing_playback"));
            }
        }
    }

    fn play_voice_briefing_clip(&mut self, message_id: Uuid, cx: &mut Context<Self>) -> bool {
        let Some(duration) = self.briefing_clips.get(&message_id).and_then(|clip| {
            crate::platform::play_briefing_audio(&clip.audio, self.state.completion_sound_volume)
        }) else {
            return false;
        };
        self.speech_playback_key = None;
        if self.briefing_queue.waiting == Some(message_id) {
            self.briefing_queue.waiting = None;
        }
        self.mark_briefed(message_id);
        self.track_voice_briefing_playback(duration, Some(message_id), cx);
        true
    }

    pub(super) fn track_voice_briefing_playback(
        &mut self,
        remaining: Duration,
        message_id: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        self.voice_briefing_playback_generation =
            self.voice_briefing_playback_generation.wrapping_add(1);
        self.voice_briefing_playback = Some(super::VoiceBriefingPlayback {
            playing: true,
            remaining,
            message_id,
        });
        self.schedule_voice_briefing_playback_tick(cx);
        cx.notify();
    }

    fn schedule_voice_briefing_playback_tick(&self, cx: &mut Context<Self>) {
        let generation = self.voice_briefing_playback_generation;
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let updated = weak.update(cx, |this, cx| {
                    if this.voice_briefing_playback_generation != generation {
                        return false;
                    }
                    let Some((playing, remaining)) = crate::platform::briefing_audio_status()
                    else {
                        crate::platform::stop_briefing_audio();
                        this.voice_briefing_playback = None;
                        this.voice_briefing_playback_generation =
                            this.voice_briefing_playback_generation.wrapping_add(1);
                        // A queued `speak` chain hands off here — pumping
                        // starts the next clip and its own tick.
                        this.pump_briefing_queue(cx);
                        this.pump_speech_queue(cx);
                        cx.notify();
                        return false;
                    };
                    if remaining.is_zero() {
                        crate::platform::stop_briefing_audio();
                        this.voice_briefing_playback = None;
                        this.voice_briefing_playback_generation =
                            this.voice_briefing_playback_generation.wrapping_add(1);
                        this.pump_briefing_queue(cx);
                        this.pump_speech_queue(cx);
                        cx.notify();
                        return false;
                    }
                    let message_id = this
                        .voice_briefing_playback
                        .and_then(|playback| playback.message_id);
                    this.voice_briefing_playback = Some(super::VoiceBriefingPlayback {
                        playing,
                        remaining,
                        message_id,
                    });
                    cx.notify();
                    playing
                });
                if !matches!(updated, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Run the summarize → speak pipeline for one reply. `play` decides
    /// whether the finished clip sounds on arrival — prefetch runs with it
    /// off and only fills the cache.
    fn start_voice_briefing(
        &mut self,
        message_id: Uuid,
        response: String,
        play: bool,
        sequence: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if sequence.is_some() && self.briefing_pending.len() >= BRIEFING_PENDING_CAP {
            return;
        }
        let viewed_session = self.viewed_briefing_session();
        let request_id = self.briefing_queue.issue();
        self.briefing_pending.insert(
            message_id,
            PendingBriefing {
                generation: request_id,
                play,
                manual: sequence.is_none(),
            },
        );
        cx.notify();
        let voice_key = self.voice_briefing_voice_key();
        let provider = self.state.voice_briefing_provider;
        let summary_model = self.state.voice_briefing_summary_model.trim().to_owned();
        let instructions = effective_voice_briefing_summary_instructions(
            &self.state.voice_briefing_summary_instructions,
            self.state.voice_briefing_summary_instructions_full_prompt,
        );
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
        let piper_speaker = self.state.voice_briefing_piper_speaker;
        let piper_voice = piper_voice_or_default(&self.state.voice_briefing_piper_voice).to_owned();
        let http = cx.http_client();
        let daemon = self.daemon.client();
        let executor = cx.background_executor().clone();
        let work = executor.spawn({
            let executor = executor.clone();
            async move {
                let key = inference_credential(&daemon, provider)?;
                let transcript = summarize(
                    &http,
                    &executor,
                    provider,
                    &key,
                    &summary_model,
                    &instructions,
                    &response,
                )
                .await
                .context("summary generation")?;
                // The gateway still wrote the transcript; only the voicing
                // switches to the local engine when Piper is selected.
                let audio = if tts_model.is_piper() {
                    synthesize_piper(&http, &executor, &piper_voice, piper_speaker, &transcript)
                        .await
                        .context("speech generation")?
                } else {
                    synthesize(&http, &executor, provider, &key, &tts_model_id, &transcript)
                        .await
                        .context("speech generation")?
                };
                anyhow::Ok((transcript, audio))
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_voice_briefing(
                    message_id,
                    request_id,
                    voice_key,
                    result,
                    sequence,
                    viewed_session,
                    cx,
                );
            });
        })
        .detach();
    }

    /// Re-voice a clip's cached transcript after the voice setting changed
    /// — same words, new voice. Only the speech half reruns: the summary
    /// stands, and Piper needs no credential for it at all.
    fn revoice_voice_briefing(
        &mut self,
        message_id: Uuid,
        transcript: String,
        cx: &mut Context<Self>,
    ) {
        if self.briefing_pending.len() >= BRIEFING_PENDING_CAP {
            return;
        }
        // The revoiced clip is a fresh utterance — let it sound on landing
        // even though an earlier voice already briefed this reply.
        self.briefed_messages.remove(&message_id);
        let viewed_session = self.viewed_briefing_session();
        let request_id = self.briefing_queue.issue();
        self.briefing_pending.insert(
            message_id,
            PendingBriefing {
                generation: request_id,
                play: true,
                manual: true,
            },
        );
        cx.notify();
        let voice_key = self.voice_briefing_voice_key();
        let provider = self.state.voice_briefing_provider;
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
        let piper_speaker = self.state.voice_briefing_piper_speaker;
        let piper_voice = piper_voice_or_default(&self.state.voice_briefing_piper_voice).to_owned();
        let http = cx.http_client();
        let daemon = self.daemon.client();
        let executor = cx.background_executor().clone();
        let work = executor.spawn({
            let executor = executor.clone();
            async move {
                let audio = if tts_model.is_piper() {
                    synthesize_piper(&http, &executor, &piper_voice, piper_speaker, &transcript)
                        .await
                        .context("speech generation")?
                } else {
                    let key = inference_credential(&daemon, provider)?;
                    synthesize(&http, &executor, provider, &key, &tts_model_id, &transcript)
                        .await
                        .context("speech generation")?
                };
                anyhow::Ok((transcript, audio))
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_voice_briefing(
                    message_id,
                    request_id,
                    voice_key,
                    result,
                    None,
                    viewed_session,
                    cx,
                );
            });
        })
        .detach();
    }

    /// Shared landing for the generate and revoice pipelines: cache the
    /// clip under the voice that rendered it, evicting the oldest past the
    /// cap, and sound it when the caller armed playback.
    fn finish_voice_briefing(
        &mut self,
        message_id: Uuid,
        request_id: u64,
        voice_key: String,
        result: anyhow::Result<(String, Vec<u8>)>,
        sequence: Option<u64>,
        viewed_session: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        // A cancel that landed mid-pipeline already dropped the entry —
        // discard the clip rather than caching it.
        let Some(PendingBriefing { play, manual, .. }) = PendingBriefing::take_current(
            &mut self.briefing_pending,
            message_id,
            request_id,
        ) else {
            return;
        };
        let sequence = if manual { None } else { sequence };
        if self.viewed_briefing_session() != viewed_session || viewed_session.is_none() {
            return;
        }
        match result {
            Ok((transcript, bytes)) => {
                // An older async completion can't displace a newer clip,
                // even after that clip started playing.
                if sequence.is_some_and(|sequence| sequence <= self.briefing_queue.accepted) {
                    cx.notify();
                    return;
                }
                // A revoice replaces the stale entry in place — keep its
                // queue slot so a voice swap can't shuffle recency.
                if !self.briefing_clips.contains_key(&message_id) {
                    self.briefing_clip_order.push_back(message_id);
                }
                self.briefing_clips.insert(
                    message_id,
                    BriefingClip {
                        audio: bytes,
                        transcript,
                        voice: voice_key,
                    },
                );
                while self.briefing_clip_order.len() > BRIEFING_CLIPS_CAP {
                    if let Some(oldest) = self.briefing_clip_order.pop_front() {
                        if self.briefing_queue.waiting == Some(oldest) {
                            self.briefing_clip_order.push_back(oldest);
                        } else {
                            self.briefing_clips.remove(&oldest);
                        }
                    }
                }
                if play && (sequence.is_none() || !self.briefed_messages.contains(&message_id))
                {
                    // AVAudioPlayer must start on the UI thread, so the
                    // bytes ride the spawn back rather than playing from
                    // the executor.
                    if let Some(sequence) = sequence {
                        if self.briefing_queue.accept(sequence, message_id) {
                            self.pump_briefing_queue(cx);
                        }
                    } else if !self.play_voice_briefing_clip(message_id, cx) {
                        self.show_toast(tr!("errors.voice_briefing_playback"));
                    }
                }
            }
            // Backend error bodies can echo the prompt — the toast stays
            // generic and the detail only hits stderr.
            Err(error) => {
                eprintln!("Goddard: voice briefing failed: {error:#}");
                if play {
                    self.show_toast(tr!("errors.voice_briefing"));
                }
            }
        }
        cx.notify();
    }
}

/// The provider's stored credential. It lives in the daemon's secret
/// store — the app's settings mirror only carries the configured flag.
/// Blocking IPC; call from a background task.
pub(super) fn inference_credential(
    client: &waku_client::DaemonClient,
    provider: InferenceProvider,
) -> anyhow::Result<String> {
    client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            waku_client::Command::GetInferenceCredential { provider },
        )
        .ok()
        .and_then(|payload| match payload {
            waku_client::ResponsePayload::InferenceCredential { credential } => credential,
            _ => None,
        })
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| anyhow!("{} has no configured credential", provider.display_name()))
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
    instructions: &str,
    response: &str,
) -> anyhow::Result<String> {
    let system = instructions;
    let body = json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": system,
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
pub(super) async fn synthesize(
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

pub(super) fn speech_parameters(model_id: &str) -> (&'static str, &'static str) {
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
pub(super) async fn post_json(
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

#[cfg(test)]
mod instructions_tests {
    use super::*;

    #[test]
    fn default_fallback_preserves_saved_custom_instructions() {
        let default = default_voice_briefing_summary_instructions();
        assert_eq!(
            effective_voice_briefing_summary_instructions("", false),
            default
        );

        let custom = "Read the complete reply exactly as written.";
        assert_eq!(
            effective_voice_briefing_summary_instructions(custom, true),
            custom
        );
        assert_eq!(
            effective_voice_briefing_summary_instructions(custom, false),
            format!("{default}\n\n{custom}")
        );
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn stale_result_preserves_the_replacement_request_for_manual_replay() {
        let id = Uuid::new_v4();
        let mut pending = HashMap::new();
        pending.insert(
            id,
            PendingBriefing {
                generation: 1,
                play: true,
                manual: false,
            },
        );
        pending.clear(); // leaving the chat cancels the old generation
        pending.insert(
            id,
            PendingBriefing {
                generation: 2,
                play: true,
                manual: true,
            },
        );
        assert!(PendingBriefing::take_current(&mut pending, id, 1).is_none());
        let replacement =
            PendingBriefing::take_current(&mut pending, id, 2).expect("replacement preserved");
        assert!(replacement.manual && replacement.play);
        assert!(pending.is_empty());
    }

    #[test]
    fn automatic_briefing_requires_the_visible_idle_chat() {
        let chat = Uuid::new_v4();
        assert!(automatic_briefing_allowed(
            Some(chat),
            chat,
            SessionStatus::Idle
        ));
        assert!(!automatic_briefing_allowed(None, chat, SessionStatus::Idle));
        assert!(!automatic_briefing_allowed(
            Some(Uuid::new_v4()),
            chat,
            SessionStatus::Idle
        ));
        for status in [
            SessionStatus::Connecting,
            SessionStatus::Working,
            SessionStatus::Background,
        ] {
            assert!(!automatic_briefing_allowed(Some(chat), chat, status));
        }
    }

    #[test]
    fn newest_generated_briefing_is_the_only_waiting_clip() {
        let mut queue = BriefingQueue::default();
        let first = queue.issue();
        let first_id = Uuid::new_v4();
        assert!(queue.accept(first, first_id));
        let declined = queue.issue();
        // A declined or failed generation never reaches accept.
        assert_eq!(queue.waiting, Some(first_id));
        let newest = queue.issue();
        let newest_id = Uuid::new_v4();
        assert!(queue.accept(newest, newest_id));
        assert_eq!(queue.waiting.take(), Some(newest_id));
        assert!(!queue.accept(declined, Uuid::new_v4()));
        assert!(queue.waiting.is_none());
    }
}
