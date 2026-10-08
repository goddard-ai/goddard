//! Voice briefing: automatic work happens while a chat is away — summary
//! text refreshes for the latest eligible completion and one eager audio
//! attempt per absence voices it — so arrival can speak without waiting.
//! A turn settling on screen is read, never briefed; leaving consumes the
//! visit's claim. Manual replay bypasses autoplay and the Jev gate.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use chrono::Timelike;
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
const BRIEFING_QUEUE_GAP: Duration = Duration::from_millis(1500);
/// Pipelines in flight at once; past this a settle simply misses its
/// prefetch and generates on arrival instead.
const BRIEFING_PENDING_CAP: usize = 4;
/// Away completions remembered per absence — well past any realistic burst.
const BRIEFING_CANDIDATES_CAP: usize = 16;
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

/// Who a running job serves — cancellation scopes read it. Leaving a visit
/// drops arrival work, the eager switch drops eager work, and only an
/// explicit footer or palette request is manual.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BriefingWork {
    Eager,
    Arrival,
    Manual,
}

/// What a running job produces. Gate evals decide a candidate's
/// eligibility; a summary job lands as prepared text; audio and full
/// pipelines end in the clip cache.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BriefingStage {
    Gate,
    Summary,
    Audio,
    Full,
}

/// Identity prevents a cancelled async result from consuming a later request
/// for the same message. Manual activation can claim an existing pipeline.
pub(super) struct PendingBriefing {
    generation: u64,
    session_id: Uuid,
    play: bool,
    work: BriefingWork,
    stage: BriefingStage,
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

/// Where a recorded away completion stands against the length floor and
/// the Jev gate. A rejection is final for the absence — it never displaces
/// an older eligible target.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BriefingEligibility {
    Unchecked,
    Checking,
    Eligible,
    Ineligible,
}

/// A settled off-screen reply that may earn a briefing — captured at
/// completion, before arrival's acknowledgement, so clearing unread state
/// never erases it.
struct BriefingCandidate {
    message_id: Uuid,
    turn_id: Option<Uuid>,
    eligibility: BriefingEligibility,
}

/// One visit's automatic briefing state. Arrival resolves the captured
/// candidates into a single claim; leaving, playing, failing, or a manual
/// takeover consumes it — nothing retries or re-arms inside a visit.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum BriefingVisit {
    #[default]
    Away,
    Resolving,
    Claimed,
    Done,
}

/// Per-chat briefing state — this absence's completions, what was prepared
/// for them, and the current visit's claim. Runtime only: a restart neither
/// prepares nor plays old unread history.
#[derive(Default)]
pub(super) struct SessionBriefing {
    /// Away completions this absence, oldest first.
    candidates: Vec<BriefingCandidate>,
    visit: BriefingVisit,
    /// The completion the current visit claimed for briefing.
    claim: Option<Uuid>,
    /// The latest eligible away completion — the eager target.
    target: Option<Uuid>,
    /// Latest prepared summary text and the reply it voices.
    summary: Option<(Uuid, String)>,
    /// The newest summary request waiting behind the running job — one
    /// replaceable slot, never a queue.
    summary_wanted: Option<Uuid>,
    /// A reply whose speculative summary already failed this absence — the
    /// job does not loop; arrival retries it once as the claim.
    summary_failed: Option<Uuid>,
    /// Generation ids of the session's in-flight automatic jobs — at most
    /// one summary and one audio job run at a time.
    summary_job: Option<u64>,
    audio_job: Option<u64>,
    /// This absence's one speculative audio attempt, spent at job start —
    /// a failed attempt stays spent.
    eager_audio_used: bool,
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

    fn take_ready(&mut self, playback_allowed: bool) -> Option<Uuid> {
        if playback_allowed {
            self.waiting.take()
        } else {
            None
        }
    }

    fn has_ready(&self, playback_allowed: bool) -> bool {
        playback_allowed && self.waiting.is_some()
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

/// The next step toward an arrival claim, walking candidates newest-first:
/// claim the first eligible reply, wait on a check already asked, ask the
/// first unchecked one, or exhaust the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CandidateScan {
    Claim(Uuid),
    Check(Uuid),
    Wait,
    Done,
}

fn scan_briefing_candidates(
    candidates: &[BriefingCandidate],
    briefed: &HashSet<Uuid>,
) -> CandidateScan {
    for candidate in candidates.iter().rev() {
        if briefed.contains(&candidate.message_id) {
            continue;
        }
        match candidate.eligibility {
            BriefingEligibility::Eligible => {
                return CandidateScan::Claim(candidate.message_id);
            }
            BriefingEligibility::Checking => return CandidateScan::Wait,
            BriefingEligibility::Unchecked => {
                return CandidateScan::Check(candidate.message_id);
            }
            BriefingEligibility::Ineligible => {}
        }
    }
    CandidateScan::Done
}

impl Waku {
    /// The chat whose transcript is on screen — navigation only; the
    /// window's focus state is the caller's job.
    pub(super) fn viewed_briefing_session(&self) -> Option<Uuid> {
        if self.settings_page.is_some() || self.selected_terminal.is_some() {
            return None;
        }
        match self.navigation_location() {
            Some(NavigationLocation::Task(id)) => Some(id),
            _ => None,
        }
    }

    /// Viewed means on screen *and* in a foreground window: a backgrounded
    /// or minimized app is away, and activating it is an arrival at the
    /// active session.
    fn briefing_is_viewed(&self, session_id: Uuid, has_active_window: bool) -> bool {
        has_active_window && self.viewed_briefing_session() == Some(session_id)
    }

    /// The session's eligibility marks on one candidate. A rejection is
    /// final — nothing resurrects it inside the absence.
    fn set_briefing_eligibility(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        eligibility: BriefingEligibility,
    ) {
        if let Some(candidate) = self
            .briefings
            .get_mut(&session_id)
            .and_then(|state| {
                state
                    .candidates
                    .iter_mut()
                    .find(|candidate| candidate.message_id == message_id)
            })
            .filter(|candidate| candidate.eligibility != BriefingEligibility::Ineligible)
        {
            candidate.eligibility = eligibility;
        }
    }

    /// Everything the automatic paths need before a provider call is worth
    /// making: the feature armed, a credential for the summary model, and a
    /// resolvable speech model.
    fn briefing_config_ready(&self) -> bool {
        let provider = self.state.voice_briefing_provider;
        self.state
            .inference
            .get(&provider)
            .is_some_and(|entry| entry.credential_configured)
            && !self.state.voice_briefing_summary_model.trim().is_empty()
            && (self.state.voice_briefing_tts_model != VoiceBriefingTtsModel::Custom
                || !self.state.voice_briefing_tts_custom_model.trim().is_empty())
    }

    /// Speculation is on only while the whole chain is armed — feature,
    /// autoplay, and the eager switch — and a provider can actually answer.
    fn eager_briefing_active(&self) -> bool {
        self.state.voice_briefing_enabled
            && self.state.voice_briefing_autoplay
            && self.state.voice_briefing_eager
            && self.briefing_config_ready()
    }

