//! VoicePad — press the VP mic button beside send in the composer
//! and dictate. A card replaces the chat content while a streaming
//! transcription session (`microsoft/mai-transcribe-2-streaming` over the
//! AI gateway's WebSocket endpoint) appends speech to a current paragraph;
//! saying "okay next" starts a new one, a red dot marks the append point,
//! and Enter sends the whole transcript as one message. Clicking a
//! paragraph opens an annotation box where speech becomes bullets under
//! that paragraph — a box on a bullet nests one level deeper — and
//! saying "okay, let's make an edit" turns the rest of the box's
//! dictation into a rewrite instruction for the annotated paragraph.
//! Each chat keeps its own scratchpad like a composer
//! draft — switching chats auto-pauses the outgoing one (its socket
//! closes; it lands muted) — while Hide keeps the same chat's session
//! recording out of view. One mic stream runs at a time, owned by the
//! visible chat.
//!
//! Audio is captured through the shared `voice_gate` engine tap in
//! `platform.rs` — the scratchpad's sink is the one path that forwards mic
//! samples off the machine, and it exists only while a session owns
//! capture: mute, cancel, and a chat switch all tear it down.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Context as _;
use gpui::{DispatchPhase, Font, ease_in_out};
use serde_json::Value;
use tungstenite::handshake::client::Request;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{HandshakeError, Message, WebSocket};
use waku_protocol::inference::InferenceProvider;

use super::*;

/// The gateway's streaming-transcription WebSocket, model id in the query —
/// the same `ai-model-id` plumbing the REST calls carry as a header.
const TRANSCRIPTION_MODEL_ID: &str = "microsoft/mai-transcribe-2-streaming";
const TRANSCRIPTION_URL: &str = "wss://ai-gateway.vercel.sh/v4/ai/transcription-model?ai-model-id=microsoft%2Fmai-transcribe-2-streaming";
/// The bearer key can't ride a header on every WebSocket implementation, so
/// the gateway also accepts it as a `Sec-WebSocket-Protocol` entry — the
/// same workaround OpenAI's realtime API documents.
const TRANSCRIPTION_SUBPROTOCOL: &str = "ai-gateway-transcription.v1";
const TRANSCRIPTION_AUTH_PREFIX: &str = "ai-gateway-auth.";
/// PCM the model expects: 16 kHz, 16-bit signed little-endian mono.
const TRANSCRIPTION_SAMPLE_RATE: f64 = 16_000.0;
/// One WebSocket frame stays well under the gateway's 256 KiB limit.
const MAX_AUDIO_FRAME_BYTES: usize = 48 * 1024;
/// Tap blocks (~100 ms) buffered between the audio thread and the socket.
/// A full queue drops blocks — dictation cares about now, not backlog.
const AUDIO_QUEUE_CAP: usize = 256;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Steady-state socket poll — short enough that queued audio and inbound
/// transcript frames both flow without a dedicated writer thread.
const READ_POLL: Duration = Duration::from_millis(25);
/// A reported pause gets this long to produce a resume marker or fresh
/// transcript while audio streams before the worker calls the stream
/// wedged — the gateway envelope defines no resume frame, so reconnecting
/// is the only recovery a stuck pause leaves.
const PAUSE_WATCHDOG: Duration = Duration::from_secs(15);
/// No transcript activity for this long stops the recording on its own —
/// a pad left listening in the background closes its socket and lands
/// muted rather than holding the stream open forever.
const SILENCE_AUTO_STOP: Duration = Duration::from_secs(4 * 60);
/// Between reconnect attempts — short enough to feel continuous
/// mid-dictation, long enough not to hammer a down gateway.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
/// Consecutive sessions that die before producing any stream output before
/// the worker stops reconnecting and drops the session to Retry.
const MAX_DEAD_SESSIONS: u32 = 5;
/// A session that stayed up this long counts as healthy however it ended —
/// its death never counts toward the dead-session budget.
const HEALTHY_SESSION: Duration = Duration::from_secs(30);
/// Cancel asks first once the transcript is worth keeping.
const CANCEL_CONFIRM_PARAGRAPHS: usize = 2;
const CANCEL_CONFIRM_CHARS: usize = 280;
/// The scratchpad card matches the composer card's width.
const CARD_MAX_WIDTH: f32 = CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0;
const CARD_RADIUS: f32 = 24.0;
/// The gradient cover's rise above the composer card's top edge — the
/// frame's 178px cover hides ~95px behind the card and clears it by ~83.
const GRADIENT_RISE: f32 = 83.0;
/// The fade completes a third of the way down the cover — the frame's
/// gradient vector runs from {0.5, 0} to {0.5, 0.334}, everything below
/// solid surface.
const GRADIENT_FADE_END: f32 = 0.334;
/// The workspace footer's strip under the composer card — 4px top pad,
/// 28px row, 8px bottom pad in `render_workspace_footer`. The panel's bottom
/// edge stops at the composer card's, leaving that strip — and the screen
/// edge — outside the card.
const FOOTER_STRIP: f32 = 40.0;
/// The hint line's bottom edge floats this far above the composer's top
/// edge.
const HINT_CLEARANCE: f32 = 16.0;
/// The control row's bottom edge floats this far above the composer
/// card's top edge — the frame's 12px gap.
const CONTROLS_CLEARANCE: f32 = 12.0;
/// The control row's right edge sits this far inside the card's right
/// edge — the frame's 18px inset.
const CONTROLS_INSET: f32 = 18.0;
/// The title's inset from the card's top edge — it scrolls with the
/// transcript as the column's first row.
const TITLE_TOP_INSET: f32 = 24.0;
/// The gap under the title before the first transcript row.
const TITLE_BOTTOM_GAP: f32 = 30.0;
/// A radius just under half the 32px pill height gives the frame's capsule
/// silhouette while staying unsaturated, so the renderer's smoothed-corner
/// shoulder still engages — the frame's squircles. `rounded_full` saturates
/// and stays a plain circular capsule.
const PILL_RADIUS: f32 = 15.5;
/// The composer's VP pill uses the same unsaturated-radius trick at the
/// frame's new 24px height.
const VP_PILL_RADIUS: f32 = 11.5;
/// The annotation bullet marker — a drawn disc reads heavier than the "•"
/// text glyph, which renders as a ~4px speck at body size.
const BULLET_SIZE: f32 = 6.0;
/// Each nesting level steps in by one marker column — the disc plus its
/// gap — so a child's disc sits under its parent's text.
const BULLET_INDENT: f32 = BULLET_SIZE + 8.0;
/// The deepest bullet level — nine rows of indentation counting the
/// paragraph's own bullets; further annotation keeps landing at the cap,
/// reading as siblings of the deepest row.
const MAX_BULLET_DEPTH: usize = 8;
/// The spoken edit phrase never runs longer than this once tokenized —
/// "okay let's make an edit" as six words when "let's" splits.
const EDIT_COMMAND_MAX_WORDS: usize = 6;
const RECORDING_RED: u32 = 0xF0344E;
const RECORDING_GLOW: u32 = 0xFF85B6;
/// The live record dot's breathing cycle — one full opacity sweep,
/// ~70% to 100% and back.
const RECORDING_DOT_PERIOD: Duration = Duration::from_millis(1500);
/// The text model that scrubs finished dictation — reached through the
/// same Vercel AI Gateway credential the transcription socket uses.
const CLEANUP_MODEL_ID: &str = "alibaba/qwen3.8-27b";
/// A partial that only shrinks the interim tail or rewrites its trailing
/// punctuation holds for this long before it may repaint — recognizer
/// revisions flap on a 10–50ms cadence, so a shorter hold still flickers.
const INTERIM_DWELL: Duration = Duration::from_millis(200);
/// A landed append paints in over this long — long enough to read as
/// materializing, short enough not to lag behind speech.
const WORD_FADE: Duration = Duration::from_millis(150);
/// A cleanup answer's crossfade: the row's prior text lifts and dissolves
/// while the rewritten span resolves beneath it.
const CLEANUP_MORPH: Duration = Duration::from_millis(240);
/// The cleanup call's whole brief: fix dictation artifacts without
/// rewriting. The span it sees is raw speech-to-text, never a draft.
const CLEANUP_INSTRUCTIONS: &str = "Clean up raw dictated speech-to-text. Remove filler words (um, uh, ah), false starts, and stuttered repetitions; fix obvious transcription errors; add light punctuation and capitalization. Keep the speaker's words and meaning exactly — rewrite as little as possible, never summarize, reorder, or answer. Reply with only the cleaned text — no quotes or commentary.";
/// The voice-edit call's brief: the annotation box's instruction applies
/// to the paragraph it hangs under — a deliberate rewrite, not a scrub.
const EDIT_INSTRUCTIONS: &str = "Apply the editing instruction to the text. Change only what the instruction asks for and keep the rest as written. Reply with only the rewritten text — no quotes or commentary.";

/// One tap block of mono-mixed PCM plus its sample rate.
struct AudioChunk {
    samples: Vec<f32>,
    rate: f64,
}

/// Worker-thread → event-pump traffic. `generation` guards a reconnect's
/// events from landing on a worker that has since been replaced — the mic
/// prompt's answer carries generation 0 and predates every worker.
pub(super) enum ScratchpadEvent {
    /// The mic permission prompt answered.
    MicAccess(bool),
    /// The socket connected and sent its start frame.
    Connected,
    /// Finalized transcript text — appends through the command filter.
    Final(String),
    /// A `transcript-final` part — the segment's whole text.
    FinalSegment(String),
    /// The whole provisional suffix — replaces the current interim and
    /// runs the same command scan, so "okay next" breaks on the partial
    /// rather than waiting for the segment to finalize.
    Partial(String),
    /// The stream failed or ended and reconnects ran out — the transcript
    /// stays and the panel offers Retry.
    Failed,
    /// The stream dropped mid-session and the worker is already
    /// reconnecting — surfaces as `Connecting`, transcript intact.
    Reconnecting,
    /// The model reported a pause — a state marker, not speech. Audio
    /// keeps streaming; a resume marker or fresh transcript text lifts it.
    Paused,
    /// The model resumed after a pause.
    Resumed,
    /// The silence auto-stop fired — no transcript activity outlived
    /// `SILENCE_AUTO_STOP`. The worker closed its socket and exited on
    /// its own; the session lands muted, and unmuting reconnects it.
    Stopped,
    /// The cleanup model settled on a dictated span — `cleaned` is its
    /// rewrite, `None` when the call failed and the raw text keeps
    /// standing. Either way the span's in-flight marker clears; a late
    /// answer still verifies against the raw text before it lands.
    /// `edit` marks an explicit voice-edit rewrite — it may land over a
    /// paragraph the typed-edit guard protects, since the spoken
    /// instruction sanctions the change.
    Cleaned {
        target: CleanTarget,
        start: usize,
        raw: String,
        cleaned: Option<String>,
        edit: bool,
    },
}

/// The buffer a cleanup answer writes into: a committed transcript node,
/// or the open annotation box's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CleanTarget {
    Node(ScratchpadNode),
    Annotation,
}

/// A completed dictated span queued for the cleanup model — the byte
/// offset it occupied when it closed plus its raw text; both verify again
/// when the answer lands, so a shifted or edited span is left alone.
/// `edit` carries a spoken rewrite instruction — the call applies it to
/// `raw` (the target's whole text) instead of scrubbing it.
#[derive(Clone)]
pub(super) struct CleanupRequest {
    pub target: CleanTarget,
    pub start: usize,
    pub raw: String,
    pub edit: Option<String>,
}

/// Where the capture side of a session stands. `Connecting` also covers
/// the credential fetch and the permission prompt's flight time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScratchpadStatus {
    Connecting,
    Live,
    /// The mic permission was refused — the panel opens in an error state
    /// with Retry / Open Settings instead of a dead surface.
    MicDenied,
    /// The model reported a transcription pause while the stream stays
    /// open — audio still flows and fresh speech or a reconnect lifts it.
    /// Distinct from `ConnectionLost`: the worker is alive.
    Paused,
    /// The stream dropped or errored and the worker gave up reconnecting.
    /// The transcript is kept; Retry reconnects and audio resumes live —
    /// nothing is queued for later.
    ConnectionLost,
}

/// The VP button's posture for one composer card.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScratchpadButtonState {
    Idle,
    Recording,
    Muted,
}

/// A dictation session bound to the chat it started in — the
/// `voice_scratchpads` map key carries that chat's id.
pub(super) struct VoiceScratchpad {
    transcript: ScratchpadTranscript,
    status: ScratchpadStatus,
    muted: bool,
    hidden: bool,
    /// The session currently owns the tap: the sink is attached and a
    /// worker runs (or is launching) for it. Only the selected chat's
    /// unmuted scratchpad may hold this — a switch or a mute clears it.
    capture_live: bool,
    /// The picked mic is unplugged — capture parks until it returns rather
    /// than silently hopping to another device.
    input_unavailable: bool,
    /// The Cancel affordance armed its confirmation.
    confirm_discard: bool,
    /// Bumped on each (re)connect and on pause so a retired worker's
    /// events land nowhere.
    generation: u64,
    /// Flags the current worker out; the audio channel disconnecting says
    /// the same. A respawn mints a fresh flag so firing it stays final.
    stop: Arc<AtomicBool>,
    audio_tx: Sender<AudioChunk>,
    audio_rx: Receiver<AudioChunk>,
    scroll: ScrollHandle,
    scrollbar: Rc<ScrollbarState>,
    /// The scratchpad text's selection registry — the copy action reaches
    /// it while the panel owns the column.
    pub(super) selection: TranscriptSelection,
    /// Auto-scroll follows the append point until the user scrolls up.
    follow_tail: bool,
    /// The transcript surface's handle — a settled selection or a Tab
    /// landing moves typing here so selection edits reach the transcript.
    edit_focus: FocusHandle,
    /// Per-row handles for the annotate affordance — `p{index}` for a
    /// paragraph, `b{index}-{bullet}` for a bullet — created lazily the
    /// way the sidebar's group rows are.
    row_focuses: HashMap<String, FocusHandle>,
    /// Render-layer records of landed cleanup answers — the model swap
    /// stays instant; each morph paints the prior text dissolving over
    /// the rewritten span for [`CLEANUP_MORPH`].
    cleanup_morphs: Vec<CleanupMorph>,
    /// Appended text still inside its [`WORD_FADE`], keyed like
    /// `row_focuses` plus `"interim"` and `"annotation"` for the tails
    /// and the open box.
    text_fades: HashMap<String, TextFade>,
    /// What the append point's row painted last frame — settled text
    /// plus the interim tail. A delivery committing painted gray text
    /// records an [`InterimCrossfade`] against it.
    painted_row: Option<PaintedRow>,
    /// A partial→final landing mid-crossfade — the displaced tail
    /// dissolves in place while the divergent residual resolves.
    interim_crossfade: Option<InterimCrossfade>,
    // The pill handles stay live for the floating controls row — the top
    // bar that hosted it is gone.
    #[allow(dead_code)]
    mute_focus: FocusHandle,
    #[allow(dead_code)]
    hide_focus: FocusHandle,
    #[allow(dead_code)]
    cancel_focus: FocusHandle,
    keep_focus: FocusHandle,
    discard_focus: FocusHandle,
}

impl VoiceScratchpad {
    fn new(cx: &mut Context<Waku>) -> Self {
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(AUDIO_QUEUE_CAP);
        Self {
            transcript: ScratchpadTranscript::default(),
            status: ScratchpadStatus::Connecting,
            muted: false,
            hidden: false,
            capture_live: false,
            input_unavailable: false,
            confirm_discard: false,
            generation: 0,
            stop: Arc::new(AtomicBool::new(false)),
            audio_tx,
            audio_rx,
            scroll: ScrollHandle::new(),
            scrollbar: ScrollbarState::new(),
            selection: TranscriptSelection::default(),
            follow_tail: true,
            edit_focus: cx.focus_handle(),
            row_focuses: HashMap::new(),
            cleanup_morphs: Vec::new(),
            text_fades: HashMap::new(),
            painted_row: None,
            interim_crossfade: None,
            mute_focus: cx.focus_handle(),
            hide_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            keep_focus: cx.focus_handle(),
            discard_focus: cx.focus_handle(),
        }
    }

    /// A landed cleanup answer rewrote `target` — record the transition
    /// the rows paint: `old` dissolves over the rewritten text for
    /// [`CLEANUP_MORPH`]. The slot's fade resets too so the swap doesn't
    /// also read as an append.
    fn morph_landed(&mut self, target: CleanTarget, old: String) {
        let new = self.transcript.clean_target_text(target).to_owned();
        self.cleanup_morphs.retain(|morph| morph.target != target);
        self.cleanup_morphs.push(CleanupMorph {
            target,
            old,
            new,
            started: Instant::now(),
        });
        self.text_fades.insert(
            scratchpad_fade_key(target),
            TextFade {
                settled: self.fade_slot_len(target),
                fresh: Vec::new(),
            },
        );
    }

    /// The painted length of a fade slot's buffer — the annotation box's
    /// slot covers its composed display string, interim included.
    fn fade_slot_len(&self, target: CleanTarget) -> usize {
        match target {
            CleanTarget::Node(node) => self.transcript.node_text(node).len(),
            CleanTarget::Annotation => self.annotation_display_len(),
        }
    }

    /// The annotation box's painted text length — what
    /// `render_annotation_box` builds.
    fn annotation_display_len(&self) -> usize {
        let mut text = self.transcript.annotation_text.clone();
        append_word_text(
            &mut text,
            &strip_interim_terminators(&self.transcript.annotation_interim),
        );
        text.len()
    }

