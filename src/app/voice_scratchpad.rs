//! VoicePad — press the VP mic button beside send in the composer
//! and dictate. A card replaces the chat content while a streaming
//! transcription session (`microsoft/mai-transcribe-2-streaming` over the
//! AI gateway's WebSocket endpoint) appends speech to a current paragraph;
//! saying "okay next" starts a new one, a red dot marks the append point,
//! and Enter sends the whole transcript as one message. Clicking a
//! paragraph opens an annotation box where speech becomes bullets under
//! that paragraph. Each chat keeps its own scratchpad like a composer
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
use gpui::DispatchPhase;
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
const RECORDING_RED: u32 = 0xF0344E;
const RECORDING_GLOW: u32 = 0xFF85B6;
/// The live record dot's breathing cycle — one full opacity sweep,
/// ~70% to 100% and back.
const RECORDING_DOT_PERIOD: Duration = Duration::from_millis(1500);
/// The text model that scrubs finished dictation — reached through the
/// same Vercel AI Gateway credential the transcription socket uses.
const CLEANUP_MODEL_ID: &str = "alibaba/qwen3.8-27b";
/// The cleanup call's whole brief: fix dictation artifacts without
/// rewriting. The span it sees is raw speech-to-text, never a draft.
const CLEANUP_INSTRUCTIONS: &str = "Clean up raw dictated speech-to-text. Remove filler words (um, uh, ah), false starts, and stuttered repetitions; fix obvious transcription errors; add light punctuation and capitalization. Keep the speaker's words and meaning exactly — rewrite as little as possible, never summarize, reorder, or answer. Reply with only the cleaned text — no quotes or commentary.";

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
    Cleaned {
        target: CleanTarget,
        start: usize,
        raw: String,
        cleaned: Option<String>,
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
#[derive(Clone)]
pub(super) struct CleanupRequest {
    pub target: CleanTarget,
    pub start: usize,
    pub raw: String,
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
    /// landing moves typing here so edits reach the transcript.
    edit_focus: FocusHandle,
    /// Per-row handles for the annotate affordance — `p{index}` for a
    /// paragraph, `b{index}-{bullet}` for a bullet — created lazily the
    /// way the sidebar's group rows are.
    row_focuses: HashMap<String, FocusHandle>,
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
            mute_focus: cx.focus_handle(),
            hide_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            keep_focus: cx.focus_handle(),
            discard_focus: cx.focus_handle(),
        }
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

/// One dictated paragraph and the bullet annotations parked under it.
#[derive(Default)]
struct ScratchpadParagraph {
    text: String,
    bullets: Vec<String>,
    /// A manual edit touched this paragraph — the cleanup model leaves
    /// its spans alone rather than overwrite the user's words.
    edited: bool,
}

/// Where an open annotation box writes: a paragraph's bullet list as a
/// whole, or — after a bullet click — the slot right after that bullet,
/// advancing once per commit so consecutive "okay next" bullets land in
/// order under it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AnnotationTarget {
    Paragraph(usize),
    Bullet {
        paragraph: usize,
        bullet: usize,
        insert: usize,
    },
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
    /// Interim speech at the main append point.
    interim: String,
    /// Where typed text and deletes land while the transcript surface holds
    /// focus — set by a selection edit or arrow press.
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
            match split_next_command(rest) {
                None => {
                    self.push_text(rest);
                    break;
                }
                Some((before, after)) => {
                    self.push_text(before);
                    self.commit_next();
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
            self.cleanup_requests
                .push(CleanupRequest { target, start, raw });
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
            match split_next_command(rest) {
                None => break,
                Some((before, after)) => {
                    self.push_text(before);
                    self.commit_next();
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
        if self.annotation_target.is_some() {
            self.annotation_interim = rest;
        } else {
            self.interim = rest;
        }
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
    /// as a bullet at the box's slot — under its paragraph, or right after
    /// its bullet — and the box reopens empty; otherwise it closes the
    /// current paragraph — a redundant command on an empty one moves
    /// nothing. The provisional suffix survives: it sits past the
    /// consumed command and belongs to the new append point.
    fn commit_next(&mut self) {
        match self.annotation_target {
            Some(AnnotationTarget::Paragraph(target)) => {
                let bullet = std::mem::take(&mut self.annotation_text).trim().to_owned();
                self.annotation_clean_from = 0;
                if !bullet.is_empty()
                    && let Some(paragraph) = self.paragraphs.get_mut(target)
                {
                    paragraph.bullets.push(bullet.clone());
                    // The box's whole text just became a bullet — clean it
                    // where it landed, not in the emptied box.
                    self.cleanup_requests.push(CleanupRequest {
                        target: CleanTarget::Node(ScratchpadNode::Bullet(
                            target,
                            paragraph.bullets.len() - 1,
                        )),
                        start: 0,
                        raw: bullet,
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
                    && let Some(target) = self.paragraphs.get_mut(paragraph)
                {
                    let at = insert.min(target.bullets.len());
                    target.bullets.insert(at, text.clone());
                    self.cleanup_requests.push(CleanupRequest {
                        target: CleanTarget::Node(ScratchpadNode::Bullet(paragraph, at)),
                        start: 0,
                        raw: text,
                    });
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
    /// behind would pin stale dimmed text on the last row.
    fn annotate(&mut self, index: usize) {
        if index < self.paragraphs.len() {
            // Speech retargets into the box — the closing append point's
            // tail is finished dictation, so it cleans now.
            self.flush_open_tail();
            self.annotation_target = Some(AnnotationTarget::Paragraph(index));
            self.annotation_interim = std::mem::take(&mut self.interim);
            self.caret = None;
            self.caret_anchor = None;
        }
    }

    /// Open the annotation box on a bullet — its commits land as sibling
    /// bullets right after it, matching how paragraphs nest theirs. The
    /// provisional suffix retargets with the append point, same as a
    /// paragraph box.
    fn annotate_bullet(&mut self, paragraph: usize, bullet: usize) {
        if self
            .paragraphs
            .get(paragraph)
            .is_some_and(|target| bullet < target.bullets.len())
        {
            self.flush_open_tail();
            self.annotation_target = Some(AnnotationTarget::Bullet {
                paragraph,
                bullet,
                insert: bullet + 1,
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
    /// lands as a bullet at its slot — then close, leaving the append
    /// point back on the live row. The still-provisional tail goes in
    /// with it, folded as strip credit so the stream's re-delivery of
    /// those words doesn't append them a second time.
    fn commit_annotation(&mut self) {
        let Some(target) = self.annotation_target.take() else {
            return;
        };
        let mut text = std::mem::take(&mut self.annotation_text).trim().to_owned();
        let interim = std::mem::take(&mut self.annotation_interim)
            .trim()
            .to_owned();
        self.fold_span(&interim);
        append_word_text(&mut text, &interim);
        self.annotation_clean_from = 0;
        if text.is_empty() {
            return;
        }
        match target {
            AnnotationTarget::Paragraph(index) => {
                if let Some(paragraph) = self.paragraphs.get_mut(index) {
                    paragraph.bullets.push(text.clone());
                    // Click-out commits the same bullet "okay next" would —
                    // clean it where it landed.
                    self.cleanup_requests.push(CleanupRequest {
                        target: CleanTarget::Node(ScratchpadNode::Bullet(
                            index,
                            paragraph.bullets.len() - 1,
                        )),
                        start: 0,
                        raw: text,
                    });
                }
            }
            AnnotationTarget::Bullet {
                paragraph, insert, ..
            } => {
                if let Some(target_paragraph) = self.paragraphs.get_mut(paragraph) {
                    let at = insert.min(target_paragraph.bullets.len());
                    target_paragraph.bullets.insert(at, text.clone());
                    self.cleanup_requests.push(CleanupRequest {
                        target: CleanTarget::Node(ScratchpadNode::Bullet(paragraph, at)),
                        start: 0,
                        raw: text,
                    });
                }
            }
        }
    }

    /// Whether `target` has a cleanup call queued or in flight — the
    /// spinner marker at the node's tail.
    fn is_cleaning(&self, target: CleanTarget) -> bool {
        self.cleanup_requests
            .iter()
            .chain(&self.cleanup_inflight)
            .any(|request| request.target == target)
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
                .map(|bullet| bullet.as_str())
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
                .and_then(|paragraph| paragraph.bullets.get_mut(bullet)),
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
            for (bullet, text) in paragraph.bullets.iter().enumerate() {
                nodes.push((ScratchpadNode::Bullet(index, bullet), text.len()));
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

    /// Splice `text` in at the caret, leaving it just past what landed.
    fn insert_at(&mut self, caret: &mut CaretPos, text: &str) {
        if let Some(buffer) = self.node_text_mut(caret.node) {
            caret.offset = caret.offset.min(buffer.len());
            buffer.insert_str(caret.offset, text);
            caret.offset += text.len();
            self.note_user_edit(caret.node);
        }
    }

    /// Delete at the caret — a grapheme, or a word with `word` — before or
    /// after it. At a node's edge the delete reaches into the neighbor's
    /// edge run the way a field would join lines, and the caret rides the
    /// cut.
    fn delete_at(&mut self, caret: &mut CaretPos, forward: bool, word: bool) {
        let offset = caret.offset.min(self.node_text(caret.node).len());
        let (node, range, landing) = if forward {
            let text = self.node_text(caret.node);
            if offset < text.len() {
                let end = if word {
                    next_word_boundary(text, offset)
                } else {
                    text[offset..]
                        .grapheme_indices(true)
                        .nth(1)
                        .map_or(text.len(), |(index, _)| offset + index)
                };
                (
                    caret.node,
                    offset..end,
                    CaretPos {
                        node: caret.node,
                        offset,
                    },
                )
            } else {
                let Some(node) = self.neighbor_node(caret.node, true) else {
                    return;
                };
                let text = self.node_text(node);
                if text.is_empty() {
                    return;
                }
                let end = if word {
                    next_word_boundary(text, 0)
                } else {
                    text.grapheme_indices(true)
                        .nth(1)
                        .map_or(text.len(), |(index, _)| index)
                };
                (node, 0..end, *caret)
            }
        } else if offset > 0 {
            let text = self.node_text(caret.node);
            let start = if word {
                previous_word_boundary(text, offset)
            } else {
                text[..offset]
                    .grapheme_indices(true)
                    .next_back()
                    .map_or(0, |(index, _)| index)
            };
            (
                caret.node,
                start..offset,
                CaretPos {
                    node: caret.node,
                    offset: start,
                },
            )
        } else {
            let Some(node) = self.neighbor_node(caret.node, false) else {
                return;
            };
            let len = self.node_text(node).len();
            if len == 0 {
                return;
            }
            let text = self.node_text(node);
            let start = if word {
                previous_word_boundary(text, len)
            } else {
                text[..len]
                    .grapheme_indices(true)
                    .next_back()
                    .map_or(0, |(index, _)| index)
            };
            (
                node,
                start..len,
                CaretPos {
                    node,
                    offset: start,
                },
            )
        };
        let len = self.node_text(node).len();
        let range = range.start.min(len)..range.end.min(len);
        let mut edited = false;
        if !range.is_empty()
            && let Some(text) = self.node_text_mut(node)
        {
            text.replace_range(range, "");
            edited = true;
        }
        if edited {
            self.note_user_edit(node);
        }
        *caret = landing;
        self.collapse_emptied(caret);
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
                if !self.paragraphs[paragraph].bullets[bullet].is_empty() {
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
                    Some(bullets.len())
                }
                Some(AnnotationTarget::Bullet {
                    paragraph: target,
                    insert,
                    ..
                }) if target == index => Some(insert.min(bullets.len())),
                _ => None,
            };
            if let Some(slot) = pending_slot {
                let mut pending = self.annotation_text.trim().to_owned();
                append_word_text(&mut pending, self.annotation_interim.trim());
                if !pending.is_empty() {
                    bullets.insert(slot, pending);
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
                out.push_str("- ");
                out.push_str(bullet.trim());
            }
        }
        out
    }

    /// The cleanup model's answer for one flushed span: replace it only
    /// when the buffer still holds the exact raw text — at the recorded
    /// offset, or uniquely elsewhere once later replacements shifted
    /// things. An edited paragraph or a missing match keeps the raw text.
    fn apply_cleanup(
        &mut self,
        target: CleanTarget,
        start: usize,
        raw: &str,
        cleaned: &str,
    ) -> bool {
        let edited = match target {
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
        let span = start
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
            });
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
                        .any(|bullet| !bullet.trim().is_empty())
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
        .filter(|c| !matches!(c, '.' | '!' | '?' | '…'))
        .collect()
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
fn split_next_command(text: &str) -> Option<(&str, &str)> {
    let words = word_spans(text);
    for pair in words.windows(2) {
        let (a_start, a_end) = pair[0];
        let (b_start, b_end) = pair[1];
        let a = &text[a_start..a_end];
        if (a.eq_ignore_ascii_case("ok") || a.eq_ignore_ascii_case("okay"))
            && text[b_start..b_end].eq_ignore_ascii_case("next")
        {
            let before = text[..a_start].trim_end();
            let after = text[b_end..].trim_start_matches(|c: char| !c.is_alphanumeric());
            return Some((before, after));
        }
    }
    None
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

    /// Keystrokes on the scratchpad's transcript surface — typed text
    /// replaces the selection or lands at the caret an earlier edit left,
    /// Backspace/Delete cut the same, arrows walk or extend the caret
    /// through paragraphs and bullets, and ⌘A selects all. Anything else
    /// propagates, so with nothing to edit a character keeps its
    /// type-to-focus trip to the composer.
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
            let text = text.to_owned();
            if has_selection {
                scratchpad.transcript.caret =
                    scratchpad.transcript.apply_selection_edit(&spans, &text);
            } else if let Some(mut caret) = scratchpad.transcript.caret {
                scratchpad.transcript.insert_at(&mut caret, &text);
                scratchpad.transcript.caret = Some(caret);
            } else {
                // No edit target — the composer field takes the keystroke.
                return;
            }
            scratchpad.transcript.caret_anchor = None;
            scratchpad.selection.selection.borrow_mut().clear();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match keystroke.key.as_str() {
            "backspace" | "delete" => {
                if has_selection {
                    scratchpad.transcript.caret =
                        scratchpad.transcript.apply_selection_edit(&spans, "");
                    scratchpad.selection.selection.borrow_mut().clear();
                    scratchpad.transcript.caret_anchor = None;
                } else if let Some(mut caret) = scratchpad.transcript.caret {
                    scratchpad.transcript.delete_at(
                        &mut caret,
                        keystroke.key == "delete",
                        modifiers.alt,
                    );
                    scratchpad.transcript.caret = Some(caret);
                } else {
                    return;
                }
                cx.stop_propagation();
                cx.notify();
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
                        let body = serde_json::json!({
                            "model": CLEANUP_MODEL_ID,
                            "messages": [
                                {"role": "system", "content": CLEANUP_INSTRUCTIONS},
                                {"role": "user", "content": request.raw},
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
            } = event
            {
                if let Some(scratchpad) = self.voice_scratchpads.get_mut(&session_id) {
                    // The call settled — its tail spinner clears whether
                    // the answer lands or the failure kept the raw text.
                    let inflight = &mut scratchpad.transcript.cleanup_inflight;
                    if let Some(at) = inflight.iter().position(|request| {
                        request.target == target && request.start == start && request.raw == raw
                    }) {
                        inflight.remove(at);
                        changed = true;
                    }
                    if let Some(cleaned) = cleaned {
                        changed |= scratchpad
                            .transcript
                            .apply_cleanup(target, start, &raw, &cleaned);
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
                                // Registered ahead of install's listeners:
                                // bubble order is reverse registration, so
                                // the drag's release() settles the spans
                                // before this observes them. A selection
                                // that lands takes keyboard focus — typing
                                // then edits the transcript, not the draft.
                                window.on_mouse_event({
                                    let selection = selection.clone();
                                    let focus = edit_focus.clone();
                                    move |_: &MouseUpEvent, phase, window, cx| {
                                        if phase != DispatchPhase::Bubble {
                                            return;
                                        }
                                        if selection.selection.borrow().is_empty() {
                                            return;
                                        }
                                        window.focus(&focus, cx);
                                    }
                                });
                                md::render::install_selection_input(
                                    region, window, &selection, None,
                                );
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
                // "Unmute" is the wider label — the pill holds its width
                // across the toggle.
                Some(tr!("voice_scratchpad.unmute")),
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
    /// column is the edit surface — a settled selection or a Tab landing
    /// puts its focus here so typing edits the transcript.
    fn render_scratchpad_rows(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(scratchpad) = self.selected_voice_scratchpad_mut() else {
            return div().into_any_element();
        };
        // Every row's focus handle exists before the paint loop — it then
        // works off a shared borrow so the status row and an open
        // annotation box can render through `self` beside it.
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
        }
        let Some(scratchpad) = self.selected_voice_scratchpad() else {
            return div().into_any_element();
        };
        let transcript = &scratchpad.transcript;
        let muted = scratchpad.muted;
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
            // click handlers sort out annotation.
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
            let flat = scratchpad_paragraph_text(
                paragraph,
                is_current && !annotating,
                &transcript.interim,
                &ui_family,
                theme,
            );
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
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    if let Some(scratchpad) = this.selected_voice_scratchpad_mut()
                        && !scratchpad_click_was_drag(event, &scratchpad.selection)
                    {
                        scratchpad.transcript.annotate(index);
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
                            scratchpad.transcript.annotate(index);
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
                let flat = md::render::FlatText {
                    text: bullet.clone().into(),
                    runs: vec![TextRun {
                        len: bullet.len(),
                        font: font(ui_family.clone()),
                        color: theme.text_secondary,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
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
                    .pl(px(18.0))
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
                &ScratchpadParagraph::default(),
                true,
                &transcript.interim,
                &ui_family,
                theme,
            );
            blocks = blocks.child(
                div()
                    .w_full()
                    .py(px(2.0))
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
                    ),
            );
        }
        blocks
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
            .into_any_element()
    }

    /// The failure row at the top of the content — mic denial, a dropped
    /// stream, or a reported pause — with its remedies beside it.
    /// `Connecting` earns no chrome; the dot's presence already reads as
    /// waiting. `Paused` shows no Retry — the worker is still live, and a
    /// second spawn would split the shared audio queue.
    fn render_scratchpad_status(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let scratchpad = self.selected_voice_scratchpad()?;
        let (message, can_retry, system_settings) = match scratchpad.status {
            ScratchpadStatus::Connecting | ScratchpadStatus::Live => return None,
            ScratchpadStatus::Paused => (tr!("voice_scratchpad.paused"), false, false),
            ScratchpadStatus::MicDenied => (tr!("voice_scratchpad.mic_denied"), true, true),
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
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(scratchpad) = self.selected_voice_scratchpad() else {
            return div().into_any_element();
        };
        let mut text = scratchpad.transcript.annotation_text.clone();
        let interim = strip_interim_terminators(&scratchpad.transcript.annotation_interim);
        append_word_text(&mut text, &interim);
        if text.is_empty() {
            text = tr!("voice_scratchpad.annotation_hint");
        }
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
                scratchpad.muted,
                scratchpad.status,
                theme,
            ))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(px(4.0))
                    .child(text)
                    .when(
                        scratchpad.transcript.is_cleaning(CleanTarget::Annotation),
                        |row| row.child(scratchpad_cleanup_spinner(13.0, theme)),
                    ),
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

/// Whether a click on the transcript surface was really a drag — the
/// pointer ran past the 4px slop the annotation press uses, or the
/// gesture left a settled selection. Row click handlers gate on it so a
/// drag-select never fires the annotate or click-out actions the press
/// would have meant.
fn scratchpad_click_was_drag(click: &ClickEvent, selection: &TranscriptSelection) -> bool {
    if !selection.selection.borrow().is_empty() {
        return true;
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

/// A paragraph's FlatText: finalized speech in ink, the interim suffix
/// dimmed — the "appears dimmed, snaps to black on finalize" rule.
fn scratchpad_paragraph_text(
    paragraph: &ScratchpadParagraph,
    with_interim: bool,
    interim: &str,
    ui_family: &SharedString,
    theme: &Theme,
) -> md::render::FlatText {
    let mut text = paragraph.text.clone();
    let split = text.len();
    if with_interim {
        append_word_text(&mut text, &strip_interim_terminators(interim));
    }
    let mut runs = Vec::new();
    if split > 0 {
        runs.push(TextRun {
            len: split,
            font: font(ui_family.clone()),
            color: theme.text,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    let tail = text.len().saturating_sub(split);
    if tail > 0 {
        runs.push(TextRun {
            len: tail,
            font: font(ui_family.clone()),
            color: theme.text_tertiary,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    md::render::FlatText {
        text: text.into(),
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
        // A bullet-targeted box inserts right after its bullet and the
        // slot advances per commit — consecutive "okay next" bullets keep
        // speech order under it.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("first okay next");
        transcript.append_finalized("second okay next");
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("inserted one okay next");
        transcript.append_finalized("inserted two okay next");
        assert_eq!(
            transcript.paragraphs[0].bullets,
            vec!["first", "inserted one", "inserted two", "second"]
        );
    }

    #[test]
    fn open_bullet_box_sends_pending_text_at_the_slot() {
        // Enter sends what the user sees — an open box's uncommitted text
        // commits at its insert slot, not at the list's end.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan");
        transcript.annotate(0);
        transcript.append_finalized("first okay next");
        transcript.append_finalized("second okay next");
        transcript.annotate_bullet(0, 0);
        transcript.append_finalized("still talking");
        assert_eq!(
            transcript.to_message(),
            "the plan\n- first\n- still talking\n- second"
        );
    }

    #[test]
    fn click_out_commits_the_annotation_at_its_slot() {
        // Clicking away finishes the open box — nothing dictated into it
        // is lost, and the append point returns to the live row.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("the plan okay next");
        transcript.annotate(1);
        transcript.append_finalized("first okay next");
        transcript.annotate_bullet(1, 0);
        transcript.append_finalized("still talking");
        transcript.set_interim("for a".to_owned());
        transcript.commit_annotation();
        assert!(transcript.annotation_target.is_none());
        assert_eq!(
            transcript.paragraphs[1].bullets,
            vec!["first", "still talking for a"]
        );
        // The provisional tail's re-delivery strips instead of appending a
        // second copy, and fresh speech lands on the live row again.
        transcript.append_finalized("for a moment more");
        assert_eq!(transcript.paragraphs[1].text, "moment more");
    }

    #[test]
    fn selection_edit_replaces_across_paragraph_and_bullet() {
        // A drag over a paragraph's tail into a bullet cuts both painted
        // spans; the typed text lands where the grab began.
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("alpha beta okay next");
        transcript.annotate(1);
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
                key: md::selection::TextKey::new("vs-b-1", 0),
                range: 0..5,
                text: "gamma delta".into(),
                block_break: false,
                copy: Rc::default(),
            },
        ];
        let caret = transcript.apply_selection_edit(&spans, "omega");
        assert_eq!(transcript.paragraphs[0].text, "alpha omega");
        assert_eq!(transcript.paragraphs[1].bullets[0], " delta");
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
        transcript.append_finalized("keep me okay next");
        transcript.annotate(1);
        transcript.append_finalized("gone okay next");
        transcript.exit_annotation();
        let mut caret = CaretPos {
            node: ScratchpadNode::Bullet(1, 0),
            offset: 0,
        };
        transcript.delete_at(&mut caret, true, true);
        assert!(transcript.paragraphs[1].bullets.is_empty());
        assert!(matches!(
            caret,
            CaretPos {
                node: ScratchpadNode::Paragraph(1),
                offset: 0
            }
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
        assert!(transcript.apply_cleanup(target, start, &raw, "First thought."));
        assert_eq!(transcript.paragraphs[0].text, "First thought. And second.");
        // A missing match leaves the raw text alone.
        assert!(!transcript.apply_cleanup(
            CleanTarget::Node(ScratchpadNode::Paragraph(0)),
            0,
            "not in the transcript",
            "whatever"
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
        assert!(transcript.apply_cleanup(first.target, first.start, &first.raw, "Filler."));
        assert!(transcript.apply_cleanup(
            second.target,
            second.start,
            &second.raw,
            "Everything else continues."
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
            "Different."
        ));
        assert_eq!(transcript.paragraphs[0].text, "note same. and same. Tail");
    }

    #[test]
    fn cleanup_never_rewrites_a_user_edited_paragraph() {
        let mut transcript = ScratchpadTranscript::default();
        transcript.append_finalized("Um dictated words. More");
        let request = &transcript.cleanup_requests[0];
        let (target, start, raw) = (request.target, request.start, request.raw.clone());
        let mut caret = CaretPos {
            node: ScratchpadNode::Paragraph(0),
            offset: 0,
        };
        transcript.insert_at(&mut caret, "typed ");
        assert!(!transcript.apply_cleanup(target, start, &raw, "Dictated words."));
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
}
