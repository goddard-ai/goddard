//! Voice Scratchpad — press the VS mic button beside send in the composer
//! and dictate. A card replaces the chat content while a streaming
//! transcription session (`microsoft/mai-transcribe-2-streaming` over the
//! AI gateway's WebSocket endpoint) appends speech to a current paragraph;
//! saying "okay next" starts a new one, a red dot marks the append point,
//! and Enter sends the whole transcript as one message. Clicking a
//! paragraph opens an annotation box where speech becomes bullets under
//! that paragraph. One session runs at a time, bound to the chat it
//! started in; Hide or navigating away keeps it recording.
//!
//! Audio is captured through the shared `voice_gate` engine tap in
//! `platform.rs` — the scratchpad's sink is the one path that forwards mic
//! samples off the machine, and it exists only while a session is live.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Context as _;
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
/// Cancel asks first once the transcript is worth keeping.
const CANCEL_CONFIRM_PARAGRAPHS: usize = 2;
const CANCEL_CONFIRM_CHARS: usize = 280;
/// The scratchpad card matches the composer card's width.
const CARD_MAX_WIDTH: f32 = CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0;
const CARD_RADIUS: f32 = 24.0;
/// The gradient cover runs behind the composer — its visible fade depth on
/// top of whatever the lane measures.
const GRADIENT_VISIBLE: f32 = 41.0;
/// The hint line floats this far above the composer's top edge.
const HINT_CLEARANCE: f32 = 34.0;
const TOP_BAR_HEIGHT: f32 = 69.0;
const RECORDING_RED: u32 = 0xF0344E;
const RECORDING_GLOW: u32 = 0xFF85B6;

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
    /// The stream failed or ended on its own — the transcript stays and
    /// the panel offers Retry.
    Failed,
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
    /// The stream dropped or errored. The transcript is kept; Retry
    /// reconnects and audio resumes live — nothing is queued for later.
    ConnectionLost,
}

/// The VS button's posture for one composer card.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScratchpadButtonState {
    Idle,
    Recording,
    Muted,
    /// A session lives on another chat — the button dims and explains.
    Elsewhere,
}

/// A dictation session bound to the chat it started in.
pub(super) struct VoiceScratchpad {
    session_id: Uuid,
    transcript: ScratchpadTranscript,
    status: ScratchpadStatus,
    muted: bool,
    hidden: bool,
    /// The Cancel affordance armed its confirmation.
    confirm_discard: bool,
    /// Bumped on each (re)connect so a retired worker's events land nowhere.
    generation: u64,
    /// Flags the worker out; the audio channel disconnecting says the same.
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
    mute_focus: FocusHandle,
    hide_focus: FocusHandle,
    cancel_focus: FocusHandle,
    keep_focus: FocusHandle,
    discard_focus: FocusHandle,
}

impl VoiceScratchpad {
    fn new(session_id: Uuid, cx: &mut Context<Waku>) -> Self {
        let (audio_tx, audio_rx) = crossbeam_channel::bounded(AUDIO_QUEUE_CAP);
        Self {
            session_id,
            transcript: ScratchpadTranscript::default(),
            status: ScratchpadStatus::Connecting,
            muted: false,
            hidden: false,
            confirm_discard: false,
            generation: 0,
            stop: Arc::new(AtomicBool::new(false)),
            audio_tx,
            audio_rx,
            scroll: ScrollHandle::new(),
            scrollbar: ScrollbarState::new(),
            selection: TranscriptSelection::default(),
            follow_tail: true,
            mute_focus: cx.focus_handle(),
            hide_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            keep_focus: cx.focus_handle(),
            discard_focus: cx.focus_handle(),
        }
    }
}

/// One dictated paragraph and the bullet annotations parked under it.
#[derive(Default)]
struct ScratchpadParagraph {
    text: String,
    bullets: Vec<String>,
}

/// Rolling cap on `finalized_tail` — dedup only needs the stream's recent
/// end.
const FINALIZED_TAIL_CAP: usize = 16 * 1024;