    /// A slot's fresh spans as `(byte offset, alpha)` boundaries — a run
    /// from each offset to the next boundary paints at that alpha while
    /// its [`WORD_FADE`] runs.
    fn fade_boundaries(&self, key: &str, now: Instant) -> Vec<(usize, f32)> {
        self.text_fades
            .get(key)
            .map(|fade| {
                fade.fresh
                    .iter()
                    .map(|&(start, at)| {
                        let t = (now.duration_since(at).as_secs_f32()
                            / WORD_FADE.as_secs_f32())
                        .min(1.0);
                        (start, ease_out_quint()(t))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The transition a row should paint for `target`, if one's in
    /// flight — `Some` only while the buffer still matches the swap.
    fn cleanup_morph(&self, target: CleanTarget) -> Option<&CleanupMorph> {
        self.cleanup_morphs
            .iter()
            .find(|morph| morph.target == target)
    }

    /// Retire finished morphs and any whose buffer moved on since the
    /// answer landed.
    fn prune_cleanup_morphs(&mut self, now: Instant) {
        self.cleanup_morphs.retain(|morph| {
            now.duration_since(morph.started) < CLEANUP_MORPH
                && self.transcript.clean_target_text(morph.target) == morph.new
        });
    }

    /// Whether any morph, crossfade, or word-fade still needs repaint
    /// ticks.
    fn motion_live(&self) -> bool {
        !self.cleanup_morphs.is_empty()
            || self.interim_crossfade.is_some()
            || self.text_fades.values().any(|fade| !fade.fresh.is_empty())
    }

    /// What the append point paints this frame — the open box's composed
    /// text, else the current paragraph's text plus its interim tail
    /// joined the way `scratchpad_paragraph_text` joins it (the live
    /// row while no paragraph exists yet).
    fn painted_append_row(&self) -> PaintedRow {
        let transcript = &self.transcript;
        if transcript.annotation_target.is_some() {
            let mut painted = transcript.annotation_text.clone();
            let tail = strip_interim_terminators(&transcript.annotation_interim);
            append_word_text(&mut painted, &tail);
            return PaintedRow {
                key: "annotation".to_owned(),
                settled: transcript.annotation_text.len(),
                tail_start: painted.len() - tail.len(),
                painted,
                edited: false,
            };
        }
        let index = transcript.paragraphs.len().saturating_sub(1);
        let paragraph = transcript.paragraphs.last();
        let text = paragraph.map(|paragraph| paragraph.text.as_str()).unwrap_or("");
        let mut painted = text.to_owned();
        let tail = strip_interim_terminators(&transcript.interim);
        append_word_text(&mut painted, &tail);
        PaintedRow {
            key: format!("p{index}"),
            settled: text.len(),
            tail_start: painted.len() - tail.len(),
            painted,
            edited: paragraph.is_some_and(|paragraph| paragraph.edited),
        }
    }

    /// The row `key` paints now — its settled text plus the interim tail
    /// when the append point still lives there. `None` when the row is
    /// gone: a closed box or a discarded paragraph.
    fn painted_row_for(&self, key: &str) -> Option<PaintedRow> {
        let painted = self.painted_append_row();
        if painted.key == key {
            return Some(painted);
        }
        let index = key.strip_prefix('p')?.parse::<usize>().ok()?;
        let paragraph = self.transcript.paragraphs.get(index)?;
        Some(PaintedRow {
            key: key.to_owned(),
            settled: paragraph.text.len(),
            tail_start: paragraph.text.len(),
            painted: paragraph.text.clone(),
            edited: paragraph.edited,
        })
    }

    /// Diff the append point's painted row against last frame's record —
    /// a delivery that grew settled text over painted gray is a landing
    /// and gets its crossfade. The landing can close the interim's row
    /// behind it ("okay next" commits the paragraph while the tail moves
    /// on), so the diff follows the record's own key rather than the
    /// live append point. The record refreshes every frame either way,
    /// motion or not, so a stale row can't mint a landing later.
    fn note_painted_row(&mut self, animate: bool, now: Instant) {
        let painted = self.painted_append_row();
        let same_row = self
            .painted_row
            .as_ref()
            .is_some_and(|prev| prev.key == painted.key);
        let prev = self.painted_row.replace(painted);
        if !animate {
            return;
        }
        let Some(prev) = prev else {
            return;
        };
        let current_row;
        let current = if same_row {
            self.painted_row.as_ref().expect("stored this pass")
        } else {
            current_row = self.painted_row_for(&prev.key);
            match current_row.as_ref() {
                Some(row) => row,
                None => return,
            }
        };
        landing_crossfade(
            &mut self.text_fades,
            &mut self.interim_crossfade,
            prev,
            current,
            now,
        );
    }

    /// Pause the capture side: the worker exits and closes its socket,
    /// its generation retires so late events and a credential fetch in
    /// flight both land nowhere, and audio queued for the dead stream
    /// drops rather than leaking into the reconnect. The transcript and
    /// panel state survive — resume reconnects through `muted`.
    fn stop_capture(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.generation = self.generation.wrapping_add(1);
        while self.audio_rx.try_recv().is_ok() {}
        self.capture_live = false;
    }
}

/// A cleanup transition in flight on one buffer: `new` must still match
/// the painted text — a mismatch means the buffer moved past the answer
/// and the morph is stale.
struct CleanupMorph {
    target: CleanTarget,
    /// The buffer's text before the swap — it lifts and dissolves.
    old: String,
    /// The buffer's text after — it resolves in underneath.
    new: String,
    started: Instant,
}

/// One painted slot's word-fade bookkeeping.
struct TextFade {
    /// Buffer length already painted settled — growth past it is fresh.
    settled: usize,
    /// Spans appended inside their [`WORD_FADE`]: `(byte offset, landed)`.
    fresh: Vec<(usize, Instant)>,
}

/// Diff one painted buffer's length for the word-fade: growth past the
/// settled length marks a fresh span, a shrink clears them — rewrites
/// and user-typed text (`fade_in` false) land instantly. Free function
/// so the render prep pass can call it on the field while the transcript
/// stays borrowed.
fn note_text_fade(
    fades: &mut HashMap<String, TextFade>,
    key: String,
    len: usize,
    fade_in: bool,
    now: Instant,
) {
    let entry = fades.entry(key).or_insert_with(|| TextFade {
        settled: len,
        fresh: Vec::new(),
    });
    entry
        .fresh
        .retain(|(_, at)| now.duration_since(*at) < WORD_FADE);
    if len < entry.settled {
        entry.fresh.clear();
    } else if len > entry.settled && fade_in {
        entry.fresh.push((entry.settled, now));
    }
    entry.settled = len;
}

/// A painted append-point row's composition: settled text plus the
/// stripped interim tail appended the way the row joins it — the shape
/// a landing's crossfade diffs.
struct PaintedRow {
    /// The fade key of the row described — "p{n}" or "annotation".
    key: String,
    /// The settled text's byte length inside `painted`; the interim
    /// tail (join space included) follows it.
    settled: usize,
    /// Where the tail's visible bytes start inside `painted`.
    tail_start: usize,
    /// The whole painted string.
    painted: String,
    /// A user edit touched this buffer — its landings stay instant.
    edited: bool,
}

/// A partial→final landing mid-crossfade: `ghost` (the row's painted
/// text before the delivery) dissolves in place while the divergent
/// residual resolves beneath it — matched bytes never left the screen,
/// so nothing blinks out and back in.
struct InterimCrossfade {
    /// The fade key of the row it belongs to.
    key: String,
    /// What the row painted before the landing — dissolves as an
    /// overlay pinned to the row's text block.
    ghost: String,
    /// Byte offset in `ghost` where its gray tail began — the settled
    /// head dissolves in ink, the tail in the interim's gray.
    tail_start: usize,
    started: Instant,
}

/// `current` is the row `prev` recorded, painted now. Settled growth
/// that kept the settled prefix and had a gray tail to displace is a
/// landing: the displaced paint dissolves while the divergent residual
/// resolves. The residual's first byte rewrites the landing's own fresh
/// spans — appended at `prev.settled` — so matched text resolves
/// already visible instead of fading in from nothing. Free function so
/// the prep pass can call it on fields while the transcript stays
/// borrowed.
fn landing_crossfade(
    fades: &mut HashMap<String, TextFade>,
    crossfade: &mut Option<InterimCrossfade>,
    prev: PaintedRow,
    current: &PaintedRow,
    now: Instant,
) {
    if current.settled <= prev.settled
        || current.edited
        || prev.tail_start >= prev.painted.len()
        || current.painted.as_bytes()[..prev.settled]
            != prev.painted.as_bytes()[..prev.settled]
    {
        return;
    }
    // The byte the landing's paint first diverges at — everything
    // before it was already on screen.
    let mut residual = prev.settled
        + prev.painted.as_bytes()[prev.settled..]
            .iter()
            .zip(&current.painted.as_bytes()[prev.settled..])
            .take_while(|(a, b)| a == b)
            .count();
    while !current.painted.is_char_boundary(residual) {
        residual -= 1;
    }
    // Spans the growth pushed are the landing's, not appends — drop
    // them wherever the slot recorded them so the matched prefix never
    // starts a fade.
    if let Some(entry) = fades.get_mut(&prev.key) {
        entry.fresh.retain(|&(start, _)| start < prev.settled);
    }
    // The residual resumes the resolve at its divergent byte: inside
    // the settled text on its own slot, inside the tail on the interim
    // slot (the box diffs its composed string, so its offsets are the
    // painted ones throughout).
    let (slot, local, region) = if prev.key == "annotation" {
        ("annotation", residual, current.painted.len())
    } else if residual < current.settled {
        (prev.key.as_str(), residual, current.settled)
    } else {
        (
            "interim",
            residual.saturating_sub(current.tail_start),
            current.painted.len().saturating_sub(current.tail_start),
        )
    };
    if local < region
        && let Some(entry) = fades.get_mut(slot)
    {
        entry.fresh.push((local, now));
    }
    *crossfade = Some(InterimCrossfade {
        key: prev.key,
        ghost: prev.painted,
        tail_start: prev.tail_start,
        started: now,
    });
}

/// The fade slot a cleanup target maps to — the same key the prep pass
/// diffs that buffer under.
fn scratchpad_fade_key(target: CleanTarget) -> String {
    match target {
        CleanTarget::Annotation => "annotation".to_owned(),
        CleanTarget::Node(ScratchpadNode::Paragraph(index)) => format!("p{index}"),
        CleanTarget::Node(ScratchpadNode::Bullet(index, bullet)) => {
            format!("b{index}-{bullet}")
        }
    }
}

/// One dictated paragraph and the bullet annotations parked under it.
/// `bullets` stays a flat list in paint order — `depth` counts how far a
/// row nests under the bullet that spawned it.
#[derive(Default)]
struct ScratchpadParagraph {
    text: String,
    bullets: Vec<ScratchpadBullet>,
    /// A manual edit touched this paragraph — the cleanup model leaves
    /// its spans alone rather than overwrite the user's words.
    edited: bool,
}

/// One annotation row — `depth` 0 hangs off the paragraph itself, each
/// bullet-annotated commit nests one deeper up to [`MAX_BULLET_DEPTH`].
#[derive(Clone, Debug)]
struct ScratchpadBullet {
    text: String,
    depth: usize,
}

/// Test assertions compare a bullet list against bare strings.
impl PartialEq<&str> for ScratchpadBullet {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

/// The flat index just past `bullet`'s whole subtree — where a child of
/// it lands.
fn descendant_end(bullets: &[ScratchpadBullet], bullet: usize) -> usize {
    let depth = bullets[bullet].depth;
    bullet
        + 1
        + bullets[bullet + 1..]
            .iter()
            .take_while(|next| next.depth > depth)
            .count()
}

/// Where an open annotation box writes: a paragraph's bullet list as a
/// whole, or — after a bullet click — the child slot at the end of that
/// bullet's subtree, advancing once per commit so consecutive "okay
/// next" bullets land in order under it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AnnotationTarget {
    Paragraph(usize),
    Bullet {
        paragraph: usize,
        bullet: usize,
        insert: usize,
    },
}

/// A spoken command the dictation fold consumes.
enum ScratchpadCommand {
    /// "Okay next" — commit the append point and move on.
    Next,
    /// "Okay, let's make an edit" — arm the annotation box's rewrite.
    Edit,
}

/// A transcript buffer a caret or edit addresses — a paragraph's body or
/// one of its bullets. Painted element keys map back onto these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScratchpadNode {
    Paragraph(usize),
    Bullet(usize, usize),
}

/// A byte offset into one node's committed text.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CaretPos {
    node: ScratchpadNode,
    offset: usize,
}

/// Rolling cap on `finalized_tail` — dedup only needs the stream's recent
/// end.
const FINALIZED_TAIL_CAP: usize = 16 * 1024;

/// The scratchpad's text model: ordered paragraphs, per-paragraph bullets,
/// the live interim suffix, and the annotation box's target and content.
#[derive(Default)]
pub(super) struct ScratchpadTranscript {
    paragraphs: Vec<ScratchpadParagraph>,
    /// The node an open annotation box writes into.
    annotation_target: Option<AnnotationTarget>,
    /// Finalized speech currently inside the open annotation box.
    annotation_text: String,
    /// Interim speech while annotating — provisional until it settles.
    annotation_interim: String,
    /// Armed by a spoken "okay, let's make an edit" in a paragraph's box:
    /// `(paragraph, offset)` — the byte offset into `annotation_text`
    /// where the instruction begins. Dictation past it rewrites that
    /// paragraph at commit instead of landing as a bullet.
    annotation_edit: Option<(usize, usize)>,
    /// Interim speech at the main append point.
    interim: String,
    /// When the live interim last changed on screen — the hysteresis
    /// clock a shrinking partial's hold runs against. One stamp covers
    /// whichever slot is live; only one is painted at a time.
    interim_written: Option<Instant>,
    /// The caret the surface paints — set by a selection edit, an arrow
    /// press, or a click on an unwritten line. It marks where dictation
    /// lands, never a typing target: keys only edit through a selection.
    caret: Option<CaretPos>,
    /// The fixed end of a shift-grown selection; `None` for mouse drags,
    /// which the `Selection` anchors itself.
    caret_anchor: Option<CaretPos>,
    /// The raw finalized stream's recent tail. `transcript-final` dedup
    /// compares against it — the folded text can't serve, since the
    /// command filter removes "okay next" spans before they land.
    finalized_tail: String,
    /// Words committed early out of interim partials — a spoken "okay
    /// next" folds its span the moment the recognizer emits it, ahead
    /// of the finalized delivery. The stream re-delivers those words
    /// later, so finals and partials both strip this credit rather
    /// than append the span twice or fire the command again.
    interim_folded: VecDeque<String>,
    /// A punctuation-only chunk just glued or dropped — the recognizer's
    /// stray emission after a pause. The next delivery's leading
    /// punctuation run is the same emission spilling over, so it strips
    /// before anything lands.
    stray_punct: bool,
    /// The current paragraph's byte offset where uncleaned dictation
    /// begins — finished sentences queue from here once the next one
    /// opens, so the live tail is never rewritten mid-flight.
    main_clean_from: usize,
    /// The same boundary inside the open annotation box's text.
    annotation_clean_from: usize,
    /// Finished spans waiting for the cleanup model — drained by the
    /// event pump into background gateway calls.
    cleanup_requests: Vec<CleanupRequest>,
    /// Spans whose cleanup call has gone out — each clears when its
    /// `Cleaned` event lands, answer or failure. The spinner at a
    /// cleaning node's tail reads this.
    cleanup_inflight: Vec<CleanupRequest>,
}

impl ScratchpadTranscript {
    /// The paragraph new speech appends to — created lazily so an empty
    /// session owns no visible rows.
    fn current(&mut self) -> &mut ScratchpadParagraph {
        if self.paragraphs.is_empty() {
            self.paragraphs.push(ScratchpadParagraph::default());
        }
        self.paragraphs
            .last_mut()
            .expect("a paragraph always exists")
    }

    /// Fold one finalized segment into the transcript, consuming any "okay
    /// next" commands it carries. While a paragraph is annotated the command
    /// commits the box as a bullet and keeps the box open for the next one;
    /// otherwise it closes the current paragraph.
    fn append_finalized(&mut self, segment: &str) {
        // Words an interim fold already committed arrive again here —
        // strip them so the early "okay next" split can't double-append.
        let rest = self.strip_folded(segment, true);
        // The delivery supersedes the interim's leading words; its
        // still-provisional tail stays dimmed rather than flashing away.
        self.retract_interim(rest, rest.len() != segment.len());
        self.finalized_tail.push_str(segment);
        if self.finalized_tail.len() > FINALIZED_TAIL_CAP {
            let cut = self.finalized_tail.len() - FINALIZED_TAIL_CAP;
            let boundary = (cut..self.finalized_tail.len())
                .find(|&i| self.finalized_tail.is_char_boundary(i))
                .unwrap_or(self.finalized_tail.len());
            self.finalized_tail.drain(..boundary);
        }
        let mut rest = rest;
        loop {
            // The command can straddle this chunk's leading edge — the
            // append point's trailing word pairs with the chunk's first.
            if let Some(after) = seam_command(self.append_point_text(), rest) {
                self.strip_append_point_word();
                self.commit_next();
                rest = after;
                continue;
            }
            if self.edit_armable()
                && let Some((taken, after)) =
                    seam_edit_command(self.append_point_text(), rest)
            {
                self.strip_append_point_words(taken);
                self.arm_edit();
                rest = after;
                continue;
            }
            match self.split_command(rest) {
                None => {
                    self.push_text(rest);
                    break;
                }
                Some((ScratchpadCommand::Next, before, after)) => {
                    self.push_text(before);
                    self.commit_next();
                    rest = after;
                }
                Some((ScratchpadCommand::Edit, before, after)) => {
                    self.push_text(before);
                    self.arm_edit();
                    rest = after;
                }
            }
        }
        self.flush_completed_sentences();
    }

    /// Queue the append point's finished sentences for cleanup once a
    /// following sentence has started — the trailing open sentence stays
    /// raw so the live text is never rewritten under the user.
    fn flush_completed_sentences(&mut self) {
        let (from, target) = if self.annotation_target.is_some() {
            (self.annotation_clean_from, CleanTarget::Annotation)
        } else {
            let Some(index) = self.paragraphs.len().checked_sub(1) else {
                return;
            };
            (
                self.main_clean_from,
                CleanTarget::Node(ScratchpadNode::Paragraph(index)),
            )
        };
        let text = self.append_point_text();
        let from = from.min(text.len());
        let Some(end) = completed_sentence_end(&text[from..]).map(|end| from + end) else {
            return;
        };
        if let Some((start, raw)) = clean_span(text, from, end) {
            self.cleanup_requests.push(CleanupRequest {
                target,
                start,
                raw,
                edit: None,
            });
        }
        if self.annotation_target.is_some() {
            self.annotation_clean_from = end;
        } else {
            self.main_clean_from = end;
        }
    }

    /// Queue whatever the closing append point still holds — the whole
    /// pending tail is finished dictation once speech moves on. Paragraph
    /// commits call this before pushing the fresh paragraph; annotation
    /// commits take the box's text directly into their bullet request.
    fn flush_open_tail(&mut self) {
        if self.annotation_target.is_some() {
            let from = self.annotation_clean_from.min(self.annotation_text.len());
            self.annotation_clean_from = self.annotation_text.len();
            let request = clean_span(&self.annotation_text, from, self.annotation_text.len()).map(
                |(start, raw)| CleanupRequest {
                    target: CleanTarget::Annotation,
                    start,
                    raw,
                    edit: None,
                },
            );
            if let Some(request) = request {
                self.cleanup_requests.push(request);
            }
            return;
        }
        let Some(index) = self.paragraphs.len().checked_sub(1) else {
            return;
        };
        let from = self.main_clean_from.min(self.paragraphs[index].text.len());
        self.main_clean_from = self.paragraphs[index].text.len();
        let request = clean_span(
            &self.paragraphs[index].text,
            from,
            self.paragraphs[index].text.len(),
        )
        .map(|(start, raw)| CleanupRequest {
            target: CleanTarget::Node(ScratchpadNode::Paragraph(index)),
            start,
            raw,
            edit: None,
        });
        if let Some(request) = request {
            self.cleanup_requests.push(request);
        }
    }

    /// The text at the append point new speech writes into — the open
    /// annotation box's content, else the current paragraph's text.
    fn append_point_text(&self) -> &str {
        if self.annotation_target.is_some() {
            &self.annotation_text
        } else {
            self.paragraphs
                .last()
                .map(|paragraph| paragraph.text.as_str())
                .unwrap_or_default()
        }
    }

    /// The append point has no word a punctuation chunk can attach to —
    /// it's empty (a fresh paragraph or annotation box) or already
    /// closed by terminal punctuation.
    fn append_point_bare(&self) -> bool {
        self.append_point_text()
            .trim_end()
            .chars()
            .next_back()
            .is_none_or(is_sentence_punct)
    }

    /// Remove the append point's trailing word — a command's first half
    /// consumed it — along with any punctuation and whitespace that rode
    /// along.
    fn strip_append_point_word(&mut self) {
        let text = if self.annotation_target.is_some() {
            &mut self.annotation_text
        } else {
            match self.paragraphs.last_mut() {
                Some(paragraph) => &mut paragraph.text,
                None => return,
            }
        };
        let word_start = text
            .trim_end_matches(|c: char| !c.is_alphanumeric())
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_alphanumeric())
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        text.truncate(word_start);
        text.truncate(text.trim_end().len());
    }

    /// Remove the append point's trailing `count` words — a multi-word
    /// command consumed them — each with the punctuation it carried.
    fn strip_append_point_words(&mut self, count: usize) {
        for _ in 0..count {
            self.strip_append_point_word();
        }
    }

    /// Whether the edit phrase can arm right now: a paragraph's
    /// annotation box is open and hasn't already heard it. On a bullet's
    /// box or the live row the same words are plain speech.
    fn edit_armable(&self) -> bool {
        matches!(self.annotation_target, Some(AnnotationTarget::Paragraph(_)))
            && self.annotation_edit.is_none()
    }

    /// The edit phrase just consumed — dictation past this point is the
    /// rewrite instruction for the annotated paragraph, recorded as the
    /// split offset inside the box's text.
    fn arm_edit(&mut self) {
        if let Some(AnnotationTarget::Paragraph(index)) = self.annotation_target {
            self.annotation_edit = Some((index, self.annotation_text.len()));
        }
    }

    /// The earliest voice command in `text` — "okay next" anywhere, plus
    /// "okay, let's make an edit" while it can still arm. Returns which
    /// fired and the text on either side of its consumed span.
    fn split_command<'a>(
        &self,
        text: &'a str,
    ) -> Option<(ScratchpadCommand, &'a str, &'a str)> {
        let mut hit = next_command_span(text).map(|span| (ScratchpadCommand::Next, span));
        if self.edit_armable()
            && let Some(span) = edit_command_span(text)
            && hit.as_ref().is_none_or(|(_, (start, _))| span.0 < *start)
        {
            hit = Some((ScratchpadCommand::Edit, span));
        }
        let (command, (start, end)) = hit?;
        Some((
            command,
            text[..start].trim_end(),
            text[end..].trim_start_matches(|c: char| !c.is_alphanumeric()),
        ))
    }

    /// A `transcript-final` part: the segment's complete text. Models that
    /// also emit deltas repeat themselves here — the raw stream's tail is
    /// the dedup key, since the command filter strips "okay next" spans
    /// out of what reaches paragraphs.
    fn apply_final_segment(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.tail_delivered(text) {
            // A fully re-delivered segment drains any folded credit the
            // delivery skipped over — leftover words would sit at the
            // queue's front stripping unrelated speech later.
            self.interim_folded.clear();
            self.retract_interim(text, false);
        } else {
            // Deltas may have committed the segment's head already —
            // strip that overlap before the fold sees it.
            let rest = self.strip_delivered(text);
            if rest.trim().is_empty() {
                self.interim_folded.clear();
                self.retract_interim(text, false);
            } else {
                self.append_finalized(rest);
            }
        }
    }

    /// Whether the raw finalized stream already delivered `text` — a
    /// whitespace-insensitive suffix compare, since delta chunking decides
    /// where spaces land.
    fn tail_delivered(&self, text: &str) -> bool {
        let text_words: Vec<&str> = text.split_whitespace().collect();
        let tail_words: Vec<&str> = self.finalized_tail.split_whitespace().collect();
        text_words.len() <= tail_words.len()
            && tail_words[tail_words.len() - text_words.len()..] == text_words[..]
    }

    /// The provisional suffix at the active append point. Each partial
    /// replaces it whole — after dropping words the finalized stream or
    /// an earlier fold already owns, and after consuming any "okay next"
    /// the recognizer has emitted: the paragraph break lands when the
    /// command is spoken, not when the segment finalizes.
    fn set_interim(&mut self, text: String) {
        self.set_interim_at(text, Instant::now());
    }

    /// `set_interim` stamped by the caller — tests drive the dwell clock.
    fn set_interim_at(&mut self, text: String, now: Instant) {
        // Whether the processing below moves committed text — a consumed
        // command or folded span commits words out of the partial, and a
        // hold must never leave a consumed tail painted.
        let append_shape = (self.paragraphs.len(), self.append_point_text().len());
        let mut rest = self.strip_delivered(&text);
        rest = self.strip_folded(rest, false);
        loop {
            // The command can straddle the seam between committed text
            // and the partial — the same check the finalized fold runs.
            if let Some(after) = seam_command(self.append_point_text(), rest) {
                self.strip_append_point_word();
                self.commit_next();
                self.fold_span(&rest[..rest.len() - after.len()]);
                rest = after;
                continue;
            }
            if self.edit_armable()
                && let Some((taken, after)) =
                    seam_edit_command(self.append_point_text(), rest)
            {
                self.strip_append_point_words(taken);
                self.arm_edit();
                self.fold_span(&rest[..rest.len() - after.len()]);
                rest = after;
                continue;
            }
            match self.split_command(rest) {
                None => break,
                Some((command, before, after)) => {
                    self.push_text(before);
                    match command {
                        ScratchpadCommand::Next => self.commit_next(),
                        ScratchpadCommand::Edit => self.arm_edit(),
                    }
                    self.fold_span(&rest[..rest.len() - after.len()]);
                    rest = after;
                }
            }
        }
        // Stray punctuation obeys the finalized rule: it stays
        // provisional only glued onto the speech it trails — at a fresh
        // append point it drops, and a following delivery's leading run
        // strips as the same emission. A wordless partial keeps the
        // strip armed — the stray may still be spilling over.
        if is_punct_only(rest) {
            self.stray_punct = true;
            if self.append_point_bare() {
                rest = "";
            }
        } else {
            if self.stray_punct || self.append_point_bare() {
                rest = strip_leading_punct(rest);
            }
            if !rest.is_empty() {
                self.stray_punct = false;
            }
        }
        let rest = rest.to_owned();
        let settled = append_shape == (self.paragraphs.len(), self.append_point_text().len());
        let slot = if self.annotation_target.is_some() {
            &mut self.annotation_interim
        } else {
            &mut self.interim
        };
        if *slot == rest {
            return;
        }
        // Hysteresis: a partial that only clips the shown tail or shuffles
        // its trailing punctuation is the recognizer's token jitter — hold
        // the painted text through the dwell rather than repaint-flicker.
        // Committed text moving under the partial, growth, and real
        // rewrites all still land immediately.
        let held = settled
            && self
                .interim_written
                .is_some_and(|written| now.duration_since(written) < INTERIM_DWELL)
            && interim_hold_covers(slot, &rest);
        if held {
            return;
        }
        *slot = rest;
        self.interim_written = Some(now);
    }

    /// Commit the provisional suffix in place — muting and the silence
    /// auto-stop keep words already dictated instead of stranding them
    /// gray. Both retire the stream that heard them, so the fold credit
    /// it owed dies with it: a reconnect owes no re-delivery, and stale
    /// credit would only eat the next session's opening words.
    fn solidify_interim(&mut self) {
        self.interim_folded.clear();
        let interim = if self.annotation_target.is_some() {
            std::mem::take(&mut self.annotation_interim)
        } else {
            std::mem::take(&mut self.interim)
        };
        if interim.is_empty() {
            return;
        }
        self.push_text(&interim);
        // Committed interim is finished dictation too — any sentence it
        // closed queues for cleanup the same way a finalized delivery's
        // does. The still-open tail stays raw.
        self.flush_completed_sentences();
    }

    /// Drop `text`'s leading words the finalized stream already owns.
    /// Partials repeat the provisional span and `transcript-final`
    /// repeats deltas, so an incoming chunk's head can overlap
    /// `finalized_tail`'s end — the largest such overlap goes.
    fn strip_delivered<'a>(&self, text: &'a str) -> &'a str {
        let spans = word_spans(text);
        let tail = word_spans(&self.finalized_tail);
        for start in 0..tail.len() {
            let overlap = tail.len() - start;
            if overlap > spans.len() {
                continue;
            }
            if (0..overlap).all(|i| {
                text[spans[i].0..spans[i].1].eq_ignore_ascii_case(
                    &self.finalized_tail[tail[start + i].0..tail[start + i].1],
                )
            }) {
                return text[spans[overlap - 1].1..].trim_start();
            }
        }
        text
    }

    /// Drop `text`'s leading words that interim folds already committed.
    /// Finalized deliveries drain the credit; partials only peek — the
    /// folded words still owe the stream a delivery.
    fn strip_folded<'a>(&mut self, text: &'a str, drain: bool) -> &'a str {
        let spans = word_spans(text);
        let mut matched = 0;
        while matched < spans.len()
            && matched < self.interim_folded.len()
            && self.interim_folded[matched]
                .eq_ignore_ascii_case(&text[spans[matched].0..spans[matched].1])
        {
            matched += 1;
        }
        if drain {
            self.interim_folded.drain(..matched);
        }
        if matched == 0 {
            text
        } else {
            text[spans[matched - 1].1..].trim_start()
        }
    }

    /// Record a partial span committed into the transcript — the
    /// finalized stream re-delivers it, so the words become strip
    /// credit against the next deliveries.
    fn fold_span(&mut self, span: &str) {
        for (start, end) in word_spans(span) {
            self.interim_folded.push_back(span[start..end].to_owned());
        }
    }

    /// A delivery superseded the interim's leading words — drop the
    /// matched prefix and keep the still-provisional tail dimmed
    /// instead of flashing the whole suffix away between a final and
    /// the next partial. A mismatched head means the recognizer
    /// revised the suffix (clear it), unless folded credit says this
    /// delivery hasn't reached the provisional span yet.
    fn retract_interim(&mut self, delivered: &str, folded_drained: bool) {
        let slot = if self.annotation_target.is_some() {
            &mut self.annotation_interim
        } else {
            &mut self.interim
        };
        if slot.is_empty() {
            return;
        }
        // A punctuation-only delivery superseded nothing worded — it
        // only retires the provisional twin of the same stray emission.
        if is_punct_only(delivered) && !is_punct_only(slot) {
            return;
        }
        let spans = word_spans(slot);
        let delivered_spans = word_spans(delivered);
        let mut matched = 0;
        while matched < spans.len()
            && matched < delivered_spans.len()
            && slot[spans[matched].0..spans[matched].1].eq_ignore_ascii_case(
                &delivered[delivered_spans[matched].0..delivered_spans[matched].1],
            )
        {
            matched += 1;
        }
        if matched > 0 {
            let kept = slot[spans[matched - 1].1..].trim_start().to_owned();
            *slot = kept;
        } else if !folded_drained {
            slot.clear();
        }
    }

    /// Append plain speech to the open box or the current paragraph. A
    /// chunk that's punctuation alone is the recognizer's stray emission
    /// after a pause: it glues onto the speech it trails, or drops at a
    /// fresh append point — and either way the next chunk's leading
    /// punctuation run strips with it.
    fn push_text(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let bare = self.append_point_bare();
        if is_punct_only(text) {
            if !bare {
                if self.annotation_target.is_some() {
                    append_word_text(&mut self.annotation_text, text);
                } else {
                    append_word_text(&mut self.current().text, text);
                }
            }
            self.stray_punct = true;
            return;
        }
        let text = if self.stray_punct || bare {
            strip_leading_punct(text)
        } else {
            text
        };
        self.stray_punct = false;
        if self.annotation_target.is_some() {
            append_word_text(&mut self.annotation_text, text);
        } else {
            append_word_text(&mut self.current().text, text);
        }
    }

    /// "Okay next": inside an annotation box it commits the box's content
    /// as a bullet at the box's slot — under its paragraph, or nested
    /// under its bullet — and the box reopens empty; otherwise it closes the
    /// current paragraph — a redundant command on an empty one moves
    /// nothing. The provisional suffix survives: it sits past the
    /// consumed command and belongs to the new append point.
    fn commit_next(&mut self) {
        match self.annotation_target {
            // An armed edit ends the box on the first commit — the
            // instruction tail dispatches the paragraph rewrite instead
            // of landing as a bullet.
            Some(_) if self.annotation_edit.is_some() => self.commit_annotation(),
            Some(AnnotationTarget::Paragraph(target)) => {
                let bullet = std::mem::take(&mut self.annotation_text).trim().to_owned();
                self.annotation_clean_from = 0;
                if !bullet.is_empty()
                    && let Some(paragraph) = self.paragraphs.get_mut(target)
                {
                    paragraph.bullets.push(ScratchpadBullet {
                        text: bullet.clone(),
                        depth: 0,
                    });
                    // The box's whole text just became a bullet — clean it
                    // where it landed, not in the emptied box.
                    self.cleanup_requests.push(CleanupRequest {
                        target: CleanTarget::Node(ScratchpadNode::Bullet(
                            target,
                            paragraph.bullets.len() - 1,
                        )),
                        start: 0,
                        raw: bullet,
                        edit: None,
                    });
                }
            }
            Some(AnnotationTarget::Bullet {
                paragraph,
                bullet,
                insert,
            }) => {
                let text = std::mem::take(&mut self.annotation_text).trim().to_owned();
                self.annotation_clean_from = 0;
                if !text.is_empty()
                    && let Some(at) = self.land_sub_bullet(paragraph, bullet, insert, text)
                {
                    self.annotation_target = Some(AnnotationTarget::Bullet {
                        paragraph,
                        bullet,
                        insert: at + 1,
                    });
                }
            }
            None => {
                // Speech is moving to a fresh paragraph — the closing
                // one's tail is finished dictation, so it cleans now.
                self.flush_open_tail();
                let current = self.current();
                if !current.text.is_empty() || !current.bullets.is_empty() {
                    self.paragraphs.push(ScratchpadParagraph::default());
                }
                self.main_clean_from = 0;
            }
        }
    }

    /// Open the annotation box on a paragraph; an open box moves. The
    /// provisional suffix retargets with the append point — leaving it
    /// behind would pin stale dimmed text on the last row. Returns true
    /// when the box opened.
    ///
    /// A line with nothing written has nothing to annotate: the click
    /// lands the caret there instead — the dictation insertion point —
    /// after committing any box already open the way clicking open space
    /// would.
    fn annotate(&mut self, index: usize) -> bool {
        if index >= self.paragraphs.len() {
            return false;
        }
        if self.paragraphs[index].text.is_empty() {
            self.commit_annotation();
            self.caret = Some(CaretPos {
                node: ScratchpadNode::Paragraph(index),
                offset: 0,
            });
            self.caret_anchor = None;
            return false;
        }
        // Speech retargets into the box — the closing append point's
        // tail is finished dictation, so it cleans now.
        self.flush_open_tail();
        self.annotation_target = Some(AnnotationTarget::Paragraph(index));
        self.annotation_interim = std::mem::take(&mut self.interim);
        self.caret = None;
        self.caret_anchor = None;
        true
    }

    /// Open the annotation box on a bullet — its commits land as the
    /// bullet's children, the slot starting at the end of its subtree so
    /// consecutive "okay next" bullets keep speech order under it. The
    /// provisional suffix retargets with the append point, same as a
    /// paragraph box.
    fn annotate_bullet(&mut self, paragraph: usize, bullet: usize) {
        if let Some(target) = self.paragraphs.get(paragraph)
            && bullet < target.bullets.len()
        {
            let insert = descendant_end(&target.bullets, bullet);
            self.flush_open_tail();
            self.annotation_target = Some(AnnotationTarget::Bullet {
                paragraph,
                bullet,
                insert,
            });
            self.annotation_interim = std::mem::take(&mut self.interim);
            self.caret = None;
            self.caret_anchor = None;
        }
    }

    /// Close the annotation box — its uncommitted text waits in the box
    /// rather than jumping into the transcript.
    fn exit_annotation(&mut self) {
        self.annotation_target = None;
        self.annotation_interim.clear();
    }

    /// Finish annotating the way "okay next" would — the box's content
    /// lands as a bullet at its slot, or an armed edit dispatches its
    /// instruction tail as the paragraph's rewrite — then close, leaving
    /// the append point back on the live row. The still-provisional tail
    /// goes in with it, folded as strip credit so the stream's
    /// re-delivery of those words doesn't append them a second time.
    fn commit_annotation(&mut self) {
        let Some(target) = self.annotation_target.take() else {
            return;
        };
        let text = std::mem::take(&mut self.annotation_text);
        let interim = std::mem::take(&mut self.annotation_interim)
            .trim()
            .to_owned();
        self.fold_span(&interim);
        self.annotation_clean_from = 0;
        // An armed edit splits the box at the phrase: the tail is the
        // rewrite instruction for its paragraph — it dispatches to the
        // cleanup pipeline and never lands as a bullet. Words spoken
        // before the phrase still commit at the box's slot.
        let (text, edit) = match self.annotation_edit.take() {
            Some((paragraph, from)) => {
                let mut instruction = text.get(from..).unwrap_or_default().to_owned();
                append_word_text(&mut instruction, &interim);
                (
                    text.get(..from).unwrap_or_default().to_owned(),
                    Some((paragraph, instruction)),
                )
            }
            None => {
                let mut text = text;
                append_word_text(&mut text, &interim);
                (text, None)
            }
        };
        if let Some((index, instruction)) = edit {
            let instruction = instruction.trim();
            if !instruction.is_empty()
                && let Some(paragraph) = self.paragraphs.get(index)
                && !paragraph.text.is_empty()
            {
                // The rewrite rides the cleanup path whole — the answer
                // verifies the paragraph still holds this exact text
                // before it lands with the morph.
                self.cleanup_requests.push(CleanupRequest {
                    target: CleanTarget::Node(ScratchpadNode::Paragraph(index)),
                    start: 0,
                    raw: paragraph.text.clone(),
                    edit: Some(instruction.to_owned()),
                });
            }
        }
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        match target {
            AnnotationTarget::Paragraph(index) => {
                if let Some(paragraph) = self.paragraphs.get_mut(index) {
                    paragraph.bullets.push(ScratchpadBullet {
                        text: text.clone(),
                        depth: 0,
                    });
                    // Click-out commits the same bullet "okay next" would —
                    // clean it where it landed.
                    self.cleanup_requests.push(CleanupRequest {
                        target: CleanTarget::Node(ScratchpadNode::Bullet(
                            index,
                            paragraph.bullets.len() - 1,
                        )),
                        start: 0,
                        raw: text,
                        edit: None,
                    });
                }
            }
            AnnotationTarget::Bullet {
                paragraph,
                bullet,
                insert,
            } => {
                self.land_sub_bullet(paragraph, bullet, insert, text);
            }
        }
    }

    /// Land `text` as a child of `paragraph`'s `bullet` at the box's
    /// `insert` slot — one level deeper, capped at [`MAX_BULLET_DEPTH`],
    /// where further commits keep landing as siblings. Returns where it
    /// landed, `None` when the paragraph is gone.
    fn land_sub_bullet(
        &mut self,
        paragraph: usize,
        parent: usize,
        insert: usize,
        text: String,
    ) -> Option<usize> {
        let target = self.paragraphs.get_mut(paragraph)?;
        let depth = target
            .bullets
            .get(parent)
            .map(|parent| (parent.depth + 1).min(MAX_BULLET_DEPTH))
            .unwrap_or(0);
        let at = insert.min(target.bullets.len());
        target.bullets.insert(
            at,
            ScratchpadBullet {
                text: text.clone(),
                depth,
            },
        );
        self.cleanup_requests.push(CleanupRequest {
            target: CleanTarget::Node(ScratchpadNode::Bullet(paragraph, at)),
            start: 0,
            raw: text,
            edit: None,
        });
        Some(at)
    }

    /// Whether `target` has a cleanup call queued or in flight — the
    /// spinner marker at the node's tail.
    fn is_cleaning(&self, target: CleanTarget) -> bool {
        self.cleanup_requests
            .iter()
            .chain(&self.cleanup_inflight)
            .any(|request| request.target == target)
    }

    /// The buffer a cleanup target rewrites — `apply_cleanup`'s view of
    /// it, for the render layer's morph bookkeeping.
    fn clean_target_text(&self, target: CleanTarget) -> &str {
        match target {
            CleanTarget::Annotation => &self.annotation_text,
            CleanTarget::Node(node) => self.node_text(node),
        }
    }

    /// One node's committed text — `""` for anything not currently painted.
    fn node_text(&self, node: ScratchpadNode) -> &str {
        match node {
            ScratchpadNode::Paragraph(index) => self
                .paragraphs
                .get(index)
                .map(|paragraph| paragraph.text.as_str())
                .unwrap_or_default(),
            ScratchpadNode::Bullet(paragraph, bullet) => self
                .paragraphs
                .get(paragraph)
                .and_then(|paragraph| paragraph.bullets.get(bullet))
                .map(|bullet| bullet.text.as_str())
                .unwrap_or_default(),
        }
    }

    /// The mutable buffer for `node`. `Paragraph(0)` on an empty transcript
    /// lazily becomes the first paragraph — the live row's caret position.
    fn node_text_mut(&mut self, node: ScratchpadNode) -> Option<&mut String> {
        match node {
            ScratchpadNode::Paragraph(index) => {
                if self.paragraphs.is_empty() && index == 0 {
                    self.paragraphs.push(ScratchpadParagraph::default());
                }
                self.paragraphs
                    .get_mut(index)
                    .map(|paragraph| &mut paragraph.text)
            }
            ScratchpadNode::Bullet(paragraph, bullet) => self
                .paragraphs
                .get_mut(paragraph)
                .and_then(|paragraph| paragraph.bullets.get_mut(bullet))
                .map(|bullet| &mut bullet.text),
        }
    }

    /// The append point as a caret: the last paragraph's end, or the empty
    /// session's live row.
    fn append_point_caret(&self) -> CaretPos {
        CaretPos {
            node: ScratchpadNode::Paragraph(self.paragraphs.len().saturating_sub(1)),
            offset: self.paragraphs.last().map_or(0, |paragraph| paragraph.text.len()),
        }
    }

    /// Editable nodes in paint order — paragraph text first, then its
    /// bullets — with each node's committed length.
    fn caret_nodes(&self) -> Vec<(ScratchpadNode, usize)> {
        let mut nodes = Vec::new();
        for (index, paragraph) in self.paragraphs.iter().enumerate() {
            nodes.push((ScratchpadNode::Paragraph(index), paragraph.text.len()));
            for (bullet, row) in paragraph.bullets.iter().enumerate() {
                nodes.push((ScratchpadNode::Bullet(index, bullet), row.text.len()));
            }
        }
        if nodes.is_empty() {
            nodes.push((ScratchpadNode::Paragraph(0), 0));
        }
        nodes
    }

    /// The node before or after `node` in paint order.
    fn neighbor_node(&self, node: ScratchpadNode, forward: bool) -> Option<ScratchpadNode> {
        let nodes = self.caret_nodes();
        let index = nodes.iter().position(|(entry, _)| *entry == node)?;
        if forward {
            nodes.get(index + 1).map(|(entry, _)| *entry)
        } else {
            index.checked_sub(1).map(|index| nodes[index].0)
        }
    }

    /// The selection key `node` paints under — `vs-p-{i}` for a paragraph,
    /// `vs-b-{i}` at index j for a bullet, `vs-p-live` for the empty
    /// session's live row.
    fn node_key(&self, node: ScratchpadNode) -> md::selection::TextKey {
        match node {
            ScratchpadNode::Paragraph(index) => {
                if index < self.paragraphs.len() {
                    md::selection::TextKey::new(format!("vs-p-{index}"), 0)
                } else {
                    md::selection::TextKey::new("vs-p-live", 0)
                }
            }
            ScratchpadNode::Bullet(paragraph, bullet) => {
                md::selection::TextKey::new(format!("vs-b-{paragraph}"), bullet)
            }
        }
    }

    /// The node a painted element's key addresses — the live row reads as
    /// paragraph zero.
    fn node_for_key(&self, key: &md::selection::TextKey) -> Option<ScratchpadNode> {
        let row = key.row.as_ref();
        if let Some(rest) = row.strip_prefix("vs-p-") {
            if rest == "live" {
                return Some(ScratchpadNode::Paragraph(0));
            }
            return rest.parse().ok().map(ScratchpadNode::Paragraph);
        }
        row.strip_prefix("vs-b-")
            .and_then(|rest| rest.parse().ok())
            .map(|paragraph| ScratchpadNode::Bullet(paragraph, key.index))
    }

    /// Move the caret within its node — by grapheme, or by word — crossing
    /// into the neighbor node's edge at the boundary.
    fn move_caret(&self, caret: CaretPos, forward: bool, word: bool) -> CaretPos {
        let text = self.node_text(caret.node);
        let offset = caret.offset.min(text.len());
        let next = if word {
            if forward {
                next_word_boundary(text, offset)
            } else {
                previous_word_boundary(text, offset)
            }
        } else if forward {
            text[offset..]
                .grapheme_indices(true)
                .nth(1)
                .map_or(text.len(), |(index, _)| offset + index)
        } else {
            text[..offset]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(index, _)| index)
        };
        if next != offset {
            return CaretPos {
                offset: next,
                ..caret
            };
        }
        self.neighbor_node(caret.node, forward)
            .map(|node| CaretPos {
                node,
                offset: if forward { 0 } else { self.node_text(node).len() },
            })
            .unwrap_or(caret)
    }

    /// Up/down: step to the adjacent node at roughly the same offset.
    fn step_node(&self, caret: CaretPos, down: bool) -> CaretPos {
        self.neighbor_node(caret.node, down)
            .map(|node| CaretPos {
                node,
                offset: caret.offset.min(self.node_text(node).len()),
            })
            .unwrap_or(caret)
    }

    /// A typed or pasted change touched `node` — flag its paragraph so
    /// the cleanup model never rewrites over the user's words, and drop
    /// the pending cleanup boundary when the live paragraph is the one
    /// that changed.
    fn note_user_edit(&mut self, node: ScratchpadNode) {
        let index = match node {
            ScratchpadNode::Paragraph(index) | ScratchpadNode::Bullet(index, _) => index,
        };
        let is_last = index + 1 == self.paragraphs.len();
        if let Some(paragraph) = self.paragraphs.get_mut(index) {
            paragraph.edited = true;
            if is_last {
                self.main_clean_from = paragraph.text.len();
            }
        }
    }

    /// The model positions at the selection's painted edges — document
    /// start and end — so arrow presses can collapse or extend it.
    fn spans_endpoints(&self, spans: &[md::selection::Span]) -> Option<(CaretPos, CaretPos)> {
        let first = spans.iter().find(|span| !span.range.is_empty())?;
        let last = spans.iter().rfind(|span| !span.range.is_empty())?;
        let start_node = self.node_for_key(&first.key)?;
        let end_node = self.node_for_key(&last.key)?;
        Some((
            CaretPos {
                node: start_node,
                offset: first.range.start.min(self.node_text(start_node).len()),
            },
            CaretPos {
                node: end_node,
                offset: last.range.end.min(self.node_text(end_node).len()),
            },
        ))
    }

    /// Splice `insert` over the selection's painted `spans`: every covered
    /// committed range is cut and `insert` lands where the grab began. A
    /// span reaching past a paragraph's committed text covers painted
    /// interim — the provisional tail drops rather than survive as ghost
    /// bytes. Returns the caret the edit leaves.
    fn apply_selection_edit(
        &mut self,
        spans: &[md::selection::Span],
        insert: &str,
    ) -> Option<CaretPos> {
        if spans.is_empty() {
            return None;
        }
        self.interim.clear();
        self.annotation_interim.clear();
        let mut caret = None;
        let mut edited_nodes = Vec::new();
        for span in spans {
            let Some(node) = self.node_for_key(&span.key) else {
                continue;
            };
            let len = self.node_text(node).len();
            let range = span.range.start.min(len)..span.range.end.min(len);
            if caret.is_none() {
                caret = Some(CaretPos {
                    node,
                    offset: range.start,
                });
            }
            if !range.is_empty()
                && let Some(text) = self.node_text_mut(node)
            {
                text.replace_range(range, "");
                edited_nodes.push(node);
            }
        }
        // A grab of nothing but interim still earns its caret at the point
        // it painted — the live append point.
        let mut caret = caret.unwrap_or_else(|| self.append_point_caret());
        if !insert.is_empty()
            && let Some(text) = self.node_text_mut(caret.node)
        {
            caret.offset = caret.offset.min(text.len());
            text.insert_str(caret.offset, insert);
            caret.offset += insert.len();
            edited_nodes.push(caret.node);
        }
        for node in edited_nodes {
            self.note_user_edit(node);
        }
        self.collapse_emptied(&mut caret);
        Some(caret)
    }

    /// Drop bullets and paragraphs an edit emptied outright — the last
    /// paragraph always stays as the live append point — re-anchoring the
    /// caret to the position that followed whatever was cut.
    fn collapse_emptied(&mut self, caret: &mut CaretPos) {
        // Bottom-up so removals only renumber what came after them.
        for paragraph in (0..self.paragraphs.len()).rev() {
            for bullet in (0..self.paragraphs[paragraph].bullets.len()).rev() {
                if !self.paragraphs[paragraph].bullets[bullet].text.is_empty() {
                    continue;
                }
                self.paragraphs[paragraph].bullets.remove(bullet);
                if let ScratchpadNode::Bullet(p, b) = caret.node
                    && p == paragraph
                {
                    if b == bullet {
                        caret.node = if b < self.paragraphs[paragraph].bullets.len() {
                            ScratchpadNode::Bullet(paragraph, b)
                        } else if paragraph + 1 < self.paragraphs.len() {
                            ScratchpadNode::Paragraph(paragraph + 1)
                        } else {
                            ScratchpadNode::Paragraph(paragraph)
                        };
                        caret.offset = match caret.node {
                            ScratchpadNode::Paragraph(p) if p == paragraph => {
                                self.paragraphs[paragraph].text.len()
                            }
                            _ => 0,
                        };
                    } else if b > bullet {
                        caret.node = ScratchpadNode::Bullet(paragraph, b - 1);
                    }
                }
            }
            // A removed row orphans its children — clamp each depth to
            // its previous row's + 1 so survivors promote instead of
            // dangling off nothing.
            let mut depth = None;
            for bullet in &mut self.paragraphs[paragraph].bullets {
                bullet.depth = match depth {
                    Some(previous) => bullet.depth.min(previous + 1),
                    None => 0,
                };
                depth = Some(bullet.depth);
            }
            if self.paragraphs[paragraph].text.is_empty()
                && self.paragraphs[paragraph].bullets.is_empty()
                && paragraph + 1 < self.paragraphs.len()
            {
                self.paragraphs.remove(paragraph);
                match caret.node {
                    ScratchpadNode::Paragraph(index) if index == paragraph => {
                        // The paragraph that followed slides into its slot.
                        caret.offset = 0;
                    }
                    ScratchpadNode::Bullet(p, _) if p == paragraph => {
                        caret.node = ScratchpadNode::Paragraph(paragraph);
                        caret.offset = 0;
                    }
                    ScratchpadNode::Paragraph(index) if index > paragraph => {
                        caret.node = ScratchpadNode::Paragraph(index - 1);
                    }
                    ScratchpadNode::Bullet(p, b) if p > paragraph => {
                        caret.node = ScratchpadNode::Bullet(p - 1, b);
                    }
                    _ => {}
                }
            }
        }
    }

    /// The whole transcript as one chat message: paragraphs separated by a
    /// blank line, each paragraph's bullets listed under it. An open
    /// annotation box commits as a bullet and live interim text settles
    /// into its paragraph — what the user saw is what sends.
    fn to_message(&self) -> String {
        let mut out = String::new();
        for (index, paragraph) in self.paragraphs.iter().enumerate() {
            let mut text = paragraph.text.trim().to_owned();
            let mut bullets = paragraph.bullets.clone();
            let pending_slot = match self.annotation_target {
                Some(AnnotationTarget::Paragraph(target)) if target == index => {
                    Some((bullets.len(), 0))
                }
                Some(AnnotationTarget::Bullet {
                    paragraph: target,
                    bullet,
                    insert,
                }) if target == index => {
                    let depth = bullets
                        .get(bullet)
                        .map(|bullet| (bullet.depth + 1).min(MAX_BULLET_DEPTH))
                        .unwrap_or(0);
                    Some((insert.min(bullets.len()), depth))
                }
                _ => None,
            };
            if let Some((slot, depth)) = pending_slot {
                // An armed edit's tail is an instruction, not a bullet —
                // it stays out of the message, interim included.
                let pending = match self.annotation_edit {
                    Some((_, from)) => self
                        .annotation_text
                        .get(..from)
                        .unwrap_or_default()
                        .trim()
                        .to_owned(),
                    None => {
                        let mut pending = self.annotation_text.trim().to_owned();
                        append_word_text(&mut pending, self.annotation_interim.trim());
                        pending.trim().to_owned()
                    }
                };
                if !pending.is_empty() {
                    bullets.insert(
                        slot,
                        ScratchpadBullet {
                            text: pending,
                            depth,
                        },
                    );
                }
            }
            if self.annotation_target.is_none() && index + 1 == self.paragraphs.len() {
                append_word_text(&mut text, self.interim.trim());
            }
            if text.is_empty() && bullets.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&text);
            for bullet in &bullets {
                out.push('\n');
                for _ in 0..bullet.depth {
                    out.push_str("  ");
                }
                out.push_str("- ");
                out.push_str(bullet.text.trim());
            }
        }
        out
    }

    /// The cleanup model's answer for one flushed span: replace it only
    /// when the buffer still holds the exact raw text — at the recorded
    /// offset, or uniquely elsewhere once later replacements shifted
    /// things. An edited paragraph or a missing match keeps the raw text;
    /// `edit` marks a spoken rewrite instruction, which the typed-edit
    /// guard doesn't protect against — the instruction sanctions it.
    fn apply_cleanup(
        &mut self,
        target: CleanTarget,
        start: usize,
        raw: &str,
        cleaned: &str,
        edit: bool,
    ) -> bool {
        let edited = !edit
            && match target {
                CleanTarget::Annotation => false,
                CleanTarget::Node(ScratchpadNode::Paragraph(index)) => self
                    .paragraphs
                    .get(index)
                    .is_none_or(|paragraph| paragraph.edited),
                CleanTarget::Node(ScratchpadNode::Bullet(index, bullet)) => self
                    .paragraphs
                    .get(index)
                    .is_none_or(|paragraph| paragraph.edited || bullet >= paragraph.bullets.len()),
            };
        if edited {
            return false;
        }
        let text = match target {
            CleanTarget::Annotation => self.annotation_text.as_str(),
            CleanTarget::Node(node) => self.node_text(node),
        };
        let span = if edit {
            // A spoken rewrite covered the target's whole text — anything
            // but an exact match means the buffer moved past it.
            (text == raw).then(|| 0..raw.len())
        } else {
            start
                .checked_add(raw.len())
                .filter(|&end| text.get(start..end) == Some(raw))
                .map(|end| start..end)
                .or_else(|| {
                    // Earlier answers already rewrote neighbors — the span
                    // still applies when the raw text survives exactly once.
                    let mut hits = text.match_indices(raw);
                    match (hits.next(), hits.next()) {
                        (Some((at, _)), None) => Some(at..at + raw.len()),
                        _ => None,
                    }
                })
        };
        let Some(span) = span else {
            return false;
        };
        let buffer = match target {
            CleanTarget::Annotation => Some(&mut self.annotation_text),
            CleanTarget::Node(node) => self.node_text_mut(node),
        };
        let Some(buffer) = buffer else {
            return false;
        };
        buffer.replace_range(span.clone(), cleaned);
        let delta = cleaned.len() as i64 - raw.len() as i64;
        if delta == 0 {
            return true;
        }
        let adjust = |from: &mut usize| {
            if *from > span.start {
                *from = (*from as i64 + delta).max(0) as usize;
            }
        };
        match target {
            CleanTarget::Annotation => adjust(&mut self.annotation_clean_from),
            CleanTarget::Node(ScratchpadNode::Paragraph(index))
                if index + 1 == self.paragraphs.len() =>
            {
                adjust(&mut self.main_clean_from)
            }
            _ => {}
        }
        if let CleanTarget::Node(node) = target
            && let Some(mut caret) = self.caret
            && caret.node == node
            && caret.offset > span.start
        {
            caret.offset = (caret.offset as i64 + delta).max(span.start as i64) as usize;
            self.caret = Some(caret);
        }
        true
    }

    /// Whether the transcript holds anything Enter would send — the
    /// send affordance's draft check, without building the message.
    pub(super) fn has_content(&self) -> bool {
        !self.interim.trim().is_empty()
            || !self.annotation_interim.trim().is_empty()
            || !self.annotation_text.trim().is_empty()
            || self.paragraphs.iter().any(|paragraph| {
                !paragraph.text.trim().is_empty()
                    || paragraph
                        .bullets
                        .iter()
                        .any(|bullet| !bullet.text.trim().is_empty())
            })
    }

    /// Whether Cancel must ask first: a real second paragraph, or a
    /// transcript substantial enough to lose.
    fn substantial(&self) -> bool {
        let written = self
            .paragraphs
            .iter()
            .filter(|paragraph| !paragraph.text.is_empty() || !paragraph.bullets.is_empty())
            .count();
        written >= CANCEL_CONFIRM_PARAGRAPHS
            || self.to_message().chars().count() >= CANCEL_CONFIRM_CHARS
    }
}