    pub(super) fn sync_voice_briefing_navigation(&mut self) -> bool {
        let viewed = self.viewed_briefing_session();
        if viewed == self.briefing_viewed_session {
            return false;
        }
        if let Some(departed) = std::mem::replace(&mut self.briefing_viewed_session, viewed) {
            self.briefing_departed(departed, true);
        }
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
        true
    }

    /// The chat left view — a navigation hop or a window deactivation.
    /// The visit's claim is consumed: arrival work cancels, automatic audio
    /// stops, and a fresh absence resets the candidates and the eager audio
    /// allowance. `include_manual` matches navigation's habit of dropping a
    /// manual pipeline mid-flight; window blurs keep it.
    pub(super) fn briefing_departed(&mut self, session_id: Uuid, include_manual: bool) {
        self.briefing_pending.retain(|_, pending| {
            pending.session_id != session_id
                || (pending.work == BriefingWork::Manual && !include_manual)
        });
        self.briefing_gate_pending
            .retain(|_, pending| pending.session_id != session_id);
        if let Some(state) = self.briefings.get_mut(&session_id) {
            state.visit = BriefingVisit::Away;
            state.claim = None;
            state.candidates.clear();
            state.target = None;
            state.summary_wanted = None;
            state.summary_failed = None;
            state.summary_job = None;
            state.audio_job = None;
            state.eager_audio_used = false;
        }
        // Only automatic audio belongs to the visit — boss speech and a
        // manual replay keep their own rules.
        if self
            .voice_briefing_playback
            .is_some_and(|playback| playback.automatic)
        {
            crate::platform::stop_briefing_audio();
            self.voice_briefing_playback = None;
            self.voice_briefing_playback_generation =
                self.voice_briefing_playback_generation.wrapping_add(1);
        }
        self.briefing_queue.waiting = None;
    }

    /// The chat is gone — cancel its work and clear the opportunity.
    pub(super) fn drop_session_briefing(&mut self, session_id: Uuid) {
        self.briefings.remove(&session_id);
        self.briefing_pending
            .retain(|_, pending| pending.session_id != session_id);
        self.briefing_gate_pending
            .retain(|_, pending| pending.session_id != session_id);
    }

    /// A settled turn is a briefing candidate only while its chat is away —
    /// a completion on screen is read, never briefed. Boss chats, active
    /// goals, and ordinary sessions record alike: the settled reply stands
    /// on its own regardless of what the session does next.
    pub(super) fn note_voice_briefing_completion(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if self.briefing_is_viewed(session_id, cx.active_window().is_some()) {
            return;
        }
        let Some((message_id, turn_id, _)) = self.briefing_latest_reply(session_id) else {
            return;
        };
        {
            let state = self.briefings.entry(session_id).or_default();
            if state
                .candidates
                .iter()
                .any(|candidate| candidate.message_id == message_id)
            {
                return;
            }
            state.candidates.push(BriefingCandidate {
                message_id,
                turn_id,
                eligibility: BriefingEligibility::Unchecked,
            });
            if state.candidates.len() > BRIEFING_CANDIDATES_CAP {
                state.candidates.remove(0);
            }
        }
        if self.eager_briefing_active() {
            self.check_briefing_candidate(session_id, message_id, BriefingWork::Eager, cx);
        }
        self.advance_voice_briefing(session_id, cx);
    }

    /// Arrival: the chat's surface came into view — a selection, leaving a
    /// page, or the window reactivating. The visit resolves the captured
    /// candidates once; repeat calls inside the same visit only keep the
    /// claimed work moving.
    pub(super) fn maybe_voice_brief(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        // Sync first: the previous chat's departure is what stops its
        // audio, and it must land before this arrival can play a cached
        // clip — a later sync would pause the claim it just started.
        self.sync_voice_briefing_navigation();
        if !self.briefing_is_viewed(session_id, cx.active_window().is_some()) {
            return;
        }
        let ready = self.state.voice_briefing_enabled
            && self.state.voice_briefing_autoplay
            && self.briefing_config_ready();
        // Manual audio or a boss announcement already owns the speaker —
        // the opportunity is consumed, not deferred.
        let audio_busy =
            self.voice_briefing_playback.is_some() || !self.speech_clip_queue.is_empty();
        {
            let state = self.briefings.entry(session_id).or_default();
            match state.visit {
                BriefingVisit::Away => {
                    if !ready || audio_busy {
                        state.visit = BriefingVisit::Done;
                        return;
                    }
                    state.visit = BriefingVisit::Resolving;
                }
                BriefingVisit::Done => return,
                _ => {}
            }
        }
        self.advance_voice_briefing(session_id, cx);
    }

    /// Move one session's briefing state forward: resolve an arrival claim,
    /// keep the claimed reply's missing stages generating, or — while away
    /// and eager — retarget and coalesce speculative work. Idempotent;
    /// every state change re-enters here.
    fn advance_voice_briefing(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if !self.state.voice_briefing_enabled || !self.state.voice_briefing_autoplay {
            return;
        }
        if self.briefing_is_viewed(session_id, cx.active_window().is_some()) {
            match self.briefings.get(&session_id).map(|state| state.visit) {
                Some(BriefingVisit::Resolving) => self.resolve_briefing_arrival(session_id, cx),
                Some(BriefingVisit::Claimed) => self.ensure_briefing_claim(session_id, cx),
                _ => {}
            }
        } else {
            // A claim outliving its view means a transition was missed —
            // fold it into a departure rather than trusting the state.
            if self
                .briefings
                .get(&session_id)
                .is_some_and(|state| state.visit != BriefingVisit::Away)
            {
                self.briefing_departed(session_id, false);
            }
            self.advance_eager_briefing(session_id, cx);
        }
    }

    /// Walk the captured candidates newest-first: claim the first eligible
    /// reply, wait on a check already asked, or ask the next unchecked one.
    /// A rejection never displaces an older eligible target.
    fn resolve_briefing_arrival(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        loop {
            let scan = self
                .briefings
                .get(&session_id)
                .map(|state| scan_briefing_candidates(&state.candidates, &self.briefed_messages))
                .unwrap_or(CandidateScan::Done);
            match scan {
                CandidateScan::Claim(message_id) => {
                    let state = self.briefings.entry(session_id).or_default();
                    state.visit = BriefingVisit::Claimed;
                    state.claim = Some(message_id);
                    self.ensure_briefing_claim(session_id, cx);
                    return;
                }
                CandidateScan::Check(message_id) => {
                    self.check_briefing_candidate(
                        session_id,
                        message_id,
                        BriefingWork::Arrival,
                        cx,
                    );
                    // An async gate answer re-enters through advance; a
                    // synchronous answer just loops to the next candidate.
                    let checking = self.briefings.get(&session_id).is_some_and(|state| {
                        state
                            .candidates
                            .iter()
                            .find(|candidate| candidate.message_id == message_id)
                            .is_some_and(|candidate| {
                                candidate.eligibility == BriefingEligibility::Checking
                            })
                    });
                    if checking {
                        return;
                    }
                }
                CandidateScan::Wait => return,
                CandidateScan::Done => {
                    if let Some(state) = self.briefings.get_mut(&session_id) {
                        state.visit = BriefingVisit::Done;
                    }
                    return;
                }
            }
        }
    }