/// The scratchpad's text model: ordered paragraphs, per-paragraph bullets,
/// the live interim suffix, and the annotation box's target and content.
#[derive(Default)]
pub(super) struct ScratchpadTranscript {
    paragraphs: Vec<ScratchpadParagraph>,
    /// The paragraph an open annotation box writes into.
    annotation_target: Option<usize>,
    /// Finalized speech currently inside the open annotation box.
    annotation_text: String,
    /// Interim speech while annotating — provisional until it settles.
    annotation_interim: String,
    /// Interim speech at the main append point.
    interim: String,
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
                    return;
                }
                Some((before, after)) => {
                    self.push_text(before);
                    self.commit_next();
                    rest = after;
                }
            }
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
        let rest = rest.to_owned();
        if self.annotation_target.is_some() {
            self.annotation_interim = rest;
        } else {
            self.interim = rest;
        }
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

    /// Append plain speech to the open box or the current paragraph.
    fn push_text(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.annotation_target.is_some() {
            append_word_text(&mut self.annotation_text, text);
        } else {
            append_word_text(&mut self.current().text, text);
        }
    }

    /// "Okay next": inside an annotation box it commits the box's content
    /// as a bullet under its paragraph and the box reopens empty; otherwise
    /// it closes the current paragraph — a redundant command on an empty
    /// one moves nothing. The provisional suffix survives: it sits past
    /// the consumed command and belongs to the new append point.
    fn commit_next(&mut self) {
        if let Some(target) = self.annotation_target {
            let bullet = std::mem::take(&mut self.annotation_text).trim().to_owned();
            if !bullet.is_empty()
                && let Some(paragraph) = self.paragraphs.get_mut(target)
            {
                paragraph.bullets.push(bullet);
            }
            return;
        }
        let current = self.current();
        if !current.text.is_empty() || !current.bullets.is_empty() {
            self.paragraphs.push(ScratchpadParagraph::default());
        }
    }

    /// Open the annotation box on a paragraph; an open box moves. The
    /// provisional suffix retargets with the append point — leaving it
    /// behind would pin stale dimmed text on the last row.
    fn annotate(&mut self, index: usize) {
        if index < self.paragraphs.len() {
            self.annotation_target = Some(index);
            self.annotation_interim = std::mem::take(&mut self.interim);
        }
    }

    /// Close the annotation box — its uncommitted text waits in the box
    /// rather than jumping into the transcript.
    fn exit_annotation(&mut self) {
        self.annotation_target = None;
        self.annotation_interim.clear();
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
            if Some(index) == self.annotation_target {
                let mut pending = self.annotation_text.trim().to_owned();
                append_word_text(&mut pending, self.annotation_interim.trim());
                if !pending.is_empty() {
                    bullets.push(pending);
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

/// One transcript event out of a server frame — `true` when the part is
/// terminal and the worker winds down after reporting it.
fn dispatch_stream_part(text: &str, send: &mut impl FnMut(ScratchpadEvent)) -> bool {
    let Ok(part) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    match part.get("type").and_then(Value::as_str) {
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
        // `finish` is the stream's normal end — the session may still be
        // open from the user's side, so it lands as a reconnectable pause.
        Some("finish") | Some("error") => {
            send(ScratchpadEvent::Failed);
            return true;
        }
        _ => {}
    }
    false
}

/// The per-session worker: owns the socket, streams PCM frames from the
/// audio channel, and forwards parsed transcript events to the pump. Runs
/// on its own thread — a blocking socket is fine when it owns nothing else.
fn run_transcription_worker(
    generation: u64,
    key: String,
    audio: Receiver<AudioChunk>,
    events: Sender<(u64, ScratchpadEvent)>,
    wake: smol::channel::Sender<()>,
    stop: Arc<AtomicBool>,
) {
    let mut send = |event: ScratchpadEvent| {
        let _ = events.send((generation, event));
        signal_event_pump(&wake);
    };
    let mut socket = match connect_transcription_socket(&key) {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("Goddard: voice scratchpad connection failed: {error:#}");
            send(ScratchpadEvent::Failed);
            return;
        }
    };
    send(ScratchpadEvent::Connected);
    let start = serde_json::json!({
        "type": "transcription-stream.start",
        "inputAudioFormat": { "type": "audio/pcm", "rate": TRANSCRIPTION_SAMPLE_RATE },
    });
    if socket
        .send(Message::Text(start.to_string().into()))
        .is_err()
    {
        send(ScratchpadEvent::Failed);
        return;
    }
    let mut resampler = PcmResampler::new(TRANSCRIPTION_SAMPLE_RATE);
    let mut pcm = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = socket.send(Message::Text(
                "{\"type\":\"transcription-stream.audio-done\"}".into(),
            ));
            let _ = socket.close(None);
            return;
        }
        match audio.recv_timeout(READ_POLL) {
            Ok(chunk) => {
                // Drain whatever else landed this tick before writing, so a
                // burst ships as few frames as it can.
                resampler.push(&chunk.samples, chunk.rate, &mut pcm);
                while let Ok(chunk) = audio.try_recv() {
                    resampler.push(&chunk.samples, chunk.rate, &mut pcm);
                }
                for frame in pcm.chunks(MAX_AUDIO_FRAME_BYTES) {
                    if socket.send(Message::Binary(frame.to_vec().into())).is_err() {
                        send(ScratchpadEvent::Failed);
                        return;
                    }
                }
                pcm.clear();
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            // The sink detaches when the session ends.
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                let _ = socket.close(None);
                return;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                if dispatch_stream_part(&text, &mut send) {
                    let _ = socket.close(None);
                    return;
                }
            }
            Ok(Message::Close(_)) => {
                send(ScratchpadEvent::Failed);
                return;
            }
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
                send(ScratchpadEvent::Failed);
                return;
            }
        }
        let _ = socket.flush();
    }
}