/// The end of the next word after `offset` — TextInput's rule, kept local
/// so the scratchpad doesn't reach into the field's internals.
fn next_word_boundary(text: &str, offset: usize) -> usize {
    text[offset..]
        .split_word_bound_indices()
        .find(|(_, segment)| !segment.chars().all(char::is_whitespace))
        .map(|(index, segment)| offset + index + segment.len())
        .unwrap_or(text.len())
}

/// The start of the word ending at `offset` — the mirror of
/// [`next_word_boundary`].
fn previous_word_boundary(text: &str, offset: usize) -> usize {
    text[..offset]
        .split_word_bound_indices()
        .rev()
        .find(|(_, segment)| !segment.chars().all(char::is_whitespace))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// Append a word run to `text` with a single separating space. A run
/// opening with sentence punctuation solidifies onto the previous word —
/// chunk boundaries decide where spaces land, so a period split onto its
/// own chunk would otherwise land as "the sentence ."
fn append_word_text(text: &mut String, words: &str) {
    let words = words.trim();
    if words.is_empty() {
        return;
    }
    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        let mut punct_end = 0;
        for (index, c) in words.char_indices() {
            if is_sentence_punct(c) {
                punct_end = index + c.len_utf8();
            } else {
                break;
            }
        }
        text.push_str(&words[..punct_end]);
        let words = words[punct_end..].trim_start();
        if words.is_empty() {
            return;
        }
        text.push(' ');
        text.push_str(words);
        return;
    }
    text.push_str(words);
}

/// Punctuation that binds left — a leading run of it joins the previous
/// word with no space. `"` is excluded: at a chunk head it opens a quote
/// as often as it closes one.
fn is_sentence_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | '!' | '?' | ';' | ':' | '…' | '\'' | '’' | ')' | ']' | '}' | '%'
    )
}

/// A delivery carrying no words — sentence punctuation, whitespace, and
/// dashes only. After a pause the recognizer emits these as standalone
/// chunks that would otherwise lead the next phrase.
fn is_punct_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(is_stray_punct_char)
}

/// What a punctuation-only chunk can be made of — the set
/// `is_punct_only` accepts and `strip_leading_punct` removes.
fn is_stray_punct_char(c: char) -> bool {
    c.is_whitespace() || is_sentence_punct(c) || matches!(c, '-' | '–' | '—')
}

/// Drop a delivery's leading punctuation-and-space run — the tail of a
/// stray emission belongs to the pause, not the phrase opening here.
fn strip_leading_punct(text: &str) -> &str {
    text.trim_start_matches(is_stray_punct_char)
}

/// A finished span's raw payload — trimmed off its edges so the cleaned
/// answer replaces only the words and the spacing around it survives.
/// Returns the trim-adjusted start and the raw text, or `None` when the
/// span holds no words.
fn clean_span(text: &str, from: usize, to: usize) -> Option<(usize, String)> {
    let slice = text.get(from..to)?;
    let raw = slice.trim();
    if raw.is_empty() {
        return None;
    }
    Some((
        from + slice.len() - slice.trim_start().len(),
        raw.to_owned(),
    ))
}

/// The end of `text`'s last closed sentence — a `.`/`!`/`?`/`…` that a
/// following word has already opened. The trailing open sentence stays
/// out of the span so cleanup never rewrites the live tail.
fn completed_sentence_end(text: &str) -> Option<usize> {
    let mut end = None;
    for (index, c) in text.char_indices() {
        if !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        let after = index + c.len_utf8();
        if text[after..].trim_start().is_empty() {
            continue;
        }
        end = Some(after);
    }
    end
}

/// The gray interim tail's text without sentence terminators — partial
/// speech keeps its words but waits for finalized text to wear periods.
fn strip_interim_terminators(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| !is_interim_terminator(*c))
        .collect()
}

fn is_interim_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…')
}

/// The painted length of an interim tail once its terminators strip —
/// the number the fade diff measures, without building the string.
fn scratchpad_interim_len(interim: &str) -> usize {
    interim
        .trim()
        .chars()
        .filter(|&c| !is_interim_terminator(c))
        .map(char::len_utf8)
        .sum()
}

/// Whether swapping the shown interim for a partial's `new` tail only
/// clips characters or shuffles trailing punctuation — the jitter the
/// dwell holds back. An empty candidate is a clip too: a retracted tail
/// waits out the window like any other shrink. Nothing holds against an
/// empty slot — there are no shown characters left to protect.
fn interim_hold_covers(shown: &str, new: &str) -> bool {
    !shown.is_empty()
        && (shown.starts_with(new)
            || shown.trim_end_matches(is_stray_punct_char)
                == new.trim_end_matches(is_stray_punct_char))
}

/// `text`'s words as byte spans — maximal alphanumeric runs, the
/// boundaries the command scan and the stream-dedup compares share.
fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut word_start = None;
    for (index, c) in text.char_indices() {
        if c.is_alphanumeric() {
            word_start.get_or_insert(index);
        } else if let Some(start) = word_start.take() {
            words.push((start, index));
        }
    }
    if let Some(start) = word_start {
        words.push((start, text.len()));
    }
    words
}

/// Split `text` around the next "okay next" command — `"ok"`, `"okay"`, and
/// comma-separated spellings all count. Returns the text before and after
/// the command's consumed span; surrounding punctuation and whitespace go
/// with the command rather than into either side.
#[cfg(test)]
fn split_next_command(text: &str) -> Option<(&str, &str)> {
    let (start, end) = next_command_span(text)?;
    Some((
        text[..start].trim_end(),
        text[end..].trim_start_matches(|c: char| !c.is_alphanumeric()),
    ))
}

/// The byte span of `text`'s first "okay next" command.
fn next_command_span(text: &str) -> Option<(usize, usize)> {
    let words = word_spans(text);
    for pair in words.windows(2) {
        let (a_start, _) = pair[0];
        let (_, b_end) = pair[1];
        let a = &text[a_start..pair[0].1];
        if (a.eq_ignore_ascii_case("ok") || a.eq_ignore_ascii_case("okay"))
            && text[pair[1].0..b_end].eq_ignore_ascii_case("next")
        {
            return Some((a_start, b_end));
        }
    }
    None
}

/// The byte span of `text`'s first "okay, let's make an edit" — the
/// annotation box's edit arming phrase, matched on the same alphanumeric
/// tokens "okay next" scans, so casing, punctuation, and chunking all
/// read the same.
fn edit_command_span(text: &str) -> Option<(usize, usize)> {
    let spans = word_spans(text);
    let words: Vec<&str> = spans.iter().map(|&(start, end)| &text[start..end]).collect();
    for start in 0..words.len() {
        if let Some(len) = edit_command_len(&words[start..]) {
            return Some((spans[start].0, spans[start + len - 1].1));
        }
    }
    None
}

/// Split `text` around the first edit command — the same split shape
/// `split_next_command` returns.
#[cfg(test)]
fn split_edit_command(text: &str) -> Option<(&str, &str)> {
    let (start, end) = edit_command_span(text)?;
    Some((
        text[..start].trim_end(),
        text[end..].trim_start_matches(|c: char| !c.is_alphanumeric()),
    ))
}

/// Whether `words` opens with the edit phrase — "ok"/"okay" then
/// "let's make an edit". The recognizer delivers "let's" as one word
/// ("lets") or two ("let" "'s"), so the phrase runs five or six tokens.
/// Returns how many words it consumed.
fn edit_command_len(words: &[&str]) -> Option<usize> {
    let is = |index: usize, expect: &str| {
        words
            .get(index)
            .is_some_and(|word| word.eq_ignore_ascii_case(expect))
    };
    if !is(0, "ok") && !is(0, "okay") {
        return None;
    }
    let start = if is(1, "lets") {
        2
    } else if is(1, "let") && is(2, "s") {
        3
    } else {
        return None;
    };
    if is(start, "make") && is(start + 1, "an") && is(start + 2, "edit") {
        Some(start + 3)
    } else {
        None
    }
}

/// The trailing alphanumeric run of `text`, punctuation aside — the word a
/// straddling command would open with.
fn trailing_word(text: &str) -> Option<&str> {
    let trimmed = text.trim_end_matches(|c: char| !c.is_alphanumeric());
    if trimmed.is_empty() {
        return None;
    }
    let start = trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_alphanumeric())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    Some(&trimmed[start..])
}