    /// The claimed reply gets its missing stages generated immediately —
    /// reusing a matching pipeline, clip, or prepared text — and plays when
    /// ready. The fixed target never retargets during the visit.
    fn ensure_briefing_claim(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(claim) = self
            .briefings
            .get(&session_id)
            .and_then(|state| state.claim)
        else {
            return;
        };
        if self.briefed_messages.contains(&claim) {
            if let Some(state) = self.briefings.get_mut(&session_id) {
                state.visit = BriefingVisit::Done;
                state.claim = None;
            }
            return;
        }
        // A pipeline already running for this reply rides to playback —
        // claiming it as arrival work keeps an eager-off flip or the
        // speaker-priority rule from cancelling it.
        if let Some(pending) = self.briefing_pending.get_mut(&claim) {
            if pending.work == BriefingWork::Eager {
                pending.work = BriefingWork::Arrival;
            }
            pending.play = true;
            return;
        }
        // Speculation for replies the claim passed over is obsolete —
        // arrival owns the pipeline now.
        self.briefing_pending.retain(|message_id, pending| {
            pending.session_id != session_id
                || pending.work == BriefingWork::Manual
                || *message_id == claim
        });
        self.briefing_gate_pending
            .retain(|message_id, pending| pending.session_id != session_id || *message_id == claim);
        if let Some(state) = self.briefings.get_mut(&session_id) {
            state.summary_wanted = None;
            state.summary_job = None;
            state.audio_job = None;
        }
        let voice_key = self.voice_briefing_voice_key();
        if let Some(clip) = self.briefing_clips.get(&claim) {
            if clip.voice == voice_key {
                let sequence = self.briefing_queue.issue();
                self.briefing_queue.accept(sequence, claim);
                self.pump_briefing_queue(cx);
                return;
            }
            // The clip outlived a voice change — revoice the prepared text,
            // or the clip's own transcript, rather than re-summarizing.
            let transcript = self
                .briefings
                .get(&session_id)
                .and_then(|state| state.summary.as_ref())
                .filter(|(message_id, _)| *message_id == claim)
                .map(|(_, text)| text.clone())
                .unwrap_or_else(|| clip.transcript.clone());
            self.start_briefing_audio(
                session_id,
                claim,
                transcript,
                BriefingWork::Arrival,
                true,
                cx,
            );
            return;
        }
        if let Some(transcript) = self
            .briefings
            .get(&session_id)
            .and_then(|state| state.summary.as_ref())
            .filter(|(message_id, _)| *message_id == claim)
            .map(|(_, text)| text.clone())
        {
            self.start_briefing_audio(
                session_id,
                claim,
                transcript,
                BriefingWork::Arrival,
                true,
                cx,
            );
            return;
        }
        let Some((_, response)) = self.briefing_reply_source(session_id, claim) else {
            // The reply left the transcript — nothing left to claim.
            if let Some(state) = self.briefings.get_mut(&session_id) {
                state.visit = BriefingVisit::Done;
                state.claim = None;
            }
            return;
        };
        self.start_briefing_full(session_id, claim, response, BriefingWork::Arrival, true, cx);
    }

    /// Away-side speculation: the latest eligible completion becomes the
    /// target, its summary text refreshes through one coalesced job, and
    /// the absence's single eager audio attempt voices it once ready.
    fn advance_eager_briefing(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if !self.eager_briefing_active() {
            return;
        }
        let live_jobs: HashSet<u64> = self
            .briefing_pending
            .values()
            .map(|pending| pending.generation)
            .collect();
        let target = {
            let Some(state) = self.briefings.get_mut(&session_id) else {
                return;
            };
            if state
                .summary_job
                .is_some_and(|generation| !live_jobs.contains(&generation))
            {
                state.summary_job = None;
            }
            if state
                .audio_job
                .is_some_and(|generation| !live_jobs.contains(&generation))
            {
                state.audio_job = None;
            }
            let target = state
                .candidates
                .iter()
                .rev()
                .find(|candidate| {
                    candidate.eligibility == BriefingEligibility::Eligible
                        && !self.briefed_messages.contains(&candidate.message_id)
                })
                .map(|candidate| candidate.message_id);
            state.target = target;
            if let Some(target) = target
                && state.summary.as_ref().map(|(id, _)| *id) != Some(target)
                && state.summary_failed != Some(target)
            {
                state.summary_wanted = Some(target);
            }
            target
        };
        if target.is_some() {
            self.drive_briefing_summary(session_id, cx);
        }
        let Some(target) = target else {
            return;
        };
        let audio_due = self.briefings.get(&session_id).is_some_and(|state| {
            !state.eager_audio_used
                && state.audio_job.is_none()
                && state.summary.as_ref().is_some_and(|(id, _)| *id == target)
        }) && !self.briefing_pending.contains_key(&target)
            && !self
                .briefing_clips
                .get(&target)
                .is_some_and(|clip| clip.voice == self.voice_briefing_voice_key());
        if !audio_due {
            return;
        }
        let Some(transcript) = self
            .briefings
            .get(&session_id)
            .and_then(|state| state.summary.as_ref())
            .filter(|(id, _)| *id == target)
            .map(|(_, text)| text.clone())
        else {
            return;
        };
        // The attempt is spent at start — a failure does not free it.
        if let Some(state) = self.briefings.get_mut(&session_id) {
            state.eager_audio_used = true;
        }
        self.start_briefing_audio(
            session_id,
            target,
            transcript,
            BriefingWork::Eager,
            false,
            cx,
        );
    }

    /// One automatic summary job per session — `summary_wanted` holds the
    /// newest outstanding reply so a settled job starts its replacement,
    /// never a backlog.
    fn drive_briefing_summary(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let live_jobs: HashSet<u64> = self
            .briefing_pending
            .values()
            .map(|pending| pending.generation)
            .collect();
        let Some(state) = self.briefings.get_mut(&session_id) else {
            return;
        };
        if state
            .summary_job
            .is_some_and(|generation| live_jobs.contains(&generation))
        {
            return;
        }
        state.summary_job = None;
        let Some(wanted) = state.summary_wanted.take() else {
            return;
        };
        // A pipeline for the same reply — automatic or manual — covers the
        // text already; its finish stores it.
        if self.briefing_pending.contains_key(&wanted) {
            return;
        }
        let Some((_, response)) = self.briefing_reply_source(session_id, wanted) else {
            return;
        };
        self.start_briefing_summary(session_id, wanted, response, BriefingWork::Eager, false, cx);
    }

