//! Press to Talk — hold ⌥Space on an eligible composer and speak; the
//! settled transcript lands as one VoicePad paragraph without opening the
//! panel. The mode rides the existing VoicePad feature: off by default,
//! inert while the scratchpad experiment is off, and never a recorder of
//! its own — the held chord is the only capture gesture, so opening or
//! unmuting a pad in this mode stays a navigation act.
//!
//! The state machine below owns the hold's lifecycle — key repeats and
//! release order, permission and worker startup, cancellation on every
//! teardown boundary, and the final-only commit — while the `impl Waku`
//! half performs what it decides: resolving the mic grant, attaching the
//! audio tap, spawning the configured provider's worker, and landing the
//! accepted text on the bound chat's scratchpad. A later unit renders the
//! bubble and the pill off `phase`, `interim`, and `notice` without
//! duplicating any of this machinery.
//!
//! Two engines can transcribe a hold: the streaming AI Gateway session —
//! deltas and partials while held, a bounded drain for the final after
//! release — and the daemon-owned Whistle model, which answers one clip
//! at a time with no interim text. Holds and ordinary VoicePad buffer
//! until explicit finish, then combine bounded requests into one result.

use std::sync::atomic::AtomicBool;
use std::thread;

use tungstenite::Message;
use unicode_segmentation::UnicodeSegmentation;
use waku_client::persistence::VoiceTranscriptionBackend;
use waku_protocol::inference::InferenceProvider;

use super::sessions::TYPING_OWNED_CONTEXTS;
use super::voice_scratchpad::{
    AUDIO_QUEUE_CAP, AudioChunk, MAX_AUDIO_FRAME_BYTES, PcmResampler, READ_POLL, ScratchpadEvent,
    TRANSCRIPTION_SAMPLE_RATE, connect_transcription_socket, dispatch_stream_part, strip_delivered,
    tail_delivered,
};
use super::*;

/// The final drain's bound: releasing the hold sends `audio-done` and the
/// worker keeps the socket open this long for the settled tail before
/// reporting what it has — long enough for the model's flush, short
/// enough that a wedged stream can't park the hold forever.
const FINAL_DRAIN: Duration = Duration::from_secs(6);
/// A hold's transcript needs this many words to land outside the
/// transcript-annotation context — punctuation-only tokens and recognizer
/// noise don't count, and CJK ideographs count by character since UAX #29
/// breaks each into its own word.
const MIN_WORDS: usize = 3;

/// Engine-start retries inside the input grace — a refused start while a
/// Bluetooth headset flips profiles is a momentary verdict, not a dead
/// mic. At the input-poll cadence this bounds the wait to ~1.5 s.
const PRESS_TO_TALK_START_RETRIES: u32 = 3;

/// The VoicePad owner a hold records for — the key its paragraph lands
/// under in `voice_scratchpads`. A chat session owns its pad today; a
/// deliverable-owned VoicePad keys the same map by its own id later, so
/// nothing below assumes the owner is a chat.
pub(super) type VoicePadOwner = Uuid;

/// The composer a hold records for, bound when ⌥Space goes down — commits
/// and cancel checks hold it to that context; an eligible-context change
/// cancels rather than re-binding mid-utterance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PressToTalkContext {
    /// A draft composer — the three-word minimum applies.
    Composer { owner: VoicePadOwner },
    /// A transcript-annotation comment field — any non-whitespace final
    /// text lands, so a one-word answer can sit beside its span. The
    /// accepted text pastes into the field itself, never a pad.
    Annotation { owner: VoicePadOwner },
}

impl PressToTalkContext {
    /// The VoicePad owner this context's paragraph belongs to.
    pub(super) fn owner(&self) -> VoicePadOwner {
        match self {
            Self::Composer { owner } | Self::Annotation { owner } => *owner,
        }
    }
}

/// One hold's lifecycle. `Starting` covers the permission prompt and the
/// worker's socket-or-model warmup; `Finishing` covers the bounded drain
/// after the keys come up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PressToTalkPhase {
    Ready,
    Starting,
    Recording,
    Finishing,
}

/// The inline status a later unit surfaces beside the composer — a hold's
/// own progress, or the outcome that outlives it. Carried as a semantic
/// state, never color alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PressToTalkNotice {
    Idle,
    Starting,
    Recording,
    Finishing,
    /// The last hold landed a paragraph.
    Recorded,
    /// The worker's final held no usable text.
    NoSpeech,
    /// The final text fell under the word minimum — annotation holds are
    /// exempt, so this only lands on composer holds.
    TooShort,
    /// Another capture — ordinary VoicePad or composer dictation — owns
    /// the mic, so the chord was consumed without starting.
    Busy,
    /// The mic grant was refused — macOS settings are the recovery.
    MicDenied,
    /// Startup or transcription failed; the cause is the message.
    Error(String),
    /// Escape, navigation, focus loss, or the setting going off retired
    /// the hold — its unfinished text was discarded.
    Cancelled,
}

/// Worker-thread → event-pump traffic for the hold. `generation` binds
/// every event to the hold that minted it — a canceled hold's late
/// results land nowhere.
pub(super) enum PressToTalkEvent {
    /// The mic permission prompt answered.
    MicAccess(bool),
    /// Capture is live — the socket sent its start frame, or the clip
    /// worker began buffering.
    Connected,
    /// The provisional suffix — display only, never committed.
    Partial(String),
    /// A finalized stream delta.
    Final(String),
    /// A whole segment's settled text — replaces the interim it
    /// previewed. Whistle's one-shot answer arrives as one of these.
    Segment(String),
    /// The worker ended — `Ok` after a normal drain, `Err(cause)` on a
    /// connection, startup, or transcription failure. No further events
    /// for the hold follow either way.
    Completed(Result<(), String>),
    AudioFailed {
        pcm: Vec<i16>,
        cause: String,
    },
}

/// What a state-machine step asks the app to perform — the machine
/// decides, `apply_press_to_talk_directives` performs, so every test
/// stays free of a mic, a socket, and a daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PressToTalkDirective {
    /// Resolve the mic grant — the async answer re-enters as `MicAccess`.
    ResolvePermission,
    /// Grant in hand: attach the tap, start the engine, spawn the
    /// configured provider's worker.
    StartCapture,
    /// A normal release — stop audio now and let the worker settle its
    /// final result.
    FinishCapture,
    /// A cancel boundary — drop the tap, retire the worker, discard the
    /// in-flight transcript.
    AbortCapture,
    /// Land the hold's text as one paragraph on the bound owner's
    /// scratchpad — bound at start, carried here because the machine's
    /// reset clears `context` before the app performs the commit. The
    /// full context rides along so the bubble can anchor where the hold
    /// started.
    Commit {
        context: PressToTalkContext,
        text: String,
    },
}

/// One hold's worth of state plus the chord's live halves. Repeats and
/// release order read `space_down`/`alt_down`; every async answer checks
/// `generation`.
pub(super) struct PressToTalk {
    phase: PressToTalkPhase,
    /// The context the hold started in — `None` while ready.
    context: Option<PressToTalkContext>,
    /// The chord's physical halves. A start needs a fresh Space-down
    /// after both released, so an Alt re-press under a held Space can't
    /// roll one hold into the next — the plan's "fully released and a
    /// fresh press" rule.
    space_down: bool,
    alt_down: bool,
    /// Minted per start and bumped per cancel — every async answer and
    /// worker event carries it, and a stale one lands nowhere.
    generation: u64,
    /// The provisional text the worker last reported — for the
    /// indicator, never for the transcript.
    interim: String,
    /// Finalized text so far — what a `Completed` event commits once the
    /// acceptance check passes.
    settled: String,
    /// What the status row announces — an outcome survives its hold's
    /// reset so too-short and error feedback outlive the attempt.
    notice: PressToTalkNotice,
    /// The context the most recent ended hold was bound to — pairs with
    /// `notice` so an outcome lands on the surface it was recorded in.
    last_context: Option<PressToTalkContext>,
    /// Retires the worker when its events would land on a dead hold.
    stop: Arc<AtomicBool>,
    audio_tx: Sender<AudioChunk>,
    audio_rx: Receiver<AudioChunk>,
    failed_audio: Option<(PressToTalkContext, Vec<i16>)>,
}