/// Whether a command spans the seam between the append point's committed
/// `tail` and the incoming chunk `rest`: the tail ends on "ok"/"okay" and
/// `rest` opens with "next". Returns `rest` past the command, punctuation
/// and whitespace going with it.
fn seam_command<'a>(tail: &str, rest: &'a str) -> Option<&'a str> {
    let word = trailing_word(tail)?;
    if !word.eq_ignore_ascii_case("ok") && !word.eq_ignore_ascii_case("okay") {
        return None;
    }
    let start = rest.find(|c: char| c.is_alphanumeric())?;
    let end = rest[start..]
        .find(|c: char| !c.is_alphanumeric())
        .map(|i| start + i)
        .unwrap_or(rest.len());
    if !rest[start..end].eq_ignore_ascii_case("next") {
        return None;
    }
    Some(rest[end..].trim_start_matches(|c: char| !c.is_alphanumeric()))
}

/// Whether the edit phrase straddles the seam between the append point's
/// committed `tail` and the incoming chunk `rest` — `seam_command`'s
/// multi-word twin. Returns how many committed words the phrase took
/// plus `rest` past the command's consumed span.
fn seam_edit_command<'a>(tail: &str, rest: &'a str) -> Option<(usize, &'a str)> {
    let tail_spans = word_spans(tail);
    let rest_spans = word_spans(rest);
    // The phrase needs at least one word off each side to be a seam —
    // wholly committed or wholly incoming runs are the other scans'.
    let take = tail_spans.len().min(EDIT_COMMAND_MAX_WORDS - 1);
    for taken in (1..=take).rev() {
        let mut words: Vec<&str> = tail_spans[tail_spans.len() - taken..]
            .iter()
            .map(|&(start, end)| &tail[start..end])
            .collect();
        words.extend(rest_spans.iter().map(|&(start, end)| &rest[start..end]));
        let Some(len) = edit_command_len(&words) else {
            continue;
        };
        if len <= taken || len - taken > rest_spans.len() {
            continue;
        }
        let end = rest_spans[len - taken - 1].1;
        return Some((
            taken,
            rest[end..].trim_start_matches(|c: char| !c.is_alphanumeric()),
        ));
    }
    None
}

/// Streaming resampler: f32 mono at the device rate → s16le mono bytes at
/// the model's rate. Linear interpolation is plenty for speech; `carry`
/// keeps one sample of overlap so chunk seams don't click.
struct PcmResampler {
    out_rate: f64,
    /// The last sample of the previous chunk — position 0 for the cursor.
    carry: f32,
    /// The next output point, measured in input samples after `carry`.
    cursor: f64,
}

impl PcmResampler {
    fn new(out_rate: f64) -> Self {
        Self {
            out_rate,
            carry: 0.0,
            cursor: 1.0,
        }
    }

    /// Resample `input` (mono f32 at `in_rate`), appending s16le bytes.
    fn push(&mut self, input: &[f32], in_rate: f64, out: &mut Vec<u8>) {
        if input.is_empty() || in_rate <= 0.0 {
            return;
        }
        let step = in_rate / self.out_rate;
        let len = input.len() as f64;
        // Positions measure input samples since `carry`: position p in
        // 1..=len maps to input[p-1], position 0 is the carry itself.
        let at = |position: f64| -> f32 {
            if position <= 0.0 {
                self.carry
            } else {
                input[(position as usize - 1).min(input.len() - 1)]
            }
        };
        while self.cursor <= len {
            let base = self.cursor.floor();
            let frac = (self.cursor - base) as f32;
            let value = at(base) + (at(base + 1.0) - at(base)) * frac;
            let sample = (value.clamp(-1.0, 1.0) * 32767.0) as i16;
            out.extend_from_slice(&sample.to_le_bytes());
            self.cursor += step;
        }
        self.carry = input[input.len() - 1];
        self.cursor -= len;
    }
}

/// Connect the transcription WebSocket: TCP connect and the TLS/upgrade
/// handshake share socket timeouts, then the socket drops to its short
/// read poll.
fn connect_transcription_socket(key: &str) -> anyhow::Result<WebSocket<MaybeTlsStream<TcpStream>>> {
    // tungstenite builds `ClientConfig::builder()`, which panics when no
    // process crypto provider is installed and the crate's features can't
    // pick one — this graph enables both aws-lc-rs and ring. The app's
    // other rustls users pass explicit configs, so nothing installs a
    // default before this point; a repeat install errors benignly.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let url = url::Url::parse(TRANSCRIPTION_URL)?;
    let host = url
        .host_str()
        .context("transcription gateway URL has no host")?;
    let port = url.port_or_known_default().unwrap_or(443);
    let mut stream = None;
    for address in (host, port).to_socket_addrs()? {
        if let Ok(connected) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            stream = Some(connected);
            break;
        }
    }
    let stream = stream.context("could not reach the transcription gateway")?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let request = Request::builder()
        .method("GET")
        .uri(TRANSCRIPTION_URL)
        .header("Host", host)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .header(
            "Sec-WebSocket-Protocol",
            format!("{TRANSCRIPTION_SUBPROTOCOL}, {TRANSCRIPTION_AUTH_PREFIX}{key}"),
        )
        .header("authorization", format!("Bearer {key}"))
        .header("ai-gateway-protocol-version", "0.0.1")
        .header("ai-model-id", TRANSCRIPTION_MODEL_ID)
        .body(())?;
    let mut attempt = tungstenite::client_tls_with_config(
        request,
        stream,
        Some(WebSocketConfig::default()),
        None,
    );
    // Socket timeouts surface as `Interrupted` mid-handshake — resume the
    // exchange instead of restarting it.
    let mut socket = loop {
        attempt = match attempt {
            Ok((socket, _)) => break Ok(socket),
            Err(HandshakeError::Interrupted(mid)) => mid.handshake(),
            Err(HandshakeError::Failure(error)) => {
                break Err(error).context("transcription gateway handshake failed");
            }
        };
    }?;
    // Steady state: reads poll on a short timeout so the same thread can
    // interleave outbound audio and inbound transcript frames.
    if let MaybeTlsStream::Rustls(tls) = socket.get_mut() {
        tls.sock.set_read_timeout(Some(READ_POLL))?;
        tls.sock.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
    }
    Ok(socket)
}

/// Whether a transcript part's whole payload is a pause/resume state
/// marker — the model reports transcription state as transcript-channel
/// text rather than a dedicated stream part. Whole-payload match only, so
/// the same words inside real dictation still transcribe.
fn transcription_marker(text: &str) -> Option<ScratchpadEvent> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("transcription paused") {
        Some(ScratchpadEvent::Paused)
    } else if text.eq_ignore_ascii_case("transcription resumed") {
        Some(ScratchpadEvent::Resumed)
    } else {
        None
    }
}

/// One transcript event out of a server frame — `true` when the part is
/// terminal and the worker decides between reconnecting and giving up.
fn dispatch_stream_part(text: &str, send: &mut impl FnMut(ScratchpadEvent)) -> bool {
    let Ok(part) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let part_type = part.get("type").and_then(Value::as_str);
    // Pause and resume markers ride the transcript channel — intercept them
    // before they land as dictation.
    if matches!(
        part_type,
        Some("transcript-delta") | Some("transcript-partial") | Some("transcript-final")
    ) && let Some(marker) = part
        .get("delta")
        .or_else(|| part.get("text"))
        .and_then(Value::as_str)
        .and_then(transcription_marker)
    {
        send(marker);
        return false;
    }
    match part_type {
        Some("transcript-delta") => {
            if let Some(delta) = part.get("delta").and_then(Value::as_str)
                && !delta.is_empty()
            {
                send(ScratchpadEvent::Final(delta.to_owned()));
            }
        }
        Some("transcript-partial") => {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                send(ScratchpadEvent::Partial(text.to_owned()));
            }
        }
        Some("transcript-final") => {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                send(ScratchpadEvent::FinalSegment(text.to_owned()));
            }
        }
        // Pause and resume as dedicated parts — the spellings the gateway
        // could pick if it reports the state outside the transcript
        // channel. The envelope's forward-compatibility rule ignores
        // unknown types, so these arms only grow the covered set.
        Some("transcription-paused" | "transcription.paused" | "transcript-paused") => {
            send(ScratchpadEvent::Paused);
        }
        Some("transcription-resumed" | "transcription.resumed" | "transcript-resumed") => {
            send(ScratchpadEvent::Resumed);
        }
        // `finish` is the stream's normal end and `error` its failure —
        // both terminal here; the session may still be open from the
        // user's side, so the worker treats either as reconnectable.
        Some("finish") => return true,
        Some("error") => {
            let detail = part.get("error").unwrap_or(&part);
            eprintln!("Goddard: voice scratchpad stream error part: {detail}");
            return true;
        }
        // Envelope parts with nothing to surface on this UI.
        Some("stream-start" | "response-metadata" | "raw") => {}
        // Drift lands here — name the type so a new part is visible
        // instead of silently dropped.
        Some(other) => {
            eprintln!("Goddard: voice scratchpad ignored stream part type {other:?}");
        }
        None => {}
    }
    false
}

/// How a socket's service ended — `Stop` unwinds the worker, `Ended`
/// earns a reconnect.
enum StreamEnd {
    Stop,
    Ended,
}

/// The per-session worker: owns the socket, streams PCM frames from the
/// audio channel, and forwards parsed transcript events to the pump. Runs
/// on its own thread — a blocking socket is fine when it owns nothing else.
/// A session outlives its socket: a dropped or wedged stream reconnects in
/// place while the audio channel buffers across the gap, and only a streak
/// of dead-on-arrival sessions drops the panel to Retry.
fn run_transcription_worker(
    session_id: Uuid,
    generation: u64,
    key: String,
    audio: Receiver<AudioChunk>,
    events: Sender<(Uuid, u64, ScratchpadEvent)>,
    wake: smol::channel::Sender<()>,
    stop: Arc<AtomicBool>,
) {
    // The pause clock and the session's output flag ride the one event
    // channel — `send` sees every dispatch, and the poll loop reads them
    // back for the watchdog and the dead-session count.
    let paused_since = Cell::new(None::<Instant>);
    let produced_output = Cell::new(false);
    // The silence auto-stop's clock — only inbound transcript text counts
    // as the user producing a phrase; markers and worker events don't.
    let last_speech = Cell::new(Instant::now());
    let mut send = |event: ScratchpadEvent| {
        match event {
            ScratchpadEvent::Paused => paused_since.set(Some(Instant::now())),
            ScratchpadEvent::Final(_)
            | ScratchpadEvent::FinalSegment(_)
            | ScratchpadEvent::Partial(_) => {
                paused_since.set(None);
                last_speech.set(Instant::now());
            }
            ScratchpadEvent::Resumed | ScratchpadEvent::Connected => {
                paused_since.set(None);
            }
            _ => {}
        }
        // Events dispatched off the wire — transcript text or a state
        // marker — prove the session produced output; worker-made events
        // (`Connected`, `Reconnecting`, `Failed`) don't.
        if matches!(
            event,
            ScratchpadEvent::Final(_)
                | ScratchpadEvent::FinalSegment(_)
                | ScratchpadEvent::Partial(_)
                | ScratchpadEvent::Paused
                | ScratchpadEvent::Resumed
        ) {
            produced_output.set(true);
        }
        let _ = events.send((session_id, generation, event));
        signal_event_pump(&wake);
    };
    let mut dead_sessions = 0u32;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let mut socket = match connect_transcription_socket(&key) {
            Ok(socket) => socket,
            Err(error) => {
                eprintln!("Goddard: voice scratchpad connection failed: {error:#}");
                dead_sessions += 1;
                if dead_sessions >= MAX_DEAD_SESSIONS {
                    send(ScratchpadEvent::Failed);
                    return;
                }
                send(ScratchpadEvent::Reconnecting);
                thread::sleep(RECONNECT_DELAY);
                continue;
            }
        };
        // A stop that landed during the handshake owns this socket's
        // whole life — close it rather than open a stream for a dead
        // session.
        if stop.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return;
        }
        send(ScratchpadEvent::Connected);
        produced_output.set(false);
        let connected_at = Instant::now();
        let start = serde_json::json!({
            "type": "transcription-stream.start",
            "inputAudioFormat": { "type": "audio/pcm", "rate": TRANSCRIPTION_SAMPLE_RATE },
        });
        let end = if socket
            .send(Message::Text(start.to_string().into()))
            .is_err()
        {
            StreamEnd::Ended
        } else {
            let mut resampler = PcmResampler::new(TRANSCRIPTION_SAMPLE_RATE);
            let mut pcm = Vec::new();
            // Audio the model heard nothing about while paused — a pause
            // that outlives its grace while speech still streams is a wedge.
            let mut sent_since_pause = false;
            loop {
                if stop.load(Ordering::Relaxed) {
                    let _ = socket.send(Message::Text(
                        "{\"type\":\"transcription-stream.audio-done\"}".into(),
                    ));
                    let _ = socket.close(None);
                    break StreamEnd::Stop;
                }
                match audio.recv_timeout(READ_POLL) {
                    Ok(chunk) => {
                        // Drain whatever else landed this tick before
                        // writing, so a burst ships as few frames as it can.
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
                            break StreamEnd::Ended;
                        }
                        sent_since_pause |= paused_since.get().is_some();
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    // The sink detaches when the session ends.
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        let _ = socket.close(None);
                        break StreamEnd::Stop;
                    }
                }
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        if dispatch_stream_part(&text, &mut send) {
                            let _ = socket.close(None);
                            break StreamEnd::Ended;
                        }
                    }
                    Ok(Message::Close(_)) => break StreamEnd::Ended,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(error))
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        // The read poll expired — fall through and loop.
                    }
                    Err(error) => {
                        eprintln!("Goddard: voice scratchpad stream failed: {error}");
                        break StreamEnd::Ended;
                    }
                }
                if paused_since.get().is_some_and(|since| since.elapsed() > PAUSE_WATCHDOG)
                    && sent_since_pause
                {
                    eprintln!(
                        "Goddard: voice scratchpad pause outlived its resume window — reconnecting"
                    );
                    let _ = socket.close(None);
                    break StreamEnd::Ended;
                }
                if last_speech.get().elapsed() > SILENCE_AUTO_STOP {
                    eprintln!("Goddard: voice scratchpad auto-stopping after silence");
                    send(ScratchpadEvent::Stopped);
                    let _ = socket.send(Message::Text(
                        "{\"type\":\"transcription-stream.audio-done\"}".into(),
                    ));
                    let _ = socket.close(None);
                    break StreamEnd::Stop;
                }
                let _ = socket.flush();
            }
        };
        match end {
            StreamEnd::Stop => return,
            StreamEnd::Ended => {
                paused_since.set(None);
                // A session that produced speech or outlived its warmup
                // died healthy — reconnect. A dead-on-arrival session
                // counts against the budget.
                if produced_output.get() || connected_at.elapsed() >= HEALTHY_SESSION {
                    dead_sessions = 0;
                } else {
                    dead_sessions += 1;
                }
                if dead_sessions >= MAX_DEAD_SESSIONS {
                    send(ScratchpadEvent::Failed);
                    return;
                }
                send(ScratchpadEvent::Reconnecting);
                thread::sleep(RECONNECT_DELAY);
            }
        }
    }
}

impl Waku {
    /// The visible chat's scratchpad — the only one that may own capture.
    pub(super) fn selected_voice_scratchpad(&self) -> Option<&VoiceScratchpad> {
        self.voice_scratchpads.get(&self.state.selected_session?)
    }

    fn selected_voice_scratchpad_mut(&mut self) -> Option<&mut VoiceScratchpad> {
        self.voice_scratchpads.get_mut(&self.state.selected_session?)
    }

    /// Whether the scratchpad panel is the chat column's content right now:
    /// a session on this chat, not hidden, under the surfaces a mounted
    /// composer implies. The experiment flag gates the whole surface — a
    /// session only exists while it is on.
    pub(super) fn voice_scratchpad_visible(&self) -> bool {
        if !self.state.voice_scratchpad_enabled {
            return false;
        }
        let Some(scratchpad) = self.selected_voice_scratchpad() else {
            return false;
        };
        if scratchpad.hidden {
            return false;
        }
        // Big Picture borrows the composer for its overlay — Enter there is
        // a Big Picture submit, not a scratchpad send.
        self.composer_mounted() && !self.big_picture.is_open()
    }

    /// The VP button's posture for the card under the pointer: idle, or
    /// live on this chat (recording or muted). Every chat gets its own
    /// button state — another chat's session no longer dims it.
    fn voice_scratchpad_button_state(&self, session_id: Option<Uuid>) -> ScratchpadButtonState {
        match session_id.and_then(|id| self.voice_scratchpads.get(&id)) {
            Some(scratchpad) if scratchpad.muted => ScratchpadButtonState::Muted,
            Some(_) => ScratchpadButtonState::Recording,
            None => ScratchpadButtonState::Idle,
        }
    }