    /// Resolve one candidate's eligibility: the local length floor first,
    /// then the optional Jev gate. Anything that keeps the gate from
    /// answering — unconfigured eval, a blind spot, a failed request —
    /// fails open and the candidate stays eligible.
    fn check_briefing_candidate(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        work: BriefingWork,
        cx: &mut Context<Self>,
    ) {
        let passes_floor = self
            .briefing_reply_source(session_id, message_id)
            .is_some_and(|(_, content)| content.chars().count() >= MIN_RESPONSE_CHARS);
        if !passes_floor {
            self.set_briefing_eligibility(session_id, message_id, BriefingEligibility::Ineligible);
            return;
        }
        let turn_id = self
            .briefings
            .get(&session_id)
            .and_then(|state| {
                state
                    .candidates
                    .iter()
                    .find(|candidate| candidate.message_id == message_id)
            })
            .and_then(|candidate| candidate.turn_id);
        if !(self.state.voice_briefing_gate_enabled
            && self.briefing_gate_pending.len() < BRIEFING_PENDING_CAP)
        {
            self.set_briefing_eligibility(session_id, message_id, BriefingEligibility::Eligible);
            return;
        }
        let Some((daemon, state)) = self.voice_briefing_gate_request(session_id, turn_id) else {
            self.set_briefing_eligibility(session_id, message_id, BriefingEligibility::Eligible);
            return;
        };
        self.set_briefing_eligibility(session_id, message_id, BriefingEligibility::Checking);
        let sequence = self.briefing_queue.issue();
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
                session_id,
                play: false,
                work,
                stage: BriefingStage::Gate,
            },
        );
        cx.notify();
        let eval = cx.background_executor().spawn(async move {
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
            let approved = eval.await;
            let _ = this.update(cx, |this, cx| {
                // A cancel that landed mid-eval drops the entry — the
                // answer, whatever it was, goes nowhere.
                if PendingBriefing::take_current(
                    &mut this.briefing_gate_pending,
                    message_id,
                    sequence,
                )
                .is_none()
                {
                    return;
                }
                this.set_briefing_eligibility(
                    session_id,
                    message_id,
                    if approved {
                        BriefingEligibility::Eligible
                    } else {
                        BriefingEligibility::Ineligible
                    },
                );
                this.advance_voice_briefing(session_id, cx);
            });
        })
        .detach();
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