impl PressToTalk {
    pub(super) fn new() -> Self {
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(AUDIO_QUEUE_CAP);
        Self {
            phase: PressToTalkPhase::Ready,
            context: None,
            space_down: false,
            alt_down: false,
            generation: 0,
            interim: String::new(),
            settled: String::new(),
            notice: PressToTalkNotice::Idle,
            last_context: None,
            stop: Arc::new(AtomicBool::new(false)),
            audio_tx,
            audio_rx,
            failed_audio: None,
        }
    }

    /// Item 2's indicator hook — the bubble/pill UI reads this.
    #[allow(dead_code)]
    pub(super) fn phase(&self) -> PressToTalkPhase {
        self.phase
    }

    /// The context the live hold is bound to — `None` at rest.
    pub(super) fn context(&self) -> Option<PressToTalkContext> {
        self.context
    }

    /// The provisional transcript — interim while recording, the settled
    /// tail while finishing. Display only; item 2's indicator reads it.
    #[allow(dead_code)]
    pub(super) fn provisional(&self) -> &str {
        if self.interim.is_empty() {
            &self.settled
        } else {
            &self.interim
        }
    }

    /// Item 2's indicator hook — the semantic state the row shows.
    #[allow(dead_code)]
    pub(super) fn notice(&self) -> &PressToTalkNotice {
        &self.notice
    }

    /// The status text a UI surfaces beside the composer — `None` while
    /// there's nothing to say. Every hold state reads as text, never
    /// color alone. Item 2's indicator reads this.
    #[allow(dead_code)]
    pub(super) fn notice_label(&self) -> Option<String> {
        match &self.notice {
            PressToTalkNotice::Idle => None,
            PressToTalkNotice::Recorded => Some(tr!("press_to_talk.recorded")),
            // Starting is too brief for a chip — no label exists for it.
            PressToTalkNotice::Starting => None,
            PressToTalkNotice::Recording => Some(tr!("press_to_talk.recording")),
            PressToTalkNotice::Finishing => Some(tr!("press_to_talk.finishing")),
            PressToTalkNotice::NoSpeech => Some(tr!("press_to_talk.no_speech")),
            PressToTalkNotice::TooShort => Some(tr!("press_to_talk.too_short")),
            PressToTalkNotice::Busy => Some(tr!("press_to_talk.busy")),
            PressToTalkNotice::MicDenied => Some(tr!("press_to_talk.mic_denied")),
            PressToTalkNotice::Error(cause) => Some(cause.clone()),
            PressToTalkNotice::Cancelled => Some(tr!("press_to_talk.cancelled")),
        }
    }

    /// A hold in flight — starting, recording, or finishing. Sends and
    /// clears on the bound session wait for this.
    pub(super) fn busy(&self) -> bool {
        self.phase != PressToTalkPhase::Ready
    }

    /// Whether the live hold claims the composer's chrome — Starting is
    /// deliberately out: capture spins up fast enough that a
    /// "Starting…" chip would only flicker ahead of the recording
    /// indicator.
    pub(super) fn claims_chrome(&self) -> bool {
        self.busy() && self.phase != PressToTalkPhase::Starting
    }

    /// The surface the current notice belongs to — the live hold's
    /// context while busy, the ended hold's context afterward, so a
    /// too-short or error outcome renders where it was recorded.
    pub(super) fn notice_context(&self) -> Option<PressToTalkContext> {
        self.context.or(self.last_context)
    }

    /// The hold is bound to `owner`'s pad — a send or clear on that
    /// owner conflicts with it.
    pub(super) fn busy_for(&self, owner: VoicePadOwner) -> bool {
        self.busy() && self.context.is_some_and(|c| c.owner() == owner)
    }

    /// A Space key-down under ⌥. `repeat` is the platform's held-key
    /// marker, `context` the caller's live eligibility — `None` means the
    /// chord isn't ours and the caller lets it travel as ordinary typing.
    /// Returns whether the chord was consumed.
    fn space_down(
        &mut self,
        repeat: bool,
        context: Option<PressToTalkContext>,
    ) -> (bool, Vec<PressToTalkDirective>) {
        // A live hold consumes the chord regardless of eligibility drift
        // — the alternative is a space leaking into a field mid-capture.
        if self.phase != PressToTalkPhase::Ready {
            // Repeats and fresh presses alike: one hold, one capture, and
            // a press during the drain can't queue a second.
            return (true, Vec::new());
        }
        let Some(context) = context else {
            return (false, Vec::new());
        };
        // The chord must have come fully up since the last hold — a fresh
        // non-repeat press while Space is still physically down isn't one.
        if repeat || self.space_down {
            return (true, Vec::new());
        }
        self.space_down = true;
        self.alt_down = true;
        self.phase = PressToTalkPhase::Starting;
        self.context = Some(context);
        self.generation = self.generation.wrapping_add(1);
        self.stop.store(true, Ordering::Relaxed);
        self.stop = Arc::new(AtomicBool::new(false));
        self.interim.clear();
        self.settled.clear();
        self.notice = PressToTalkNotice::Starting;
        (true, vec![PressToTalkDirective::ResolvePermission])
    }

    /// Space came up — the release half of the hold. Either key's release
    /// ends it, so this shares `release`'s outcome.
    fn space_up(&mut self) -> Vec<PressToTalkDirective> {
        self.space_down = false;
        self.release()
    }

    /// The modifier stream reports ⌥'s state — a release ends the hold
    /// exactly like the Space key-up, which covers the order the platform
    /// reports them in (modifier events arrive through a separate path).
    fn modifiers(&mut self, alt_down: bool) -> Vec<PressToTalkDirective> {
        if self.alt_down && !alt_down {
            self.alt_down = false;
            return self.release();
        }
        Vec::new()
    }

    /// Either key's release: a pending start dies outright — a grant that
    /// lands later must not open the mic on a hold the user already let
    /// go — and a live recording stops audio and drains its final.
    fn release(&mut self) -> Vec<PressToTalkDirective> {
        match self.phase {
            PressToTalkPhase::Starting => {
                // The generation bump retires any in-flight permission
                // answer; the abort also detaches a tap that raced ahead.
                self.generation = self.generation.wrapping_add(1);
                self.stop.store(true, Ordering::Relaxed);
                self.reset_hold();
                self.notice = PressToTalkNotice::Cancelled;
                vec![PressToTalkDirective::AbortCapture]
            }
            PressToTalkPhase::Recording => {
                self.phase = PressToTalkPhase::Finishing;
                self.notice = PressToTalkNotice::Finishing;
                vec![PressToTalkDirective::FinishCapture]
            }
            _ => Vec::new(),
        }
    }

    /// Every cancel boundary funnels here — Escape, focus loss,
    /// navigation, composer teardown, the setting going off, a clear.
    /// The unfinished hold's text discards; committed content is
    /// untouched.
    pub(super) fn cancel(&mut self) -> Vec<PressToTalkDirective> {
        self.failed_audio = None;
        if self.phase == PressToTalkPhase::Ready {
            return Vec::new();
        }
        self.generation = self.generation.wrapping_add(1);
        self.stop.store(true, Ordering::Relaxed);
        self.reset_hold();
        self.notice = PressToTalkNotice::Cancelled;
        vec![PressToTalkDirective::AbortCapture]
    }

    /// Hold-fields reset without touching the outcome notice — the
    /// caller sets the next one, or the notice stays whatever the last
    /// attempt announced. The ended hold's context is kept so the
    /// outcome lands on the surface it came from.
    fn reset_hold(&mut self) {
        self.phase = PressToTalkPhase::Ready;
        self.last_context = self.context;
        self.context = None;
        self.interim.clear();
        self.settled.clear();
        self.space_down = false;
        self.alt_down = false;
    }