    /// The VP button — a small dark pill with "VP" and a mic glyph,
    /// immediately left of send. While this chat's session is live it
    /// carries the state dot.
    pub(super) fn render_voice_scratchpad_button(
        &self,
        controls: &composer::ComposerControls,
        session_id: Option<Uuid>,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !self.state.voice_scratchpad_enabled {
            return None;
        }
        let state = self.voice_scratchpad_button_state(session_id);
        let theme = Theme::current(cx);
        let enabled = session_id.is_some();
        // The state dot is a pill interior element now, left of the label —
        // it reads against text where a corner badge sat over the edge.
        let dot = |color: Hsla| {
            div()
                .size(px(7.0))
                .flex_none()
                .rounded_full()
                .mr(px(2.0))
                .bg(color)
        };
        // The frame's insets: label 12px in, mic 1px off the label, 9px pad
        // right — and a 12px berth before send, the row's 4px gap plus the
        // margin here.
        let pill = div()
            .id(controls.chip_id("voice-scratchpad"))
            .h(px(24.0))
            .flex_none()
            .mr(px(8.0))
            .rounded(px(VP_PILL_RADIUS))
            .flex()
            .items_center()
            .gap(px(1.0))
            .pl(px(12.0))
            .pr(px(9.0))
            .bg(theme.inverse);
        let pill = match state {
            ScratchpadButtonState::Recording => pill.child(dot(rgb(RECORDING_RED).into())),
            ScratchpadButtonState::Muted => pill.child(dot(theme.text_tertiary)),
            _ => pill,
        };
        let pill = pill
            .child(
                div()
                    .text_size(sp(9.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.on_inverse)
                    .child(tr!("voice_scratchpad.short_title")),
            )
            .child(icon("icons/mic.svg", 16.0, theme.on_inverse));
        // The click's outcome decides the tooltip: a scratchpad on screen
        // cancels, a hidden one resurfaces.
        let cancels = session_id
            .and_then(|id| self.voice_scratchpads.get(&id))
            .is_some_and(|scratchpad| {
                !scratchpad.hidden && self.state.selected_session == session_id
            });
        let tooltip = match (state, cancels) {
            (ScratchpadButtonState::Recording, true) => {
                tr!("voice_scratchpad.cancel_dictating")
            }
            (ScratchpadButtonState::Muted, true) => tr!("voice_scratchpad.cancel_muted"),
            (ScratchpadButtonState::Recording, false) => tr!("voice_scratchpad.recording"),
            (ScratchpadButtonState::Muted, false) => tr!("voice_scratchpad.muted"),
            (ScratchpadButtonState::Idle, _) => tr!("voice_scratchpad.start"),
        };
        Some(
            pill.tooltip(Tooltip::text(tooltip))
                .when(!enabled, |pill| pill.opacity(0.4))
                .when(enabled, |pill| {
                    pill.cursor_default()
                        .hover(|element| element.opacity(0.9))
                        .active(|element| element.opacity(0.8))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_voice_scratchpad(window, cx);
                        }))
                }),
        )
    }

    /// The VP button's click: no session starts one on this chat, a hidden
    /// session resurfaces, and a visible one cancels — the same
    /// confirm-on-substantial rule the Cancel pill applies. Each chat owns
    /// its scratchpad — pressing the button here never touches another
    /// chat's.
    fn toggle_voice_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(this_session) = self.composer_session_id() else {
            return;
        };
        let Some(scratchpad) = self.voice_scratchpads.get_mut(&this_session) else {
            self.start_voice_scratchpad(window, cx);
            return;
        };
        if scratchpad.hidden {
            scratchpad.hidden = false;
            scratchpad.follow_tail = true;
            cx.notify();
        } else if self.state.selected_session == Some(this_session) {
            self.request_cancel_voice_scratchpad(window, cx);
        } else {
            // A scratchpad that cannot be on screen — the composer is
            // answering for another chat — still tucks out of the way.
            scratchpad.hidden = true;
            cx.notify();
        }
    }

    /// Start a session on the composer's chat. The panel opens immediately —
    /// permission and connection failures become its inline error state —
    /// and capture begins behind it when the chat is selected. A composer
    /// answering for another chat (Big Picture's target) creates the
    /// scratchpad paused: capture only ever belongs to the visible chat.
    fn start_voice_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.voice_scratchpad_enabled {
            return;
        }
        let Some(session_id) = self.composer_session_id() else {
            return;
        };
        if self.voice_scratchpads.contains_key(&session_id) {
            return;
        }
        let selected = self.state.selected_session == Some(session_id);
        let mut scratchpad = VoiceScratchpad::new(cx);
        scratchpad.muted = !selected;
        let edit_focus = scratchpad.edit_focus.clone();
        self.voice_scratchpads.insert(session_id, scratchpad);
        cx.on_focus(&edit_focus, window, move |this, window, cx| {
            // Keyboard focus lands the caret on the append point — the
            // surface's visible focus treatment. Pointer focus arrives with
            // a settled selection and leaves the caret alone.
            if !window.last_input_was_keyboard() {
                return;
            }
            if let Some(scratchpad) = this.voice_scratchpads.get_mut(&session_id)
                && scratchpad.transcript.caret.is_none()
            {
                scratchpad.transcript.caret =
                    Some(scratchpad.transcript.append_point_caret());
                cx.notify();
            }
        })
        .detach();
        if selected {
            self.ensure_voice_capture(session_id, cx);
        }
        // Focus stays in the composer — typing, Enter-to-send, and Esc all
        // keep their composer semantics while the panel is up.
        cx.notify();
    }

    /// Mic access ahead of capture: begin when granted, ask when the
    /// system has not answered yet, and leave the panel's error row when
    /// refused.
    fn ensure_voice_capture(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        match crate::platform::microphone_access() {
            crate::platform::CaptureAccess::Granted => {
                self.begin_voice_capture(session_id, cx)
            }
            crate::platform::CaptureAccess::Undetermined => {
                let tx = self.voice_scratchpad_tx.clone();
                let wake = self.event_wake_tx.clone();
                crate::platform::request_microphone_access(Box::new(move |granted| {
                    let _ = tx.try_send((session_id, 0, ScratchpadEvent::MicAccess(granted)));
                    signal_event_pump(&wake);
                }));
            }
            crate::platform::CaptureAccess::Denied => {
                if let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) {
                    scratchpad.status = ScratchpadStatus::MicDenied;
                }
            }
        }
    }

    /// Attach the audio sink and open the transcription stream for a
    /// chat's session. Mic access is already granted when this runs, and
    /// capture only ever belongs to the visible chat.
    fn begin_voice_capture(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.state.selected_session != Some(session_id) {
            return;
        }
        let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) else {
            return;
        };
        // A mute that landed while the permission prompt was in flight
        // leaves capture off — only an unmuted session owns the tap.
        if scratchpad.muted {
            return;
        }
        // Blocks queued for a dead stream drop before the new one opens —
        // audio buffered under an earlier socket is stale dictation, not a
        // backlog to replay.
        while scratchpad.audio_rx.try_recv().is_ok() {}
        let audio_tx = scratchpad.audio_tx.clone();
        crate::platform::set_voice_audio_sink(Some(Box::new(move |samples, rate| {
            // The audio thread never blocks: a full queue drops the block.
            let _ = audio_tx.try_send(AudioChunk {
                samples: samples.to_vec(),
                rate,
            });
        })));
        crate::platform::start_voice_listener();
        scratchpad.capture_live = true;
        scratchpad.input_unavailable = !crate::platform::voice_input_available();
        self.spawn_transcription_worker(session_id, cx);
    }

    /// Fetch the gateway key on the daemon, then spin the worker thread —
    /// a fresh credential per (re)connect keeps the secret out of app state.
    fn spawn_transcription_worker(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) else {
            return;
        };
        // Retire the previous worker before minting the next one's flag —
        // its Arc stays fired so two workers never share a stop signal.
        scratchpad.stop.store(true, Ordering::Relaxed);
        scratchpad.stop = Arc::new(AtomicBool::new(false));
        scratchpad.generation = scratchpad.generation.wrapping_add(1);
        let generation = scratchpad.generation;
        scratchpad.status = ScratchpadStatus::Connecting;
        let audio = scratchpad.audio_rx.clone();
        let stop = scratchpad.stop.clone();
        let events = self.voice_scratchpad_tx.clone();
        let wake = self.event_wake_tx.clone();
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
                    waku_client::ResponsePayload::InferenceCredential { credential } => credential,
                    _ => None,
                })
                .filter(|key| !key.trim().is_empty())
        });
        cx.spawn(async move |this, cx| {
            let key = work.await;
            let _ = this.update(cx, |this, cx| {
                let Some(scratchpad) = this.voice_scratchpads.get_mut(&session_id) else {
                    return;
                };
                if scratchpad.generation != generation || !scratchpad.capture_live {
                    return;
                }
                match key {
                    Some(key) => {
                        if let Err(error) = thread::Builder::new()
                            .name("voice-scratchpad-transcribe".to_owned())
                            .spawn(move || {
                                run_transcription_worker(
                                    session_id, generation, key, audio, events, wake, stop,
                                );
                            })
                        {
                            eprintln!("Goddard: could not start transcription worker: {error}");
                            scratchpad.status = ScratchpadStatus::ConnectionLost;
                        }
                    }
                    None => {
                        eprintln!(
                            "Goddard: voice scratchpad found no {} credential",
                            InferenceProvider::VercelGateway.display_name()
                        );
                        scratchpad.status = ScratchpadStatus::ConnectionLost;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Mute ends the stream itself, not just the tap: the worker flags
    /// out and closes its socket — the gateway bills per streaming hour —
    /// and the sink detaches so the mic engine can stand down. Words
    /// already dictated commit first, without fold credit: the dead
    /// stream owes no re-delivery. Unmuting reconnects through the same
    /// resume path a chat switch or the silence auto-stop leaves — the
    /// transcript and the muted flag survive in between.
    fn set_voice_scratchpad_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) else {
            return;
        };
        scratchpad.muted = muted;
        let (detach_sink, resume) = if muted {
            scratchpad.transcript.solidify_interim();
            let detach = scratchpad.capture_live;
            scratchpad.stop_capture();
            (detach, false)
        } else {
            (
                false,
                !scratchpad.capture_live && scratchpad.status != ScratchpadStatus::MicDenied,
            )
        };
        if detach_sink {
            self.detach_voice_sink();
        }
        if resume {
            self.ensure_voice_capture(session_id, cx);
        }
        // A muted stream goes quiet — spans the solidify just closed
        // can't wait on the next event to reach the model.
        self.drain_cleanup_requests(cx);
        cx.notify();
    }

    /// Whether ⌥M is the scratchpad's pause key right now: the
    /// panel owns the chat column and no annotation box has reclaimed
    /// typing for the composer draft. The transcript's live caret consumes
    /// the chord's character a level deeper, so one that lands as text
    /// never reaches the caller this gate answers for.
    pub(super) fn voice_scratchpad_alt_m_mutes(&self) -> bool {
        self.voice_scratchpad_visible()
            && self
                .selected_voice_scratchpad()
                .is_some_and(|scratchpad| scratchpad.transcript.annotation_target.is_none())
    }

    /// ⌥M's pause effect — the same toggle the control row's Mute pill
    /// fires.
    pub(super) fn voice_scratchpad_alt_m_toggle(&mut self, cx: &mut Context<Self>) {
        let muted = self
            .selected_voice_scratchpad()
            .is_some_and(|scratchpad| !scratchpad.muted);
        self.set_voice_scratchpad_muted(muted, cx);
    }

    /// Cancel — inline when the transcript is thin, confirmed once it's
    /// substantial.
    fn request_cancel_voice_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scratchpad) = self.selected_voice_scratchpad_mut() else {
            return;
        };
        if scratchpad.transcript.substantial() {
            scratchpad.confirm_discard = true;
            let focus = scratchpad.discard_focus.clone();
            window.focus(&focus, cx);
            cx.notify();
        } else {
            self.end_voice_scratchpad(cx);
        }
    }

    /// Escape inside the scratchpad: an armed discard dismisses first, a
    /// text selection or caret peels off next, then an annotation box
    /// closes, then Esc means Cancel — the same confirm rule as the button.
    pub(super) fn voice_scratchpad_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scratchpad) = self.selected_voice_scratchpad_mut() else {
            return;
        };
        if scratchpad.confirm_discard {
            scratchpad.confirm_discard = false;
            cx.notify();
            return;
        }
        if !scratchpad.selection.selection.borrow().is_empty()
            || scratchpad.transcript.caret.is_some()
        {
            scratchpad.selection.selection.borrow_mut().clear();
            scratchpad.transcript.caret = None;
            scratchpad.transcript.caret_anchor = None;
            // Typing goes home to the composer once the edit is dropped.
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
            cx.notify();
            return;
        }
        if scratchpad.transcript.annotation_target.is_some() {
            scratchpad.transcript.exit_annotation();
            cx.notify();
            return;
        }
        self.request_cancel_voice_scratchpad(window, cx);
    }

    /// Keystrokes on the scratchpad's transcript surface — a selection is
    /// the only edit target: typed text replaces it and Backspace/Delete
    /// cut it, arrows walk or extend the caret through paragraphs and
    /// bullets, and ⌘A selects all. A bare caret marks where dictation
    /// lands but takes no keys — typed input at it is swallowed, while
    /// with no caret and no grab a character keeps its type-to-focus trip
    /// to the composer.
    fn voice_scratchpad_edit_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(scratchpad) = self.selected_voice_scratchpad_mut() else {
            return;
        };
        // An open annotation box owns the stream; typed text keeps its
        // composer meaning while it's up.
        if scratchpad.transcript.annotation_target.is_some() {
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.function {
            // The one chord this surface claims — Select All is bound only
            // under the text-input context, which the rows don't carry.
            if modifiers == Modifiers::secondary_key() && keystroke.key == "a" {
                self.voice_scratchpad_select_all(cx);
                cx.stop_propagation();
            }
            return;
        }
        let spans = scratchpad.selection.selection.borrow().spans().to_vec();
        let has_selection = spans.iter().any(|span| !span.range.is_empty());
        if let Some(text) = sessions::type_to_focus_text(keystroke) {
            if !has_selection && scratchpad.transcript.caret.is_none() {
                // No edit target — the composer field takes the keystroke.
                return;
            }
            if has_selection {
                let text = text.to_owned();
                scratchpad.transcript.caret =
                    scratchpad.transcript.apply_selection_edit(&spans, &text);
                scratchpad.transcript.caret_anchor = None;
                scratchpad.selection.selection.borrow_mut().clear();
                cx.notify();
            }
            // A bare caret marks where dictation lands — it is not a
            // typing target, so the keystroke is swallowed rather than
            // written into the transcript or leaked into the draft.
            cx.stop_propagation();
            return;
        }
        match keystroke.key.as_str() {
            "backspace" | "delete" => {
                if !has_selection && scratchpad.transcript.caret.is_none() {
                    return;
                }
                if has_selection {
                    scratchpad.transcript.caret =
                        scratchpad.transcript.apply_selection_edit(&spans, "");
                    scratchpad.selection.selection.borrow_mut().clear();
                    scratchpad.transcript.caret_anchor = None;
                    cx.notify();
                }
                // The bare caret is no edit target — same swallow as
                // typed text.
                cx.stop_propagation();
            }
            "left" | "right" | "up" | "down" | "home" | "end" => {
                self.voice_scratchpad_move_caret(keystroke, has_selection, &spans, cx);
            }
            _ => {}
        }
    }

    /// An arrow/Home/End on the edit surface: plain moves walk the caret —
    /// collapsing a selection to its edge first — and shift grows the grab
    /// from its anchor, repainting the wash from the registry.
    fn voice_scratchpad_move_caret(
        &mut self,
        keystroke: &gpui::Keystroke,
        has_selection: bool,
        spans: &[md::selection::Span],
        cx: &mut Context<Self>,
    ) {
        let Some(scratchpad) = self.selected_voice_scratchpad_mut() else {
            return;
        };
        let backward = matches!(keystroke.key.as_str(), "left" | "up" | "home");
        let shift = keystroke.modifiers.shift;
        let word = keystroke.modifiers.alt;
        let node_edge = matches!(keystroke.key.as_str(), "home" | "end");
        let vertical = matches!(keystroke.key.as_str(), "up" | "down");
        let step = |caret: CaretPos| -> CaretPos {
            if node_edge {
                CaretPos {
                    node: caret.node,
                    offset: if backward {
                        0
                    } else {
                        scratchpad.transcript.node_text(caret.node).len()
                    },
                }
            } else if vertical {
                scratchpad.transcript.step_node(caret, !backward)
            } else {
                scratchpad
                    .transcript
                    .move_caret(caret, !backward, word)
            }
        };
        if shift {
            let (anchor, head) = if let Some(anchor) = scratchpad.transcript.caret_anchor {
                (
                    anchor,
                    scratchpad.transcript.caret.unwrap_or(anchor),
                )
            } else if let Some((start, end)) = scratchpad.transcript.spans_endpoints(spans) {
                (start, end)
            } else {
                let base = scratchpad
                    .transcript
                    .caret
                    .unwrap_or_else(|| scratchpad.transcript.append_point_caret());
                (base, base)
            };
            let head = step(head);
            scratchpad.transcript.caret = Some(head);
            scratchpad.transcript.caret_anchor = Some(anchor);
            // Repaint the wash from the model range — the registry maps
            // each node back to its painted element.
            let spans = {
                let registry = scratchpad.selection.registry.borrow();
                let from = registry.position(&scratchpad.transcript.node_key(anchor.node));
                let to = registry.position(&scratchpad.transcript.node_key(head.node));
                match (from, to) {
                    (Some(from), Some(to)) => {
                        registry.resolve((from, anchor.offset), (to, head.offset))
                    }
                    _ => Vec::new(),
                }
            };
            scratchpad
                .selection
                .selection
                .borrow_mut()
                .set_spans(spans);
        } else {
            let next = if has_selection {
                // A bare arrow collapses the grab to its edge — it doesn't
                // also step.
                scratchpad
                    .transcript
                    .spans_endpoints(spans)
                    .map(|(start, end)| if backward { start } else { end })
                    .unwrap_or_else(|| scratchpad.transcript.append_point_caret())
            } else {
                step(scratchpad
                    .transcript
                    .caret
                    .unwrap_or_else(|| scratchpad.transcript.append_point_caret()))
            };
            scratchpad.transcript.caret = Some(next);
            scratchpad.transcript.caret_anchor = None;
            scratchpad.selection.selection.borrow_mut().clear();
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// ⌘A on the edit surface: every painted text element selected, with
    /// the model endpoints recorded so a shift-arrow can trim the grab.
    fn voice_scratchpad_select_all(&mut self, cx: &mut Context<Self>) {
        let Some(scratchpad) = self.selected_voice_scratchpad_mut() else {
            return;
        };
        let spans = {
            let registry = scratchpad.selection.registry.borrow();
            let Some(last) = registry.entries().len().checked_sub(1) else {
                return;
            };
            registry.resolve((0, 0), (last, usize::MAX))
        };
        scratchpad
            .selection
            .selection
            .borrow_mut()
            .set_spans(spans);
        let spans = scratchpad.selection.selection.borrow().spans().to_vec();
        if let Some((start, end)) = scratchpad.transcript.spans_endpoints(&spans) {
            scratchpad.transcript.caret_anchor = Some(start);
            scratchpad.transcript.caret = Some(end);
        }
        cx.notify();
    }

    /// Enter while the panel is up sends the whole transcript as one
    /// message on the viewed chat and ends that session — a typed draft is
    /// untouched.
    pub(super) fn submit_voice_scratchpad(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let text = self
            .voice_scratchpads
            .get(&session_id)
            .map(|scratchpad| scratchpad.transcript.to_message())
            .unwrap_or_default()
            .trim()
            .to_owned();
        if text.is_empty() {
            return;
        }
        self.drop_voice_scratchpad(session_id);
        self.submit_composer_submission_to(session_id, ComposerSubmission::plain(text), cx);
    }

    /// End the visible chat's session and discard its transcript.
    pub(super) fn end_voice_scratchpad(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        self.drop_voice_scratchpad(session_id);
        cx.notify();
    }

    /// End every chat's session — the experiment flag going off takes the
    /// whole surface down, so nothing keeps a scratchpad that can no
    /// longer render.
    pub(super) fn end_all_voice_scratchpads(&mut self, cx: &mut Context<Self>) {
        if self.voice_scratchpads.is_empty() {
            return;
        }
        let mut owned_tap = false;
        for (_, scratchpad) in self.voice_scratchpads.drain() {
            owned_tap |= scratchpad.capture_live;
            scratchpad.stop.store(true, Ordering::Relaxed);
        }
        if owned_tap {
            self.detach_voice_sink();
        }
        cx.notify();
    }

    /// Remove one chat's scratchpad, tearing down the tap when it was the
    /// one holding it.
    fn drop_voice_scratchpad(&mut self, session_id: Uuid) {
        let Some(scratchpad) = self.voice_scratchpads.remove(&session_id) else {
            return;
        };
        scratchpad.stop.store(true, Ordering::Relaxed);
        if scratchpad.capture_live {
            self.detach_voice_sink();
        }
    }

    /// Selection or the session list moved: every scratchpad but the
    /// visible chat's pauses — its worker exits and its socket closes
    /// rather than holding a gateway connection in the background — and a
    /// session that no longer exists loses its scratchpad with it. Paused
    /// sessions land muted so returning shows the transcript until the
    /// user unmutes to resume.
    pub(super) fn sync_voice_scratchpad_capture(&mut self) {
        let selected = self.state.selected_session;
        let sessions = &self.state.sessions;
        let mut detached = false;
        self.voice_scratchpads.retain(|id, scratchpad| {
            let session_exists = sessions.iter().any(|session| session.id == *id);
            if !session_exists || Some(*id) != selected {
                detached |= scratchpad.capture_live;
                scratchpad.stop_capture();
                scratchpad.muted = true;
            }
            session_exists
        });
        if detached {
            self.detach_voice_sink();
        }
    }

    /// Mirror the mic's availability into every scratchpad — one engine
    /// feeds them all. The status row shows it; the tap itself rebinds
    /// inside the platform layer when the device returns.
    pub(super) fn set_voice_input_unavailable(&mut self, unavailable: bool) {
        for scratchpad in self.voice_scratchpads.values_mut() {
            scratchpad.input_unavailable = unavailable;
        }
    }

    /// The sink detaches (samples stop leaving the tap — the worker
    /// notices the channel drop on its own), and the mic engine returns
    /// to whatever else still wants it.
    fn detach_voice_sink(&mut self) {
        crate::platform::set_voice_audio_sink(None);
        self.maybe_stop_voice_listener();
    }

    /// The visible chat's scratchpad holding text Enter would send — the
    /// composer's send button mirrors its enabled state on this.
    pub(super) fn voice_scratchpad_sendable(&self) -> bool {
        self.voice_scratchpad_visible()
            && self
                .selected_voice_scratchpad()
                .is_some_and(|scratchpad| scratchpad.transcript.has_content())
    }

    /// Post every queued cleanup span to the text model — one background
    /// task each, its answer landing back as a `Cleaned` event.
    fn drain_cleanup_requests(&mut self, cx: &mut Context<Self>) {
        let requests: Vec<(Uuid, CleanupRequest)> = self
            .voice_scratchpads
            .iter_mut()
            .flat_map(|(session_id, scratchpad)| {
                let requests = std::mem::take(&mut scratchpad.transcript.cleanup_requests);
                scratchpad
                    .transcript
                    .cleanup_inflight
                    .extend(requests.iter().cloned());
                requests
                    .into_iter()
                    .map(move |request| (*session_id, request))
            })
            .collect();
        for (session_id, request) in requests {
            self.spawn_dictation_cleanup(session_id, request, cx);
        }
    }

    /// One cleanup call: fetch the gateway key from the daemon like the
    /// transcription worker does, post the finished span, and send the
    /// answer through the event channel. Every failure — no credential,
    /// a timeout, an empty or unchanged answer — keeps the raw text and
    /// reports `cleaned: None` so the span's in-flight marker still
    /// clears.
    fn spawn_dictation_cleanup(
        &mut self,
        session_id: Uuid,
        request: CleanupRequest,
        cx: &mut Context<Self>,
    ) {
        let daemon = self.daemon.client();
        let http = cx.http_client();
        let executor = cx.background_executor().clone();
        let events = self.voice_scratchpad_tx.clone();
        let wake = self.event_wake_tx.clone();
        cx.background_executor()
            .spawn(async move {
                let key = daemon
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
                    .filter(|key| !key.trim().is_empty());
                let cleaned = match key {
                    Some(key) => {
                        // A spoken edit applies its instruction to the
                        // target's whole text; a plain request just
                        // scrubs dictation artifacts.
                        let (instructions, content) = match &request.edit {
                            Some(instruction) => (
                                EDIT_INSTRUCTIONS,
                                format!(
                                    "Text:\n{}\n\nEdit instruction: {}",
                                    request.raw, instruction
                                ),
                            ),
                            None => (CLEANUP_INSTRUCTIONS, request.raw.clone()),
                        };
                        let body = serde_json::json!({
                            "model": CLEANUP_MODEL_ID,
                            "messages": [
                                {"role": "system", "content": instructions},
                                {"role": "user", "content": content},
                            ],
                        });
                        match super::voice_briefing::post_json(
                            &http,
                            &executor,
                            super::voice_briefing::CHAT_COMPLETIONS_URL,
                            &key,
                            InferenceProvider::VercelGateway,
                            None,
                            &body,
                        )
                        .await
                        {
                            Ok(parsed) => parsed
                                .pointer("/choices/0/message/content")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|text| !text.is_empty() && *text != request.raw.trim())
                                .map(str::to_owned),
                            Err(error) => {
                                eprintln!("Goddard: voice scratchpad cleanup failed: {error:#}");
                                None
                            }
                        }
                    }
                    None => None,
                };
                let _ = events.send((
                    session_id,
                    0,
                    ScratchpadEvent::Cleaned {
                        target: request.target,
                        start: request.start,
                        raw: request.raw,
                        cleaned,
                        edit: request.edit.is_some(),
                    },
                ));
                signal_event_pump(&wake);
            })
            .detach();
    }

    /// Drain worker and permission answers into the transcript model —
    /// events stamped with a retired generation are a dead worker's mail.
    pub(super) fn drain_voice_scratchpad_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        let mut detach_sink = false;
        while let Ok((session_id, generation, event)) = self.voice_scratchpad_events.try_recv() {
            // The mic answer carries generation 0 — it predates any worker.
            // A prompt answered after a switch leaves the session paused;
            // unmuting on return connects it then.
            if let ScratchpadEvent::MicAccess(granted) = event {
                let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) else {
                    continue;
                };
                changed = true;
                if !granted {
                    scratchpad.status = ScratchpadStatus::MicDenied;
                } else if self.state.selected_session == Some(session_id) {
                    self.begin_voice_capture(session_id, cx);
                }
                continue;
            }
            // A cleanup answer belongs to no worker generation — it
            // verifies against the transcript itself, so a reconnect's
            // generation bump must not drop it.
            if let ScratchpadEvent::Cleaned {
                target,
                start,
                raw,
                cleaned,
                edit,
            } = event
            {
                if let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) {
                    // The call settled — its tail spinner clears whether
                    // the answer lands or the failure kept the raw text.
                    // Clearing it before the morph records means a landed
                    // answer swaps spinner for crossfade in one paint.
                    let inflight = &mut scratchpad.transcript.cleanup_inflight;
                    if let Some(at) = inflight.iter().position(|request| {
                        request.target == target && request.start == start && request.raw == raw
                    }) {
                        inflight.remove(at);
                        changed = true;
                    }
                    if let Some(cleaned) = cleaned {
                        let before =
                            scratchpad.transcript.clean_target_text(target).to_owned();
                        if scratchpad
                            .transcript
                            .apply_cleanup(target, start, &raw, &cleaned, edit)
                        {
                            changed = true;
                            scratchpad.morph_landed(target, before);
                        }
                    }
                }
                continue;
            }
            let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) else {
                continue;
            };
            if generation != scratchpad.generation {
                continue;
            }
            changed = true;
            // Fresh transcript text lifts a pause even without a resume
            // marker.
            if scratchpad.status == ScratchpadStatus::Paused
                && matches!(
                    event,
                    ScratchpadEvent::Final(_)
                        | ScratchpadEvent::FinalSegment(_)
                        | ScratchpadEvent::Partial(_)
                )
            {
                scratchpad.status = ScratchpadStatus::Live;
            }
            match event {
                ScratchpadEvent::MicAccess(_) | ScratchpadEvent::Cleaned { .. } => {}
                ScratchpadEvent::Connected => {
                    if matches!(
                        scratchpad.status,
                        ScratchpadStatus::Connecting | ScratchpadStatus::Paused
                    ) {
                        scratchpad.status = ScratchpadStatus::Live;
                    }
                }
                ScratchpadEvent::Final(text) => {
                    scratchpad.transcript.append_finalized(&text);
                }
                ScratchpadEvent::FinalSegment(text) => {
                    scratchpad.transcript.apply_final_segment(&text);
                }
                ScratchpadEvent::Partial(text) => {
                    scratchpad.transcript.set_interim(text);
                }
                ScratchpadEvent::Paused => {
                    if scratchpad.status == ScratchpadStatus::Live {
                        scratchpad.status = ScratchpadStatus::Paused;
                    }
                }
                ScratchpadEvent::Resumed => {
                    if scratchpad.status == ScratchpadStatus::Paused {
                        scratchpad.status = ScratchpadStatus::Live;
                    }
                }
                ScratchpadEvent::Reconnecting => {
                    scratchpad.status = ScratchpadStatus::Connecting;
                    // Same bookkeeping as `Failed`: the stream that owed
                    // the folded words a delivery is dead — a reconnect's
                    // finals must not strip against it.
                    scratchpad.transcript.interim_folded.clear();
                }
                ScratchpadEvent::Failed => {
                    scratchpad.status = ScratchpadStatus::ConnectionLost;
                    // The stream that owed the folded words a delivery is
                    // dead — a reconnect's finals must not strip against it.
                    scratchpad.transcript.interim_folded.clear();
                }
                ScratchpadEvent::Stopped => {
                    // The silence auto-stop retired the worker mid-session —
                    // land the pad muted with its transcript kept; unmuting
                    // reconnects through the usual resume path. Any interim
                    // still hanging gray commits without fold credit (the
                    // dead stream owes no delivery), and leftover credit
                    // clears for the same reason as `Failed`.
                    scratchpad.transcript.solidify_interim();
                    scratchpad.muted = true;
                    scratchpad.status = ScratchpadStatus::Live;
                    detach_sink |= scratchpad.capture_live;
                    scratchpad.stop_capture();
                }
            }
        }
        if detach_sink {
            self.detach_voice_sink();
        }
        self.drain_cleanup_requests(cx);
        changed
    }

    /// The panel — one continuous card spanning the content region and
    /// reaching behind the composer lane, so the transcript scrolls under
    /// a soft edge instead of a hard clip.
    pub(super) fn render_voice_scratchpad(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let lane = self.composer_lane_height.get();
        let Some(scratchpad) = self.selected_voice_scratchpad() else {
            return div().into_any_element();
        };
        if scratchpad.follow_tail {
            scratchpad.scroll.scroll_to_bottom();
        }
        let scroll = scratchpad.scroll.clone();
        let scrollbar = scratchpad.scrollbar.clone();
        let selection = scratchpad.selection.clone();
        let edit_focus = scratchpad.edit_focus.clone();
        // The caret paints while the surface holds focus and no selection
        // or annotation box is standing in for it.
        let caret_glyph = (edit_focus.is_focused(window)
            && scratchpad.transcript.annotation_target.is_none()
            && selection.selection.borrow().is_empty())
        .then(|| {
            scratchpad
                .transcript
                .caret
                .map(|caret| (scratchpad.transcript.node_key(caret.node), caret.offset))
        })
        .flatten();
        let weak = cx.entity().downgrade();
        // The card underlaps the composer lane only as far as the composer
        // card's own bottom edge — the footer's strip stays outside it.
        let overlap = lane - FOOTER_STRIP;
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .px(px(20.0 - COMPOSER_OVERHANG))
            // The card runs through the composer lane: the negative margin
            // pulls the slot's height over the lane's, and the lane —
            // painted after — sits on top of the card's bottom edge.
            .mb(px(-overlap))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .max_w(px(CARD_MAX_WIDTH))
                    .mx_auto()
                    .relative()
                    .flex()
                    .flex_col()
                    .rounded(px(CARD_RADIUS))
                    .bg(theme.composer)
                    .overflow_hidden()
                    .child(
                        div()
                            .id("vs-content-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .on_scroll_wheel({
                                let scroll = scroll.clone();
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    // GPUI's own scroll handler already ran —
                                    // the offset is post-scroll. Reaching the
                                    // bottom re-follows the append point.
                                    let at_bottom =
                                        scroll.offset().y <= px(4.0) - scroll.max_offset().y;
                                    let _ = weak.update(cx, |this, _| {
                                        if let Some(scratchpad) = this.selected_voice_scratchpad_mut() {
                                            scratchpad.follow_tail = at_bottom;
                                        }
                                    });
                                    contain_scroll(&scroll, cx);
                                }
                            })
                            .child(self.render_scratchpad_rows(&theme, window, cx)),
                    )
                    .child(
                        // The card's outline is a child rather than the
                        // element's border: GPUI paints an element's border
                        // after its children, so an element border would sit
                        // over the gradient cover instead of dissolving into
                        // it the way the frame's bottom edge does.
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(CARD_RADIUS))
                            .border(hairline())
                            .border_color(theme.border_subtle),
                    )
                    .child(
                        // The gradient cover's solid two-thirds hides behind
                        // the composer. Painted after the scroll element and
                        // the border overlay so it obscures whatever scrolls
                        // beneath it and the card's own outline — the frame
                        // fades the transcript into the window surface, not
                        // the card.
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(overlap + GRADIENT_RISE))
                            .bg(linear_gradient(
                                180.0,
                                linear_color_stop(theme.surface.opacity(0.0), 0.0),
                                linear_color_stop(theme.surface, GRADIENT_FADE_END),
                            )),
                    )
                    .child({
                        let prefix = tr!("voice_scratchpad.hint_prefix");
                        let command = tr!("voice_scratchpad.hint_command");
                        let middle = tr!("voice_scratchpad.hint_middle");
                        let enter = tr!("voice_scratchpad.hint_enter");
                        let suffix = tr!("voice_scratchpad.hint_suffix");
                        let ui_family = crate::fonts::current(cx).ui;
                        let mut bold = font(ui_family.clone());
                        bold.weight = FontWeight::BOLD;
                        let run = |len: usize, font: gpui::Font| TextRun {
                            len,
                            font,
                            color: theme.text_tertiary,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        div()
                            .absolute()
                            .left(px(24.0))
                            .bottom(px(overlap + HINT_CLEARANCE))
                            .text_size(sp(12.0))
                            .child(
                                gpui::StyledText::new(format!(
                                    "{prefix}{command}{middle}{enter}{suffix}"
                                ))
                                .with_runs(vec![
                                    run(prefix.len(), font(ui_family.clone())),
                                    run(command.len(), bold.clone()),
                                    run(middle.len(), font(ui_family.clone())),
                                    run(enter.len(), bold),
                                    run(suffix.len(), font(ui_family)),
                                ]),
                            )
                    })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom(px(overlap))
                            .right_0()
                            .child(scrollbar::vertical(&scroll, &scrollbar)),
                    )
                    .child(
                        // Selection listeners ride a Normal hitbox over the
                        // content region — the same wiring the transcript uses.
                        canvas(
                            |bounds, window, _| {
                                window.insert_hitbox(bounds, HitboxBehavior::Normal).id
                            },
                            move |_, region, window, _cx| {
                                md::render::install_selection_input(
                                    region, window, &selection, None,
                                );
                                // Registered after install's listeners:
                                // bubble order is reverse registration, so
                                // this runs ahead of the drag's release()
                                // and sees the live gesture. A press that
                                // settled into real spans hands the surface
                                // keyboard focus — typing then edits that
                                // selection, not the draft. A mouse-up
                                // without a live drag — a click into the
                                // composer while a selection still stands —
                                // leaves focus where the press put it.
                                window.on_mouse_event({
                                    let selection = selection.clone();
                                    let focus = edit_focus.clone();
                                    move |_: &MouseUpEvent, phase, window, cx| {
                                        if phase != DispatchPhase::Bubble {
                                            return;
                                        }
                                        let grabbed = {
                                            let selection =
                                                selection.selection.borrow();
                                            selection.is_dragging()
                                                && !selection.is_empty()
                                        };
                                        if grabbed {
                                            window.focus(&focus, cx);
                                        }
                                    }
                                });
                                if let Some((key, offset)) = &caret_glyph
                                    && let Some(rect) =
                                        scratchpad_caret_rect(&selection, key, *offset)
                                {
                                    window.paint_quad(fill(rect, theme.accent));
                                }
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom(px(overlap)),
                    )
                    .child(
                        // The fixed control row: painted last so the pills
                        // sit over the gradient cover and ahead of the
                        // selection canvas's hitbox — clicks reach the
                        // buttons, content fades under them.
                        self.render_scratchpad_controls(overlap, &theme, cx),
                    ),
            )
            .into_any_element()
    }

    /// The fixed control row — Mute/Hide/Cancel pills anchored 12px above
    /// the composer card's top edge, right-aligned at the frame's 18px
    /// inset. The row is a sibling of the scroll region, so it never
    /// scrolls; the caller paints it last so it reads over the bottom fade.
    fn render_scratchpad_controls(&self, overlap: f32, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let Some(scratchpad) = self.selected_voice_scratchpad() else {
            return div();
        };
        let muted = scratchpad.muted;
        div()
            .absolute()
            .right(px(CONTROLS_INSET))
            .bottom(px(overlap + CONTROLS_CLEARANCE))
            .h(px(32.0))
            .flex()
            .items_center()
            .gap(px(9.0))
            .child(self.scratchpad_pill(
                "vs-mute",
                &scratchpad.mute_focus,
                if muted {
                    tr!("voice_scratchpad.unmute")
                } else {
                    tr!("voice_scratchpad.mute")
                },
                // "Mute" sets the width — the narrower label keeps the
                // pill constant across the toggle without over-widening.
                Some(tr!("voice_scratchpad.mute")),
                24.0,
                true,
                theme,
                |this, _window, cx| {
                    let muted = this
                        .selected_voice_scratchpad()
                        .is_some_and(|scratchpad| !scratchpad.muted);
                    this.set_voice_scratchpad_muted(muted, cx);
                },
                cx,
            ))
            .child(self.scratchpad_pill(
                "vs-hide",
                &scratchpad.hide_focus,
                tr!("voice_scratchpad.hide"),
                None,
                18.0,
                false,
                theme,
                |this, window, cx| {
                    if let Some(scratchpad) = this.selected_voice_scratchpad_mut() {
                        scratchpad.hidden = true;
                    }
                    let focus = this.composer_focus(cx);
                    window.focus(&focus, cx);
                    cx.notify();
                },
                cx,
            ))
            .child(self.scratchpad_pill(
                "vs-cancel",
                &scratchpad.cancel_focus,
                tr!("voice_scratchpad.cancel"),
                None,
                18.0,
                false,
                theme,
                |this, window, cx| this.request_cancel_voice_scratchpad(window, cx),
                cx,
            ))
    }

    /// One control-row pill: Mute wears the solid dark treatment with an
    /// enabled hairline, Hide and Cancel the card's solid fill, borderless
    /// but lifted by the frame's 1px/4px drop shadow — the frame's fills.
    fn scratchpad_pill(
        &self,
        id: &'static str,
        focus: &FocusHandle,
        label: String,
        size_to: Option<String>,
        h_pad: f32,
        primary: bool,
        theme: &Theme,
        action: fn(&mut Self, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let weak = cx.entity().downgrade();
        let label: SharedString = label.into();
        div()
            .id(id)
            .track_focus(focus)
            .tab_index(0)
            .h(px(32.0))
            .px(px(h_pad))
            .flex_none()
            .rounded(px(PILL_RADIUS))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(14.0))
            .when(primary, |pill| {
                pill.border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.inverse)
                    .text_color(theme.on_inverse)
            })
            .when(!primary, |pill| {
                pill.bg(theme.composer).text_color(theme.text).shadow(vec![
                    gpui::BoxShadow::new(px(0.0), px(1.0), gpui::black().opacity(0.043))
                        .blur_radius(px(4.0)),
                ])
            })
            .focus_visible(|pill| pill.border(hairline()).border_color(theme.accent))
            .hover(|pill| pill.opacity(0.88))
            .active(|pill| pill.opacity(0.75))
            .child(match size_to {
                // The wider label rides invisibly under the real one so
                // the pill's width never moves between states.
                Some(wide) if wide != label => div()
                    .relative()
                    .flex_none()
                    .child(div().invisible().child(wide))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(label),
                    )
                    .into_any_element(),
                _ => label.into_any_element(),
            })
            .on_click({
                let weak = weak.clone();
                move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| action(this, window, cx));
                }
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    let _ = weak.update(cx, |this, cx| action(this, window, cx));
                    cx.stop_propagation();
                }
            })
    }

    /// The transcript rows: paragraphs separated by hairlines, bullets under
    /// their paragraph, the recording dot at the live append point, and an
    /// annotation box hanging off its target while one is open. The whole
    /// column is the edit surface — a settled selection, a caret-placing
    /// click on an unwritten line, or a Tab landing puts its focus here;
    /// typing replaces the selection while a bare caret takes nothing.
    fn render_scratchpad_rows(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(scratchpad) = self.selected_voice_scratchpad_mut() else {
            return div().into_any_element();
        };
        let now = Instant::now();
        // Reduce-motion skips the transitions entirely — fades and morphs
        // settle instantly and no repaint ticks get leased.
        let animate = !cx.reduce_motion();
        scratchpad.prune_cleanup_morphs(now);
        if scratchpad
            .interim_crossfade
            .as_ref()
            .is_some_and(|crossfade| now.duration_since(crossfade.started) >= WORD_FADE)
        {
            scratchpad.interim_crossfade = None;
        }
        // Every row's focus handle exists before the paint loop — it then
        // works off a shared borrow so the status row and an open
        // annotation box can render through `self` beside it. The same
        // pass diffs each painted buffer for the word-fade — typed text
        // (`edited`) lands instantly, dictation materializes.
        let mut row_focuses = Vec::with_capacity(scratchpad.transcript.paragraphs.len());
        for (index, paragraph) in scratchpad.transcript.paragraphs.iter().enumerate() {
            let paragraph_focus = scratchpad
                .row_focuses
                .entry(format!("p{index}"))
                .or_insert_with(|| cx.focus_handle())
                .clone();
            let bullet_focuses: Vec<FocusHandle> = (0..paragraph.bullets.len())
                .map(|bullet| {
                    scratchpad
                        .row_focuses
                        .entry(format!("b{index}-{bullet}"))
                        .or_insert_with(|| cx.focus_handle())
                        .clone()
                })
                .collect();
            row_focuses.push((paragraph_focus, bullet_focuses));
            note_text_fade(
                &mut scratchpad.text_fades,
                format!("p{index}"),
                paragraph.text.len(),
                animate && !paragraph.edited,
                now,
            );
            for (bullet_index, bullet) in paragraph.bullets.iter().enumerate() {
                note_text_fade(
                    &mut scratchpad.text_fades,
                    format!("b{index}-{bullet_index}"),
                    bullet.text.len(),
                    // `edited` is paragraph-scoped — it covers the bullets
                    // a caret or selection edit touched.
                    animate && !paragraph.edited,
                    now,
                );
            }
        }
        let interim_len = scratchpad_interim_len(&scratchpad.transcript.interim);
        note_text_fade(
            &mut scratchpad.text_fades,
            "interim".to_owned(),
            interim_len,
            animate,
            now,
        );
        let annotation_len = scratchpad.annotation_display_len();
        note_text_fade(
            &mut scratchpad.text_fades,
            "annotation".to_owned(),
            annotation_len,
            animate,
            now,
        );
        // A delivery that committed painted gray text: diff the append
        // point's painted row against last frame's record — a landing
        // crossfades its displaced tail while the matched prefix skips
        // the word-fade.
        scratchpad.note_painted_row(animate, now);
        let motion_live = animate && scratchpad.motion_live();
        let Some(scratchpad) = self.selected_voice_scratchpad() else {
            return div().into_any_element();
        };
        let transcript = &scratchpad.transcript;
        // An unplugged mic grays the record dot the way muting does.
        let muted = scratchpad.muted || scratchpad.input_unavailable;
        let status = scratchpad.status;
        let annotation_target = transcript.annotation_target;
        let annotating = annotation_target.is_some();
        let selection = scratchpad.selection.clone();
        let edit_focus = scratchpad.edit_focus.clone();
        let ui_family = crate::fonts::current(cx).ui;
        let status_row = self.render_scratchpad_status(theme, cx);
        let mut blocks = div()
            .id("vs-rows")
            .w_full()
            .track_focus(&edit_focus)
            .tab_index(0)
            .px(px(24.0))
            .pt(px(TITLE_TOP_INSET))
            // Room for the hint line plus the composer overlap.
            .pb(px(lane_padding(self.composer_lane_height.get())))
            .text_size(sp(14.0))
            .text_color(theme.text)
            .on_key_down(cx.listener(Self::voice_scratchpad_edit_key))
            // A fresh press retires the caret — the drag re-selects and the
            // click handlers sort out annotation or a new insertion point.
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if let Some(scratchpad) = this.selected_voice_scratchpad_mut()
                    && scratchpad.transcript.caret.take().is_some()
                {
                    scratchpad.transcript.caret_anchor = None;
                    cx.notify();
                }
            }))
            // Painted before any row, so the frame's selection registry
            // holds exactly the text elements this frame put on screen.
            .child(md::render::frame_reset(selection.clone()))
            .child(
                // The "VoicePad" header is the column's first row — it
                // scrolls off with the transcript instead of pinning.
                div()
                    .w_full()
                    .mb(px(TITLE_BOTTOM_GAP))
                    .text_size(sp(14.0))
                    .line_height(sp(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_tertiary.opacity(0.6))
                    .child(tr!("voice_scratchpad.title")),
            )
            .children(status_row);
        let mut first_drawn = true;
        for (index, paragraph) in transcript.paragraphs.iter().enumerate() {
            let is_current = index + 1 == transcript.paragraphs.len();
            let annotated = matches!(
                annotation_target,
                Some(AnnotationTarget::Paragraph(target)) if target == index
            );
            if !first_drawn {
                blocks = blocks.child(
                    div()
                        .h(hairline())
                        .w_full()
                        .my(px(14.0))
                        .bg(theme.separator.opacity(0.4)),
                );
            }
            first_drawn = false;
            // The dot marks the point speech actually lands: the live row
            // while no annotation bubble is open, the bubble while one is.
            let show_dot = is_current && annotation_target.is_none();
            let cleaning =
                transcript.is_cleaning(CleanTarget::Node(ScratchpadNode::Paragraph(index)));
            let morph = if animate {
                scratchpad.cleanup_morph(CleanTarget::Node(ScratchpadNode::Paragraph(index)))
            } else {
                None
            };
            let morph_t = morph.map(|morph| {
                (morph.started.elapsed().as_secs_f32() / CLEANUP_MORPH.as_secs_f32()).min(1.0)
            });
            let fades = scratchpad.fade_boundaries(&format!("p{index}"), now);
            let interim_fades = if is_current && !annotating {
                scratchpad.fade_boundaries("interim", now)
            } else {
                Vec::new()
            };
            let flat = scratchpad_paragraph_text(
                &paragraph.text,
                if is_current && !annotating {
                    &transcript.interim
                } else {
                    ""
                },
                &fades,
                &interim_fades,
                morph_t.map(ease_in_out).unwrap_or(1.0),
                &ui_family,
                theme,
            );
            // The swap already landed in the model — the morph only
            // repaints: the row's prior text lifts and dissolves while the
            // rewritten span resolves beneath it.
            let ghost = morph.zip(morph_t).map(|(morph, t)| {
                scratchpad_morph_ghost(
                    &morph.old,
                    t,
                    2.0,
                    0.0,
                    if show_dot { 19.0 } else { 0.0 },
                    theme.text,
                    &ui_family,
                )
            });
            // A landing's displaced tail dissolves over the row that
            // painted it while the residual resolves beneath — the same
            // overlay the cleanup morph rides, held in place.
            let landing_ghost = if animate {
                scratchpad
                    .interim_crossfade
                    .as_ref()
                    .filter(|crossfade| crossfade.key == format!("p{index}"))
                    .map(|crossfade| {
                        let t = (now
                            .duration_since(crossfade.started)
                            .as_secs_f32()
                            / WORD_FADE.as_secs_f32())
                        .min(1.0);
                        scratchpad_landing_ghost(
                            &crossfade.ghost,
                            crossfade.tail_start,
                            1.0 - t,
                            2.0,
                            0.0,
                            if show_dot { 19.0 } else { 0.0 },
                            theme.text,
                            theme.text_tertiary,
                            &ui_family,
                        )
                    })
            } else {
                None
            };
            let paragraph_focus = row_focuses[index].0.clone();
            let paragraph_div = div()
                .id(SharedString::from(format!("vs-paragraph-{index}")))
                .w_full()
                .relative()
                .rounded(px(4.0))
                .py(px(2.0))
                .cursor_default()
                .track_focus(&paragraph_focus)
                .tab_index(0)
                .when(annotated, |row| {
                    row.bg(MarkdownPalette::from_theme(theme).annotation)
                })
                .focus_visible(|row| row.bg(theme.focus_highlight()))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_start()
                        .gap(px(4.0))
                        .child(md::render::selectable_flat_text(
                            &flat,
                            md::selection::TextKey::new(format!("vs-p-{index}"), 0),
                            selection.clone(),
                            theme.code_wash,
                            theme.selection,
                            false,
                        ))
                        .when(cleaning, |row| {
                            row.child(scratchpad_cleanup_spinner(14.0, theme))
                        })
                        .when(show_dot, |row| {
                            row.child(scratchpad_dot_on_line(14.0, muted, status, theme))
                        }),
                )
                .when_some(ghost, |row, ghost| row.child(ghost))
                .when_some(landing_ghost, |row, ghost| row.child(ghost))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    if let Some(scratchpad) = this.selected_voice_scratchpad_mut()
                        && !scratchpad_click_was_drag(event, &scratchpad.selection)
                        && !scratchpad.transcript.annotate(index)
                    {
                        // No box opened — the line had nothing written
                        // and the caret landed as the insertion point.
                        // The surface takes the keys now.
                        let focus = scratchpad.edit_focus.clone();
                        window.focus(&focus, cx);
                    }
                    this.drain_cleanup_requests(cx);
                    cx.stop_propagation();
                    cx.notify();
                }))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if !event.keystroke.modifiers.modified()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        if let Some(scratchpad) = this.selected_voice_scratchpad_mut()
                            && !scratchpad.transcript.annotate(index)
                        {
                            let focus = scratchpad.edit_focus.clone();
                            window.focus(&focus, cx);
                        }
                        this.drain_cleanup_requests(cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                }));
            blocks = blocks.child(paragraph_div.when(annotated, |row| {
                row.child(self.render_annotation_box(index, theme, window, cx))
            }));
            for (bullet_index, bullet) in paragraph.bullets.iter().enumerate() {
                let bullet_annotated = matches!(
                    annotation_target,
                    Some(AnnotationTarget::Bullet { paragraph: p, bullet: b, .. })
                        if p == index && b == bullet_index
                );
                let bullet_focus = row_focuses[index].1[bullet_index].clone();
                let morph = if animate {
                    scratchpad.cleanup_morph(CleanTarget::Node(ScratchpadNode::Bullet(
                        index,
                        bullet_index,
                    )))
                } else {
                    None
                };
                let morph_t = morph.map(|morph| {
                    (morph.started.elapsed().as_secs_f32() / CLEANUP_MORPH.as_secs_f32()).min(1.0)
                });
                let fades =
                    scratchpad.fade_boundaries(&format!("b{index}-{bullet_index}"), now);
                let mut runs = Vec::new();
                push_fade_runs(
                    &mut runs,
                    0,
                    bullet.text.len(),
                    &fades,
                    morph_t.map(ease_in_out).unwrap_or(1.0),
                    &font(ui_family.clone()),
                    theme.text_secondary,
                );
                let indent = 18.0 + bullet.depth as f32 * BULLET_INDENT;
                let ghost = morph.zip(morph_t).map(|(morph, t)| {
                    scratchpad_morph_ghost(
                        &morph.old,
                        t,
                        2.0,
                        indent + BULLET_INDENT,
                        0.0,
                        theme.text_secondary,
                        &ui_family,
                    )
                });
                let flat = md::render::FlatText {
                    text: bullet.text.clone().into(),
                    runs,
                    links: Vec::new(),
                    code_ranges: Vec::new(),
                    atom_ranges: Vec::new(),
                    annotation_refs: Vec::new(),
                    commit_refs: Vec::new(),
                    file_refs: Vec::new(),
                    math: None,
                    copy: Rc::default(),
                };
                let bullet_row = div()
                    .id(SharedString::from(format!("vs-bullet-{index}-{bullet_index}")))
                    .w_full()
                    .relative()
                    .rounded(px(4.0))
                    .pl(px(indent))
                    .py(px(2.0))
                    .flex()
                    .gap(px(8.0))
                    .cursor_default()
                    .track_focus(&bullet_focus)
                    .tab_index(0)
                    .when(bullet_annotated, |row| {
                        row.bg(MarkdownPalette::from_theme(theme).annotation)
                    })
                    .focus_visible(|row| row.bg(theme.focus_highlight()))
                    .child(
                        // The disc centers on the first text line — GPUI's
                        // default phi line height leaves `(size × φ −
                        // BULLET_SIZE) / 2` of leading over it.
                        div()
                            .flex_none()
                            .mt(px((14.0 * 1.618_034 - BULLET_SIZE).max(0.0) / 2.0))
                            .size(px(BULLET_SIZE))
                            .rounded_full()
                            .bg(theme.text_tertiary),
                    )
                    .child(md::render::selectable_flat_text(
                        &flat,
                        md::selection::TextKey::new(format!("vs-b-{index}"), bullet_index),
                        selection.clone(),
                        theme.code_wash,
                        theme.selection,
                        false,
                    ))
                    .when(
                        transcript.is_cleaning(CleanTarget::Node(ScratchpadNode::Bullet(
                            index,
                            bullet_index,
                        ))),
                        |row| row.child(scratchpad_cleanup_spinner(14.0, theme)),
                    )
                    .when_some(ghost, |row, ghost| row.child(ghost))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if let Some(scratchpad) = this.selected_voice_scratchpad_mut()
                            && !scratchpad_click_was_drag(event, &scratchpad.selection)
                        {
                            scratchpad.transcript.annotate_bullet(index, bullet_index);
                        }
                        this.drain_cleanup_requests(cx);
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            if let Some(scratchpad) = this.selected_voice_scratchpad_mut() {
                                scratchpad.transcript.annotate_bullet(index, bullet_index);
                            }
                            this.drain_cleanup_requests(cx);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }));
                blocks = blocks.child(bullet_row.when(bullet_annotated, |row| {
                    row.child(self.render_annotation_box(index, theme, window, cx))
                }));
            }
        }
        // A fresh session's empty scratchpad still shows its live row —
        // interim speech paints beside the dot until the first finalized
        // chunk gives it a paragraph to land in.
        if transcript.paragraphs.is_empty() {
            let flat = scratchpad_paragraph_text(
                "",
                &transcript.interim,
                &[],
                &scratchpad.fade_boundaries("interim", now),
                1.0,
                &ui_family,
                theme,
            );
            blocks = blocks.child(
                div()
                    .id("vs-live-row")
                    .w_full()
                    .py(px(2.0))
                    .cursor_default()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_start()
                            .gap(px(4.0))
                            .child(md::render::selectable_flat_text(
                                &flat,
                                md::selection::TextKey::new("vs-p-live", 0),
                                selection.clone(),
                                theme.code_wash,
                                theme.selection,
                                false,
                            ))
                            .child(scratchpad_dot_on_line(14.0, muted, status, theme)),
                    )
                    // Nothing is written here yet — the live row is the
                    // insertion point, so the click lands the caret and
                    // the surface's keys rather than annotating.
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if let Some(scratchpad) = this.selected_voice_scratchpad_mut()
                            && !scratchpad_click_was_drag(event, &scratchpad.selection)
                        {
                            let caret = scratchpad.transcript.append_point_caret();
                            scratchpad.transcript.caret = Some(caret);
                            scratchpad.transcript.caret_anchor = None;
                            let focus = scratchpad.edit_focus.clone();
                            window.focus(&focus, cx);
                        }
                        cx.stop_propagation();
                        cx.notify();
                    })),
            );
        }
        let element = blocks
            // Clicking open space finishes an open annotation as a bullet
            // and drops the editing caret — focus goes home to the
            // composer so typing resumes the draft. A drag that ended over
            // open space is no click-out: the gesture only meant to
            // select.
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                if let Some(scratchpad) = this.selected_voice_scratchpad_mut() {
                    if scratchpad_click_was_drag(event, &scratchpad.selection) {
                        return;
                    }
                    let mut changed = false;
                    if scratchpad.transcript.annotation_target.is_some() {
                        scratchpad.transcript.commit_annotation();
                        changed = true;
                    }
                    if scratchpad.transcript.caret.take().is_some() {
                        scratchpad.transcript.caret_anchor = None;
                        changed = true;
                    }
                    if scratchpad.edit_focus.is_focused(window) {
                        let focus = this.composer_focus(cx);
                        window.focus(&focus, cx);
                        changed = true;
                    }
                    if changed {
                        cx.notify();
                    }
                }
                this.drain_cleanup_requests(cx);
            }))
            .into_any_element();
        // Fades and morphs ride the shared pulse clock — the window
        // repaints at ~30fps only while a transition is in flight.
        if motion_live {
            motion::pulse_lease(window.current_view(), cx);
        }
        element
    }

    /// The failure row at the top of the content — mic denial, a dropped
    /// stream, or a reported pause — with its remedies beside it.
    /// `Connecting` earns no chrome; the dot's presence already reads as
    /// waiting. `Paused` shows no Retry — the worker is still live, and a
    /// second spawn would split the shared audio queue.
    fn render_scratchpad_status(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let scratchpad = self.selected_voice_scratchpad()?;
        let (message, can_retry, system_settings) = match scratchpad.status {
            ScratchpadStatus::MicDenied => (tr!("voice_scratchpad.mic_denied"), true, true),
            _ if scratchpad.input_unavailable => {
                (tr!("voice_scratchpad.mic_unavailable"), false, false)
            }
            ScratchpadStatus::Connecting | ScratchpadStatus::Live => return None,
            ScratchpadStatus::Paused => (tr!("voice_scratchpad.paused"), false, false),
            ScratchpadStatus::ConnectionLost => {
                (tr!("voice_scratchpad.connection_lost"), true, false)
            }
        };
        Some(
            div()
                .w_full()
                .mb(px(12.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(10.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.inset)
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(sp(13.0))
                        .text_color(theme.text_secondary)
                        .child(message),
                )
                .when(can_retry, |row| {
                    row.child(
                        div()
                            .id("vs-status-retry")
                            .h(px(24.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .cursor_default()
                            .hover(|row| row.bg(theme.overlay))
                            .child(tr!("voice_scratchpad.retry"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let session_id = this.state.selected_session;
                                let denied = this
                                    .selected_voice_scratchpad()
                                    .is_some_and(|scratchpad| {
                                        scratchpad.status == ScratchpadStatus::MicDenied
                                    });
                                let Some(session_id) = session_id else {
                                    return;
                                };
                                if denied {
                                    // Re-ask TCC — the prompt replays only when the
                                    // user removed access, otherwise the answer
                                    // lands immediately.
                                    match crate::platform::microphone_access() {
                                        crate::platform::CaptureAccess::Granted => {
                                            this.begin_voice_capture(session_id, cx)
                                        }
                                        crate::platform::CaptureAccess::Undetermined => {
                                            let tx = this.voice_scratchpad_tx.clone();
                                            let wake = this.event_wake_tx.clone();
                                            crate::platform::request_microphone_access(Box::new(
                                                move |granted| {
                                                    let _ = tx.try_send((
                                                        session_id,
                                                        0,
                                                        ScratchpadEvent::MicAccess(granted),
                                                    ));
                                                    signal_event_pump(&wake);
                                                },
                                            ));
                                        }
                                        crate::platform::CaptureAccess::Denied => {}
                                    }
                                } else {
                                    // Reattach the tap too — a session paused
                                    // by a chat switch needs it back before the
                                    // respawned worker has anything to stream.
                                    this.begin_voice_capture(session_id, cx);
                                }
                            })),
                    )
                })
                .when(system_settings, |row| {
                    row.child(
                        div()
                            .id("vs-status-settings")
                            .h(px(24.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .cursor_default()
                            .hover(|row| row.bg(theme.overlay))
                            .child(tr!("voice_scratchpad.open_settings"))
                            .on_click(|_, _, cx| {
                                cx.open_url(
                                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
                                );
                            }),
                    )
                }),
        )
    }

    /// The floating annotation box anchored under its paragraph — the red
    /// dot lives inside it while speech becomes its bullet.
    fn render_annotation_box(
        &self,
        _index: usize,
        theme: &Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(scratchpad) = self.selected_voice_scratchpad() else {
            return div().into_any_element();
        };
        let mut text = scratchpad.transcript.annotation_text.clone();
        let interim = strip_interim_terminators(&scratchpad.transcript.annotation_interim);
        append_word_text(&mut text, &interim);
        let morph = if !cx.reduce_motion() {
            scratchpad.cleanup_morph(CleanTarget::Annotation)
        } else {
            None
        };
        let morph_t = morph.map(|morph| {
            (morph.started.elapsed().as_secs_f32() / CLEANUP_MORPH.as_secs_f32()).min(1.0)
        });
        let fades = scratchpad.fade_boundaries("annotation", Instant::now());
        if text.is_empty() {
            text = tr!("voice_scratchpad.annotation_hint");
        }
        let text_element: AnyElement = if morph_t.is_some() || !fades.is_empty() {
            let ui_family = crate::fonts::current(cx).ui;
            let mut runs = Vec::new();
            push_fade_runs(
                &mut runs,
                0,
                text.len(),
                &fades,
                morph_t.map(ease_in_out).unwrap_or(1.0),
                &font(ui_family),
                theme.text_secondary,
            );
            gpui::StyledText::new(text).with_runs(runs).into_any_element()
        } else {
            text.into_any_element()
        };
        let ghost = morph.zip(morph_t).map(|(morph, t)| {
            scratchpad_morph_ghost(
                &morph.old,
                t,
                0.0,
                0.0,
                0.0,
                theme.text_secondary,
                &crate::fonts::current(cx).ui,
            )
        });
        let landing_ghost = if !cx.reduce_motion() {
            scratchpad
                .interim_crossfade
                .as_ref()
                .filter(|crossfade| crossfade.key == "annotation")
                .map(|crossfade| {
                    let t = (Instant::now()
                        .duration_since(crossfade.started)
                        .as_secs_f32()
                        / WORD_FADE.as_secs_f32())
                    .min(1.0);
                    scratchpad_landing_ghost(
                        &crossfade.ghost,
                        crossfade.tail_start,
                        1.0 - t,
                        0.0,
                        0.0,
                        0.0,
                        theme.text_secondary,
                        theme.text_secondary,
                        &crate::fonts::current(cx).ui,
                    )
                })
        } else {
            None
        };
        let box_content = div()
            .min_w(px(240.0))
            .max_w(px(360.0))
            .p(px(8.0))
            .rounded(px(11.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.raised)
            .shadow_lg()
            .text_size(sp(13.0))
            .text_color(theme.text_secondary)
            .flex()
            .gap(px(8.0))
            .items_start()
            // Clicks inside the box don't re-target the paragraph.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(scratchpad_dot_on_line(
                13.0,
                scratchpad.muted || scratchpad.input_unavailable,
                scratchpad.status,
                theme,
            ))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .relative()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(px(4.0))
                    .child(text_element)
                    .when(
                        scratchpad.transcript.is_cleaning(CleanTarget::Annotation),
                        |row| row.child(scratchpad_cleanup_spinner(13.0, theme)),
                    )
                    .when_some(ghost, |row, ghost| row.child(ghost))
                    .when_some(landing_ghost, |row, ghost| row.child(ghost)),
            );
        deferred(FloatingSurface::anchored_to_parent(
            box_content.into_any_element(),
            MenuAlign::BelowLeft,
            px(4.0),
            px(8.0),
        ))
        .into_any_element()
    }

    /// The discard confirmation — the same modal posture the close dialog
    /// uses, armed by Cancel once the transcript is substantial.
    pub(super) fn render_scratchpad_discard(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = Theme::current(cx);
        let scratchpad = self.selected_voice_scratchpad()?;
        if !scratchpad.confirm_discard {
            return None;
        }
        let keep_focus = scratchpad.keep_focus.clone();
        let discard_focus = scratchpad.discard_focus.clone();
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                // Clicking the scrim keeps the transcript — same as Keep.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        if let Some(scratchpad) = this.selected_voice_scratchpad_mut() {
                            scratchpad.confirm_discard = false;
                        }
                        cx.notify();
                    }),
                )
                .child(
                    canvas(
                        |bounds, window, _| {
                            window.insert_hitbox(bounds, HitboxBehavior::BlockMouse).id
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(320.0))
                        .rounded(px(14.0))
                        .border(hairline())
                        .border_color(theme.border_subtle)
                        .bg(theme.raised)
                        .shadow_lg()
                        .px(px(16.0))
                        .pt(px(14.0))
                        .pb(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .text_size(sp(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(tr!("voice_scratchpad.discard_title")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.0))
                                .justify_end()
                                .child({
                                    let focus = keep_focus;
                                    div()
                                        .id("vs-discard-keep")
                                        .track_focus(&focus)
                                        .tab_index(0)
                                        .h(px(26.0))
                                        .px(px(12.0))
                                        .rounded(px(7.0))
                                        .flex()
                                        .items_center()
                                        .text_size(sp(13.0))
                                        .text_color(theme.text)
                                        .cursor_default()
                                        .hover(|row| row.bg(theme.overlay))
                                        .focus_visible(|row| row.bg(theme.overlay))
                                        .child(tr!("voice_scratchpad.discard_keep"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(scratchpad) = this.selected_voice_scratchpad_mut() {
                                                scratchpad.confirm_discard = false;
                                            }
                                            cx.notify();
                                        }))
                                        .on_key_down(cx.listener(
                                            |this, event: &KeyDownEvent, _, cx| {
                                                if !event.keystroke.modifiers.modified()
                                                    && matches!(
                                                        event.keystroke.key.as_str(),
                                                        "enter" | "space"
                                                    )
                                                {
                                                    if let Some(scratchpad) =
                                                        this.selected_voice_scratchpad_mut()
                                                    {
                                                        scratchpad.confirm_discard = false;
                                                    }
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                }
                                            },
                                        ))
                                })
                                .child({
                                    let focus = discard_focus;
                                    div()
                                        .id("vs-discard-confirm")
                                        .track_focus(&focus)
                                        .tab_index(0)
                                        .h(px(26.0))
                                        .px(px(12.0))
                                        .rounded(px(7.0))
                                        .flex()
                                        .items_center()
                                        .text_size(sp(13.0))
                                        .text_color(theme.danger)
                                        .cursor_default()
                                        .hover(|row| row.bg(theme.overlay))
                                        .focus_visible(|row| row.bg(theme.overlay))
                                        .child(tr!("voice_scratchpad.discard"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.end_voice_scratchpad(cx);
                                        }))
                                        .on_key_down(cx.listener(
                                            |this, event: &KeyDownEvent, _, cx| {
                                                if !event.keystroke.modifiers.modified()
                                                    && matches!(
                                                        event.keystroke.key.as_str(),
                                                        "enter" | "space"
                                                    )
                                                {
                                                    this.end_voice_scratchpad(cx);
                                                    cx.stop_propagation();
                                                }
                                            },
                                        ))
                                }),
                        ),
                ),
        )
    }
}