    /// The session's latest settled reply — the completion a settle event
    /// just produced.
    fn briefing_latest_reply(&self, session_id: Uuid) -> Option<(Uuid, Option<Uuid>, String)> {
        let message = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::Assistant && !message.streaming)?;
        Some((
            message.id,
            message.turn_id,
            tail_chars(message.visible_content(), RESPONSE_INPUT_CHARS),
        ))
    }

    /// One reply's source text, fetched fresh — edits and rewinds after the
    /// settle invalidate the candidate here rather than at record time.
    fn briefing_reply_source(
        &self,
        session_id: Uuid,
        message_id: Uuid,
    ) -> Option<(Option<Uuid>, String)> {
        let message = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?
            .messages
            .iter()
            .find(|message| message.id == message_id)?;
        if message.role != MessageRole::Assistant || message.streaming {
            return None;
        }
        let content = message.visible_content();
        (!content.trim().is_empty())
            .then(|| (message.turn_id, tail_chars(content, RESPONSE_INPUT_CHARS)))
    }

    /// Stamp a reply as played. The set is a bound, not a history — past
    /// the cap it clears and an old revisit can brief again.
    fn mark_briefed(&mut self, message_id: Uuid) {
        if self.briefed_messages.len() >= BRIEFED_MESSAGES_CAP {
            self.briefed_messages.clear();
        }
        self.briefed_messages.insert(message_id);
    }

    /// The visit's claim ends by user action — stop, skip, or a manual
    /// takeover all consume it rather than pausing it.
    fn consume_briefing_claim(&mut self, message_id: Uuid) {
        if let Some(state) = self
            .briefings
            .values_mut()
            .find(|state| state.claim == Some(message_id))
        {
            state.visit = BriefingVisit::Done;
            state.claim = None;
        }
    }

    /// What would voice a briefing rendered now: `piper:<voice>` for the
    /// local engine, `provider:model` for a gateway voice. A cached clip
    /// whose key differs was rendered under an older setting.
    pub(super) fn voice_briefing_voice_key(&self) -> String {
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
        gateway_voice_key(provider, model_id, self.voice_briefing_gateway_voice())
    }

    pub(super) fn voice_briefing_gateway_voice(&self) -> &str {
        let model = self.state.voice_briefing_tts_model;
        let model_id = if model.is_custom() {
            self.state.voice_briefing_tts_custom_model.trim()
        } else {
            model
                .model_id_for(self.state.voice_briefing_provider)
                .unwrap_or_default()
        };
        let configured = self
            .state
            .voice_briefing_tts_voices
            .get(model.model_id().unwrap_or("custom"))
            .map(String::as_str)
            .unwrap_or_default();
        effective_speech_voice(model_id, configured)
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
        let session_id = self
            .state
            .sessions
            .iter()
            .find(|session| session.messages.iter().any(|m| m.id == message_id))
            .map(|session| session.id);
        // An explicit request takes over the visit: the automatic claim is
        // consumed and its remaining speculation for the session stops.
        if let Some(session_id) = session_id
            && self.briefings.get(&session_id).is_some_and(|state| {
                matches!(
                    state.visit,
                    BriefingVisit::Resolving | BriefingVisit::Claimed
                )
            })
        {
            if let Some(state) = self.briefings.get_mut(&session_id) {
                state.visit = BriefingVisit::Done;
                state.claim = None;
            }
            self.briefing_queue.waiting = None;
            self.briefing_pending.retain(|id, pending| {
                pending.session_id != session_id
                    || pending.work == BriefingWork::Manual
                    || *id == message_id
            });
            self.briefing_gate_pending
                .retain(|id, pending| pending.session_id != session_id || *id == message_id);
        }
        self.briefing_gate_pending.remove(&message_id);
        if let Some(pending) = self.briefing_pending.get_mut(&message_id) {
            pending.play = true;
            pending.work = BriefingWork::Manual;
            return;
        }
        if self.briefing_clips.contains_key(&message_id) {
            if !self.play_voice_briefing_clip(message_id, false, cx) {
                self.show_toast(tr!("errors.voice_briefing_playback"));
            }
            return;
        }
        // Prepared text for the same reply voices directly — no duplicate
        // summary call for work the automatic path already did.
        if let Some((session_id, transcript)) = session_id.and_then(|session_id| {
            self.briefings
                .get(&session_id)
                .and_then(|state| state.summary.as_ref())
                .filter(|(id, _)| *id == message_id)
                .map(|(_, text)| (session_id, text.clone()))
        }) {
            self.start_briefing_audio(
                session_id,
                message_id,
                transcript,
                BriefingWork::Manual,
                true,
                cx,
            );
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
        self.start_briefing_full(
            session_id.unwrap_or_default(),
            message_id,
            response,
            BriefingWork::Manual,
            true,
            cx,
        );
    }

    /// Drop a briefing in flight — eval, summary, or audio — and treat the
    /// reply as heard so the automatic path does not re-arm it. Cancelling
    /// the claimed reply consumes the visit's claim.
    pub(super) fn cancel_voice_briefing(&mut self, message_id: Uuid, cx: &mut Context<Self>) {
        let removed = self.briefing_pending.remove(&message_id).is_some()
            | self.briefing_gate_pending.remove(&message_id).is_some();
        for state in self.briefings.values_mut() {
            if state.claim == Some(message_id) && state.visit == BriefingVisit::Claimed {
                state.visit = BriefingVisit::Done;
                state.claim = None;
            }
        }
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

    /// Autoplay or the feature switching off ends every visit's claim and
    /// cancels all automatic work — pending, evaluating, waiting, or
    /// playing. Manual playback stays usable.
    pub(super) fn cancel_automatic_voice_briefings(&mut self) {
        self.briefing_pending
            .retain(|_, pending| pending.work == BriefingWork::Manual);
        self.briefing_gate_pending.clear();
        for state in self.briefings.values_mut() {
            if state.visit != BriefingVisit::Away {
                state.visit = BriefingVisit::Done;
                state.claim = None;
            }
            state.summary_wanted = None;
            state.summary_job = None;
            state.audio_job = None;
        }
        if self
            .voice_briefing_playback
            .is_some_and(|playback| playback.automatic)
        {
            crate::platform::stop_briefing_audio();
            self.voice_briefing_playback = None;
            self.voice_briefing_playback_generation =
                self.voice_briefing_playback_generation.wrapping_add(1);
        }
        self.briefing_queue.waiting = None;
    }

    /// The eager switch off drops only speculation — claimed arrival work
    /// and manual requests continue, and completed caches stay usable.
    pub(super) fn cancel_eager_voice_briefings(&mut self) {
        self.briefing_pending
            .retain(|_, pending| pending.work != BriefingWork::Eager);
        self.briefing_gate_pending
            .retain(|_, pending| pending.work != BriefingWork::Eager);
        let live_jobs: HashSet<u64> = self
            .briefing_pending
            .values()
            .map(|pending| pending.generation)
            .collect();
        for state in self.briefings.values_mut() {
            state.summary_wanted = None;
            if state
                .summary_job
                .is_some_and(|generation| !live_jobs.contains(&generation))
            {
                state.summary_job = None;
            }
            if state
                .audio_job
                .is_some_and(|generation| !live_jobs.contains(&generation))
            {
                state.audio_job = None;
            }
        }
    }

    /// Voice or speech-model edits strand in-flight automatic audio under
    /// the old voice — cancel those jobs; prepared text stays valid, and a
    /// matching summary voices through the new settings on the next
    /// arrival instead of looping here.
    pub(super) fn retire_stale_briefing_voices(&mut self) {
        self.briefing_pending.retain(|_, pending| {
            pending.work == BriefingWork::Manual
                || !matches!(pending.stage, BriefingStage::Audio | BriefingStage::Full)
        });
        let live_jobs: HashSet<u64> = self
            .briefing_pending
            .values()
            .map(|pending| pending.generation)
            .collect();
        for state in self.briefings.values_mut() {
            if state
                .audio_job
                .is_some_and(|generation| !live_jobs.contains(&generation))
            {
                state.audio_job = None;
            }
        }
    }

    pub(super) fn voice_briefing_playback_status(&self) -> Option<super::VoiceBriefingPlayback> {
        self.voice_briefing_playback
    }

    pub(super) fn toggle_voice_briefing_playback(&mut self, cx: &mut Context<Self>) {
        let Some(playback) = self.voice_briefing_playback else {
            return;
        };
        // Pausing automatic audio is a manual takeover — the visit's claim
        // does not resume on its own.
        if playback.automatic
            && let Some(message_id) = playback.message_id
        {
            self.consume_briefing_claim(message_id);
        }
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
            automatic: false, // Explicit pause/resume is a manual override of DND.
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
        if playback.is_none() && !can_skip {
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
                                this.track_voice_briefing_playback(
                                    remaining, message_id, false, cx,
                                );
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
        // DND can leave only a waiting clip. Skip explicitly dismisses it;
        // the quiet-hours gate itself never consumes that slot.
        if self.voice_briefing_playback.is_none()
            && self.voice_briefing_dnd_active()
            && let Some(message_id) = self.briefing_queue.waiting.take()
        {
            self.mark_briefed(message_id);
            self.consume_briefing_claim(message_id);
        }
        if let Some(message_id) = self
            .voice_briefing_playback
            .and_then(|playback| playback.message_id)
        {
            self.consume_briefing_claim(message_id);
        }
        crate::platform::stop_briefing_audio();
        self.voice_briefing_playback = None;
        self.voice_briefing_playback_generation =
            self.voice_briefing_playback_generation.wrapping_add(1);
        self.pump_briefing_queue(cx);
        self.pump_speech_queue(cx);
        cx.notify();
    }

    fn voice_briefing_dnd_active(&self) -> bool {
        let now = chrono::Local::now();
        self.state.voice_briefing_autoplay
            && self
                .state
                .voice_briefing_sleep_window
                .is_some_and(|window| window.contains((now.hour() * 60 + now.minute()) as u16))
    }

    /// Settings edits and minute-boundary wakes re-evaluate the same waiting
    /// slot. Automatic playback pauses during DND; explicit playback bypasses it.
    pub(super) fn refresh_voice_briefing_dnd(&mut self, cx: &mut Context<Self>) {
        let quiet = self.voice_briefing_dnd_active();
        if let Some(playback) = self.voice_briefing_playback.filter(|p| p.automatic) {
            if quiet && playback.playing {
                if let Some(remaining) = crate::platform::pause_briefing_audio() {
                    self.voice_briefing_playback = Some(super::VoiceBriefingPlayback {
                        playing: false,
                        remaining,
                        ..playback
                    });
                    self.voice_briefing_playback_generation =
                        self.voice_briefing_playback_generation.wrapping_add(1);
                    cx.notify();
                } else {
                    self.voice_briefing_playback = None;
                    self.voice_briefing_playback_generation =
                        self.voice_briefing_playback_generation.wrapping_add(1);
                    cx.notify();
                }
            } else if !quiet
                && !playback.playing
                && self.state.voice_briefing_autoplay
                && self.state.voice_briefing_enabled
            {
                if let Some(remaining) = crate::platform::resume_briefing_audio() {
                    self.track_voice_briefing_playback(remaining, playback.message_id, true, cx);
                } else {
                    self.voice_briefing_playback = None;
                    self.voice_briefing_playback_generation =
                        self.voice_briefing_playback_generation.wrapping_add(1);
                    cx.notify();
                }
            }
        }
        if quiet
            && (self.briefing_queue.waiting.is_some()
                || self.voice_briefing_playback.is_some_and(|p| p.automatic))
        {
            self.schedule_voice_briefing_dnd_wake(cx);
        } else {
            self.pump_briefing_queue(cx);
        }
    }

    /// Only one sleeper per app. Checking at local minute boundaries also
    /// handles wall-clock/time-zone changes and sleep/wake without a stale deadline.
    fn schedule_voice_briefing_dnd_wake(&mut self, cx: &mut Context<Self>) {
        if self.briefing_dnd_wake_pending
            || !self.state.voice_briefing_autoplay
            || !self.state.voice_briefing_enabled
        {
            return;
        }
        self.briefing_dnd_wake_pending = true;
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            loop {
                let seconds = 60 - chrono::Local::now().second();
                cx.background_executor()
                    .timer(Duration::from_secs(u64::from(seconds)))
                    .await;
                let keep_waiting = weak
                    .update(cx, |this, cx| {
                        this.refresh_voice_briefing_dnd(cx);
                        let keep_waiting = this.state.voice_briefing_autoplay
                            && this.state.voice_briefing_enabled
                            && this.voice_briefing_dnd_active()
                            && (this.briefing_queue.waiting.is_some()
                                || this.voice_briefing_playback.is_some_and(|p| p.automatic));
                        if !keep_waiting {
                            this.briefing_dnd_wake_pending = false;
                        }
                        keep_waiting
                    })
                    .unwrap_or(false);
                if !keep_waiting {
                    break;
                }
            }
        })
        .detach();
    }

    fn pump_briefing_queue(&mut self, cx: &mut Context<Self>) {
        if self.voice_briefing_playback.is_some() || self.viewed_briefing_session().is_none() {
            return;
        }
        let allowed = self.state.voice_briefing_enabled
            && self.state.voice_briefing_autoplay
            && cx.active_window().is_some()
            && !self.voice_briefing_dnd_active();
        if cx.active_window().is_none() {
            // Automatic clips never wait out a backgrounded window; the
            // cached audio stays reachable through the footer replay.
            self.briefing_queue.waiting = None;
        }
        if !allowed && self.briefing_queue.waiting.is_some() && self.voice_briefing_dnd_active() {
            self.schedule_voice_briefing_dnd_wake(cx);
        }
        if let Some(message_id) = self.briefing_queue.take_ready(allowed)
            && !self.play_voice_briefing_clip(message_id, true, cx)
        {
            self.show_toast(tr!("errors.voice_briefing_playback"));
        }
    }

    /// Leave a short pause only when another automatic briefing is ready.
    /// The playback generation makes a stale timer harmless if the user
    /// starts or stops another clip during the gap.
    fn advance_voice_briefing_queues(&mut self, cx: &mut Context<Self>) {
        let allowed = self.viewed_briefing_session().is_some()
            && self.state.voice_briefing_enabled
            && self.state.voice_briefing_autoplay
            && cx.active_window().is_some()
            && !self.voice_briefing_dnd_active();
        if !self.briefing_queue.has_ready(allowed) {
            self.pump_briefing_queue(cx);
            self.pump_speech_queue(cx);
            return;
        }

        let generation = self.voice_briefing_playback_generation;
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(BRIEFING_QUEUE_GAP).await;
            let _ = weak.update(cx, |this, cx| {
                if this.voice_briefing_playback_generation != generation
                    || this.voice_briefing_playback.is_some()
                {
                    return;
                }
                this.pump_briefing_queue(cx);
                this.pump_speech_queue(cx);
            });
        })
        .detach();
    }

    fn play_voice_briefing_clip(
        &mut self,
        message_id: Uuid,
        automatic: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(duration) = self.briefing_clips.get(&message_id).and_then(|clip| {
            crate::platform::play_briefing_audio(&clip.audio, self.state.voice_briefing_volume)
        }) else {
            return false;
        };
        self.speech_playback_key = None;
        if self.briefing_queue.waiting == Some(message_id) {
            self.briefing_queue.waiting = None;
        }
        self.mark_briefed(message_id);
        self.track_voice_briefing_playback(duration, Some(message_id), automatic, cx);
        true
    }

    pub(super) fn track_voice_briefing_playback(
        &mut self,
        remaining: Duration,
        message_id: Option<Uuid>,
        automatic: bool,
        cx: &mut Context<Self>,
    ) {
        self.voice_briefing_playback_generation =
            self.voice_briefing_playback_generation.wrapping_add(1);
        self.voice_briefing_playback = Some(super::VoiceBriefingPlayback {
            playing: true,
            automatic,
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
                    // Automatic audio belongs to the foreground — a
                    // backgrounded window stops it outright; the departure
                    // has already consumed the claim.
                    if this.voice_briefing_playback.is_some_and(|p| p.automatic)
                        && cx.active_window().is_none()
                    {
                        if let Some(message_id) = this
                            .voice_briefing_playback
                            .and_then(|playback| playback.message_id)
                        {
                            this.consume_briefing_claim(message_id);
                        }
                        crate::platform::stop_briefing_audio();
                        this.voice_briefing_playback = None;
                        this.voice_briefing_playback_generation =
                            this.voice_briefing_playback_generation.wrapping_add(1);
                        this.briefing_queue.waiting = None;
                        cx.notify();
                        return false;
                    }
                    if this.voice_briefing_playback.is_some_and(|p| p.automatic)
                        && this.voice_briefing_dnd_active()
                    {
                        this.refresh_voice_briefing_dnd(cx);
                        return false;
                    }
                    let Some((playing, remaining)) = crate::platform::briefing_audio_status()
                    else {
                        crate::platform::stop_briefing_audio();
                        if let Some(playback) = this.voice_briefing_playback
                            && let Some(message_id) = playback.message_id
                        {
                            this.consume_briefing_claim(message_id);
                        }
                        this.voice_briefing_playback = None;
                        this.voice_briefing_playback_generation =
                            this.voice_briefing_playback_generation.wrapping_add(1);
                        // A queued `speak` chain hands off here — pumping
                        // starts the next clip and its own tick.
                        this.advance_voice_briefing_queues(cx);
                        cx.notify();
                        return false;
                    };
                    if remaining.is_zero() {
                        crate::platform::stop_briefing_audio();
                        if let Some(playback) = this.voice_briefing_playback
                            && let Some(message_id) = playback.message_id
                        {
                            this.consume_briefing_claim(message_id);
                        }
                        this.voice_briefing_playback = None;
                        this.voice_briefing_playback_generation =
                            this.voice_briefing_playback_generation.wrapping_add(1);
                        this.advance_voice_briefing_queues(cx);
                        cx.notify();
                        return false;
                    }
                    let message_id = this
                        .voice_briefing_playback
                        .and_then(|playback| playback.message_id);
                    this.voice_briefing_playback = Some(super::VoiceBriefingPlayback {
                        playing,
                        automatic: this.voice_briefing_playback.is_some_and(|p| p.automatic),
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

    /// Run the summarize pipeline's text half for one reply — the eager
    /// target's refresh or a manual request's first stage. The result
    /// lands in the session's prepared slot; audio follows only when
    /// something asks for it.
    fn start_briefing_summary(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        response: String,
        work: BriefingWork,
        play: bool,
        cx: &mut Context<Self>,
    ) {
        if work != BriefingWork::Manual && self.briefing_pending.len() >= BRIEFING_PENDING_CAP {
            return;
        }
        let request_id = self.briefing_queue.issue();
        self.briefing_pending.insert(
            message_id,
            PendingBriefing {
                generation: request_id,
                session_id,
                play,
                work,
                stage: BriefingStage::Summary,
            },
        );
        if work != BriefingWork::Manual
            && let Some(state) = self.briefings.get_mut(&session_id)
        {
            state.summary_job = Some(request_id);
        }
        cx.notify();
        let provider = self.state.voice_briefing_provider;
        let summary_model = self.state.voice_briefing_summary_model.trim().to_owned();
        let instructions = effective_voice_briefing_summary_instructions(
            &self.state.voice_briefing_summary_instructions,
            self.state.voice_briefing_summary_instructions_full_prompt,
        );
        let http = cx.http_client();
        let daemon = self.daemon.client();
        let executor = cx.background_executor().clone();
        let summary = executor.spawn({
            let executor = executor.clone();
            async move {
                let key = inference_credential(&daemon, provider)?;
                summarize(
                    &http,
                    &executor,
                    provider,
                    &key,
                    &summary_model,
                    &instructions,
                    &response,
                )
                .await
                .context("summary generation")
            }
        });
        cx.spawn(async move |this, cx| {
            let result = summary.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_briefing_summary(session_id, message_id, request_id, result, cx);
            });
        })
        .detach();
    }

    /// A summary job's result: prepared text for the session's latest slot.
    /// Manual takeovers chain straight into audio; everything else re-enters
    /// the driver so a claimed reply voices and a settled job frees the
    /// slot for the newest request.
    fn finish_briefing_summary(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        request_id: u64,
        result: anyhow::Result<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) =
            PendingBriefing::take_current(&mut self.briefing_pending, message_id, request_id)
        else {
            return;
        };
        match result {
            Ok(transcript) => {
                if let Some(state) = self.briefings.get_mut(&session_id) {
                    state.summary = Some((message_id, transcript.clone()));
                    if state
                        .summary_job
                        .is_some_and(|generation| generation == request_id)
                    {
                        state.summary_job = None;
                    }
                }
                if pending.work == BriefingWork::Manual && pending.play {
                    self.start_briefing_audio(
                        session_id,
                        message_id,
                        transcript,
                        BriefingWork::Manual,
                        true,
                        cx,
                    );
                }
            }
            // Backend error bodies can echo the prompt — the toast stays
            // generic and the detail only hits stderr.
            Err(error) => {
                eprintln!("Goddard: voice briefing failed: {error:#}");
                if pending.play {
                    self.show_toast(tr!("errors.voice_briefing"));
                }
                match pending.work {
                    // The claim consumes on failure — nothing retries it
                    // inside the visit; a click is the retry.
                    BriefingWork::Arrival => self.consume_briefing_claim(message_id),
                    // Speculation marks the reply failed so the coalesced
                    // driver does not retry it into a storm. A later
                    // eligible completion retargets anyway.
                    BriefingWork::Eager => {
                        if let Some(state) = self.briefings.get_mut(&session_id) {
                            state.summary_failed = Some(message_id);
                        }
                    }
                    BriefingWork::Manual => {}
                }
            }
        }
        self.advance_voice_briefing(session_id, cx);
        cx.notify();
    }

    /// Voice a prepared transcript — the absence's one eager attempt, the
    /// claimed reply's missing stage, or a manual request reusing text the
    /// automatic path already wrote.
    fn start_briefing_audio(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        transcript: String,
        work: BriefingWork,
        play: bool,
        cx: &mut Context<Self>,
    ) {
        if work != BriefingWork::Manual && self.briefing_pending.len() >= BRIEFING_PENDING_CAP {
            return;
        }
        let request_id = self.briefing_queue.issue();
        self.briefing_pending.insert(
            message_id,
            PendingBriefing {
                generation: request_id,
                session_id,
                play,
                work,
                stage: BriefingStage::Audio,
            },
        );
        if work != BriefingWork::Manual
            && let Some(state) = self.briefings.get_mut(&session_id)
        {
            state.audio_job = Some(request_id);
        }
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
        let gateway_voice = self.voice_briefing_gateway_voice().to_owned();
        let piper_speaker = self.state.voice_briefing_piper_speaker;
        let piper_voice = piper_voice_or_default(&self.state.voice_briefing_piper_voice).to_owned();
        let http = cx.http_client();
        let daemon = self.daemon.client();
        let executor = cx.background_executor().clone();
        let audio = executor.spawn({
            let executor = executor.clone();
            async move {
                // Piper voices entirely offline — only a gateway engine
                // needs the provider credential.
                let audio = if tts_model.is_piper() {
                    synthesize_piper(&http, &executor, &piper_voice, piper_speaker, &transcript)
                        .await
                        .context("speech generation")?
                } else {
                    let key = inference_credential(&daemon, provider)?;
                    synthesize(
                        &http,
                        &executor,
                        provider,
                        &key,
                        &tts_model_id,
                        &gateway_voice,
                        &transcript,
                    )
                    .await
                    .context("speech generation")?
                };
                anyhow::Ok((transcript, audio))
            }
        });
        cx.spawn(async move |this, cx| {
            let result = audio.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_briefing_clip(message_id, request_id, voice_key, result, cx);
            });
        })
        .detach();
    }

    /// Summarize and voice one reply in a single pipeline — the claimed
    /// arrival with no prepared stages, or a manual request with nothing
    /// reusable.
    fn start_briefing_full(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        response: String,
        work: BriefingWork,
        play: bool,
        cx: &mut Context<Self>,
    ) {
        if work != BriefingWork::Manual && self.briefing_pending.len() >= BRIEFING_PENDING_CAP {
            return;
        }
        let request_id = self.briefing_queue.issue();
        self.briefing_pending.insert(
            message_id,
            PendingBriefing {
                generation: request_id,
                session_id,
                play,
                work,
                stage: BriefingStage::Full,
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
        let gateway_voice = self.voice_briefing_gateway_voice().to_owned();
        let piper_speaker = self.state.voice_briefing_piper_speaker;
        let piper_voice = piper_voice_or_default(&self.state.voice_briefing_piper_voice).to_owned();
        let http = cx.http_client();
        let daemon = self.daemon.client();
        let executor = cx.background_executor().clone();
        let work_fut = executor.spawn({
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
                    synthesize(
                        &http,
                        &executor,
                        provider,
                        &key,
                        &tts_model_id,
                        &gateway_voice,
                        &transcript,
                    )
                    .await
                    .context("speech generation")?
                };
                anyhow::Ok((transcript, audio))
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work_fut.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_briefing_clip(message_id, request_id, voice_key, result, cx);
            });
        })
        .detach();
    }

    /// Shared landing for the audio and full pipelines: cache the clip
    /// under the voice that rendered it, evicting the oldest past the cap.
    /// A claimed reply plays on landing; anything else — a stale eager
    /// target or an already spent visit — just fills the cache.
    fn finish_briefing_clip(
        &mut self,
        message_id: Uuid,
        request_id: u64,
        voice_key: String,
        result: anyhow::Result<(String, Vec<u8>)>,
        cx: &mut Context<Self>,
    ) {
        // A cancel that landed mid-pipeline already dropped the entry —
        // discard the clip rather than caching it.
        let Some(pending) =
            PendingBriefing::take_current(&mut self.briefing_pending, message_id, request_id)
        else {
            return;
        };
        let session_id = pending.session_id;
        match result {
            Ok((transcript, bytes)) => {
                if let Some(state) = self.briefings.get_mut(&session_id) {
                    state.summary = Some((message_id, transcript.clone()));
                    if state
                        .audio_job
                        .is_some_and(|generation| generation == request_id)
                    {
                        state.audio_job = None;
                    }
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
                let claimed = self.briefings.get(&session_id).is_some_and(|state| {
                    state.visit == BriefingVisit::Claimed && state.claim == Some(message_id)
                });
                if pending.work == BriefingWork::Manual && pending.play {
                    // AVAudioPlayer must start on the UI thread, so the
                    // bytes ride the spawn back rather than playing from
                    // the executor.
                    if !self.play_voice_briefing_clip(message_id, false, cx) {
                        self.show_toast(tr!("errors.voice_briefing_playback"));
                    }
                } else if pending.work != BriefingWork::Manual
                    && claimed
                    && self.briefing_is_viewed(session_id, cx.active_window().is_some())
                    && !self.briefed_messages.contains(&message_id)
                {
                    let sequence = self.briefing_queue.issue();
                    if self.briefing_queue.accept(sequence, message_id) {
                        self.pump_briefing_queue(cx);
                    }
                }
            }
            // Backend error bodies can echo the prompt — the toast stays
            // generic and the detail only hits stderr.
            Err(error) => {
                eprintln!("Goddard: voice briefing failed: {error:#}");
                if pending.play {
                    self.show_toast(tr!("errors.voice_briefing"));
                }
                if pending.work == BriefingWork::Arrival {
                    // A failed claim does not retry inside the visit —
                    // a footer click is the retry.
                    self.consume_briefing_claim(message_id);
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
    voice: &str,
    text: &str,
) -> anyhow::Result<Vec<u8>> {
    match provider {
        InferenceProvider::VercelGateway => {
            let output_format = speech_parameters(model_id).1;
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
                "voice": voice,
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

fn gateway_voice_key(provider: InferenceProvider, model_id: &str, voice: &str) -> String {
    format!("{}:{model_id}:{voice}", provider.id())
}

fn effective_speech_voice<'a>(model_id: &str, configured: &'a str) -> &'a str {
    let configured = configured.trim();
    if configured.is_empty() {
        speech_parameters(model_id).0
    } else {
        configured
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
mod voice_tests {
    use super::*;

    #[test]
    fn selected_voice_changes_clip_identity_and_blank_restores_defaults() {
        let provider = InferenceProvider::VercelGateway;
        for model in [
            "google/gemini-3.8-flash-tts",
            "openai/tts-1",
            "spacexai/grok-tts",
            "fish-audio/s1",
            "acme/custom-tts",
        ] {
            let default = effective_speech_voice(model, "  ");
            assert_eq!(default, speech_parameters(model).0);
            let selected = effective_speech_voice(model, " narrator ");
            assert_eq!(selected, "narrator");
            assert_ne!(
                gateway_voice_key(provider, model, selected),
                gateway_voice_key(provider, model, default)
            );
            assert_eq!(
                gateway_voice_key(provider, model, default),
                gateway_voice_key(provider, model, effective_speech_voice(model, ""))
            );
        }
    }
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

    fn candidate(message_id: Uuid, eligibility: BriefingEligibility) -> BriefingCandidate {
        BriefingCandidate {
            message_id,
            turn_id: None,
            eligibility,
        }
    }

    #[test]
    fn stale_result_preserves_the_replacement_request_for_manual_replay() {
        let id = Uuid::new_v4();
        let session = Uuid::new_v4();
        let mut pending = HashMap::new();
        pending.insert(
            id,
            PendingBriefing {
                generation: 1,
                session_id: session,
                play: true,
                work: BriefingWork::Eager,
                stage: BriefingStage::Full,
            },
        );
        pending.clear(); // leaving the chat cancels the old generation
        pending.insert(
            id,
            PendingBriefing {
                generation: 2,
                session_id: session,
                play: true,
                work: BriefingWork::Manual,
                stage: BriefingStage::Full,
            },
        );
        assert!(PendingBriefing::take_current(&mut pending, id, 1).is_none());
        let replacement =
            PendingBriefing::take_current(&mut pending, id, 2).expect("replacement preserved");
        assert!(replacement.work == BriefingWork::Manual && replacement.play);
        assert!(pending.is_empty());
    }

    #[test]
    fn arrival_scans_newest_first_and_rejections_never_displace() {
        let briefed = HashSet::new();
        let oldest = Uuid::new_v4();
        let middle = Uuid::new_v4();
        let newest = Uuid::new_v4();
        // The newest eligible candidate wins even with a rejection above it.
        let candidates = vec![
            candidate(oldest, BriefingEligibility::Eligible),
            candidate(middle, BriefingEligibility::Eligible),
            candidate(newest, BriefingEligibility::Ineligible),
        ];
        assert_eq!(
            scan_briefing_candidates(&candidates, &briefed),
            CandidateScan::Claim(middle)
        );
        // A pending check on a newer reply holds the claim for its answer.
        let candidates = vec![
            candidate(oldest, BriefingEligibility::Eligible),
            candidate(newest, BriefingEligibility::Checking),
        ];
        assert_eq!(
            scan_briefing_candidates(&candidates, &briefed),
            CandidateScan::Wait
        );
        // Unchecked candidates get evaluated newest-first.
        let candidates = vec![
            candidate(oldest, BriefingEligibility::Unchecked),
            candidate(newest, BriefingEligibility::Unchecked),
        ];
        assert_eq!(
            scan_briefing_candidates(&candidates, &briefed),
            CandidateScan::Check(newest)
        );
        // A reply already heard is not a candidate again.
        let mut briefed = HashSet::new();
        briefed.insert(newest);
        let candidates = vec![
            candidate(oldest, BriefingEligibility::Eligible),
            candidate(newest, BriefingEligibility::Eligible),
        ];
        assert_eq!(
            scan_briefing_candidates(&candidates, &briefed),
            CandidateScan::Claim(oldest)
        );
    }

    #[test]
    fn dnd_holds_the_latest_automatic_clip_until_playback_is_allowed() {
        let mut queue = BriefingQueue::default();
        let first = queue.issue();
        assert!(queue.accept(first, Uuid::new_v4()));
        assert!(queue.take_ready(false).is_none());
        assert!(queue.waiting.is_some());
        let newest = queue.issue();
        let id = Uuid::new_v4();
        assert!(queue.accept(newest, id));
        assert!(queue.take_ready(false).is_none());
        assert_eq!(queue.take_ready(true), Some(id));
        assert!(queue.waiting.is_none());
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

    #[test]
    fn queue_gap_is_only_needed_for_an_eligible_waiting_briefing() {
        let mut queue = BriefingQueue::default();
        assert!(!queue.has_ready(true));
        queue.waiting = Some(Uuid::new_v4());
        assert!(!queue.has_ready(false));
        assert!(queue.has_ready(true));
    }
}