    /// A worker or async answer for the hold — generation-checked first,
    /// so a canceled hold's late text lands nowhere.
    fn worker_event(
        &mut self,
        generation: u64,
        event: PressToTalkEvent,
    ) -> Vec<PressToTalkDirective> {
        if generation != self.generation {
            return Vec::new();
        }
        match event {
            PressToTalkEvent::AudioFailed { pcm, cause } => {
                if let Some(context) = self.context {
                    self.failed_audio = Some((context, pcm));
                }
                self.worker_event(generation, PressToTalkEvent::Completed(Err(cause)))
            }
            PressToTalkEvent::MicAccess(granted) => {
                if self.phase != PressToTalkPhase::Starting {
                    return Vec::new();
                }
                if granted {
                    vec![PressToTalkDirective::StartCapture]
                } else {
                    self.reset_hold();
                    self.notice = PressToTalkNotice::MicDenied;
                    Vec::new()
                }
            }
            PressToTalkEvent::Connected => {
                if self.phase == PressToTalkPhase::Starting {
                    self.phase = PressToTalkPhase::Recording;
                    self.notice = PressToTalkNotice::Recording;
                }
                Vec::new()
            }
            PressToTalkEvent::Partial(text) => {
                if matches!(
                    self.phase,
                    PressToTalkPhase::Recording | PressToTalkPhase::Finishing
                ) {
                    self.interim = text;
                }
                Vec::new()
            }
            PressToTalkEvent::Final(text) | PressToTalkEvent::Segment(text) => {
                if matches!(
                    self.phase,
                    PressToTalkPhase::Recording | PressToTalkPhase::Finishing
                ) {
                    self.push_settled(&text);
                }
                Vec::new()
            }
            PressToTalkEvent::Completed(Ok(())) => {
                if !matches!(
                    self.phase,
                    PressToTalkPhase::Recording | PressToTalkPhase::Finishing
                ) {
                    // A worker that ended before capture got going has
                    // nothing to commit — and a ready machine ignores it.
                    if self.phase == PressToTalkPhase::Starting {
                        self.reset_hold();
                    }
                    return Vec::new();
                }
                let text = self.final_text();
                let context = self.context;
                self.reset_hold();
                match (context, accepted(context, &text)) {
                    (_, Acceptance::Empty) => {
                        self.notice = PressToTalkNotice::NoSpeech;
                        Vec::new()
                    }
                    (_, Acceptance::Short) => {
                        self.notice = PressToTalkNotice::TooShort;
                        Vec::new()
                    }
                    (Some(context), Acceptance::Accepted) => {
                        self.notice = PressToTalkNotice::Recorded;
                        vec![PressToTalkDirective::Commit { context, text }]
                    }
                    (None, Acceptance::Accepted) => Vec::new(),
                }
            }
            PressToTalkEvent::Completed(Err(cause)) => {
                let was_active = self.phase != PressToTalkPhase::Ready;
                self.reset_hold();
                if was_active {
                    self.notice = PressToTalkNotice::Error(if self.failed_audio.is_some() {
                        format!("{cause}. {}", tr!("press_to_talk.retry_recording"))
                    } else {
                        cause
                    });
                }
                Vec::new()
            }
        }
    }

    /// The text a `Completed` commits — the settled stream only. Interim
    /// stays display-only: a trailing partial is the recognizer's guess,
    /// not a final, and never counts toward the word minimum.
    fn final_text(&self) -> String {
        self.settled
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Append stream text to the settled transcript — a segment whose
    /// head repeats delivered words sheds the overlap, a delta already
    /// covered by the tail is dropped.
    fn push_settled(&mut self, text: &str) {
        let text = text.replace('\n', " ");
        if text.trim().is_empty() || tail_delivered(&self.settled, &text) {
            return;
        }
        let piece = strip_delivered(&self.settled, &text);
        if piece.trim().is_empty() {
            return;
        }
        if !self.settled.is_empty() && !self.settled.ends_with(' ') {
            self.settled.push(' ');
        }
        self.settled.push_str(piece.trim_end());
        self.interim.clear();
    }
}

/// Where a final transcript lands relative to the acceptance rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Acceptance {
    /// Nothing usable — whitespace or less.
    Empty,
    /// Real text under the composer minimum.
    Short,
    Accepted,
}

/// The final-result gate: annotation holds take any non-whitespace text,
/// composer holds need `MIN_WORDS` words. Words come from UAX #29 so
/// punctuation-only tokens are out and spaceless scripts segment by
/// character rather than counting a whole sentence as one.
fn accepted(context: Option<PressToTalkContext>, text: &str) -> Acceptance {
    if text.trim().is_empty() {
        return Acceptance::Empty;
    }
    let words = text.unicode_words().count();
    match context {
        Some(PressToTalkContext::Annotation { .. }) => Acceptance::Accepted,
        _ if words >= MIN_WORDS => Acceptance::Accepted,
        _ => Acceptance::Short,
    }
}

impl Waku {
    /// The composer the next hold would record for — `None` while nothing
    /// eligible is on screen. A focused recording bubble records for its
    /// own context, the transcript-annotation comment field is always
    /// eligible, overlay or not, and the draft composer needs its
    /// screen free of pages, overlays, Big Picture, and foreign typing
    /// owners.
    fn press_to_talk_target(&self, window: &Window, cx: &App) -> Option<PressToTalkContext> {
        if !self.state.press_to_talk_enabled || !self.state.voice_scratchpad_enabled {
            return None;
        }
        // The bubble's own chrome never disarms the chord — its text,
        // trash button, or edit field holding focus records for the
        // context the bubble was captured from.
        if let Some(context) = self.focused_press_to_talk_bubble_context(window, cx) {
            return Some(context);
        }
        if let Some(editor) = self.annotation_editor.as_ref() {
            let owner = match &editor.target {
                // The transcript-annotation contexts waive the word
                // minimum; the file and plan editors are composers in the
                // ordinary sense and keep it.
                annotations::AnnotationTarget::Transcript => {
                    return self
                        .state
                        .selected_session
                        .map(|owner| PressToTalkContext::Annotation { owner });
                }
                annotations::AnnotationTarget::SideChat(owner) => {
                    return Some(PressToTalkContext::Annotation { owner: *owner });
                }
                annotations::AnnotationTarget::Plan(owner) => *owner,
                annotations::AnnotationTarget::File(_) => self.surface_voice_pad_owner()?,
            };
            return Some(PressToTalkContext::Composer { owner });
        }
        if self.settings_page.is_some()
            || self.big_picture.is_open()
            || self.file_finder.is_open()
            || !self.composer_mounted()
            || self.keyboard_owning_overlay_open()
        {
            return None;
        }
        // A typing owner that isn't an eligible composer field keeps its
        // keys — the composer itself, a visible side chat's composer, and
        // the annotation field all count as ours.
        if window.context_stack().iter().any(|context| {
            TYPING_OWNED_CONTEXTS
                .iter()
                .any(|owned| context.contains(owned))
        }) && !self.press_to_talk_composer_focused(window, cx)
        {
            return None;
        }
        let owner = match self.last_focused_composer_target() {
            model_picker::ModelPickerTarget::SideChat(session_id) => Some(session_id),
            // The surface's owner — a live deliverable page's pad, the
            // selected chat's otherwise.
            _ => self.surface_voice_pad_owner(),
        }?;
        Some(PressToTalkContext::Composer { owner })
    }