/// Whether a click on the transcript surface was really a drag — a drag
/// is still in flight, the gesture left a settled selection, or the
/// pointer ran past the 4px slop the annotation press uses. Row click
/// handlers gate on it so a drag-select never fires the annotate or
/// click-out actions the press would have meant — including handlers
/// registered after the selection listeners, which run ahead of the
/// drag's release() in bubble order and would otherwise see an empty
/// settled grab.
fn scratchpad_click_was_drag(click: &ClickEvent, selection: &TranscriptSelection) -> bool {
    {
        let selection = selection.selection.borrow();
        if selection.is_dragging() || !selection.is_empty() {
            return true;
        }
    }
    match click {
        ClickEvent::Mouse(event) => {
            let moved = event.up.position - event.down.position;
            moved.x.abs() > px(4.0) || moved.y.abs() > px(4.0)
        }
        _ => false,
    }
}

/// Bottom padding under the transcript rows — the cover's whole height plus
/// the controls' own 12px clearance, so the last paragraph and the live row
/// rest fully above the fade, clear of the floating pills.
fn lane_padding(lane: f32) -> f32 {
    lane - FOOTER_STRIP + GRADIENT_RISE + CONTROLS_CLEARANCE
}

/// The caret quad for `offset` in the registered element `key` — an accent
/// sliver on its glyph row, the same 1.5px the composer's caret paints.
fn scratchpad_caret_rect(
    selection: &TranscriptSelection,
    key: &md::selection::TextKey,
    offset: usize,
) -> Option<Bounds<Pixels>> {
    let registry = selection.registry.borrow();
    let entry = registry.entries().iter().find(|entry| entry.key == *key)?;
    if entry.geometry.is_missing() {
        return None;
    }
    let md::render::TextGeometry::Text(layout) = &entry.geometry else {
        return None;
    };
    let origin = layout.position_for_index(offset.min(entry.text.len()))?;
    Some(Bounds::new(origin, size(px(1.5), layout.line_height())))
}