impl Waku {
    /// Whether the scratchpad panel is the chat column's content right now:
    /// a live session, not hidden, on its own chat, under the surfaces a
    /// mounted composer implies. The experiment flag gates the whole
    /// surface — a session only exists while it is on.
    pub(super) fn voice_scratchpad_visible(&self) -> bool {
        if !self.state.voice_scratchpad_enabled {
            return false;
        }
        let Some(scratchpad) = &self.voice_scratchpad else {
            return false;
        };
        if scratchpad.hidden || Some(scratchpad.session_id) != self.state.selected_session {
            return false;
        }
        // Big Picture borrows the composer for its overlay — Enter there is
        // a Big Picture submit, not a scratchpad send.
        self.composer_mounted() && !self.big_picture.is_open()
    }

    /// The VS button's posture for the card under the pointer: idle, live
    /// on this chat (recording or muted), or disabled while another chat
    /// holds the session.
    fn voice_scratchpad_button_state(&self, session_id: Option<Uuid>) -> ScratchpadButtonState {
        match &self.voice_scratchpad {
            None => ScratchpadButtonState::Idle,
            Some(scratchpad) if Some(scratchpad.session_id) == session_id => {
                if scratchpad.muted {
                    ScratchpadButtonState::Muted
                } else {
                    ScratchpadButtonState::Recording
                }
            }
            Some(_) => ScratchpadButtonState::Elsewhere,
        }
    }