    /// The context the bubble's chrome records for while it holds focus
    /// — its text region, its trash button, or the bound edit field.
    /// Editing the latest recording keeps the chord live for that
    /// bubble's own context rather than leaking ⌥Space into the field.
    fn focused_press_to_talk_bubble_context(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<PressToTalkContext> {
        let focused = window.focused(cx)?;
        if focused == self.press_to_talk_bubble_input.read(cx).focus() {
            let edit = self.press_to_talk_bubble_edit.as_ref()?;
            let bubble = self
                .voice_scratchpads
                .get(&edit.owner)
                .and_then(|scratchpad| scratchpad.press_to_talk_bubble);
            return Some(voice_scratchpad::press_to_talk_bubble_edit_context(
                edit, bubble,
            ));
        }
        let focuses = self.transcript_control_focuses.borrow();
        self.voice_scratchpads.values().find_map(|scratchpad| {
            let bubble = scratchpad.press_to_talk_bubble?;
            let owner = bubble.context.owner();
            (focuses
                .get(&format!("ptt-bubble-text-{owner}"))
                .is_some_and(|handle| *handle == focused)
                || focuses
                    .get(&format!("ptt-bubble-trash-{owner}"))
                    .is_some_and(|handle| *handle == focused))
            .then_some(bubble.context)
        })
    }

    /// Whether the focused typing surface is one of the composer's own —
    /// the main field, a visible side chat's, or the annotation comment.
    fn press_to_talk_composer_focused(&self, window: &Window, cx: &App) -> bool {
        let Some(focused) = window.focused(cx) else {
            return false;
        };
        if focused == self.composer.read(cx).focus()
            || focused == self.annotation_comment_input.read(cx).focus()
        {
            return true;
        }
        self.visible_side_chat_id()
            .and_then(|id| self.side_chat_composers.get(&id))
            .is_some_and(|chat| focused == chat.composer.read(cx).focus())
    }

    /// ⌥Space down on the root dispatch: claim the chord for a hold when
    /// the mode is on and a composer is eligible — while a hold is live
    /// the claim stands regardless, so the keystroke can never leak into
    /// a field mid-capture. Returns whether the chord was consumed; any
    /// other shape travels on as typing.
    pub(super) fn press_to_talk_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.keystroke.key != "space" || event.keystroke.modifiers != Modifiers::alt() {
            return false;
        }
        // The mic is single-owner: another VoicePad stream or composer
        // dictation holding the tap leaves the chord consumed and
        // announced rather than fighting for it.
        let target = self.press_to_talk_target(window, cx);
        if target.is_some() && self.press_to_talk_capture_conflict() {
            self.press_to_talk.notice = PressToTalkNotice::Busy;
            cx.stop_propagation();
            cx.notify();
            return true;
        }
        let (claimed, directives) = self.press_to_talk.space_down(event.is_held, target);
        if !claimed {
            return false;
        }
        cx.stop_propagation();
        // A fresh hold dismisses the previous bubble the moment capture
        // is accepted — a failed or too-short attempt never restores it.
        // An edit open on that bubble commits first: the chord firing
        // from inside the field must not drop typed text.
        if directives.contains(&PressToTalkDirective::ResolvePermission) {
            self.commit_press_to_talk_bubble_edit(cx);
            for scratchpad in self.voice_scratchpads.values_mut() {
                scratchpad.press_to_talk_bubble = None;
            }
        }
        self.apply_press_to_talk_directives(directives, cx);
        true
    }

    /// Space's key-up — one half of the chord's release.
    pub(super) fn press_to_talk_key_up(
        &mut self,
        event: &gpui::KeyUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "space" {
            return;
        }
        let directives = self.press_to_talk.space_up();
        if !directives.is_empty() {
            self.apply_press_to_talk_directives(directives, cx);
        }
    }

    /// ⌥'s release arrives on the modifier stream — the other half.
    /// Window deactivation can swallow this event entirely; the
    /// activation observer cancels the hold on its own.
    pub(super) fn press_to_talk_modifiers_changed(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let directives = self.press_to_talk.modifiers(event.modifiers.alt);
        if !directives.is_empty() {
            self.apply_press_to_talk_directives(directives, cx);
        }
    }