/// Split `text[lo..hi]` into styled runs at each `(offset, alpha)` fade
/// boundary — the span from a boundary to the next one (or `hi`) paints
/// at `color` scaled by `alpha × scale`. Boundaries outside the region
/// just set the alpha it opens with.
fn push_fade_runs(
    runs: &mut Vec<TextRun>,
    lo: usize,
    hi: usize,
    fades: &[(usize, f32)],
    scale: f32,
    font: &Font,
    color: Hsla,
) {
    let mut pos = lo;
    let mut alpha = 1.0;
    for &(start, next) in fades {
        let start = start.clamp(lo, hi);
        if start > pos {
            runs.push(TextRun {
                len: start - pos,
                font: font.clone(),
                color: color.opacity(alpha * scale),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            pos = start;
        }
        alpha = next;
    }
    if pos < hi {
        runs.push(TextRun {
            len: hi - pos,
            font: font.clone(),
            color: color.opacity(alpha * scale),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
}

/// A paragraph's FlatText: finalized speech in ink, the interim suffix
/// dimmed — the "appears dimmed, snaps to black on finalize" rule.
/// `fades` are committed-text `(offset, alpha)` boundaries and
/// `interim_fades` the same relative to the stripped interim tail;
/// `text_alpha` dims the committed region wholesale during a morph's
/// resolve.
fn scratchpad_paragraph_text(
    text: &str,
    interim: &str,
    fades: &[(usize, f32)],
    interim_fades: &[(usize, f32)],
    text_alpha: f32,
    ui_family: &SharedString,
    theme: &Theme,
) -> md::render::FlatText {
    let mut full = text.to_owned();
    let split = full.len();
    let stripped = strip_interim_terminators(interim);
    append_word_text(&mut full, &stripped);
    let interim_start = full.len() - stripped.len();
    let ui_font = font(ui_family.clone());
    let mut runs = Vec::new();
    push_fade_runs(&mut runs, 0, split, fades, text_alpha, &ui_font, theme.text);
    let shifted: Vec<(usize, f32)> = interim_fades
        .iter()
        .map(|&(start, alpha)| (start + interim_start, alpha))
        .collect();
    push_fade_runs(
        &mut runs,
        split,
        full.len(),
        &shifted,
        1.0,
        &ui_font,
        theme.text_tertiary,
    );
    md::render::FlatText {
        text: full.into(),
        runs,
        links: Vec::new(),
        code_ranges: Vec::new(),
        atom_ranges: Vec::new(),
        annotation_refs: Vec::new(),
        commit_refs: Vec::new(),
        file_refs: Vec::new(),
        math: None,
        copy: Rc::default(),
    }
}

/// The glowing record dot: the frame's solid core and hairline, with its
/// pink glow painted as the design's inner shadow. While live it breathes
/// — the shared pulse clock sweeps its opacity between ~70% and 100% over
/// a second and a half; muted and dead states keep their dim look and
/// stay still.
fn scratchpad_dot(muted: bool, status: ScratchpadStatus, theme: &Theme) -> AnyElement {
    let live = !muted
        && !matches!(
            status,
            ScratchpadStatus::MicDenied | ScratchpadStatus::ConnectionLost
        );
    let core: Hsla = if live {
        rgb(RECORDING_RED).into()
    } else {
        theme.text_tertiary
    };
    let glow: Hsla = if live {
        rgb(RECORDING_GLOW).into()
    } else {
        theme.text_tertiary.opacity(0.5)
    };
    let dot = move || {
        div()
            .flex_none()
            .size(px(15.0))
            .rounded_full()
            .bg(core)
            .border(px(1.0))
            .border_color(gpui::black().opacity(0.10))
            .shadow(vec![
                gpui::BoxShadow::new(px(0.0), px(2.0), glow)
                    .blur_radius(px(4.0))
                    .inset(),
            ])
    };
    if !live {
        return dot().into_any_element();
    }
    // Phase 0 sits at full opacity — reduce-motion's constant first frame
    // keeps the dot's original solid look.
    motion::pulse(RECORDING_DOT_PERIOD, move |phase| {
        dot()
            .opacity(0.85 + 0.15 * (phase * std::f32::consts::TAU).cos())
            .into_any_element()
    })
    .into_any_element()
}

/// The dissolving half of a cleanup morph: the buffer's prior text
/// painted at `1 − t` and lifting `3t`px while the rewritten span
/// resolves beneath. `top`/`left`/`right` pin its wrap column to the
/// row's text block.
fn scratchpad_morph_ghost(
    old: &str,
    t: f32,
    top: f32,
    left: f32,
    right: f32,
    color: Hsla,
    ui_family: &SharedString,
) -> Div {
    div()
        .absolute()
        .top(px(top - 3.0 * t))
        .left(px(left))
        .right(px(right))
        .child(
            gpui::StyledText::new(old.to_owned()).with_runs(vec![TextRun {
                len: old.len(),
                font: font(ui_family.clone()),
                color: color.opacity(1.0 - t),
                background_color: None,
                underline: None,
                strikethrough: None,
            }]),
        )
}

/// The dissolving half of an interim landing: the row's painted text
/// before the delivery — the settled head in `ink`, the gray tail from
/// `tail_start` in `tail` — at `alpha` while the residual resolves
/// beneath it. Unlike the cleanup morph the overlay holds its place:
/// the tail dissolves where it sat. `top`/`left`/`right` pin its wrap
/// column to the row's text block.
fn scratchpad_landing_ghost(
    ghost: &str,
    tail_start: usize,
    alpha: f32,
    top: f32,
    left: f32,
    right: f32,
    ink: Hsla,
    tail: Hsla,
    ui_family: &SharedString,
) -> Div {
    let tail_start = tail_start.min(ghost.len());
    let mut runs = Vec::with_capacity(2);
    for (start, end, color) in [
        (0, tail_start, ink),
        (tail_start, ghost.len(), tail),
    ] {
        if end > start {
            runs.push(TextRun {
                len: end - start,
                font: font(ui_family.clone()),
                color: color.opacity(alpha),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
    }
    div()
        .absolute()
        .top(px(top))
        .left(px(left))
        .right(px(right))
        .child(gpui::StyledText::new(ghost.to_owned()).with_runs(runs))
}

/// The dot's offset centers it on the first text line — GPUI's default phi
/// line height leaves `(size × φ − 15) / 2` of leading over a 15px dot.
fn scratchpad_dot_on_line(
    text_size: f32,
    muted: bool,
    status: ScratchpadStatus,
    theme: &Theme,
) -> Div {
    div()
        .flex_none()
        .mt(px((text_size * 1.618_034 - 15.0).max(0.0) / 2.0))
        .child(scratchpad_dot(muted, status, theme))
}

/// The cleanup spinner at a cleaning node's tail — the same shared-clock
/// loader every spinner rides, offset to sit on the first text line the
/// way the record dot does.
fn scratchpad_cleanup_spinner(text_size: f32, theme: &Theme) -> Div {
    div()
        .flex_none()
        .mt(px((text_size * 1.618_034 - 12.0).max(0.0) / 2.0))
        .child(motion::spin(icon(
            "icons/loader-circle.svg",
            12.0,
            theme.text_tertiary,
        )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_command_splits_on_word_pairs() {
        assert_eq!(split_next_command("okay next"), Some(("", "")));
        assert_eq!(split_next_command("ok next"), Some(("", "")));
        assert_eq!(
            split_next_command("first point. Okay, next. second point"),
            Some(("first point.", "second point"))
        );
        // "okaynext" is one word — no command.
        assert_eq!(split_next_command("okaynext"), None);
        // "next" alone is not a command.
        assert_eq!(split_next_command("next"), None);
        // The command is consumed mid-sentence too — the tradeoff the
        // design accepts for a word-boundary scan.
        assert_eq!(
            split_next_command("say okay next please"),
            Some(("say", "please"))
        );
        assert_eq!(split_next_command("okay next week"), Some(("", "week")));
        // Trailing punctuation rides with the command.
        assert_eq!(
            split_next_command("a point. ok next. then this"),
            Some(("a point.", "then this"))
        );
    }

    #[test]
    fn finalized_text_builds_paragraphs() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("First thought, then");
        transcript.append_finalized("okay next");
        transcript.append_finalized("the second idea");
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[0].text, "First thought, then");
        assert_eq!(transcript.paragraphs[1].text, "the second idea");
        assert_eq!(
            transcript.to_message(),
            "First thought, then\n\nthe second idea"
        );
    }

    #[test]
    fn next_command_split_across_chunks_still_breaks() {
        // Streaming deltas are word-level — the command's two halves
        // almost always arrive in separate finalized chunks.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("First thought okay");
        transcript.append_finalized("next second idea");
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[0].text, "First thought");
        assert_eq!(transcript.paragraphs[1].text, "second idea");
        assert_eq!(transcript.to_message(), "First thought\n\nsecond idea");
    }

    #[test]
    fn next_command_seam_ignores_punctuation_and_case() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("end of point. Okay,");
        transcript.append_finalized("NEXT, moving on");
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[0].text, "end of point.");
        assert_eq!(transcript.paragraphs[1].text, "moving on");
        // A tail that isn't "ok"/"okay" leaves "next" as plain speech.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("come");
        transcript.append_finalized("next week");
        assert_eq!(transcript.to_message(), "come next week");
    }

    #[test]
    fn final_segment_after_deltas_does_not_repeat() {
        // The whole segment arrives again as `transcript-final` — dedup
        // against the raw stream, not folded text (which no longer holds
        // the consumed command).
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("one okay ");
        transcript.append_finalized("next two");
        transcript.apply_final_segment("one okay next two");
        assert_eq!(transcript.to_message(), "one\n\ntwo");
    }

    #[test]
    fn annotation_commit_split_across_chunks() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("fix the login okay");
        transcript.append_finalized("next check the redirect");
        assert_eq!(transcript.paragraphs[0].bullets, vec!["fix the login"]);
        assert_eq!(transcript.annotation_text, "check the redirect");
    }

    #[test]
    fn multiple_commands_in_one_segment() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("one okay next two ok next three");
        assert_eq!(transcript.paragraphs.len(), 3);
        assert_eq!(transcript.to_message(), "one\n\ntwo\n\nthree");
    }

    #[test]
    fn next_command_on_empty_paragraph_moves_nothing() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("okay next");
        // One empty paragraph exists — the command didn't stack another.
        assert_eq!(transcript.paragraphs.len(), 1);
        assert!(transcript.to_message().is_empty());
    }

    #[test]
    fn annotation_mode_commits_bullets() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan okay next");
        transcript.annotate(0);
        transcript.append_finalized("fix the login flow");
        transcript.append_finalized("okay next");
        transcript.append_finalized("check the redirect");
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[0].bullets, vec!["fix the login flow"]);
        // The open box's pending text becomes a bullet on send.
        assert_eq!(
            transcript.to_message(),
            "the plan\n- fix the login flow\n- check the redirect"
        );
    }

    #[test]
    fn interim_is_replaced_not_appended() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.set_interim("hel".to_owned());
        transcript.set_interim("hello".to_owned());
        transcript.append_finalized("hello");
        transcript.set_interim("wor".to_owned());
        assert_eq!(transcript.to_message(), "hello wor");
    }

    #[test]
    fn interim_command_breaks_the_paragraph_early() {
        // The paragraph break lands on the partial carrying the command —
        // the finalized segment re-delivers the consumed span and must
        // neither append it again nor break a second time.
        let mut transcript = ScratchpadTranscript::default();
        transcript.set_interim("first".to_owned());
        transcript.set_interim("first point okay next".to_owned());
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[0].text, "first point");
        transcript.set_interim("first point okay next second".to_owned());
        assert_eq!(transcript.interim, "second");
        transcript.apply_final_segment("first point okay next second idea");
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[1].text, "second idea");
        assert_eq!(transcript.to_message(), "first point\n\nsecond idea");
    }

    #[test]
    fn interim_command_breaks_early_over_deltas() {
        // Same early split, but the folded span returns as word-level
        // deltas — each strips its folded credit piecewise.
        let mut transcript = ScratchpadTranscript::default();
        transcript.set_interim("first point okay next second".to_owned());
        assert_eq!(transcript.paragraphs.len(), 2);
        transcript.append_finalized("first point ");
        transcript.append_finalized("okay next ");
        transcript.append_finalized("second idea");
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[0].text, "first point");
        assert_eq!(transcript.paragraphs[1].text, "second idea");
        assert_eq!(transcript.to_message(), "first point\n\nsecond idea");
    }

    #[test]
    fn interim_command_straddles_the_finalized_seam() {
        // "okay" already committed when the partial opens with "next" —
        // the seam check runs on partials too.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("first point okay");
        transcript.set_interim("next, moving on".to_owned());
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.paragraphs[0].text, "first point");
        assert_eq!(transcript.interim, "moving on");
        transcript.append_finalized("next moving on");
        assert_eq!(transcript.paragraphs[1].text, "moving on");
        assert_eq!(transcript.to_message(), "first point\n\nmoving on");
    }

    #[test]
    fn interim_drops_words_the_stream_already_finalized() {
        // A partial's head can repeat words a delta just committed —
        // the delivered overlap strips off instead of duplicating.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the sentence");
        transcript.set_interim("the sentence trails".to_owned());
        assert_eq!(transcript.interim, "trails");
        assert_eq!(transcript.to_message(), "the sentence trails");
    }

    #[test]
    fn interim_command_commits_the_annotation_bullet() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.set_interim("fix the login okay next".to_owned());
        assert_eq!(transcript.paragraphs[0].bullets, vec!["fix the login"]);
        transcript.apply_final_segment("fix the login okay next check the redirect");
        assert_eq!(transcript.annotation_text, "check the redirect");
    }

    #[test]
    fn punctuation_solidifies_onto_the_prior_word() {
        // Delta chunking can split a period or comma onto its own chunk —
        // it attaches to the last word, not after a fresh space.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the sentence");
        transcript.append_finalized(".");
        transcript.append_finalized("and a clause");
        transcript.append_finalized(", too");
        assert_eq!(transcript.to_message(), "the sentence. and a clause, too");
    }

    #[test]
    fn stray_punctuation_drops_instead_of_leading_a_phrase() {
        // After a pause the recognizer emits punctuation alone — it glues
        // onto the speech it trails, or drops at a fresh append point,
        // including the fresh paragraph an "okay next" just opened.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized(".");
        transcript.append_finalized("first phrase");
        transcript.append_finalized(",");
        transcript.append_finalized("okay next ,-");
        transcript.append_finalized("second");
        assert_eq!(transcript.to_message(), "first phrase,\n\nsecond");
    }

    #[test]
    fn stray_punctuation_strips_off_the_next_delivery() {
        // The stray emission can spill into the following chunk — its
        // leading punctuation run strips rather than head the phrase.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("first okay next");
        transcript.append_finalized(",-");
        transcript.append_finalized("., second");
        assert_eq!(transcript.to_message(), "first\n\nsecond");
    }

    #[test]
    fn interim_stray_punctuation_follows_the_same_rule() {
        // A punctuation-only partial drops at a fresh append point and
        // arms the strip for the next partial.
        let mut transcript = ScratchpadTranscript::default();
        transcript.set_interim(".".to_owned());
        assert!(transcript.interim.is_empty());
        transcript.set_interim("., next".to_owned());
        assert_eq!(transcript.interim, "next");
        // Glued onto speech it trails, it stays provisional — and its
        // final retires the provisional twin rather than the interim.
        transcript.append_finalized("the sentence");
        transcript.set_interim(".".to_owned());
        assert_eq!(transcript.interim, ".");
        assert_eq!(transcript.to_message(), "the sentence.");
        transcript.append_finalized(".");
        assert_eq!(transcript.paragraphs[0].text, "the sentence.");
        assert!(transcript.interim.is_empty());
    }

    #[test]
    fn interim_keeps_its_undelivered_tail() {
        // Finalizing the interim's head leaves the still-provisional
        // tail dimmed in place — clearing it flashed the suffix away
        // until the next partial repainted it.
        let mut transcript = ScratchpadTranscript::default();
        transcript.set_interim("the sentence trails".to_owned());
        transcript.append_finalized("the sentence");
        assert_eq!(transcript.interim, "trails");
        assert_eq!(transcript.to_message(), "the sentence trails");
        // A delivery that rewrites the interim's head supersedes it.
        transcript.set_interim("old guess".to_owned());
        transcript.append_finalized("new words");
        assert!(transcript.interim.is_empty());
    }

    #[test]
    fn mute_solidifies_the_pending_interim() {
        // Muting commits the gray suffix in place — with no fold credit,
        // since the torn-down stream owes no re-delivery: speech opening
        // on the same words after the reconnect appends in full.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the sentence");
        transcript.set_interim("still forming".to_owned());
        transcript.solidify_interim();
        assert!(transcript.interim.is_empty());
        assert_eq!(transcript.to_message(), "the sentence still forming");
        transcript.set_interim("still forming, continued".to_owned());
        assert_eq!(transcript.interim, "still forming, continued");
    }

    #[test]
    fn auto_stop_solidifies_without_fold_credit() {
        // The stopped stream owes no delivery — the committed words carry
        // no strip credit, so a reconnect's speech appends in full.
        let mut transcript = ScratchpadTranscript::default();
        transcript.set_interim("half a thought".to_owned());
        transcript.solidify_interim();
        assert!(transcript.interim.is_empty());
        assert_eq!(transcript.to_message(), "half a thought");
        transcript.append_finalized("half a thought again");
        assert_eq!(transcript.to_message(), "half a thought half a thought again");
    }

    #[test]
    fn mute_solidifies_the_annotation_interim() {
        // An open box's provisional text commits into the box the same
        // way — muted dictation keeps the words it already showed.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.set_interim("note forming".to_owned());
        transcript.solidify_interim();
        assert!(transcript.annotation_interim.is_empty());
        assert_eq!(transcript.annotation_text, "note forming");
        assert_eq!(transcript.to_message(), "the plan\n- note forming");
    }

    #[test]
    fn bullet_annotation_commits_at_the_slot_in_order() {
        // A bullet-targeted box nests its commits under the bullet and
        // the slot advances per commit — consecutive "okay next" bullets
        // keep speech order as children.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("first okay next");
        transcript.append_finalized("second okay next");
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("inserted one okay next");
        transcript.append_finalized("inserted two okay next");
        let bullets = &transcript.paragraphs[0].bullets;
        assert_eq!(
            *bullets,
            vec!["first", "inserted one", "inserted two", "second"]
        );
        assert_eq!(
            bullets.iter().map(|bullet| bullet.depth).collect::<Vec<_>>(),
            vec![0, 1, 1, 0]
        );
    }

    #[test]
    fn open_bullet_box_sends_pending_text_at_the_slot() {
        // Enter sends what the user sees — an open box's uncommitted text
        // commits at its insert slot, nested under its bullet.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("first okay next");
        transcript.append_finalized("second okay next");
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("still talking");
        assert_eq!(
            transcript.to_message(),
            "the plan\n- first\n  - still talking\n- second"
        );
    }

    #[test]
    fn click_out_commits_the_annotation_at_its_slot() {
        // Clicking away finishes the open box — nothing dictated into it
        // is lost, and the append point returns to the live row.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan okay next");
        transcript.annotate(0);
        transcript.append_finalized("first okay next");
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("still talking");
        transcript.set_interim("for a".to_owned());
        transcript.commit_annotation();
        assert!(transcript.annotation_target.is_none());
        assert_eq!(
            transcript.paragraphs[0].bullets,
            vec!["first", "still talking for a"]
        );
        // The provisional tail's re-delivery strips instead of appending a
        // second copy, and fresh speech lands on the live row again.
        transcript.append_finalized("for a moment more");
        assert_eq!(transcript.paragraphs[1].text, "moment more");
    }

    #[test]
    fn edit_command_splits_on_word_runs() {
        // "Let's" arrives as "lets" or "let" + "'s" — both read as the
        // phrase, and casing and punctuation ride with the command.
        assert_eq!(split_edit_command("okay let's make an edit"), Some(("", "")));
        assert_eq!(split_edit_command("ok lets make an edit"), Some(("", "")));
        assert_eq!(
            split_edit_command("note. Okay, let's make an edit — shorten it"),
            Some(("note.", "shorten it"))
        );
        // Plain speech isn't a command.
        assert_eq!(split_edit_command("let's make an edit"), None);
        assert_eq!(split_edit_command("okay make an edit"), None);
        assert_eq!(split_edit_command("okay let's make edits"), None);
        assert_eq!(split_edit_command("okay let's make an"), None);
    }

    #[test]
    fn edit_phrase_commits_a_rewrite_and_keeps_the_prior_note() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan is ready");
        transcript.annotate(0);
        transcript.append_finalized("a note okay let's make an edit shorten it okay next");
        // The "okay next" ended the instruction — the rewrite dispatched
        // and the box closed; only the pre-phrase words became a bullet.
        assert!(transcript.annotation_target.is_none());
        assert_eq!(transcript.paragraphs[0].bullets, vec!["a note"]);
        let (target, start, raw, instruction) = transcript
            .cleanup_requests
            .iter()
            .find(|request| request.edit.is_some())
            .map(|request| {
                (
                    request.target,
                    request.start,
                    request.raw.clone(),
                    request.edit.clone(),
                )
            })
            .expect("the commit queued the paragraph rewrite");
        assert_eq!(target, CleanTarget::Node(ScratchpadNode::Paragraph(0)));
        assert_eq!(start, 0);
        assert_eq!(raw, "the plan is ready");
        assert_eq!(instruction.as_deref(), Some("shorten it"));
        // Its answer lands through the same verified swap a cleanup takes.
        assert!(transcript.apply_cleanup(target, start, &raw, "Plan ready.", true));
        assert_eq!(transcript.paragraphs[0].text, "Plan ready.");
    }

    #[test]
    fn edit_phrase_straddles_the_committed_seam() {
        // Chunked delivery splits the phrase anywhere — the committed
        // tail pairs with the chunk's head like "okay next" does.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("note okay lets make");
        transcript.append_finalized("an edit tighten the prose");
        assert!(transcript.annotation_edit.is_some());
        assert_eq!(transcript.annotation_text, "note tighten the prose");
        // The provisional tail joins the instruction at commit.
        transcript.set_interim("please".to_owned());
        transcript.commit_annotation();
        assert_eq!(transcript.paragraphs[0].bullets, vec!["note"]);
        let instruction = transcript
            .cleanup_requests
            .iter()
            .find_map(|request| request.edit.clone())
            .expect("the rewrite queued");
        assert_eq!(instruction, "tighten the prose please");
    }

    #[test]
    fn interim_edit_phrase_arms_before_the_segment_lands() {
        // The partial carrying the phrase arms the box ahead of the
        // finalized delivery — which then strips the folded command
        // instead of arming twice or appending its words.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.set_interim("okay lets make an edit tighten it".to_owned());
        assert!(transcript.annotation_edit.is_some());
        assert_eq!(transcript.annotation_interim, "tighten it");
        transcript.apply_final_segment("okay lets make an edit tighten it");
        assert_eq!(transcript.annotation_text, "tighten it");
        assert!(transcript.annotation_interim.is_empty());
    }

    #[test]
    fn edit_phrase_without_an_instruction_rewrites_nothing() {
        // The phrase alone arms the box but dispatches nothing — no
        // request, no bullet, the paragraph untouched.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("okay let's make an edit");
        assert!(transcript.annotation_edit.is_some());
        transcript.commit_annotation();
        assert!(transcript.paragraphs[0].bullets.is_empty());
        assert!(
            transcript
                .cleanup_requests
                .iter()
                .all(|request| request.edit.is_none())
        );
        assert_eq!(transcript.paragraphs[0].text, "the plan");
    }

    #[test]
    fn edit_phrase_off_a_paragraph_box_is_plain_speech() {
        // The phrase only commands while a paragraph's box is open — in
        // main dictation and on a bullet's box it transcribes as words.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("note okay let's make an edit later");
        assert!(transcript.annotation_edit.is_none());
        assert_eq!(
            transcript.paragraphs[0].text,
            "note okay let's make an edit later"
        );
        transcript.annotate(0);
        transcript.append_finalized("a bullet okay next");
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("okay let's make an edit okay next");
        assert!(transcript.annotation_edit.is_none());
        assert_eq!(
            transcript.paragraphs[0].bullets,
            vec!["a bullet", "okay let's make an edit"]
        );
    }

    #[test]
    fn bullet_annotation_nests_one_level_deeper() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("first okay next");
        transcript.append_finalized("second okay next");
        // A box on a bullet writes its children — consecutive "okay next"
        // commits keep speech order at the next depth.
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("child one okay next");
        transcript.append_finalized("child two okay next");
        // And a child's own box nests again, past existing siblings.
        transcript.annotate_bullet(0, 2);
        transcript.append_finalized("grandchild okay next");
        transcript.exit_annotation();
        let rows: Vec<(&str, usize)> = transcript.paragraphs[0]
            .bullets
            .iter()
            .map(|bullet| (bullet.text.as_str(), bullet.depth))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("first", 0),
                ("child one", 1),
                ("child two", 1),
                ("grandchild", 2),
                ("second", 0),
            ]
        );
        assert_eq!(
            transcript.to_message(),
            "the plan\n- first\n  - child one\n  - child two\n    - grandchild\n- second"
        );
    }

    #[test]
    fn bullet_depth_caps_at_nine_levels() {
        // Nine rows in the commits stop nesting — they keep landing in
        // order as siblings at the cap rather than marching further right.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.paragraphs[0].bullets.push(ScratchpadBullet {
            text: "deep".to_owned(),
            depth: MAX_BULLET_DEPTH,
        });
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("still talking okay next");
        transcript.append_finalized("one more okay next");
        let bullets = &transcript.paragraphs[0].bullets;
        assert_eq!(*bullets, vec!["deep", "still talking", "one more"]);
        assert!(
            bullets
                .iter()
                .all(|bullet| bullet.depth == MAX_BULLET_DEPTH)
        );
    }

    #[test]
    fn voice_edit_verifies_the_whole_paragraph_before_landing() {
        // The rewrite only lands while the paragraph still reads exactly
        // as it did at dispatch — a typed change in between keeps it
        // untouched.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("okay let's make an edit tighten it");
        transcript.commit_annotation();
        let (target, raw) = transcript
            .cleanup_requests
            .iter()
            .find(|request| request.edit.is_some())
            .map(|request| (request.target, request.raw.clone()))
            .expect("the rewrite queued");
        let spans = vec![md::selection::Span {
            key: md::selection::TextKey::new("vs-p-0", 0),
            range: "the plan".len().."the plan".len(),
            text: "the plan".into(),
            block_break: false,
            copy: Rc::default(),
        }];
        transcript.apply_selection_edit(&spans, " changed");
        assert!(!transcript.apply_cleanup(target, 0, &raw, "Plan.", true));
        assert_eq!(transcript.paragraphs[0].text, "the plan changed");
    }

    #[test]
    fn voice_edit_applies_over_a_typed_paragraph() {
        // Typing arms a paragraph's no-rewrite guard against the cleanup
        // pass — a spoken edit is the user's own instruction, so it
        // still lands while its raw text verifies whole.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        let spans = vec![md::selection::Span {
            key: md::selection::TextKey::new("vs-p-0", 0),
            range: "the plan".len().."the plan".len(),
            text: "the plan".into(),
            block_break: false,
            copy: Rc::default(),
        }];
        transcript.apply_selection_edit(&spans, " here");
        transcript.annotate(0);
        transcript.append_finalized("okay let's make an edit tighten it");
        transcript.commit_annotation();
        let (target, raw) = transcript
            .cleanup_requests
            .iter()
            .find(|request| request.edit.is_some())
            .map(|request| (request.target, request.raw.clone()))
            .expect("the rewrite queued");
        assert_eq!(raw, "the plan here");
        assert!(transcript.apply_cleanup(target, 0, &raw, "The plan.", true));
        assert_eq!(transcript.paragraphs[0].text, "The plan.");
    }

    #[test]
    fn clicking_an_unwritten_line_lands_the_caret_not_the_box() {
        // The empty paragraph "okay next" left open has nothing to
        // annotate — activating it becomes the dictation insertion point
        // instead of opening a box.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan okay next");
        assert!(!transcript.annotate(1));
        assert!(transcript.annotation_target.is_none());
        assert!(matches!(
            transcript.caret,
            Some(CaretPos {
                node: ScratchpadNode::Paragraph(1),
                offset: 0
            })
        ));
        // The caret sits on the live row — fresh speech writes there.
        transcript.append_finalized("second line");
        assert_eq!(transcript.paragraphs[1].text, "second line");
        // A written line still opens the box, and a missing row opens
        // nothing without touching the box already up.
        assert!(transcript.annotate(0));
        assert!(matches!(
            transcript.annotation_target,
            Some(AnnotationTarget::Paragraph(0))
        ));
        assert!(!transcript.annotate(9));
        assert!(matches!(
            transcript.annotation_target,
            Some(AnnotationTarget::Paragraph(0))
        ));
        // Activating an unwritten line while a box is open commits the
        // box's content at its slot first, the way clicking open space
        // does.
        transcript.append_finalized("a note");
        transcript.exit_annotation();
        transcript.append_finalized("okay next");
        transcript.annotate(0);
        transcript.append_finalized("under the plan");
        assert!(!transcript.annotate(2));
        assert!(transcript.annotation_target.is_none());
        assert_eq!(
            transcript.paragraphs[0].bullets,
            vec!["a note under the plan"]
        );
        assert!(matches!(
            transcript.caret,
            Some(CaretPos {
                node: ScratchpadNode::Paragraph(2),
                offset: 0
            })
        ));
    }

    #[test]
    fn click_gate_covers_every_drag_form() {
        let click = |down: (f32, f32), up: (f32, f32)| {
            ClickEvent::Mouse(gpui::MouseClickEvent {
                down: MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(px(down.0), px(down.1)),
                    ..Default::default()
                },
                up: MouseUpEvent {
                    button: MouseButton::Left,
                    position: point(px(up.0), px(up.1)),
                    ..Default::default()
                },
            })
        };
        let selection = TranscriptSelection::default();
        // A press that stayed inside the slop and selected nothing is a
        // click — it activates the row.
        assert!(!scratchpad_click_was_drag(
            &click((10.0, 10.0), (11.0, 11.0)),
            &selection
        ));
        // A press that ran past the slop is a drag even when it selected
        // nothing — annotate must never see it.
        assert!(scratchpad_click_was_drag(
            &click((10.0, 10.0), (30.0, 10.0)),
            &selection
        ));
        // A drag still in flight counts too: a click listener registered
        // after the selection's runs ahead of release() in bubble order
        // and would otherwise see an empty settled grab.
        selection
            .selection
            .borrow_mut()
            .begin(md::selection::TextKey::new("vs-p-0", 0), 0);
        assert!(scratchpad_click_was_drag(
            &click((10.0, 10.0), (10.0, 10.0)),
            &selection
        ));
        // release() on a click-on-text clears the live flag — a real
        // click still annotates.
        selection.selection.borrow_mut().release();
        assert!(!scratchpad_click_was_drag(
            &click((10.0, 10.0), (10.0, 10.0)),
            &selection
        ));
        // A settled grab counts whatever the pointer did — a keyboard
        // click on a focused row can't annotate over a selection either.
        selection
            .selection
            .borrow_mut()
            .set_spans(vec![md::selection::Span {
                key: md::selection::TextKey::new("vs-p-0", 0),
                range: 0..4,
                text: "word".into(),
                block_break: false,
                copy: Rc::default(),
            }]);
        assert!(scratchpad_click_was_drag(
            &ClickEvent::Keyboard(gpui::KeyboardClickEvent::default()),
            &selection
        ));
    }

    #[test]
    fn selection_edit_replaces_across_paragraph_and_bullet() {
        // A drag over a paragraph's tail into a bullet cuts both painted
        // spans; the typed text lands where the grab began.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("alpha beta okay next");
        transcript.annotate(0);
        transcript.append_finalized("gamma delta okay next");
        transcript.exit_annotation();
        let spans = vec![
            md::selection::Span {
                key: md::selection::TextKey::new("vs-p-0", 0),
                range: 6..10,
                text: "alpha beta".into(),
                block_break: false,
                copy: Rc::default(),
            },
            md::selection::Span {
                key: md::selection::TextKey::new("vs-b-0", 0),
                range: 0..5,
                text: "gamma delta".into(),
                block_break: false,
                copy: Rc::default(),
            },
        ];
        let caret = transcript.apply_selection_edit(&spans, "omega");
        assert_eq!(transcript.paragraphs[0].text, "alpha omega");
        assert_eq!(transcript.paragraphs[0].bullets[0], " delta");
        assert!(matches!(
            caret,
            Some(CaretPos {
                node: ScratchpadNode::Paragraph(0),
                offset
            }) if offset == "alpha omega".len()
        ));
    }

    #[test]
    fn delete_empties_a_bullet_and_reanchors_the_caret() {
        // Cutting a bullet's last word removes its row outright — the
        // caret lands on the paragraph the row hung under.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("keep me okay next second line");
        transcript.annotate(1);
        transcript.append_finalized("gone okay next");
        transcript.exit_annotation();
        let spans = vec![md::selection::Span {
            key: md::selection::TextKey::new("vs-b-1", 0),
            range: 0.."gone".len(),
            text: "gone".into(),
            block_break: false,
            copy: Rc::default(),
        }];
        let caret = transcript.apply_selection_edit(&spans, "");
        assert!(transcript.paragraphs[1].bullets.is_empty());
        assert!(matches!(
            caret,
            Some(CaretPos {
                node: ScratchpadNode::Paragraph(1),
                offset
            }) if offset == "second line".len()
        ));
    }

    #[test]
    fn substantial_arms_the_discard_confirm() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("short");
        assert!(!transcript.substantial());
        transcript.append_finalized("okay next more words");
        assert!(transcript.substantial());
        // ...or one long paragraph over the character bar.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized(&"x".repeat(CANCEL_CONFIRM_CHARS));
        assert!(transcript.substantial());
    }

    #[test]
    fn resampler_downmixes_rate_and_format() {
        let mut resampler = PcmResampler::new(8_000.0);
        let input: Vec<f32> = (0..480).map(|i| (i as f32 / 480.0) * 2.0 - 1.0).collect();
        let mut out = Vec::new();
        resampler.push(&input, 48_000.0, &mut out);
        // 480 samples at 48 kHz is 10 ms — 80 output samples at 8 kHz.
        assert_eq!(out.len(), 80 * 2);
        // Bytes are s16 little-endian.
        let first = i16::from_le_bytes([out[0], out[1]]);
        assert!(first < 0);
        // Silence in, silence out.
        let mut resampler = PcmResampler::new(16_000.0);
        let mut out = Vec::new();
        resampler.push(&[0.0; 4800], 48_000.0, &mut out);
        assert!(out.iter().all(|byte| *byte == 0));
        assert_eq!(out.len(), 1600 * 2);
        // Clipping saturates instead of wrapping.
        let mut out = Vec::new();
        resampler.push(&[2.0; 48], 48_000.0, &mut out);
        let peak = i16::from_le_bytes([out[out.len() - 2], out[out.len() - 1]]);
        assert_eq!(peak, 32767);
    }

    fn dispatched_parts(text: &str) -> (Vec<ScratchpadEvent>, bool) {
        let mut events = Vec::new();
        let terminal = dispatch_stream_part(text, &mut |event| events.push(event));
        (events, terminal)
    }

    #[test]
    fn stream_part_pause_marker_is_state_not_text() {
        for frame in [
            r#"{"type":"transcript-partial","text":"Transcription Paused"}"#,
            r#"{"type":"transcript-delta","delta":"transcription paused"}"#,
            r#"{"type":"transcript-final","text":"  Transcription Paused  "}"#,
        ] {
            let (events, terminal) = dispatched_parts(frame);
            assert!(!terminal, "{frame}");
            assert!(
                matches!(events.as_slice(), [ScratchpadEvent::Paused]),
                "{frame}"
            );
        }
        let (events, terminal) =
            dispatched_parts(r#"{"type":"transcript-partial","text":"Transcription Resumed"}"#);
        assert!(!terminal);
        assert!(matches!(events.as_slice(), [ScratchpadEvent::Resumed]));
        // The words still dictate inside a longer span — only a
        // whole-payload marker is state.
        let (events, _) = dispatched_parts(
            r#"{"type":"transcript-partial","text":"note the transcription paused state"}"#,
        );
        assert!(matches!(events.as_slice(), [ScratchpadEvent::Partial(_)]));
    }

    #[test]
    fn stream_part_dedicated_pause_types() {
        for ty in ["transcription-paused", "transcription.paused", "transcript-paused"] {
            let (events, terminal) = dispatched_parts(&format!(r#"{{"type":"{ty}"}}"#));
            assert!(!terminal, "{ty}");
            assert!(matches!(events.as_slice(), [ScratchpadEvent::Paused]), "{ty}");
        }
        for ty in ["transcription-resumed", "transcription.resumed", "transcript-resumed"] {
            let (events, terminal) = dispatched_parts(&format!(r#"{{"type":"{ty}"}}"#));
            assert!(!terminal, "{ty}");
            assert!(
                matches!(events.as_slice(), [ScratchpadEvent::Resumed]),
                "{ty}"
            );
        }
    }

    #[test]
    fn stream_part_terminal_parts_end_the_socket() {
        // `finish` and `error` end the socket without pre-empting the
        // worker's reconnect verdict — no `Failed` event leaves dispatch.
        let (events, terminal) = dispatched_parts(r#"{"type":"finish","text":"done"}"#);
        assert!(terminal);
        assert!(events.is_empty());
        let (events, terminal) =
            dispatched_parts(r#"{"type":"error","error":{"name":"Error","message":"upstream"}}"#);
        assert!(terminal);
        assert!(events.is_empty());
    }

    #[test]
    fn completed_sentence_end_defers_the_open_tail() {
        // Only a closed sentence — terminator followed by fresh words —
        // counts; the trailing open sentence stays out.
        assert_eq!(completed_sentence_end("one. two"), Some(4));
        assert_eq!(completed_sentence_end("one. two. three"), Some(9));
        assert_eq!(completed_sentence_end("one."), None);
        assert_eq!(completed_sentence_end("one. "), None);
        assert_eq!(completed_sentence_end("one"), None);
        assert_eq!(completed_sentence_end("wait! really? and"), Some(13));
    }

    #[test]
    fn finished_sentences_queue_for_cleanup_behind_the_live_one() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("Um first uh thought. And the");
        assert_eq!(transcript.cleanup_requests.len(), 1);
        let request = &transcript.cleanup_requests[0];
        assert_eq!(
            request.target,
            CleanTarget::Node(ScratchpadNode::Paragraph(0))
        );
        assert_eq!(request.raw, "Um first uh thought.");
        assert_eq!(
            transcript.paragraphs[0].text,
            "Um first uh thought. And the"
        );
        // The second chunk's own closed sentence queues next — the still
        // open tail stays deferred.
        transcript.append_finalized("second one lands. More");
        assert_eq!(transcript.cleanup_requests.len(), 2);
        assert_eq!(
            transcript.cleanup_requests[1].raw,
            "And the second one lands."
        );
    }

    #[test]
    fn next_command_queues_the_closing_paragraphs_tail() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("first point okay");
        transcript.append_finalized("next second point");
        assert_eq!(transcript.paragraphs.len(), 2);
        assert_eq!(transcript.cleanup_requests.len(), 1);
        assert_eq!(transcript.cleanup_requests[0].raw, "first point");
        assert_eq!(
            transcript.cleanup_requests[0].target,
            CleanTarget::Node(ScratchpadNode::Paragraph(0))
        );
        // The new paragraph's boundary starts clean.
        transcript.append_finalized("with more. Another");
        assert_eq!(transcript.cleanup_requests.len(), 2);
        assert_eq!(
            transcript.cleanup_requests[1].target,
            CleanTarget::Node(ScratchpadNode::Paragraph(1))
        );
    }

    #[test]
    fn annotation_commit_queues_the_bullet_it_landed_in() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("fix the login okay");
        transcript.append_finalized("next keep going");
        assert_eq!(transcript.paragraphs[0].bullets, vec!["fix the login"]);
        // Opening the box flushed the paragraph's tail; the commit's
        // request names the bullet, not the emptied box.
        let bullet_request = transcript
            .cleanup_requests
            .iter()
            .find(|request| request.raw == "fix the login")
            .expect("the commit queues its bullet for cleanup");
        assert_eq!(
            bullet_request.target,
            CleanTarget::Node(ScratchpadNode::Bullet(0, 0))
        );
        assert_eq!(bullet_request.start, 0);
    }

    #[test]
    fn cleanup_answer_replaces_its_span_in_place() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("Um first uh thought. And second.");
        let request = &transcript.cleanup_requests[0];
        let (target, start, raw) = (request.target, request.start, request.raw.clone());
        assert!(transcript.apply_cleanup(target, start, &raw, "First thought.", false));
        assert_eq!(transcript.paragraphs[0].text, "First thought. And second.");
        // A missing match leaves the raw text alone.
        assert!(!transcript.apply_cleanup(
            CleanTarget::Node(ScratchpadNode::Paragraph(0)),
            0,
            "not in the transcript",
            "whatever",
            false
        ));
    }

    #[test]
    fn cleanup_answer_finds_a_shifted_span_but_not_an_ambiguous_one() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("um filler. And the rest");
        transcript.append_finalized("goes on. Live");
        assert_eq!(transcript.cleanup_requests.len(), 2);
        let first = transcript.cleanup_requests.remove(0);
        let second = transcript.cleanup_requests.remove(0);
        // The first answer rewrites shorter, shifting the second span's
        // recorded offset — the unique match still finds it.
        assert!(transcript.apply_cleanup(first.target, first.start, &first.raw, "Filler.", false));
        assert!(transcript.apply_cleanup(
            second.target,
            second.start,
            &second.raw,
            "Everything else continues.",
            false
        ));
        assert_eq!(
            transcript.paragraphs[0].text,
            "Filler. Everything else continues. Live"
        );
        // Raw text that appears twice can't be placed once the recorded
        // offset misses — it stays raw.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("note same. and same. Tail");
        assert!(!transcript.apply_cleanup(
            CleanTarget::Node(ScratchpadNode::Paragraph(0)),
            usize::MAX,
            "same.",
            "Different.",
            false
        ));
        assert_eq!(transcript.paragraphs[0].text, "note same. and same. Tail");
    }

    #[test]
    fn cleanup_never_rewrites_a_user_edited_paragraph() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("Um dictated words. More");
        let request = &transcript.cleanup_requests[0];
        let (target, start, raw) = (request.target, request.start, request.raw.clone());
        let spans = vec![md::selection::Span {
            key: md::selection::TextKey::new("vs-p-0", 0),
            range: 0..0,
            text: "Um dictated words. More".into(),
            block_break: false,
            copy: Rc::default(),
        }];
        transcript.apply_selection_edit(&spans, "typed ");
        assert!(!transcript.apply_cleanup(target, start, &raw, "Dictated words.", false));
        assert_eq!(
            transcript.paragraphs[0].text,
            "typed Um dictated words. More"
        );
    }

    #[test]
    fn interim_display_drops_sentence_terminators() {
        assert_eq!(strip_interim_terminators("wait. really?"), "wait really");
        assert_eq!(strip_interim_terminators("e.g. something"), "eg something");
        assert_eq!(strip_interim_terminators("done…"), "done");
        assert_eq!(strip_interim_terminators("comma, stays"), "comma, stays");
        assert_eq!(strip_interim_terminators("   "), "");
    }

    #[test]
    fn partial_shrink_holds_through_the_dwell() {
        // The recognizer clipping the tail inside the dwell keeps the
        // painted text — the comma doesn't flicker.
        let mut transcript = ScratchpadTranscript::default();
        let t0 = Instant::now();
        transcript.set_interim_at("the tail,".to_owned(), t0);
        transcript.set_interim_at("the tail".to_owned(), t0 + Duration::from_millis(40));
        assert_eq!(transcript.interim, "the tail,");
        // The same clip lands once the shown text is dwell-old.
        transcript.set_interim_at("the tail".to_owned(), t0 + Duration::from_millis(400));
        assert_eq!(transcript.interim, "the tail");
    }

    #[test]
    fn partial_punctuation_swaps_hold_but_rewrites_land() {
        let mut transcript = ScratchpadTranscript::default();
        let t0 = Instant::now();
        transcript.set_interim_at("the end.".to_owned(), t0);
        transcript.set_interim_at("the end,".to_owned(), t0 + Duration::from_millis(30));
        assert_eq!(transcript.interim, "the end.");
        // Growth and real rewrites are never held.
        transcript.set_interim_at("the end. really".to_owned(), t0 + Duration::from_millis(60));
        assert_eq!(transcript.interim, "the end. really");
        transcript.set_interim_at("wait, no".to_owned(), t0 + Duration::from_millis(90));
        assert_eq!(transcript.interim, "wait, no");
    }

    #[test]
    fn held_partial_never_pins_a_consumed_command() {
        // The "okay next" partial commits words out of the interim — a
        // hold there would paint the consumed span as a ghost tail in
        // the new paragraph.
        let mut transcript = ScratchpadTranscript::default();
        let t0 = Instant::now();
        transcript.set_interim_at("the end okay".to_owned(), t0);
        transcript.set_interim_at("the end okay next".to_owned(), t0 + Duration::from_millis(30));
        assert_eq!(transcript.paragraphs.len(), 2);
        assert!(transcript.interim.is_empty());
    }

    #[test]
    fn interim_hold_covers_only_tail_jitter() {
        assert!(interim_hold_covers("the tail,", "the tail"));
        assert!(interim_hold_covers("the tail", ""));
        assert!(interim_hold_covers("the end.", "the end,"));
        assert!(interim_hold_covers("the end…", "the end"));
        assert!(!interim_hold_covers("the tail", "the taill"));
        assert!(!interim_hold_covers("the tail", "the head"));
        // An empty slot has nothing to protect — fresh text lands.
        assert!(!interim_hold_covers("", "."));
    }

    #[test]
    fn word_fade_marks_growth_and_forgets_shrinks() {
        let mut fades = HashMap::new();
        let t0 = Instant::now();
        // First sighting seeds — nothing is "fresh" before it.
        note_text_fade(&mut fades, "p0".to_owned(), 5, true, t0);
        assert!(fades["p0"].fresh.is_empty());
        note_text_fade(&mut fades, "p0".to_owned(), 12, true, t0);
        assert_eq!(fades["p0"].fresh, vec![(5, t0)]);
        // A second append lands its own span; expired ones prune.
        note_text_fade(&mut fades, "p0".to_owned(), 20, true, t0 + WORD_FADE);
        assert_eq!(fades["p0"].fresh, vec![(12, t0 + WORD_FADE)]);
        // Shrinks clear — a rewrite is not an append.
        note_text_fade(&mut fades, "p0".to_owned(), 3, true, t0 + WORD_FADE);
        assert!(fades["p0"].fresh.is_empty());
        assert_eq!(fades["p0"].settled, 3);
        // Typed text never fades.
        note_text_fade(&mut fades, "p0".to_owned(), 8, false, t0 + WORD_FADE);
        assert!(fades["p0"].fresh.is_empty());
    }

    #[test]
    fn fade_runs_split_at_boundaries() {
        let ui_font = font("test-ui");
        let color: Hsla = rgb(0xFFFFFF).into();
        let mut runs = Vec::new();
        push_fade_runs(&mut runs, 0, 10, &[(5, 0.5)], 1.0, &ui_font, color);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len, 5);
        assert!((runs[0].color.a - 1.0).abs() < 0.01);
        assert_eq!(runs[1].len, 5);
        assert!((runs[1].color.a - 0.5).abs() < 0.01);
        // A boundary at the region's start dims the whole span.
        let mut runs = Vec::new();
        push_fade_runs(&mut runs, 0, 10, &[(0, 0.25)], 1.0, &ui_font, color);
        assert_eq!(runs.len(), 1);
        assert!((runs[0].color.a - 0.25).abs() < 0.01);
        // A morph's scale multiplies through.
        let mut runs = Vec::new();
        push_fade_runs(&mut runs, 0, 10, &[(5, 0.5)], 0.5, &ui_font, color);
        assert!((runs[1].color.a - 0.25).abs() < 0.01);
    }

    fn painted(key: &str, settled: usize, tail_start: usize, text: &str) -> PaintedRow {
        PaintedRow {
            key: key.to_owned(),
            settled,
            tail_start,
            painted: text.to_owned(),
            edited: false,
        }
    }

    #[test]
    fn landing_crossfades_the_displaced_tail() {
        let t0 = Instant::now();
        let mut fades = HashMap::new();
        // Last frame painted "hello" settled plus a gray " wurld"; the
        // final lands "hello world." and keeps "again" provisional.
        let prev = painted("p0", 5, 6, "hello wurld");
        let current = painted("p0", 12, 13, "hello world. again");
        note_text_fade(&mut fades, "p0".to_owned(), 5, true, t0);
        note_text_fade(&mut fades, "p0".to_owned(), 12, true, t0);
        assert_eq!(fades["p0"].fresh, vec![(5, t0)]);
        let mut crossfade = None;
        landing_crossfade(&mut fades, &mut crossfade, prev, &current, t0);
        let crossfade = crossfade.expect("the landing records");
        assert_eq!(crossfade.key, "p0");
        assert_eq!(crossfade.ghost, "hello wurld");
        assert_eq!(crossfade.tail_start, 6);
        // "hello w" was already on screen — the resolve resumes where
        // the paint first diverges rather than covering the whole span.
        assert_eq!(fades["p0"].fresh, vec![(7, t0)]);
    }

    #[test]
    fn landing_into_the_kept_tail_resolves_the_interim_slot() {
        let t0 = Instant::now();
        let mut fades = HashMap::new();
        // The final is the whole displaced tail — "again" commits — and
        // the next partial's " more" keeps painting provisional.
        let prev = painted("p0", 5, 6, "hello again");
        let current = painted("p0", 11, 12, "hello again more");
        note_text_fade(&mut fades, "p0".to_owned(), 5, true, t0);
        note_text_fade(&mut fades, "p0".to_owned(), 11, true, t0);
        note_text_fade(&mut fades, "interim".to_owned(), 5, true, t0);
        note_text_fade(&mut fades, "interim".to_owned(), 4, true, t0);
        let mut crossfade = None;
        landing_crossfade(&mut fades, &mut crossfade, prev, &current, t0);
        assert!(crossfade.is_some());
        // The committed span matched the displaced tail whole — nothing
        // in it fades — and the kept tail's divergence resolves through
        // the interim slot.
        assert!(fades["p0"].fresh.is_empty());
        assert_eq!(fades["interim"].fresh, vec![(0, t0)]);
    }

    #[test]
    fn landing_diffs_the_boxs_composed_row() {
        let t0 = Instant::now();
        let mut fades = HashMap::new();
        // The annotation slot diffs its composed string, so the
        // residual's boundary lands in painted coordinates.
        let prev = painted("annotation", 5, 6, "hello wurld");
        let current = painted("annotation", 12, 13, "hello world. again");
        note_text_fade(&mut fades, "annotation".to_owned(), 11, true, t0);
        note_text_fade(&mut fades, "annotation".to_owned(), 18, true, t0);
        assert_eq!(fades["annotation"].fresh, vec![(11, t0)]);
        let mut crossfade = None;
        landing_crossfade(&mut fades, &mut crossfade, prev, &current, t0);
        assert!(crossfade.is_some());
        assert_eq!(fades["annotation"].fresh, vec![(7, t0)]);
    }

    #[test]
    fn landing_needs_a_displaced_tail() {
        let t0 = Instant::now();
        let mut fades = HashMap::new();
        let mut crossfade = None;
        // Growth with no gray showing is a plain append.
        landing_crossfade(
            &mut fades,
            &mut crossfade,
            painted("p0", 5, 5, "hello"),
            &painted("p0", 12, 12, "hello world."),
            t0,
        );
        assert!(crossfade.is_none());
        // A rewrite that changed settled bytes isn't an append — the
        // cleanup morph owns that repaint.
        landing_crossfade(
            &mut fades,
            &mut crossfade,
            painted("p0", 5, 6, "h3llo wurld"),
            &painted("p0", 12, 13, "hello world. again"),
            t0,
        );
        assert!(crossfade.is_none());
        // A user-edited buffer lands instantly.
        landing_crossfade(
            &mut fades,
            &mut crossfade,
            painted("p0", 5, 6, "hello wurld"),
            &PaintedRow {
                edited: true,
                ..painted("p0", 12, 13, "hello world. again")
            },
            t0,
        );
        assert!(crossfade.is_none());
    }
}