    /// The VS button — a small dark pill with "VS" and a mic glyph,
    /// immediately left of send. While its own session is live it carries
    /// the state dot; while another chat's session is live it dims and
    /// explains itself.
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
        let enabled = state != ScratchpadButtonState::Elsewhere && session_id.is_some();
        let dot = |color: Hsla| {
            div()
                .absolute()
                .top(px(-2.0))
                .right(px(-2.0))
                .size(px(7.0))
                .rounded_full()
                .bg(color)
        };
        let pill = div()
            .id(controls.chip_id("voice-scratchpad"))
            .h(px(20.0))
            .flex_none()
            .rounded_full()
            .relative()
            .flex()
            .items_center()
            .gap(px(1.0))
            .pl(px(8.0))
            .pr(px(4.0))
            .bg(theme.inverse)
            .child(
                div()
                    .text_size(sp(9.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.on_inverse)
                    .child("VS"),
            )
            .child(icon("icons/mic.svg", 16.0, theme.on_inverse));
        let pill = match state {
            ScratchpadButtonState::Recording => pill.child(dot(rgb(RECORDING_RED).into())),
            ScratchpadButtonState::Muted => pill.child(dot(theme.text_tertiary)),
            _ => pill,
        };
        let tooltip = match state {
            ScratchpadButtonState::Elsewhere => tr!("voice_scratchpad.active_elsewhere"),
            ScratchpadButtonState::Recording => tr!("voice_scratchpad.recording"),
            ScratchpadButtonState::Muted => tr!("voice_scratchpad.muted"),
            ScratchpadButtonState::Idle => tr!("voice_scratchpad.start"),
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

    /// The VS button's click: no session starts one on this chat, a hidden
    /// session resurfaces, a visible one hides. Buttons on other chats are
    /// disabled — one mic stream at a time.
    fn toggle_voice_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this_session = self.composer_session_id();
        match &mut self.voice_scratchpad {
            Some(scratchpad) if Some(scratchpad.session_id) == this_session => {
                scratchpad.hidden = !scratchpad.hidden;
                if !scratchpad.hidden {
                    scratchpad.follow_tail = true;
                }
                cx.notify();
            }
            Some(_) => {}
            None => self.start_voice_scratchpad(window, cx),
        }
    }

    /// Start a session on the composer's chat. The panel opens immediately —
    /// permission and connection failures become its inline error state —
    /// and capture begins behind it.
    fn start_voice_scratchpad(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.voice_scratchpad_enabled || self.voice_scratchpad.is_some() {
            return;
        }
        let Some(session_id) = self.composer_session_id() else {
            return;
        };
        self.voice_scratchpad = Some(VoiceScratchpad::new(session_id, cx));
        match crate::platform::microphone_access() {
            crate::platform::CaptureAccess::Granted => self.begin_voice_capture(cx),
            crate::platform::CaptureAccess::Undetermined => {
                let tx = self.voice_scratchpad_tx.clone();
                let wake = self.event_wake_tx.clone();
                crate::platform::request_microphone_access(Box::new(move |granted| {
                    let _ = tx.try_send((0, ScratchpadEvent::MicAccess(granted)));
                    signal_event_pump(&wake);
                }));
            }
            crate::platform::CaptureAccess::Denied => {
                if let Some(scratchpad) = &mut self.voice_scratchpad {
                    scratchpad.status = ScratchpadStatus::MicDenied;
                }
            }
        }
        // Focus stays in the composer — typing, Enter-to-send, and Esc all
        // keep their composer semantics while the panel is up.
        cx.notify();
    }

    /// Attach the audio sink and open the transcription stream. Mic access
    /// is already granted when this runs.
    fn begin_voice_capture(&mut self, cx: &mut Context<Self>) {
        let Some(scratchpad) = &self.voice_scratchpad else {
            return;
        };
        let audio_tx = scratchpad.audio_tx.clone();
        crate::platform::set_voice_audio_sink(Some(Box::new(move |samples, rate| {
            // The audio thread never blocks: a full queue drops the block.
            let _ = audio_tx.try_send(AudioChunk {
                samples: samples.to_vec(),
                rate,
            });
        })));
        crate::platform::set_voice_audio_sink_muted(scratchpad.muted);
        crate::platform::start_voice_listener();
        self.spawn_transcription_worker(cx);
    }

    /// Fetch the gateway key on the daemon, then spin the worker thread —
    /// a fresh credential per (re)connect keeps the secret out of app state.
    fn spawn_transcription_worker(&mut self, cx: &mut Context<Self>) {
        let Some(scratchpad) = &mut self.voice_scratchpad else {
            return;
        };
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
                let Some(scratchpad) = this.voice_scratchpad.as_mut() else {
                    return;
                };
                if scratchpad.generation != generation {
                    return;
                }
                match key {
                    Some(key) => {
                        if let Err(error) = thread::Builder::new()
                            .name("voice-scratchpad-transcribe".to_owned())
                            .spawn(move || {
                                run_transcription_worker(
                                    generation, key, audio, events, wake, stop,
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

    /// Mute pauses capture at the tap — blocks stop copying before they
    /// ever reach the socket — while the session and connection stay up.
    fn set_voice_scratchpad_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        let Some(scratchpad) = &mut self.voice_scratchpad else {
            return;
        };
        scratchpad.muted = muted;
        crate::platform::set_voice_audio_sink_muted(muted);
        cx.notify();
    }

    /// Cancel — inline when the transcript is thin, confirmed once it's
    /// substantial.
    fn request_cancel_voice_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scratchpad) = &mut self.voice_scratchpad else {
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

    /// Escape inside the scratchpad: an armed discard dismisses first, an
    /// annotation box closes next, then Esc means Cancel — the same
    /// confirm rule as the button.
    pub(super) fn voice_scratchpad_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scratchpad) = &mut self.voice_scratchpad else {
            return;
        };
        if scratchpad.confirm_discard {
            scratchpad.confirm_discard = false;
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

    /// Enter while the panel is up sends the whole transcript as one
    /// message on the bound chat and ends the session — a typed draft is
    /// untouched.
    pub(super) fn submit_voice_scratchpad(&mut self, cx: &mut Context<Self>) {
        let text = self
            .voice_scratchpad
            .as_ref()
            .map(|scratchpad| scratchpad.transcript.to_message())
            .unwrap_or_default()
            .trim()
            .to_owned();
        if text.is_empty() {
            return;
        }
        let Some(scratchpad) = self.voice_scratchpad.take() else {
            return;
        };
        scratchpad.stop.store(true, Ordering::Relaxed);
        self.teardown_voice_scratchpad(cx);
        self.submit_composer_submission_to(
            scratchpad.session_id,
            ComposerSubmission::plain(text),
            cx,
        );
    }

    /// End the session and discard the transcript.
    pub(super) fn end_voice_scratchpad(&mut self, cx: &mut Context<Self>) {
        let Some(scratchpad) = self.voice_scratchpad.take() else {
            return;
        };
        scratchpad.stop.store(true, Ordering::Relaxed);
        self.teardown_voice_scratchpad(cx);
    }

    /// Shared teardown: the sink detaches (samples stop leaving the tap —
    /// the worker notices the channel drop on its own), and the mic engine
    /// returns to whatever else still wants it.
    fn teardown_voice_scratchpad(&mut self, cx: &mut Context<Self>) {
        crate::platform::set_voice_audio_sink(None);
        crate::platform::set_voice_audio_sink_muted(false);
        self.maybe_stop_voice_listener();
        cx.notify();
    }

    /// Drain worker and permission answers into the transcript model —
    /// events stamped with a retired generation are a dead worker's mail.
    pub(super) fn drain_voice_scratchpad_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok((generation, event)) = self.voice_scratchpad_events.try_recv() {
            let Some(scratchpad) = &mut self.voice_scratchpad else {
                continue;
            };
            // The mic answer carries generation 0 — it predates any worker.
            let current = matches!(event, ScratchpadEvent::MicAccess(_))
                || generation == scratchpad.generation;
            if !current {
                continue;
            }
            changed = true;
            match event {
                ScratchpadEvent::MicAccess(granted) => {
                    if granted {
                        self.begin_voice_capture(cx);
                    } else {
                        scratchpad.status = ScratchpadStatus::MicDenied;
                    }
                }
                ScratchpadEvent::Connected => {
                    if scratchpad.status == ScratchpadStatus::Connecting {
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
                ScratchpadEvent::Failed => {
                    scratchpad.status = ScratchpadStatus::ConnectionLost;
                    // The stream that owed the folded words a delivery is
                    // dead — a reconnect's finals must not strip against it.
                    scratchpad.transcript.interim_folded.clear();
                }
            }
        }
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
        let Some(scratchpad) = &self.voice_scratchpad else {
            return div().into_any_element();
        };
        if scratchpad.follow_tail {
            scratchpad.scroll.scroll_to_bottom();
        }
        let scroll = scratchpad.scroll.clone();
        let scrollbar = scratchpad.scrollbar.clone();
        let selection = scratchpad.selection.clone();
        let weak = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .px(px(20.0 - COMPOSER_OVERHANG))
            // The card runs through the composer lane: the negative margin
            // pulls the slot's height over the lane's, and the lane —
            // painted after — sits on top of the card's bottom edge.
            .mb(px(-lane))
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
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.composer)
                    .overflow_hidden()
                    .child(self.render_scratchpad_top_bar(&theme, cx))
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
                                        if let Some(scratchpad) = &mut this.voice_scratchpad {
                                            scratchpad.follow_tail = at_bottom;
                                        }
                                    });
                                    contain_scroll(&scroll, cx);
                                }
                            })
                            .child(self.render_scratchpad_rows(&theme, window, cx)),
                    )
                    .child(
                        // The gradient cover's solid half hides behind the
                        // composer; only its fade reaches into the content.
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(lane + GRADIENT_VISIBLE))
                            .bg(linear_gradient(
                                180.0,
                                linear_color_stop(theme.composer.opacity(0.0), 0.0),
                                linear_color_stop(theme.composer, 1.0),
                            )),
                    )
                    .child({
                        let prefix = tr!("voice_scratchpad.hint_prefix");
                        let command = tr!("voice_scratchpad.hint_command");
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
                            .bottom(px(lane + HINT_CLEARANCE))
                            .text_size(sp(14.0))
                            .child(
                                gpui::StyledText::new(format!("{prefix}{command}{suffix}"))
                                    .with_runs(vec![
                                        run(prefix.len(), font(ui_family.clone())),
                                        run(command.len(), bold),
                                        run(suffix.len(), font(ui_family)),
                                    ]),
                            )
                    })
                    .child(
                        div()
                            .absolute()
                            .top(px(TOP_BAR_HEIGHT))
                            .bottom(px(lane))
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
                            move |_, region, window, _| {
                                md::render::install_selection_input(
                                    region, window, &selection, None,
                                )
                            },
                        )
                        .absolute()
                        .top(px(TOP_BAR_HEIGHT))
                        .left_0()
                        .right_0()
                        .bottom(px(lane)),
                    ),
            )
            .into_any_element()
    }

    /// The 69px top bar: title left, Mute/Hide/Cancel pills right, hairline
    /// under it.
    fn render_scratchpad_top_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let Some(scratchpad) = &self.voice_scratchpad else {
            return div();
        };
        let muted = scratchpad.muted;
        div()
            .h(px(TOP_BAR_HEIGHT))
            .flex_none()
            .px(px(24.0))
            .flex()
            .items_center()
            .border_b(hairline())
            .border_color(theme.separator)
            .child(
                div()
                    .text_size(sp(14.0))
                    .font_weight(FontWeight::EXTRA_BOLD)
                    .text_color(theme.text_tertiary)
                    .child(tr!("voice_scratchpad.title")),
            )
            .child(div().flex_1())
            .child(
                div()
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
                        true,
                        theme,
                        |this, _window, cx| {
                            let muted = this
                                .voice_scratchpad
                                .as_ref()
                                .is_some_and(|scratchpad| !scratchpad.muted);
                            this.set_voice_scratchpad_muted(muted, cx);
                        },
                        cx,
                    ))
                    .child(self.scratchpad_pill(
                        "vs-hide",
                        &scratchpad.hide_focus,
                        tr!("voice_scratchpad.hide"),
                        false,
                        theme,
                        |this, window, cx| {
                            if let Some(scratchpad) = &mut this.voice_scratchpad {
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
                        false,
                        theme,
                        |this, window, cx| this.request_cancel_voice_scratchpad(window, cx),
                        cx,
                    )),
            )
    }

    /// One top-bar pill: Mute wears the solid dark treatment, Hide and
    /// Cancel the light gradient with a hairline.
    fn scratchpad_pill(
        &self,
        id: &'static str,
        focus: &FocusHandle,
        label: String,
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
            .px(px(18.0))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(14.0))
            .when(primary, |pill| {
                pill.bg(theme.inverse).text_color(theme.on_inverse)
            })
            .when(!primary, |pill| {
                pill.border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(theme.raised.opacity(0.5), 0.0),
                        linear_color_stop(theme.raised, 1.0),
                    ))
                    .text_color(theme.text)
            })
            .focus_visible(|pill| pill.border(hairline()).border_color(theme.accent))
            .hover(|pill| pill.opacity(0.88))
            .active(|pill| pill.opacity(0.75))
            .child(label)
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
    /// annotation box hanging off its paragraph while one is open.
    fn render_scratchpad_rows(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(scratchpad) = &self.voice_scratchpad else {
            return div().into_any_element();
        };
        let transcript = &scratchpad.transcript;
        let muted = scratchpad.muted;
        let status = scratchpad.status;
        let annotation_target = transcript.annotation_target;
        let selection = scratchpad.selection.clone();
        let ui_family = crate::fonts::current(cx).ui;
        let mut blocks = div()
            .id("vs-rows")
            .w_full()
            .px(px(24.0))
            .pt(px(18.0))
            // Room for the hint line plus the composer overlap.
            .pb(px(lane_padding(self.composer_lane_height.get())))
            .text_size(sp(14.0))
            .text_color(theme.text)
            // Painted before any row, so the frame's selection registry
            // holds exactly the text elements this frame put on screen.
            .child(md::render::frame_reset(selection.clone()))
            .children(self.render_scratchpad_status(theme, cx));
        let mut first_drawn = true;
        for (index, paragraph) in transcript.paragraphs.iter().enumerate() {
            let is_current = index + 1 == transcript.paragraphs.len();
            let annotated = annotation_target == Some(index);
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
            let show_dot = is_current && !annotated;
            let flat = scratchpad_paragraph_text(
                paragraph,
                is_current && !annotated,
                &transcript.interim,
                &ui_family,
                theme,
            );
            let paragraph_div = div()
                .id(SharedString::from(format!("vs-paragraph-{index}")))
                .w_full()
                .relative()
                .rounded(px(4.0))
                .py(px(2.0))
                .cursor_default()
                .when(annotated, |row| {
                    row.bg(MarkdownPalette::from_theme(theme).annotation)
                })
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
                        .when(show_dot, |row| {
                            row.child(scratchpad_dot(muted, status, theme))
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(scratchpad) = &mut this.voice_scratchpad {
                        scratchpad.transcript.annotate(index);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }));
            blocks = blocks.child(paragraph_div.when(annotated, |row| {
                row.child(self.render_annotation_box(index, theme, window, cx))
            }));
            for (bullet_index, bullet) in paragraph.bullets.iter().enumerate() {
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
                blocks = blocks.child(
                    div()
                        .w_full()
                        .pl(px(18.0))
                        .py(px(2.0))
                        .flex()
                        .gap(px(8.0))
                        .child(div().flex_none().text_color(theme.text_tertiary).child("•"))
                        .child(md::render::selectable_flat_text(
                            &flat,
                            md::selection::TextKey::new(format!("vs-b-{index}"), bullet_index),
                            selection.clone(),
                            theme.code_wash,
                            theme.selection,
                            false,
                        )),
                );
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
                            .child(scratchpad_dot(muted, status, theme)),
                    ),
            );
        }
        blocks
            // Clicking open space leaves annotation mode.
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(scratchpad) = &mut this.voice_scratchpad
                    && scratchpad.transcript.annotation_target.is_some()
                {
                    scratchpad.transcript.exit_annotation();
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    /// The failure row at the top of the content — mic denial or a dropped
    /// stream — with its remedies beside it. `Connecting` earns no chrome;
    /// the dot's presence already reads as waiting.
    fn render_scratchpad_status(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let scratchpad = self.voice_scratchpad.as_ref()?;
        let (message, system_settings) = match scratchpad.status {
            ScratchpadStatus::Connecting | ScratchpadStatus::Live => return None,
            ScratchpadStatus::MicDenied => (tr!("voice_scratchpad.mic_denied"), true),
            ScratchpadStatus::ConnectionLost => (tr!("voice_scratchpad.connection_lost"), false),
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
                .child(
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
                            let denied = this
                                .voice_scratchpad
                                .as_ref()
                                .is_some_and(|scratchpad| {
                                    scratchpad.status == ScratchpadStatus::MicDenied
                                });
                            if denied {
                                // Re-ask TCC — the prompt replays only when the
                                // user removed access, otherwise the answer
                                // lands immediately.
                                match crate::platform::microphone_access() {
                                    crate::platform::CaptureAccess::Granted => {
                                        this.begin_voice_capture(cx)
                                    }
                                    crate::platform::CaptureAccess::Undetermined => {
                                        let tx = this.voice_scratchpad_tx.clone();
                                        let wake = this.event_wake_tx.clone();
                                        crate::platform::request_microphone_access(Box::new(
                                            move |granted| {
                                                let _ = tx.try_send((
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
                                this.spawn_transcription_worker(cx);
                            }
                        })),
                )
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
        let Some(scratchpad) = &self.voice_scratchpad else {
            return div().into_any_element();
        };
        let mut text = scratchpad.transcript.annotation_text.clone();
        let interim = scratchpad.transcript.annotation_interim.trim().to_owned();
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
            .child(scratchpad_dot(scratchpad.muted, scratchpad.status, theme))
            .child(div().min_w_0().flex_1().child(text));
        deferred(FloatingSurface::anchored_to_parent(
            box_content.into_any_element(),
            MenuAlign::BelowLeft,
            px(6.0),
            px(8.0),
        ))
        .into_any_element()
    }

    /// The discard confirmation — the same modal posture the close dialog
    /// uses, armed by Cancel once the transcript is substantial.
    pub(super) fn render_scratchpad_discard(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = Theme::current(cx);
        let scratchpad = self.voice_scratchpad.as_ref()?;
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
                        if let Some(scratchpad) = &mut this.voice_scratchpad {
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
                                            if let Some(scratchpad) = &mut this.voice_scratchpad {
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
                                                        &mut this.voice_scratchpad
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

/// Bottom padding under the transcript rows — enough room for the hint line
/// plus whatever the composer lane overlaps.
fn lane_padding(lane: f32) -> f32 {
    lane + GRADIENT_VISIBLE + 30.0
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
        append_word_text(&mut text, interim.trim());
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

/// The glowing record dot: solid core plus a tight pink halo on a soft
/// under-shadow — the design's pink drop-shadow read.
fn scratchpad_dot(muted: bool, status: ScratchpadStatus, theme: &Theme) -> Div {
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
    div()
        .flex_none()
        .size(px(15.0))
        .rounded_full()
        .bg(core)
        .border(px(1.0))
        .border_color(gpui::black().opacity(0.10))
        .shadow(vec![
            gpui::BoxShadow::new(px(0.0), px(2.0), glow.opacity(0.5)).blur_radius(px(4.0)),
        ])
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
}