    /// Escape's peel for the hold: an active capture cancels before any
    /// deeper Escape meaning runs. Returns whether it consumed the key.
    pub(super) fn press_to_talk_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.press_to_talk.busy() {
            return false;
        }
        let directives = self.press_to_talk.cancel();
        self.apply_press_to_talk_directives(directives, cx);
        true
    }

    /// The window's backgrounding — modifier releases can go missing
    /// across it, so this is the hold's reliable "let go" boundary.
    pub(super) fn press_to_talk_window_deactivated(&mut self, cx: &mut Context<Self>) {
        let directives = self.press_to_talk.cancel();
        self.apply_press_to_talk_directives(directives, cx);
    }

    /// Whether the context a hold, bubble, or clear-recovery was bound
    /// to is still on screen — the annotation binding lives while its
    /// editor does; the composer binding lives while the composer stays
    /// mounted and its chat is still the visible target.
    pub(super) fn press_to_talk_context_alive(&self, context: PressToTalkContext) -> bool {
        match context {
            PressToTalkContext::Annotation { owner } => {
                self.annotation_editor
                    .as_ref()
                    .and_then(|editor| match &editor.target {
                        annotations::AnnotationTarget::Transcript => self.state.selected_session,
                        annotations::AnnotationTarget::SideChat(id)
                        | annotations::AnnotationTarget::Plan(id) => Some(*id),
                        annotations::AnnotationTarget::File(_) => self.surface_voice_pad_owner(),
                    })
                    == Some(owner)
            }
            PressToTalkContext::Composer { owner } => {
                self.composer_mounted()
                    && self.settings_page.is_none()
                    && (self.voice_pad_owner_on_screen(owner)
                        || self.visible_side_chat_id() == Some(owner))
            }
        }
    }

    /// Navigation, page opens, and composer teardown: the hold's bound
    /// context is rechecked for liveness — gone or changed means cancel,
    /// since a final result must never land on a different screen.
    /// Bubbles and the clear-recovery snapshot die with the contexts
    /// that anchored them — returning does not restore either.
    pub(super) fn press_to_talk_navigation(&mut self, cx: &mut Context<Self>) {
        self.pause_dictation_on_navigation(cx);
        if self.press_to_talk.busy()
            && !self
                .press_to_talk
                .context()
                .is_some_and(|context| self.press_to_talk_context_alive(context))
        {
            let directives = self.press_to_talk.cancel();
            self.apply_press_to_talk_directives(directives, cx);
        }
        if self
            .press_to_talk
            .failed_audio
            .as_ref()
            .is_some_and(|(context, _)| !self.press_to_talk_context_alive(*context))
        {
            self.press_to_talk.failed_audio = None;
        }
        let mut expired_bubbles = Vec::new();
        let mut expired_undos = Vec::new();
        for (owner, scratchpad) in &self.voice_scratchpads {
            if let Some(bubble) = scratchpad.press_to_talk_bubble
                && !self.press_to_talk_context_alive(bubble.context)
            {
                expired_bubbles.push((*owner, bubble));
            }
            if let Some(undo) = &scratchpad.clear_undo
                && !self.press_to_talk_context_alive(undo.context)
            {
                expired_undos.push(*owner);
            }
        }
        let mut changed = false;
        for (owner, bubble) in expired_bubbles {
            if self
                .press_to_talk_bubble_edit
                .as_ref()
                .is_some_and(|edit| edit.owner == owner && edit.paragraph == bubble.paragraph)
            {
                self.press_to_talk_bubble_edit = None;
            }
            if let Some(scratchpad) = self.voice_scratchpads.get_mut(&owner) {
                scratchpad.press_to_talk_bubble = None;
            }
            changed = true;
        }
        for owner in expired_undos {
            if let Some(scratchpad) = self.voice_scratchpads.get_mut(&owner) {
                scratchpad.clear_undo = None;
            }
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// ⌘Z's one dedicated meaning while a hold or a bubble owns the
    /// shortcut: consumed outright during recording/finalization, and a
    /// whole-bubble removal once a paragraph is up. Intercepted ahead of
    /// binding dispatch so the same event can never reach the field's
    /// undo stack or the workspace's draft-use undo. Returns without
    /// claiming anything when neither applies, leaving ordinary undo
    /// routing intact.
    pub(super) fn press_to_talk_undo_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.press_to_talk.busy() {
            cx.stop_propagation();
            return;
        }
        let bubble = self
            .voice_scratchpads
            .iter()
            .find_map(|(owner, scratchpad)| {
                let bubble = scratchpad.live_bubble()?;
                self.press_to_talk_context_alive(bubble.context)
                    .then_some((*owner, bubble))
            });
        let Some((owner, bubble)) = bubble else {
            return;
        };
        self.remove_press_to_talk_bubble(owner, bubble, window, cx);
        cx.stop_propagation();
    }

    /// The settings toggle: off cancels an in-flight hold; on ends
    /// ordinary continuous capture through the existing mute path — the
    /// held chord is the only recorder from here.
    pub(super) fn set_press_to_talk_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            let directives = self.press_to_talk.cancel();
            self.apply_press_to_talk_directives(directives, cx);
            // The bubble is the mode's chrome — the mode going off puts
            // it away; the committed paragraphs stay in their pads.
            for scratchpad in self.voice_scratchpads.values_mut() {
                scratchpad.press_to_talk_bubble = None;
            }
            self.press_to_talk_bubble_edit = None;
        } else if self
            .selected_voice_scratchpad()
            .is_some_and(|scratchpad| scratchpad.capture_live)
        {
            self.set_voice_scratchpad_muted(true, cx);
        }
        self.state.press_to_talk_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The transcription-engine selector — applies to the next worker;
    /// a running stream finishes on the engine it started with.
    pub(super) fn set_voice_transcription_backend(
        &mut self,
        backend: VoiceTranscriptionBackend,
        cx: &mut Context<Self>,
    ) {
        self.state.voice_transcription_backend = backend;
        self.save();
        cx.notify();
    }

    /// Another capture owns the mic — a live ordinary VoicePad stream or
    /// a composer dictation in flight.
    fn press_to_talk_capture_conflict(&self) -> bool {
        self.voice_scratchpads
            .values()
            .any(|scratchpad| scratchpad.capture_live)
            || !matches!(
                self.dictation_state,
                DictationState::Idle | DictationState::Error(_)
            )
            || crate::platform::dictation_capture_active()
    }

    /// `ResolvePermission` — the same grant flow VoicePad uses, reported
    /// back on the hold's channel with its generation.
    fn begin_press_to_talk_capture(&mut self, cx: &mut Context<Self>) {
        let generation = self.press_to_talk.generation;
        match crate::platform::microphone_access() {
            crate::platform::CaptureAccess::Granted => self.start_press_to_talk_capture(cx),
            crate::platform::CaptureAccess::Undetermined => {
                let tx = self.press_to_talk_tx.clone();
                let wake = self.event_wake_tx.clone();
                crate::platform::request_microphone_access(Box::new(move |granted| {
                    let _ = tx.try_send((generation, PressToTalkEvent::MicAccess(granted)));
                    signal_event_pump(&wake);
                }));
            }
            crate::platform::CaptureAccess::Denied => {
                let directives = self
                    .press_to_talk
                    .worker_event(generation, PressToTalkEvent::MicAccess(false));
                self.apply_press_to_talk_directives(directives, cx);
            }
        }
    }

    /// `StartCapture` — grant in hand: a fresh audio channel, the tap,
    /// the engine, then the configured provider's worker. Every async
    /// leg re-checks the generation before a thread spawns, so a release
    /// or cancel during warmup can't open the mic afterward.
    fn start_press_to_talk_capture(&mut self, cx: &mut Context<Self>) {
        let generation = self.press_to_talk.generation;
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(AUDIO_QUEUE_CAP);
        self.press_to_talk.audio_tx = audio_tx.clone();
        self.press_to_talk.audio_rx = audio_rx;
        if self
            .press_to_talk
            .failed_audio
            .as_ref()
            .is_some_and(|(owner, _)| Some(*owner) == self.press_to_talk.context)
        {
            let (_, pcm) = self
                .press_to_talk
                .failed_audio
                .take()
                .expect("failed hold checked");
            let daemon = self.daemon.client();
            let events = self.press_to_talk_tx.clone();
            let wake = self.event_wake_tx.clone();
            let stop = self.press_to_talk.stop.clone();
            let audio = self.press_to_talk.audio_rx.clone();
            cx.background_executor()
                .spawn(async move {
                    run_press_to_talk_whistle_worker(
                        generation,
                        daemon,
                        audio,
                        events,
                        wake,
                        stop,
                        Some(pcm),
                    );
                })
                .detach();
            return;
        }
        crate::platform::set_voice_audio_sink(Some(Box::new(move |samples, rate| {
            let _ = audio_tx.try_send(AudioChunk {
                samples: samples.to_vec(),
                rate,
            });
        })));
        if !crate::platform::start_voice_listener() {
            self.retry_press_to_talk_engine_start(generation, PRESS_TO_TALK_START_RETRIES, cx);
            return;
        }
        let events = self.press_to_talk_tx.clone();
        let wake = self.event_wake_tx.clone();
        let stop = self.press_to_talk.stop.clone();
        let audio = self.press_to_talk.audio_rx.clone();
        match self.state.voice_transcription_backend {
            VoiceTranscriptionBackend::AiGateway => {
                let daemon = self.daemon.client();
                let work = cx.background_executor().spawn(async move {
                    daemon
                        .request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::GetInferenceCredential {
                                provider: InferenceProvider::VercelGateway,
                            },
                        )
                        .ok()
                        .and_then(|payload| match payload {
                            waku_client::ResponsePayload::InferenceCredential { credential } => {
                                credential
                            }
                            _ => None,
                        })
                        .filter(|key| !key.trim().is_empty())
                });
                cx.spawn(async move |this, cx| {
                    let key = work.await;
                    let _ = this.update(cx, |this, cx| {
                        if this.press_to_talk.generation != generation
                            || this.press_to_talk.phase != PressToTalkPhase::Starting
                        {
                            return;
                        }
                        match key {
                            Some(key) => {
                                if let Err(error) = thread::Builder::new()
                                    .name("press-to-talk-transcribe".to_owned())
                                    .spawn(move || {
                                        run_press_to_talk_gateway_worker(
                                            generation, key, audio, events, wake, stop,
                                        );
                                    })
                                {
                                    this.press_to_talk_startup_failed(
                                        generation,
                                        format!("could not start transcription worker: {error}"),
                                        cx,
                                    );
                                }
                            }
                            None => this.press_to_talk_startup_failed(
                                generation,
                                tr!("press_to_talk.no_credential"),
                                cx,
                            ),
                        }
                    });
                })
                .detach();
            }
            VoiceTranscriptionBackend::Whistle => {
                let daemon = self.daemon.client();
                let prepare = daemon.clone();
                let work = cx
                    .background_executor()
                    .spawn(async move { ensure_whistle_model(&prepare).await });
                cx.spawn(async move |this, cx| {
                    let result = work.await;
                    let _ = this.update(cx, |this, cx| {
                        if this.press_to_talk.generation != generation
                            || this.press_to_talk.phase != PressToTalkPhase::Starting
                        {
                            return;
                        }
                        match result {
                            Ok(()) => {
                                if let Err(error) = thread::Builder::new()
                                    .name("press-to-talk-whistle".to_owned())
                                    .spawn(move || {
                                        run_press_to_talk_whistle_worker(
                                            generation, daemon, audio, events, wake, stop, None,
                                        );
                                    })
                                {
                                    this.press_to_talk_startup_failed(
                                        generation,
                                        format!("could not start transcription worker: {error}"),
                                        cx,
                                    );
                                }
                            }
                            Err(error) => this.press_to_talk_startup_failed(
                                generation,
                                format!("{error:#}"),
                                cx,
                            ),
                        }
                    });
                })
                .detach();
            }
        }
    }

    /// A refused engine start during warmup: a Bluetooth headset mid
    /// profile-switch reads as a missing input for a moment, so retry on
    /// the input-poll cadence inside the grace window before calling the
    /// mic unavailable. A release or cancel bumps the hold's generation
    /// and the wake lands nowhere.
    fn retry_press_to_talk_engine_start(
        &mut self,
        generation: u64,
        retries: u32,
        cx: &mut Context<Self>,
    ) {
        if retries == 0 {
            self.press_to_talk_startup_failed(
                generation,
                tr!("press_to_talk.mic_unavailable"),
                cx,
            );
            return;
        }
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(super::speech::VOICE_INPUT_RETRY)
                .await;
            let _ = weak.update(cx, |this, cx| {
                if this.press_to_talk.generation != generation
                    || this.press_to_talk.phase != PressToTalkPhase::Starting
                {
                    return;
                }
                if crate::platform::start_voice_listener() {
                    this.start_press_to_talk_capture(cx);
                } else {
                    this.retry_press_to_talk_engine_start(generation, retries - 1, cx);
                }
            });
        })
        .detach();
    }

    /// A startup leg failed after the tap attached — report it through
    /// the event path so the machine's failure handling owns the outcome.
    fn press_to_talk_startup_failed(
        &mut self,
        generation: u64,
        cause: String,
        cx: &mut Context<Self>,
    ) {
        self.abort_press_to_talk_capture();
        let directives = self
            .press_to_talk
            .worker_event(generation, PressToTalkEvent::Completed(Err(cause)));
        self.apply_press_to_talk_directives(directives, cx);
    }

    /// `FinishCapture` — audio stops now; the detached sink drops the
    /// closure's sender and a fresh channel retires the hold's own, which
    /// is each worker's cue to settle its final and report `Completed`.
    fn finish_press_to_talk_capture(&mut self) {
        self.detach_voice_sink();
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(AUDIO_QUEUE_CAP);
        self.press_to_talk.audio_tx = audio_tx;
        self.press_to_talk.audio_rx = audio_rx;
    }

    /// `AbortCapture` — the hold is dead: the tap drops, the stop flag
    /// retires the worker, a fresh channel disconnects its audio source,
    /// and its remaining events land on a bumped generation.
    fn abort_press_to_talk_capture(&mut self) {
        self.press_to_talk.failed_audio = None;
        self.press_to_talk.stop.store(true, Ordering::Relaxed);
        self.detach_voice_sink();
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(AUDIO_QUEUE_CAP);
        self.press_to_talk.audio_tx = audio_tx;
        self.press_to_talk.audio_rx = audio_rx;
    }

    /// `Commit` — the accepted final lands where the hold was bound. An
    /// annotation hold pastes at the comment field's caret — the field
    /// the editor is open on is the destination, never the pad. A
    /// composer hold lands as one paragraph on the bound owner's
    /// scratchpad, created muted and hidden when that pad never opened,
    /// and becomes the pad's bubble. Typed drafts are untouched.
    fn commit_press_to_talk(
        &mut self,
        context: PressToTalkContext,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        if let PressToTalkContext::Annotation { .. } = context {
            self.annotation_comment_input
                .update(cx, |input, cx| input.insert_text(text, cx));
            cx.notify();
            return;
        }
        let owner = context.owner();
        let committed = {
            let scratchpad = self.voice_scratchpads.entry(owner).or_insert_with(|| {
                let mut scratchpad = voice_scratchpad::VoiceScratchpad::new(cx);
                scratchpad.muted = true;
                scratchpad.hidden = true;
                scratchpad
            });
            scratchpad
                .transcript
                .commit_press_to_talk(text)
                .and_then(|paragraph| {
                    scratchpad.press_to_talk_bubble = Some(voice_scratchpad::PressToTalkBubble {
                        paragraph,
                        context,
                        dismissed: false,
                    });
                    scratchpad
                        .transcript
                        .paragraph_text(paragraph)
                        .map(|text| (paragraph, text.to_owned()))
                })
        };
        // The bubble's field opens already bound to the paragraph —
        // editable on display, no click-to-edit step — without stealing
        // focus from wherever the user is typing.
        if let Some((paragraph, text)) = committed {
            self.bind_press_to_talk_bubble(owner, paragraph, text, cx);
        }
        cx.notify();
    }

    /// The machine's answers, performed.
    pub(super) fn apply_press_to_talk_directives(
        &mut self,
        directives: Vec<PressToTalkDirective>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for directive in directives {
            changed = true;
            match directive {
                PressToTalkDirective::ResolvePermission => self.begin_press_to_talk_capture(cx),
                PressToTalkDirective::StartCapture => self.start_press_to_talk_capture(cx),
                PressToTalkDirective::FinishCapture => self.finish_press_to_talk_capture(),
                PressToTalkDirective::AbortCapture => self.abort_press_to_talk_capture(),
                PressToTalkDirective::Commit { context, text } => {
                    self.commit_press_to_talk(context, &text, cx)
                }
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// Worker answers and async results for the hold — the pump drains
    /// them through the machine so stale generations land nowhere.
    pub(super) fn drain_press_to_talk_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok((generation, event)) = self.press_to_talk_events.try_recv() {
            changed = true;
            let directives = self.press_to_talk.worker_event(generation, event);
            self.apply_press_to_talk_directives(directives, cx);
        }
        changed
    }
}

/// The Whistle half of capture startup: report unavailable, download
/// when the model is missing, fail otherwise — the same contract
/// composer dictation follows, run on a background executor. Ordinary
/// VoicePad's Whistle worker prepares through the same check.
pub(super) async fn ensure_whistle_model(daemon: &waku_client::DaemonClient) -> anyhow::Result<()> {
    let status = daemon.request(
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
    anyhow::ensure!(
        available,
        "Whistle transcription is unavailable on this Mac"
    );
    if downloaded {
        return Ok(());
    }
    match daemon.request(
        Uuid::nil(),
        Uuid::nil(),
        waku_client::Command::DownloadWhistleModel,
    )? {
        waku_client::ResponsePayload::WhistleStatus {
            downloaded: true, ..
        } => Ok(()),
        _ => anyhow::bail!("Whistle model download did not complete"),
    }
}

/// Buffered mic audio for an explicit hold; request bounds apply after release.
struct WhistleCollector {
    resampler: PcmResampler,
    pcm: Vec<u8>,
}

impl WhistleCollector {
    fn new() -> Self {
        Self {
            resampler: PcmResampler::new(TRANSCRIPTION_SAMPLE_RATE),
            pcm: Vec::new(),
        }
    }

    /// Resample a tap block into the clip — returns `true` once the cap
    /// says the clip is full.
    fn push(&mut self, chunk: &AudioChunk) {
        self.resampler
            .push(&chunk.samples, chunk.rate, &mut self.pcm);
    }

    /// The clip as the daemon's `i16` vector — `None` on silence.
    fn pcm(&self) -> Option<Vec<i16>> {
        if self.pcm.is_empty() {
            return None;
        }
        Some(
            self.pcm
                .chunks_exact(2)
                .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                .collect(),
        )
    }
}

/// The AI Gateway hold: one socket, no reconnects — a hold is seconds,
/// not a session. On release the audio channel's sender drops, the
/// worker sends `audio-done`, drains the settled tail for up to
/// `FINAL_DRAIN`, and reports `Completed`. Anything terminal —
/// connect failure, stream error, `finish` — ends the hold the same
/// way: what's settled still evaluates.
fn run_press_to_talk_gateway_worker(
    generation: u64,
    key: String,
    audio: Receiver<AudioChunk>,
    events: Sender<(u64, PressToTalkEvent)>,
    wake: smol::channel::Sender<()>,
    stop: Arc<AtomicBool>,
) {
    let send = |event: PressToTalkEvent| {
        let _ = events.send((generation, event));
        signal_event_pump(&wake);
    };
    // Stream parts translate onto the hold's vocabulary — pause markers
    // and session bookkeeping mean nothing to a bounded hold.
    let send_part = |event: ScratchpadEvent, send: &dyn Fn(PressToTalkEvent)| match event {
        ScratchpadEvent::Final(text) => send(PressToTalkEvent::Final(text)),
        ScratchpadEvent::FinalSegment(text) => send(PressToTalkEvent::Segment(text)),
        ScratchpadEvent::Partial(text) => send(PressToTalkEvent::Partial(text)),
        _ => {}
    };
    let mut socket = match connect_transcription_socket(&key) {
        Ok(socket) => socket,
        Err(error) => {
            send(PressToTalkEvent::Completed(Err(format!("{error:#}"))));
            return;
        }
    };
    if stop.load(Ordering::Relaxed) {
        let _ = socket.close(None);
        return;
    }
    let start = serde_json::json!({
        "type": "transcription-stream.start",
        "inputAudioFormat": { "type": "audio/pcm", "rate": TRANSCRIPTION_SAMPLE_RATE },
    });
    if socket
        .send(Message::Text(start.to_string().into()))
        .is_err()
    {
        let _ = socket.close(None);
        send(PressToTalkEvent::Completed(Err(
            "transcription stream rejected its start frame".to_owned(),
        )));
        return;
    }
    send(PressToTalkEvent::Connected);
    let mut resampler = PcmResampler::new(TRANSCRIPTION_SAMPLE_RATE);
    let mut pcm = Vec::new();
    // `audio-ended` records whether the release's audio-done went out —
    // the drain sends it itself on every other exit path.
    let mut audio_ended = false;
    let mut terminal = false;
    while !terminal {
        if stop.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return;
        }
        match audio.recv_timeout(READ_POLL) {
            Ok(chunk) => {
                resampler.push(&chunk.samples, chunk.rate, &mut pcm);
                while let Ok(chunk) = audio.try_recv() {
                    resampler.push(&chunk.samples, chunk.rate, &mut pcm);
                }
                let mut failed = false;
                for frame in pcm.chunks(MAX_AUDIO_FRAME_BYTES) {
                    if socket.send(Message::Binary(frame.to_vec().into())).is_err() {
                        failed = true;
                        break;
                    }
                }
                pcm.clear();
                if failed {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                audio_ended = socket
                    .send(Message::Text(
                        "{\"type\":\"transcription-stream.audio-done\"}".into(),
                    ))
                    .is_ok();
                break;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let mut forward = |event| send_part(event, &send);
                if dispatch_stream_part(&text, &mut forward) {
                    terminal = true;
                }
            }
            Ok(Message::Close(_)) => terminal = true,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => {
                eprintln!("Goddard: press-to-talk stream failed: {error}");
                terminal = true;
            }
        }
        let _ = socket.flush();
    }
    // The drain: the release — or a terminal frame — leaves the socket
    // this long to deliver the settled tail.
    if !audio_ended {
        let _ = socket.send(Message::Text(
            "{\"type\":\"transcription-stream.audio-done\"}".into(),
        ));
    }
    let deadline = Instant::now() + FINAL_DRAIN;
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return;
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let mut forward = |event| send_part(event, &send);
                if dispatch_stream_part(&text, &mut forward) {
                    break;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
        if Instant::now() >= deadline {
            break;
        }
        let _ = socket.flush();
    }
    let _ = socket.close(None);
    send(PressToTalkEvent::Completed(Ok(())));
}

/// The Whistle hold: buffer until release, then transcribe bounded clips
/// and publish a single combined answer. No partials exist to stream; the indicator shows the hold's
/// own state until the answer lands.
fn run_press_to_talk_whistle_worker(
    generation: u64,
    daemon: waku_client::DaemonClient,
    audio: Receiver<AudioChunk>,
    events: Sender<(u64, PressToTalkEvent)>,
    wake: smol::channel::Sender<()>,
    stop: Arc<AtomicBool>,
    retry: Option<Vec<i16>>,
) {
    let send = |event: PressToTalkEvent| {
        let _ = events.send((generation, event));
        signal_event_pump(&wake);
    };
    send(PressToTalkEvent::Connected);
    let mut collector = WhistleCollector::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        // The gateway worker's poll cadence — the hold keeps a sender
        // after the sink drops, so a plain `recv` would park a canceled
        // hold forever.
        match audio.recv_timeout(READ_POLL) {
            Ok(chunk) => collector.push(&chunk),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let Some(pcm) = retry.or_else(|| collector.pcm()) else {
        send(PressToTalkEvent::Completed(Ok(())));
        return;
    };
    let result = super::speech::transcribe_dictation(&pcm, |clip| {
        anyhow::ensure!(!stop.load(Ordering::Relaxed), "recording cancelled");
        daemon.request(
            Uuid::nil(),
            Uuid::nil(),
            waku_client::Command::Transcribe {
                pcm: clip,
                language: None,
                keywords: None,
            },
        )
    });
    if stop.load(Ordering::Relaxed) {
        return;
    }
    match result {
        Ok(text) => {
            if !text.is_empty() {
                send(PressToTalkEvent::Segment(text));
            }
            send(PressToTalkEvent::Completed(Ok(())));
        }
        Err(error) => {
            send(PressToTalkEvent::AudioFailed {
                pcm,
                cause: format!("{error:#}"),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: Uuid = Uuid::nil();
    const COMPOSER: PressToTalkContext = PressToTalkContext::Composer { owner: OWNER };
    const ANNOTATION: PressToTalkContext = PressToTalkContext::Annotation { owner: OWNER };

    fn hold() -> (PressToTalk, Vec<PressToTalkDirective>) {
        let mut machine = PressToTalk::new();
        let (claimed, directives) = machine.space_down(false, Some(COMPOSER));
        assert!(claimed);
        (machine, directives)
    }

    #[test]
    fn space_down_without_a_context_is_not_claimed() {
        // The setting off or the screen ineligible reads the same to the
        // machine — the chord travels on as ordinary typing.
        let mut machine = PressToTalk::new();
        let (claimed, directives) = machine.space_down(false, None);
        assert!(!claimed);
        assert!(directives.is_empty());
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
    }

    #[test]
    fn a_fresh_press_starts_one_hold() {
        let (machine, directives) = hold();
        assert_eq!(machine.phase, PressToTalkPhase::Starting);
        assert_eq!(machine.context, Some(COMPOSER));
        assert_eq!(directives, vec![PressToTalkDirective::ResolvePermission]);
        // The permission grant lands capture.
        let mut machine = machine;
        let directives =
            machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        assert_eq!(directives, vec![PressToTalkDirective::StartCapture]);
        let directives = machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        assert!(directives.is_empty());
        assert_eq!(machine.phase, PressToTalkPhase::Recording);
    }

    #[test]
    fn a_starting_hold_claims_no_chrome() {
        // Capture spins up fast enough that a "Starting microphone…" chip
        // would only flicker — the indicator appears once recording is
        // actually live and stays through the drain.
        let (mut machine, _) = hold();
        assert!(machine.busy());
        assert!(!machine.claims_chrome());
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        assert!(machine.claims_chrome());
        machine.release();
        assert!(machine.claims_chrome());
        machine.worker_event(machine.generation, PressToTalkEvent::Completed(Ok(())));
        assert!(!machine.claims_chrome());
    }

    #[test]
    fn key_repeats_never_start_a_second_capture() {
        let (mut machine, _) = hold();
        let (claimed, directives) = machine.space_down(true, Some(COMPOSER));
        assert!(claimed);
        assert!(directives.is_empty());
        // A fresh non-repeat press while the hold lives is consumed too —
        // the chord never leaks mid-capture.
        let (claimed, directives) = machine.space_down(false, Some(COMPOSER));
        assert!(claimed);
        assert!(directives.is_empty());
    }

    #[test]
    fn release_either_key_finishes_a_recording() {
        for release_alt_first in [false, true] {
            let (mut machine, _) = hold();
            machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
            machine.worker_event(machine.generation, PressToTalkEvent::Connected);
            let directives = if release_alt_first {
                machine.modifiers(false)
            } else {
                machine.space_up()
            };
            assert_eq!(directives, vec![PressToTalkDirective::FinishCapture]);
            assert_eq!(machine.phase, PressToTalkPhase::Finishing);
            // The other half's release afterward is inert.
            let directives = if release_alt_first {
                machine.space_up()
            } else {
                machine.modifiers(false)
            };
            assert!(directives.is_empty());
        }
    }

    #[test]
    fn releasing_during_startup_prevents_delayed_capture() {
        let (mut machine, _) = hold();
        let directives = machine.space_up();
        assert_eq!(directives, vec![PressToTalkDirective::AbortCapture]);
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
        // The prompt answering a beat later must not open the mic — the
        // generation moved on.
        let directives =
            machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        assert!(directives.is_empty());
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
        // A fresh hold still works afterward.
        let (claimed, directives) = machine.space_down(false, Some(COMPOSER));
        assert!(claimed);
        assert_eq!(directives, vec![PressToTalkDirective::ResolvePermission]);
    }

    #[test]
    fn a_held_space_blocks_a_new_hold_until_it_comes_up() {
        let (mut machine, _) = hold();
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        // Alt releases while Space is still physically held — the hold
        // finishes, but no fresh press can start until Space comes up.
        let directives = machine.modifiers(false);
        assert_eq!(directives, vec![PressToTalkDirective::FinishCapture]);
        let (claimed, directives) = machine.space_down(false, Some(COMPOSER));
        assert!(claimed);
        assert!(directives.is_empty());
        assert_eq!(machine.phase, PressToTalkPhase::Finishing);
        // Space's release clears the chord; the drain finishing lands the
        // machine ready, and the next press is fresh.
        machine.space_up();
        machine.worker_event(machine.generation, PressToTalkEvent::Completed(Ok(())));
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
        let (claimed, directives) = machine.space_down(false, Some(COMPOSER));
        assert!(claimed);
        assert_eq!(directives, vec![PressToTalkDirective::ResolvePermission]);
    }

    #[test]
    fn cancel_discards_the_in_flight_hold() {
        let (mut machine, _) = hold();
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::Partial("almost there".to_owned()),
        );
        let generation = machine.generation;
        let directives = machine.cancel();
        assert_eq!(directives, vec![PressToTalkDirective::AbortCapture]);
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
        // The late result lands nowhere.
        let directives = machine.worker_event(generation, PressToTalkEvent::Completed(Ok(())));
        assert!(directives.is_empty());
        assert!(machine.settled.is_empty());
    }

    #[test]
    fn finals_commit_only_the_settled_text() {
        // A qualifying partial cannot rescue a too-short final — interim
        // is display-only and never counts toward the commit.
        let (mut machine, _) = hold();
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::Final("review this".to_owned()),
        );
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::Partial("review this change".to_owned()),
        );
        machine.space_up();
        let directives =
            machine.worker_event(machine.generation, PressToTalkEvent::Completed(Ok(())));
        assert!(directives.is_empty());
        assert_eq!(machine.notice, PressToTalkNotice::TooShort);
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
    }

    #[test]
    fn word_count_boundaries_outside_annotations() {
        // Under three commits nothing — punctuation-only tokens and
        // whitespace count zero.
        for text in ["", "   ", "yes", "thank you", "!! ..", "yes!"] {
            let (mut machine, _) = hold();
            machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
            machine.worker_event(machine.generation, PressToTalkEvent::Connected);
            machine.worker_event(
                machine.generation,
                PressToTalkEvent::Segment(text.to_owned()),
            );
            machine.space_up();
            let directives =
                machine.worker_event(machine.generation, PressToTalkEvent::Completed(Ok(())));
            assert!(directives.is_empty(), "{text:?} should not commit");
            assert!(
                matches!(
                    machine.notice,
                    PressToTalkNotice::TooShort | PressToTalkNotice::NoSpeech
                ),
                "{text:?}"
            );
            assert_eq!(machine.phase, PressToTalkPhase::Ready);
        }
        // Three qualifies.
        let (mut machine, _) = hold();
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::Segment("review this change".to_owned()),
        );
        machine.space_up();
        let directives =
            machine.worker_event(machine.generation, PressToTalkEvent::Completed(Ok(())));
        assert_eq!(
            directives,
            vec![PressToTalkDirective::Commit {
                context: COMPOSER,
                text: "review this change".to_owned(),
            }],
        );
    }

    #[test]
    fn spaceless_scripts_segment_by_character() {
        // UAX #29 breaks each ideograph into a word — a whole sentence is
        // not one token.
        assert!(matches!(
            accepted(Some(COMPOSER), "レビューしてください"),
            Acceptance::Accepted
        ));
        assert!(matches!(
            accepted(Some(COMPOSER), "好的"),
            Acceptance::Short
        ));
    }

    #[test]
    fn annotation_holds_take_any_non_whitespace() {
        for (text, expect_commit) in [("yes", true), ("thank you", true), ("  ", false)] {
            let mut machine = PressToTalk::new();
            let (claimed, _) = machine.space_down(false, Some(ANNOTATION));
            assert!(claimed);
            machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
            machine.worker_event(machine.generation, PressToTalkEvent::Connected);
            machine.worker_event(
                machine.generation,
                PressToTalkEvent::Segment(text.to_owned()),
            );
            machine.space_up();
            let directives =
                machine.worker_event(machine.generation, PressToTalkEvent::Completed(Ok(())));
            if expect_commit {
                assert_eq!(
                    directives,
                    vec![PressToTalkDirective::Commit {
                        context: ANNOTATION,
                        text: text.trim().to_owned(),
                    }],
                );
            } else {
                assert!(directives.is_empty());
                assert_eq!(machine.notice, PressToTalkNotice::NoSpeech);
            }
        }
    }

    #[test]
    fn worker_failures_report_without_committing() {
        let (mut machine, _) = hold();
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::Final("half a thought".to_owned()),
        );
        machine.space_up();
        let directives = machine.worker_event(
            machine.generation,
            PressToTalkEvent::Completed(Err("socket dropped".to_owned())),
        );
        assert!(directives.is_empty());
        assert_eq!(
            machine.notice,
            PressToTalkNotice::Error("socket dropped".to_owned())
        );
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
    }

    #[test]
    fn a_cancelled_hold_leaves_its_outcome_on_the_bound_context() {
        let (mut machine, _) = hold();
        let directives = machine.cancel();
        assert_eq!(directives, vec![PressToTalkDirective::AbortCapture]);
        assert_eq!(machine.notice, PressToTalkNotice::Cancelled);
        assert_eq!(machine.notice_context(), Some(COMPOSER));
        // Releasing mid-startup reads the same way — the aborted hold
        // announces where it was bound.
        let (mut machine, _) = hold();
        machine.space_up();
        assert_eq!(machine.notice, PressToTalkNotice::Cancelled);
        assert_eq!(machine.notice_context(), Some(COMPOSER));
        // The live hold's own context wins while one is bound — the
        // outcome of a previous attempt can't float on another surface.
        let (mut machine, _) = hold();
        machine.space_up();
        let (claimed, _) = machine.space_down(false, Some(ANNOTATION));
        assert!(claimed);
        assert_eq!(machine.notice_context(), Some(ANNOTATION));
    }

    #[test]
    fn permission_denied_reports_and_recovers() {
        let (mut machine, _) = hold();
        let directives =
            machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(false));
        assert!(directives.is_empty());
        assert_eq!(machine.notice, PressToTalkNotice::MicDenied);
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
        let (claimed, _) = machine.space_down(false, Some(COMPOSER));
        assert!(claimed);
        assert_eq!(machine.phase, PressToTalkPhase::Starting);
    }

    #[test]
    fn failed_whistle_hold_retains_audio_and_cancel_retires_it() {
        let (mut machine, _) = hold();
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        machine.space_up();
        let pcm = vec![9; 65 * 16000];
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::AudioFailed {
                pcm: pcm.clone(),
                cause: "failed second clip".into(),
            },
        );
        assert_eq!(machine.phase, PressToTalkPhase::Ready);
        assert_eq!(machine.failed_audio, Some((COMPOSER, pcm)));
        assert!(matches!(machine.notice, PressToTalkNotice::Error(_)));
        machine.cancel();
        assert!(machine.failed_audio.is_none());
    }

    #[test]
    fn whistle_hold_waits_for_release_past_thirty_seconds() {
        let (daemon, requests) = super::super::speech::tests::whistle_daemon();
        let (audio_tx, audio) = crossbeam_channel::unbounded();
        let (events, received) = crossbeam_channel::unbounded();
        let (wake, _) = smol::channel::unbounded();
        let worker = thread::spawn(move || {
            run_press_to_talk_whistle_worker(
                1,
                daemon,
                audio,
                events,
                wake,
                Arc::new(AtomicBool::new(false)),
                None,
            )
        });
        assert!(matches!(
            received.recv_timeout(Duration::from_secs(5)).unwrap().1,
            PressToTalkEvent::Connected
        ));
        audio_tx
            .send(AudioChunk {
                samples: vec![0.5; 65 * 16000],
                rate: 16000.0,
            })
            .unwrap();
        assert!(
            requests.recv_timeout(Duration::from_millis(200)).is_err(),
            "request before release"
        );
        assert!(received.try_recv().is_err(), "text before release");
        drop(audio_tx);
        let (_, event) = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            matches!(event, PressToTalkEvent::Segment(text) if text == "test segment test segment test segment")
        );
        assert!(matches!(
            received.recv_timeout(Duration::from_secs(5)).unwrap().1,
            PressToTalkEvent::Completed(Ok(()))
        ));
        worker.join().unwrap();
        assert_eq!(requests.try_iter().sum::<usize>(), 65 * 16000);
    }

    #[test]
    fn settled_dedup_strips_overlapping_delivery() {
        let (mut machine, _) = hold();
        machine.worker_event(machine.generation, PressToTalkEvent::MicAccess(true));
        machine.worker_event(machine.generation, PressToTalkEvent::Connected);
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::Final("please review".to_owned()),
        );
        // The segment repeats its head — the overlap goes, not the tail.
        machine.worker_event(
            machine.generation,
            PressToTalkEvent::Segment("please review this change".to_owned()),
        );
        machine.space_up();
        let directives =
            machine.worker_event(machine.generation, PressToTalkEvent::Completed(Ok(())));
        assert_eq!(
            directives,
            vec![PressToTalkDirective::Commit {
                context: COMPOSER,
                text: "please review this change".to_owned(),
            }],
        );
    }
}
