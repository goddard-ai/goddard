//! [`BlockTree`] → GPUI elements.
//!
//! Two properties drive every decision here.
//!
//! **One shaped element per block.** A paragraph becomes a single
//! [`StyledText`] over one flat string with `TextRun`s for its inline styles —
//! not one element per line, and not an element per styled span. Layout cost
//! per block is therefore one measured-layout node and one `shape_text` call,
//! which GPUI's line-layout cache reuses verbatim across frames when the text
//! and wrap width are unchanged.
//! Math paragraphs use one measured element with retained native glyph layouts
//! and cached formula images (see [`math_text`]); ordinary prose stays on the
//! StyledText path.
//!
//! **Color is paint, geometry is layout.** Syntax highlighting, inline-code
//! washes and the selection wash are all painted from geometry read back out of
//! the text's own [`TextLayout`], so none of them can change a row's measured
//! height. That is what lets a streaming code block colorize progressively
//! without ever reflowing, and what keeps the transcript's row measurements
//! stable while a selection is dragged across it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use gpui::{
    Action, AnyElement, BorderStyle, Bounds, ClipboardItem, CursorStyle, DispatchPhase, Div, Font,
    FontStyle, FontWeight, HitboxId, Hsla, InteractiveText, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, SharedString,
    StrikethroughStyle, StyledText, TextLayout, TextRun, UnderlineStyle, Window, canvas, div, font,
    img, point, prelude::*, px, quad, relative, size,
};
use regex::Regex;
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

use super::highlight::{self, Lang, TokenClass};
use super::mend::PENDING_LINK_URL;
use super::parser::{Block, IncrementalParser, InlineRun, ListItem, TableAlign, TopBlock};
use super::selection::{
    CopySpec, RegisteredText, SelectionRegistry, SelectionState, Span, TextKey, line_range,
    word_range,
};
use super::veil::{RowVeil, apply_veil};
use crate::fonts::Fonts;
use crate::theme::{Theme, hairline};
use crate::ui::menu::{ContextMenuHandle, MenuItem, context_menu};
use crate::ui::tooltip::Tooltip;

mod math_text;

/// Selection geometry: the laid-out text handle for one painted element.
#[derive(Clone)]
pub enum TextGeometry {
    Text(TextLayout),
    Math(math_text::Geometry),
}

impl TextGeometry {
    fn bounds(&self) -> Bounds<Pixels> {
        match self {
            Self::Text(layout) => layout.bounds(),
            Self::Math(layout) => layout.bounds(),
        }
    }

    fn index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        match self {
            Self::Text(layout) => layout.index_for_position(position),
            Self::Math(layout) => layout.index_for_position(position),
        }
    }

    pub(crate) fn is_missing(&self) -> bool {
        match self {
            Self::Text(layout) => layout_missing(layout),
            Self::Math(layout) => layout.is_missing(),
        }
    }
}

/// The transcript's shared selection handles, specialised to real geometry.
pub type TranscriptSelection = SelectionState<TextGeometry>;

/// An optional app-owned override for clicked markdown links.
///
/// The markdown renderer stays unaware of projects and workspace surfaces;
/// callers that do have that context can intercept a link, while every other
/// markdown view continues to use GPUI's ordinary URL opener.
pub type LinkHandler = Rc<dyn Fn(&str, &mut Window, &mut gpui::App)>;

/// The rows a right-clicked `@`-mention contributes to the row's context
/// menu, built from the mention's resolved absolute path. The renderer stays
/// unaware of remotes and workspace surfaces, so the caller decides what a
/// path can do.
pub type FileRefMenuItems = Rc<dyn Fn(&str, &mut gpui::App) -> Vec<MenuItem>>;

// ── Layout metrics ─────────────────────────────────────────────────────────
//
// Everything in this block participates in measurement, so these are the only
// numbers that can change a transcript row's height.

/// Paragraph and inline metrics for one text scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub text_size: f32,
    pub line_height: f32,
    pub code_text_size: f32,
    pub code_line_height: f32,
    /// Vertical gap between sibling blocks.
    pub block_gap: f32,
}

impl Metrics {
    /// Assistant response scale, matching the transcript's body text.
    pub const BODY: Self = Self {
        text_size: 14.0,
        line_height: 21.0,
        code_text_size: 13.0,
        code_line_height: 19.5,
        block_gap: 10.0,
    };

    /// User-message scale. Markdown blocks keep the bubble's established body
    /// geometry instead of making every existing plain prompt subtly reflow.
    pub const USER_MESSAGE: Self = Self {
        text_size: 14.5,
        line_height: 21.0,
        code_text_size: 13.0,
        code_line_height: 19.5,
        block_gap: 10.0,
    };

    /// Compact scale for reasoning, tool detail, and other secondary content.
    /// One notch under [`Metrics::BODY`], not a miniature: secondary reading
    /// text stays close to prose size and leans on color for its hierarchy.
    pub const COMPACT: Self = Self {
        text_size: 13.5,
        line_height: 19.5,
        code_text_size: 13.0,
        code_line_height: 19.5,
        block_gap: 7.0,
    };

    /// The UI and code font sizes the constants above were authored against.
    /// [`Metrics::scaled`] is the identity at these values, so the settings'
    /// defaults reproduce the authored transcript exactly.
    const AUTHORED_UI_FONT_SIZE: f32 = 14.0;
    const AUTHORED_CODE_FONT_SIZE: f32 = 13.0;

    /// These metrics rescaled to the user's font settings: prose follows the
    /// UI font size, code spans and blocks follow the code font size, and each
    /// surface keeps its authored proportions. Values land on half pixels so
    /// scaled text stays as crisp as the authored sizes.
    pub fn scaled(self, ui_font_size: f32, code_font_size: f32) -> Self {
        let ui = ui_font_size / Self::AUTHORED_UI_FONT_SIZE;
        let code = code_font_size / Self::AUTHORED_CODE_FONT_SIZE;
        let half = |value: f32| (value * 2.0).round() / 2.0;
        Self {
            text_size: half(self.text_size * ui),
            line_height: half(self.line_height * ui),
            code_text_size: half(self.code_text_size * code),
            code_line_height: half(self.code_line_height * code),
            block_gap: half(self.block_gap * ui),
        }
    }

    /// Document scale for a full-page reading surface: prose at the user's UI
    /// font size and code at the code font size, keeping [`Metrics::BODY`]'s
    /// proportions.
    pub fn document(text_size: f32, code_text_size: f32) -> Self {
        Self {
            text_size,
            line_height: (text_size * 1.55).round(),
            code_text_size,
            code_line_height: (code_text_size * 1.5).round(),
            block_gap: (text_size * 0.72).round(),
        }
    }
}

/// Inline-code wash geometry. Paint-only: the box overhangs the glyphs
/// horizontally and insets vertically inside the line box.
const CODE_WASH_RADIUS: f32 = 4.0;
const CODE_WASH_PAD_X: f32 = 2.5;
const CODE_WASH_INSET_Y: f32 = 1.5;

/// Heading scale relative to body text, by level.
fn heading_metrics(level: u8, metrics: &Metrics) -> (f32, f32, FontWeight) {
    let (scale, weight) = match level {
        1 => (1.45, FontWeight::BOLD),
        2 => (1.28, FontWeight::BOLD),
        3 => (1.14, FontWeight::SEMIBOLD),
        4 => (1.05, FontWeight::SEMIBOLD),
        _ => (1.0, FontWeight::SEMIBOLD),
    };
    let size = (metrics.text_size * scale).round();
    (size, (size * 1.42).round(), weight)
}

// ── Palette ────────────────────────────────────────────────────────────────

/// Colors for markdown paint, resolved once per render from the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub text: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    pub ghost: Hsla,
    pub border: Hsla,
    /// Outlines on filled blocks — tables, code blocks — whose fill already
    /// delimits them.
    pub border_subtle: Hsla,
    /// Decorative rules — quote bars, horizontal rules.
    pub separator: Hsla,
    pub inset: Hsla,
    pub overlay: Hsla,
    pub code_text: Hsla,
    pub code_wash: Hsla,
    pub selection: Hsla,
    pub search_match: Hsla,
    pub active_search_match: Hsla,
    /// Soft fill marking a commented transcript passage.
    pub annotation: Hsla,
    /// Keyboard-focus highlight wash, in place of a ring.
    pub focus: Hsla,
    pub accent: Hsla,
    pub added: Hsla,
    pub removed: Hsla,
    syntax: crate::theme::SyntaxColors,
    is_dark: bool,
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let search_yellow = gpui::hsla(
            48.0 / 360.0,
            0.95,
            if theme.is_dark { 0.48 } else { 0.55 },
            1.0,
        );
        let active_search_orange = gpui::hsla(
            30.0 / 360.0,
            1.0,
            if theme.is_dark { 0.50 } else { 0.54 },
            1.0,
        );
        Self {
            text: theme.text,
            secondary: theme.text_secondary,
            tertiary: theme.text_tertiary,
            ghost: theme.text_ghost,
            border: theme.border,
            border_subtle: theme.border_subtle,
            separator: theme.separator,
            inset: theme.inset,
            overlay: theme.overlay,
            code_text: theme.code_text,
            code_wash: theme.code_wash,
            selection: theme.selection,
            search_match: search_yellow.opacity(if theme.is_dark { 0.18 } else { 0.20 }),
            active_search_match: active_search_orange.opacity(if theme.is_dark {
                0.78
            } else {
                0.70
            }),
            annotation: search_yellow.opacity(if theme.is_dark { 0.16 } else { 0.18 }),
            focus: theme.focus_highlight(),
            accent: theme.accent,
            added: theme.success,
            removed: theme.danger,
            syntax: theme.syntax,
            is_dark: theme.is_dark,
        }
    }

    /// Token colors come from the active palette — the default themes carry
    /// Goddard's restrained set, named schemes carry their own syntax hues.
    /// Shared with the code editor, so both surfaces colour code identically.
    pub fn token(&self, class: TokenClass) -> Hsla {
        match class {
            TokenClass::Keyword => self.syntax.keyword,
            TokenClass::Literal => self.syntax.literal,
            TokenClass::String => self.syntax.string,
            TokenClass::Comment => self.syntax.comment,
            TokenClass::Number => self.syntax.number,
            TokenClass::Type => self.syntax.ty,
            TokenClass::Function => self.syntax.function,
            TokenClass::Meta => self.syntax.meta,
            TokenClass::Added => self.added,
            TokenClass::Removed => self.removed,
        }
    }
}

// ── Flattened inline text ──────────────────────────────────────────────────

/// One block's inline content, ready to shape: a flat string, the `TextRun`s
/// that tile it exactly, plus the byte ranges that need paint-only decoration.
#[derive(Debug)]
pub struct FlatText {
    pub text: SharedString,
    pub runs: Vec<TextRun>,
    pub links: Vec<(Range<usize>, String)>,
    pub code_ranges: Vec<Range<usize>>,
    /// `Annotation N` citations that resolve against a submitted annotation
    /// set: byte ranges paired with the label's 1-based index. Painted as a
    /// dotted underline; hovering previews the annotation.
    pub annotation_refs: Vec<(Range<usize>, usize)>,
    /// Candidate Git commit SHAs: byte ranges paired with the written SHA.
    /// A range gets the dotted underline and hover affordance only once the
    /// app confirms the SHA resolves — see `resolved_commits` on the
    /// selection state.
    pub commit_refs: Vec<(Range<usize>, String)>,
    /// `@`-mention file references: byte ranges painted with a dotted
    /// underline. The click itself rides `links` — each range has a matching
    /// entry there whose "URL" is the mention's resolved absolute path — and
    /// a right-click contributes that path's actions to the row's menu.
    pub file_refs: Vec<Range<usize>>,
    pub math: Option<Rc<math_text::MathData>>,
    /// How the flat text maps back to markdown for copy; default emits the
    /// flat text unchanged.
    pub copy: Rc<CopySpec>,
}

/// One literal find-in-page hit inside a shaped markdown text element.
///
/// `ordinal` is the same stable per-row element ordinal used by [`TextKey`],
/// so a search performed before an off-screen row is mounted can still point
/// at the exact range the renderer will paint after navigation reveals it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextSearchMatch {
    pub ordinal: usize,
    pub range: Range<usize>,
}

/// Paint-only search state for one markdown row. The match list is prepared
/// when the query changes; rendering only indexes the ranges for each visible
/// text element and never scans the message again on a frame.
#[derive(Clone)]
pub struct SearchHighlights {
    pub matches: Rc<Vec<TextSearchMatch>>,
    pub active: Option<TextSearchMatch>,
}

/// Flatten inline runs for shaping. Pure given the palette, families, and
/// base weight.
pub fn flatten(
    runs: &[InlineRun],
    palette: &Palette,
    families: &Fonts,
    base_weight: FontWeight,
    base_color: Hsla,
) -> FlatText {
    let mut text = String::new();
    let mut out: Vec<TextRun> = Vec::with_capacity(runs.len());
    let mut links: Vec<(Range<usize>, String)> = Vec::new();
    let mut code_ranges: Vec<Range<usize>> = Vec::new();
    let mut math = Vec::new();
    let mut fragments = Vec::new();

    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        let start = text.len();
        text.push_str(&run.text);
        let end = text.len();
        if let Some(fragment) = markdown_fragment(run) {
            fragments.push((start..end, fragment));
        }
        if run.style.math {
            math.push(math_text::MathSpan {
                range: start..end,
                latex: std::sync::Arc::from(run.text.as_str()),
                display: false,
            });
        }

        let mut run_font = font(if run.style.code {
            families.code.clone()
        } else {
            families.ui.clone()
        });
        run_font.weight = if run.style.bold && base_weight < FontWeight::SEMIBOLD {
            FontWeight::SEMIBOLD
        } else {
            base_weight
        };
        run_font.style = if run.style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

        if run.style.code {
            // Merge neighbouring code runs so their washes form one box.
            match code_ranges.last_mut() {
                Some(range) if range.end == start => range.end = end,
                _ => code_ranges.push(start..end),
            }
        }
        if let Some(url) = &run.style.link {
            // A still-streaming link keeps link styling — so the URL settling
            // changes nothing visually — but must not become clickable.
            if url != PENDING_LINK_URL {
                match links.last_mut() {
                    Some((range, last)) if range.end == start && last == url => range.end = end,
                    _ => links.push((start..end, url.clone())),
                }
            }
        }

        out.push(TextRun {
            len: run.text.len(),
            font: run_font,
            color: if run.style.code {
                palette.code_text
            } else {
                base_color
            },
            // Inline code's wash is painted as *rounded* quads by the canvas
            // underlay; a run background could only ever be a square box.
            background_color: None,
            underline: run.style.link.is_some().then_some(UnderlineStyle {
                color: Some(palette.tertiary),
                thickness: px(1.0),
                wavy: false,
            }),
            strikethrough: run.style.strikethrough.then_some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(palette.tertiary),
            }),
        });
    }

    FlatText {
        text: text.into(),
        runs: out,
        links,
        code_ranges,
        annotation_refs: Vec::new(),
        commit_refs: Vec::new(),
        file_refs: Vec::new(),
        math: (!math.is_empty()).then(|| Rc::new(math_text::MathData::new(math))),
        copy: Rc::new(CopySpec {
            prefix: Rc::default(),
            suffix: Rc::default(),
            fragments,
        }),
    }
}

/// The markdown for one inline run, or `None` when it renders exactly as
/// written. Styles compose inside-out: code is atomic (its contents carry no
/// emphasis), then emphasis markers, then the link. Whitespace-only runs stay
/// plain — `** **` would just decorate the gap.
fn markdown_fragment(run: &InlineRun) -> Option<Rc<str>> {
    let style = &run.style;
    let linked = style
        .link
        .as_deref()
        .is_some_and(|url| url != PENDING_LINK_URL);
    let styled =
        style.bold || style.italic || style.code || style.strikethrough || style.math || linked;
    if !styled || run.text.trim().is_empty() {
        return None;
    }
    let mut out = run.text.clone();
    if style.code {
        // One more backtick than the longest interior run keeps `a`b`
        // pasteable; the pad spaces are required when the content touches a
        // tick.
        let interior = run
            .text
            .split(|ch| ch != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat(interior + 1);
        out = if run.text.starts_with('`') || run.text.ends_with('`') {
            format!("{fence} {out} {fence}")
        } else {
            format!("{fence}{out}{fence}")
        };
    } else {
        if style.strikethrough {
            out = format!("~~{out}~~");
        }
        if style.bold {
            out = format!("**{out}**");
        }
        if style.italic {
            out = format!("*{out}*");
        }
        if style.math {
            out = format!("${out}$");
        }
    }
    if let Some(url) = style.link.as_deref().filter(|url| *url != PENDING_LINK_URL) {
        out = format!("[{out}]({url})");
    }
    Some(Rc::from(out))
}

/// `Annotation N` — the citation label the annotation prompt header teaches
/// the agent to answer with. Word-bounded on both sides and ASCII
/// case-insensitive on the word.
static ANNOTATION_REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bannotation[ \t]+(\d+)\b").unwrap());

/// The `(byte range, 1-based label)` pairs of every resolvable `Annotation N`
/// citation in a flat text. Matches inside code or link ranges are skipped —
/// quoted or formatted mentions are not citations — and a label beyond
/// `limit` resolves to nothing, so it stays plain text rather than promising
/// a tooltip that does not exist.
fn annotation_references(flat: &FlatText, limit: usize) -> Vec<(Range<usize>, usize)> {
    let overlaps = |range: &Range<usize>| decorated_range(flat, range);
    ANNOTATION_REFERENCE
        .captures_iter(flat.text.as_ref())
        .filter_map(|captures| {
            let range = captures.get(0)?.range();
            let index = captures.get(1)?.as_str().parse::<usize>().ok()?;
            (index >= 1 && index <= limit && !overlaps(&range)).then_some((range, index))
        })
        .collect()
}

/// A word-bounded hexadecimal run long enough to name a Git commit. Links and
/// rendered math keep their existing affordance; inline code still counts
/// because agents conventionally put SHAs in backticks.
static COMMIT_REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[0-9a-f]{7,40}\b").unwrap());

/// A hyphenated hexadecimal token — UUID-shaped ids like
/// `3f8a2b1c-9d4e-4f5a-8b6c-7d8e9f0a1b2c`. Their segments are word-bounded
/// hex runs `COMMIT_REFERENCE` would flag, but a SHA never contains a hyphen:
/// the whole token is one identifier, not a commit. A segment that trails
/// off into non-hex word characters (`abcdef1-based`) fails the trailing
/// `\b`, so prose suffixes keep their SHA reading.
static HYPHENATED_HEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[0-9a-f]+(?:-[0-9a-f]+)+\b").unwrap());

fn commit_references(flat: &FlatText) -> Vec<(Range<usize>, String)> {
    let hyphenated: Vec<Range<usize>> = HYPHENATED_HEX
        .find_iter(flat.text.as_ref())
        .map(|found| found.range())
        .collect();
    // A hex match is contiguous, so it cannot span a hyphen: any overlap with
    // a hyphenated token means it lies wholly inside one of its segments.
    let in_hyphenated = |range: &Range<usize>| {
        hyphenated
            .iter()
            .any(|token| token.start < range.end && range.start < token.end)
    };
    COMMIT_REFERENCE
        .find_iter(flat.text.as_ref())
        .filter_map(|found| {
            let range = found.range();
            // Inline code remains a SHA reference; fenced blocks never reach
            // this pass. Links and rendered math own their own interaction.
            (!linked_or_math_range(flat, &range) && !in_hyphenated(&range))
                .then(|| (range, found.as_str().to_ascii_lowercase()))
        })
        .collect()
}

/// An `@path` token — the composer file mention's submitted form. The `@`
/// must not follow a word character, so `user@host` is not a mention, and the
/// path runs to whitespace; punctuation typed right after it is prose, not
/// path, and gets trimmed below.
static FILE_MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^\w@])(@\S+)").unwrap());

/// The `(byte range, absolute target)` pairs of every `@`-mention in a flat
/// text. Relative tokens resolve against the session workspace, matching how
/// the composer writes them; mentions inside code, links, or math keep their
/// own meaning and are skipped.
fn file_references(flat: &FlatText, workspace: &Path) -> Vec<(Range<usize>, String)> {
    FILE_MENTION
        .captures_iter(flat.text.as_ref())
        .filter_map(|captures| {
            let found = captures.get(1)?;
            let token = found.as_str().trim_end_matches(|c| {
                matches!(
                    c,
                    '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"' | '`'
                )
            });
            let mention = token.strip_prefix('@')?;
            if mention.is_empty() {
                return None;
            }
            let range = found.start()..found.start() + token.len();
            if decorated_range(flat, &range) {
                return None;
            }
            let path = Path::new(mention);
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                workspace.join(path)
            };
            // Mentions spell paths with forward slashes; keep the resolved
            // target consistent on Windows, where `join` would mix in `\`.
            Some((range, resolved.to_string_lossy().replace('\\', "/")))
        })
        .collect()
}

fn decorated_range(flat: &FlatText, range: &Range<usize>) -> bool {
    flat.code_ranges
        .iter()
        .any(|code| code.start < range.end && range.start < code.end)
        || linked_or_math_range(flat, range)
}

fn linked_or_math_range(flat: &FlatText, range: &Range<usize>) -> bool {
    flat.links
        .iter()
        .any(|(link, _)| link.start < range.end && range.start < link.end)
        || flat.math.as_ref().is_some_and(|math| {
            math.spans
                .iter()
                .any(|span| span.range.start < range.end && range.start < span.range.end)
        })
}

/// A flat string with uniform styling, for non-markdown transcript text.
pub fn flatten_plain(
    text: impl Into<SharedString>,
    family: impl Into<SharedString>,
    weight: FontWeight,
    color: Hsla,
) -> FlatText {
    let text: SharedString = text.into();
    let mut run_font = font(family);
    run_font.weight = weight;
    let runs = if text.is_empty() {
        Vec::new()
    } else {
        vec![TextRun {
            len: text.len(),
            font: run_font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }]
    };
    FlatText {
        text,
        runs,
        links: Vec::new(),
        code_ranges: Vec::new(),
        annotation_refs: Vec::new(),
        commit_refs: Vec::new(),
        file_refs: Vec::new(),
        math: None,
        copy: Rc::default(),
    }
}

/// Guided-reading tunables, using the scales the bionic-reading tools
/// popularized: `fixation` is how much of each word is emphasized (1–5,
/// mapping to ~20–60%), `saccade` is the letter distance the eye jumps
/// between emphasized words (10–50), and `opacity` fades the unemphasized
/// text (0–100 percent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuidedReading {
    pub fixation: u8,
    pub saccade: u8,
    pub opacity: u8,
}

impl Default for GuidedReading {
    fn default() -> Self {
        Self {
            fixation: 3,
            saccade: 10,
            opacity: 100,
        }
    }
}

/// Guided reading: split each prose run so a word's leading graphemes shape
/// at `SEMIBOLD` and the rest keeps the run's weight — the emphasis pattern
/// sold elsewhere as "bionic reading". Only `runs` densifies; the flat string
/// and every byte-range index (`links`, `code_ranges`, `copy`) are untouched.
///
/// Two kinds of text are never split. Runs already at `SEMIBOLD` or heavier —
/// markdown bold, headings — gain nothing but shaping cost. And anything
/// monospace or inside `code_ranges`/math stays whole: a mid-word style
/// boundary drops kerning and breaks joining scripts, so eligibility is
/// restricted to words made of Latin, Greek, or Cyrillic letters anyway.
pub fn apply_fixation(flat: &mut FlatText, code_family: &SharedString, guided: &GuidedReading) {
    let mut protected: Vec<Range<usize>> = flat.code_ranges.clone();
    if let Some(math) = &flat.math {
        protected.extend(math.spans.iter().map(|span| span.range.clone()));
    }
    protected.sort_by_key(|range| range.start);

    let alpha = (f32::from(guided.opacity) / 100.0).clamp(0.0, 1.0);
    // The saccade distance is counted across the whole element so a word
    // split across styled runs can't cheat the jump — and starts full, so
    // each element's first word is always anchored.
    let mut distance = usize::from(guided.saccade.clamp(10, 50));
    let mut runs: Vec<TextRun> = Vec::with_capacity(flat.runs.len() * 2);
    let mut offset = 0;
    for run in flat.runs.drain(..) {
        let end = offset + run.len;
        if run.font.weight >= FontWeight::SEMIBOLD || run.font.family == *code_family {
            // Untouchable text still occupies space the jump crosses.
            distance += flat.text[offset..end].graphemes(true).count();
            offset = end;
            runs.push(run);
            continue;
        }
        let segments = fixation_segments(
            &flat.text,
            offset..end,
            |at| {
                protected
                    .iter()
                    .any(|range| range.start <= at && at < range.end)
            },
            guided,
            &mut distance,
        );
        for (range, fixated) in segments {
            let mut split = run.clone();
            split.len = range.len();
            if fixated {
                split.font.weight = FontWeight::SEMIBOLD;
            } else if alpha < 1.0 {
                split.color = split.color.opacity(alpha);
            }
            runs.push(split);
        }
        offset = end;
    }
    flat.runs = runs;
}

/// Tile `range` of `text` into `(byte_range, fixated)` segments: each maximal
/// run of eligible letters becomes a word, and the word the `saccade` jump
/// lands on gets its leading share — the `fixation` fraction — emphasized.
/// `distance` counts the graphemes traversed since the last fixated word,
/// whitespace included; a word is anchored when the jump crosses it, so a
/// word longer than the remaining distance still gets anchored at its start.
/// Everything else — digits, punctuation, protected spans, jumped-over
/// words — is emitted plain. Splits only ever land on grapheme boundaries;
/// cutting inside a cluster would shape a dangling combining mark.
fn fixation_segments(
    text: &str,
    range: Range<usize>,
    in_protected: impl Fn(usize) -> bool,
    guided: &GuidedReading,
    distance: &mut usize,
) -> Vec<(Range<usize>, bool)> {
    let saccade_letters = usize::from(guided.saccade.clamp(10, 50));
    // Fixation level → share of the word emphasized, in tenths.
    let fraction_tenths = usize::from(guided.fixation.clamp(1, 5)) + 1;
    let mut segments: Vec<(Range<usize>, bool)> = Vec::new();
    let mut word: Vec<Range<usize>> = Vec::new();
    let mut cursor = range.start;
    // Adjacent plain spans coalesce so a jumped-over word and its whitespace
    // cost one run, not three.
    let push_plain = |segments: &mut Vec<(Range<usize>, bool)>, range: Range<usize>| {
        if range.is_empty() {
            return;
        }
        if let Some(last) = segments.last_mut()
            && !last.1
        {
            last.0.end = range.end;
        } else {
            segments.push((range, false));
        }
    };
    let flush_word = |word: &mut Vec<Range<usize>>,
                      segments: &mut Vec<(Range<usize>, bool)>,
                      cursor: &mut usize,
                      distance: &mut usize,
                      text: &str| {
        let Some(first) = word.first() else { return };
        let start = first.start;
        if *cursor < start {
            push_plain(segments, *cursor..start);
            *distance += text[*cursor..start].graphemes(true).count();
        }
        let end = word.last().map_or(start, |cluster| cluster.end);
        *distance += word.len();
        if *distance >= saccade_letters {
            *distance = 0;
            // Fixation prefix: the level's share of the word's grapheme
            // clusters, at least 1.
            let fix = ((word.len() * fraction_tenths + 9) / 10).max(1);
            let fix_end = word[(fix - 1).min(word.len() - 1)].end;
            segments.push((start..fix_end, true));
            push_plain(segments, fix_end..end);
        } else {
            push_plain(segments, start..end);
        }
        *cursor = end;
        word.clear();
    };
    for (index, cluster) in text[range.clone()].grapheme_indices(true) {
        let start = range.start + index;
        let eligible = cluster.chars().next().is_some_and(|c| {
            c.is_alphabetic()
                && matches!(c.script(), Script::Latin | Script::Greek | Script::Cyrillic)
        }) && !in_protected(start);
        if eligible {
            word.push(start..start + cluster.len());
        } else {
            flush_word(&mut word, &mut segments, &mut cursor, distance, text);
        }
    }
    flush_word(&mut word, &mut segments, &mut cursor, distance, text);
    *distance += text[cursor..range.end].graphemes(true).count();
    push_plain(&mut segments, cursor..range.end);
    segments
}

// ── Per-message state ──────────────────────────────────────────────────────

/// Everything the renderer keeps between frames for one markdown body.
///
/// The flatten cache is keyed by element ordinal and pruned only back to the
/// parser's stable prefix, so a streamed delta rebuilds the final block's
/// elements and reuses every settled one.
pub struct MarkdownView {
    parser: IncrementalParser,
    /// Mended replacement for the final block while streaming.
    tail: Vec<TopBlock>,
    flats: RefCell<HashMap<usize, Rc<FlatText>>>,
    /// First element ordinal belonging to the final block — the only block an
    /// append can change. Recorded during render, because only the renderer
    /// knows how many text elements each block expands into.
    volatile_from: Cell<usize>,
    /// Style the cached flats were built for. Colors and families live inside
    /// `TextRun`s, so a theme or font change has to drop them or the
    /// transcript keeps painting the old faces — same for guided reading,
    /// which densifies the runs themselves.
    style: RefCell<Option<(Palette, Metrics, Fonts, Option<GuidedReading>)>>,
    /// Per-element opacity spans for the live response. Text is committed to
    /// layout immediately; only these paint colors animate.
    veil: RefCell<RowVeil>,
    /// Code-block ordinals currently showing successful copy feedback. Kept
    /// outside the parsed/flattened caches so a three-second icon change never
    /// invalidates text shaping.
    copied_code_blocks: Rc<RefCell<HashMap<usize, u64>>>,
    /// Drag-resized table column fractions and the in-flight drag. Shared
    /// with the painted move/up listeners, which is why it lives behind `Rc`
    /// like `copied_code_blocks`.
    table_resize: Rc<RefCell<TableResize>>,
    streaming: Cell<bool>,
}

/// User-set column widths for one markdown body's tables, plus the drag that
/// produces them. Widths are fractions of the table width keyed by the table
/// block's base ordinal — stable across re-renders, same scheme as the
/// flatten cache.
#[derive(Default)]
struct TableResize {
    widths: HashMap<usize, Vec<f32>>,
    drag: Option<TableDrag>,
}

/// An in-flight column-boundary drag. `original` holds the fractions at
/// mouse-down so the gesture computes one delta from the press instead of
/// accumulating error across repaints.
struct TableDrag {
    table: usize,
    /// Left column of the resized pair; the boundary sits between it and
    /// `column + 1`.
    column: usize,
    start_x: Pixels,
    original: Vec<f32>,
}

/// Smallest share of the table a dragged column can take.
const MIN_COLUMN_FRACTION: f32 = 0.06;

/// Width of a resize handle's hit region, centered on the boundary.
const RESIZE_HANDLE_WIDTH: f32 = 9.0;

/// Arrow-key step for a focused resize handle, as a fraction of the table.
const RESIZE_KEY_STEP: f32 = 0.04;

/// The pair of fractions a boundary drag or key step produces: the left
/// column takes `delta` from the right, both clamped to the floor while
/// their sum stays constant.
fn resized_pair(original: &[f32], column: usize, delta: f32) -> Option<(f32, f32)> {
    let (left, right) = (*original.get(column)?, *original.get(column + 1)?);
    let pair = left + right;
    if pair < MIN_COLUMN_FRACTION * 2.0 {
        return None;
    }
    let left = (left + delta).clamp(MIN_COLUMN_FRACTION, pair - MIN_COLUMN_FRACTION);
    Some((left, pair - left))
}

impl Default for MarkdownView {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownView {
    pub fn new() -> Self {
        Self {
            parser: IncrementalParser::new(),
            tail: Vec::new(),
            flats: RefCell::new(HashMap::new()),
            volatile_from: Cell::new(0),
            style: RefCell::new(None),
            veil: RefCell::new(RowVeil::default()),
            copied_code_blocks: Rc::new(RefCell::new(HashMap::new())),
            table_resize: Rc::new(RefCell::new(TableResize::default())),
            streaming: Cell::new(false),
        }
    }

    /// A view attached to an already-streaming body. Its first rendered text
    /// becomes the full-opacity baseline; later appends fade normally.
    pub fn seeded() -> Self {
        let view = Self::new();
        *view.veil.borrow_mut() = RowVeil::seeded();
        view
    }

    /// Reattach an existing parsed view without animating text that arrived
    /// while its session was off screen.
    pub fn seed_streaming_baseline(&self) {
        *self.veil.borrow_mut() = RowVeil::seeded();
    }

    /// Point the view at `text`. `mend` closes hanging inline markers, which is
    /// wanted while a response streams and not once it has settled.
    /// Bytes of source this view retains. Parsed structures run to roughly
    /// seventeen times this, so it is the honest unit for bounding a cache.
    pub fn source_len(&self) -> usize {
        self.parser.text().len()
    }

    pub fn set_text(&mut self, text: &str, mend: bool) {
        let was_streaming = self.streaming.replace(mend);
        if !mend && was_streaming {
            *self.veil.borrow_mut() = RowVeil::default();
        } else if mend && !was_streaming && !self.parser.text().is_empty() {
            // A completed body that starts streaming again already has a
            // rendered baseline. Do not make that history dissolve again.
            *self.veil.borrow_mut() = RowVeil::seeded();
        }
        let changed = self.parser.text() != text;
        let append = !changed || text.starts_with(self.parser.text());
        if changed {
            self.parser.set_text(text);
        }
        // The mended display tail depends only on the source and the
        // streaming flag. Deriving it re-mends — and, with a hanging marker,
        // re-parses — the final block, and `set_text` runs for every visible
        // row on every frame, so a frame that changed neither input must not
        // pay for it.
        if changed || mend != was_streaming {
            let tail = if mend {
                self.parser.display_tail().unwrap_or_default()
            } else {
                Vec::new()
            };
            if changed || tail != self.tail {
                self.tail = tail;
                // Markdown block structure only ever extends the final block,
                // so every element before it is still valid. A streamed delta
                // thus re-flattens one block instead of the whole response.
                let boundary = if append { self.volatile_from.get() } else { 0 };
                self.flats
                    .borrow_mut()
                    .retain(|ordinal, _| *ordinal < boundary);
            }
        }
    }

    pub fn is_fading(&self) -> bool {
        self.streaming.get() && self.veil.borrow().is_fading()
    }

    /// Drop cached flats if the style they were built for no longer applies.
    /// `guided_reading` lives inside `TextRun`s, so the toggle invalidates
    /// the same way a theme or font change does.
    fn sync_style(
        &self,
        palette: &Palette,
        metrics: &Metrics,
        families: &Fonts,
        guided_reading: Option<GuidedReading>,
    ) {
        let current = (*palette, *metrics, families.clone(), guided_reading);
        let mut style = self.style.borrow_mut();
        if style.as_ref() != Some(&current) {
            *style = Some(current);
            self.flats.borrow_mut().clear();
        }
    }

    /// Flattened inline content for the element at `ordinal`, built on miss.
    fn flat(&self, ordinal: usize, build: impl FnOnce() -> FlatText) -> Rc<FlatText> {
        self.flats
            .borrow_mut()
            .entry(ordinal)
            .or_insert_with(|| Rc::new(build()))
            .clone()
    }

    /// The source byte range of the top-level block `index` — the same index
    /// an element ordinal encodes, see [`block_index_of_ordinal`]. Lets a
    /// selection made on rendered text point back into the source, the way
    /// the file preview maps its annotations onto the underlying file.
    pub fn block_source_range(&self, index: usize) -> Option<Range<usize>> {
        let all = &self.parser.tree().blocks;
        let settled = if self.tail.is_empty() {
            all.len()
        } else {
            self.parser.display_tail_start()
        };
        if index < settled {
            all.get(index).map(|top| top.range.clone())
        } else {
            self.tail.get(index - settled).map(|top| top.range.clone())
        }
    }

    /// Display blocks in document order: the settled prefix, then the mended
    /// tail when one is active.
    fn blocks(&self) -> impl Iterator<Item = &Block> + '_ {
        let all = &self.parser.tree().blocks;
        let settled = if self.tail.is_empty() {
            all.len()
        } else {
            self.parser.display_tail_start()
        };
        all[..settled]
            .iter()
            .chain(self.tail.iter())
            .map(|top| &top.block)
    }

    /// Whether a table appears anywhere, including nested in a quote or list
    /// item. Table columns size as fractions of their container, so a table
    /// claims no intrinsic width — a shrink-wrapped parent like the user
    /// bubble has to offer it the full row explicitly.
    pub fn contains_table(&self) -> bool {
        self.blocks().any(block_contains_table)
    }
}

fn block_contains_table(block: &Block) -> bool {
    match block {
        Block::Table { .. } => true,
        Block::BlockQuote { children } => children.iter().any(block_contains_table),
        Block::List { items, .. } => items
            .iter()
            .any(|item| item.blocks.iter().any(block_contains_table)),
        _ => false,
    }
}

// ── Render context ─────────────────────────────────────────────────────────

/// Everything a render pass needs, plus the element counter that assigns
/// document-ordered keys. Keys stay stable frame to frame as long as the block
/// structure does, which is what lets a selection survive scrolling.
pub struct Ctx<'a> {
    row: Rc<str>,
    palette: &'a Palette,
    metrics: Metrics,
    /// The faces inline code and prose shape against. Captured per pass
    /// because the cache keys on it — a font change must not leak into flats
    /// built for the previous family.
    families: Fonts,
    selection: TranscriptSelection,
    search: Option<SearchHighlights>,
    link_handler: Option<LinkHandler>,
    /// Remote image URL → already-downloaded local file, for surfaces that
    /// cache media off-thread (the GitHub detail). A miss falls through to
    /// `image_placeholder`, then to GPUI fetching the URL itself.
    image_resolver: Option<Rc<dyn Fn(&str) -> Option<PathBuf>>>,
    /// Element standing in for an image still being fetched. Consulted only
    /// when `image_resolver` has no answer yet.
    image_placeholder: Option<Rc<dyn Fn(&str) -> Option<AnyElement>>>,
    /// How many `Annotation N` labels this row can cite — the size of the
    /// annotation set its most recent annotated submission carried. Zero on
    /// rows and surfaces that cannot cite annotations.
    annotation_ref_labels: usize,
    /// Whether hexadecimal commit references get the transcript's hover and
    /// click affordance.
    commit_refs: bool,
    /// The workspace `@`-mentions in this row resolve against, when the row
    /// presents them as file links (user prompts).
    file_link_root: Option<PathBuf>,
    /// Cross-frame flatten cache, when this render has one to consult.
    cache: Option<&'a MarkdownView>,
    next_ordinal: Cell<usize>,
    /// Set while rendering the first element of a block, for copy spacing.
    starts_block: Cell<bool>,
    /// Markdown prefix every element in the current container contributes to
    /// copy: a blockquote pushes `> `, a list item pushes its continuation
    /// indent. Scoped — renderers restore it when the container ends.
    copy_margin: Cell<Option<Rc<str>>>,
    /// One-shot markdown prefix for the next registered element — a list
    /// marker like `- ` or `3. ` — replacing the margin for that element.
    copy_lead: Cell<Option<Rc<str>>>,
    animate_streaming: bool,
    math_enabled: bool,
    /// Guided reading: split prose runs so word-leading graphemes shape at
    /// `SEMIBOLD` — the parameters when the experiment is on. Style-
    /// affecting, so it joins `MarkdownView::sync_style`'s cache stamp.
    guided_reading: Option<GuidedReading>,
    /// The enclosing context menu inner affordances contribute actions to —
    /// formulas and `@`-mention file references. `None` where the surface has
    /// no menu to join.
    context_menu: Option<ContextMenuHandle>,
    /// Whether [`markdown`] wraps the body in that menu itself — standalone
    /// surfaces only; a transcript row's own menu already wraps it.
    wrap_context_menu: bool,
    /// Builds the menu rows a right-clicked `@`-mention contributes.
    file_ref_items: Option<FileRefMenuItems>,
    now: Instant,
}

impl<'a> Ctx<'a> {
    pub fn new(
        row: impl Into<Rc<str>>,
        palette: &'a Palette,
        metrics: Metrics,
        selection: TranscriptSelection,
    ) -> Self {
        Self {
            row: row.into(),
            palette,
            metrics,
            families: Fonts::default(),
            selection,
            search: None,
            link_handler: None,
            image_resolver: None,
            image_placeholder: None,
            annotation_ref_labels: 0,
            commit_refs: false,
            file_link_root: None,
            cache: None,
            next_ordinal: Cell::new(0),
            starts_block: Cell::new(true),
            copy_margin: Cell::new(None),
            copy_lead: Cell::new(None),
            animate_streaming: true,
            math_enabled: true,
            guided_reading: None,
            context_menu: None,
            wrap_context_menu: false,
            file_ref_items: None,
            now: Instant::now(),
        }
    }

    pub fn selection(&self) -> &TranscriptSelection {
        &self.selection
    }

    pub fn families(&self) -> &Fonts {
        &self.families
    }

    pub fn with_link_handler(mut self, handler: LinkHandler) -> Self {
        self.link_handler = Some(handler);
        self
    }

    /// Map remote image URLs to local files — see `image_resolver`.
    pub fn with_image_resolver(mut self, resolver: Rc<dyn Fn(&str) -> Option<PathBuf>>) -> Self {
        self.image_resolver = Some(resolver);
        self
    }

    /// Substitute an element for an image whose fetch is still in flight —
    /// see `image_placeholder`.
    pub fn with_image_placeholder(
        mut self,
        placeholder: Rc<dyn Fn(&str) -> Option<AnyElement>>,
    ) -> Self {
        self.image_placeholder = Some(placeholder);
        self
    }

    /// Enable `Annotation N` citation marks for this row. `count` is the
    /// resolvable annotation set's size; a larger label gets no affordance.
    pub fn with_annotation_labels(mut self, count: usize) -> Self {
        self.annotation_ref_labels = count;
        self
    }

    /// Enable commit-SHA references for this surface.
    pub fn with_commit_refs(mut self, enabled: bool) -> Self {
        self.commit_refs = enabled;
        self
    }

    /// Enable `@`-mention file references for this row, resolved against
    /// `workspace`. `None` leaves the tokens plain text.
    pub fn with_file_link_root(mut self, workspace: Option<PathBuf>) -> Self {
        self.file_link_root = workspace;
        self
    }

    pub fn with_search_highlights(mut self, highlights: SearchHighlights) -> Self {
        self.search = Some(highlights);
        self
    }

    pub fn with_streaming_animation(mut self, animate: bool) -> Self {
        self.animate_streaming = animate;
        self
    }

    pub fn with_families(mut self, families: Fonts) -> Self {
        self.families = families;
        self
    }

    pub fn with_math_enabled(mut self, enabled: bool) -> Self {
        self.math_enabled = enabled;
        self
    }

    /// Guided reading: word-leading graphemes in prose shape semibold —
    /// see [`apply_fixation`].
    pub fn with_guided_reading(mut self, guided: Option<GuidedReading>) -> Self {
        self.guided_reading = guided;
        self
    }

    /// Contribute formula and file-reference actions to an existing menu —
    /// a transcript row's message menu.
    pub fn with_context_menu(mut self, menu: ContextMenuHandle) -> Self {
        self.context_menu = Some(menu);
        self.wrap_context_menu = false;
        self
    }

    /// Give a standalone Markdown surface its own context menu.
    pub fn with_standalone_context_menu(mut self, menu: ContextMenuHandle) -> Self {
        self.context_menu = Some(menu);
        self.wrap_context_menu = true;
        self
    }

    /// Enable `@`-mention context actions — see `file_ref_items`.
    pub fn with_file_ref_items(mut self, items: FileRefMenuItems) -> Self {
        self.file_ref_items = Some(items);
        self
    }

    fn with_cache(&self, view: &'a MarkdownView) -> Self {
        Self {
            row: self.row.clone(),
            palette: self.palette,
            metrics: self.metrics,
            families: self.families.clone(),
            selection: self.selection.clone(),
            search: self.search.clone(),
            link_handler: self.link_handler.clone(),
            image_resolver: self.image_resolver.clone(),
            image_placeholder: self.image_placeholder.clone(),
            annotation_ref_labels: self.annotation_ref_labels,
            commit_refs: self.commit_refs,
            file_link_root: self.file_link_root.clone(),
            cache: Some(view),
            next_ordinal: Cell::new(self.next_ordinal.get()),
            starts_block: Cell::new(self.starts_block.get()),
            copy_margin: Cell::new(self.copy_margin()),
            copy_lead: Cell::new(None),
            animate_streaming: self.animate_streaming,
            math_enabled: self.math_enabled,
            guided_reading: self.guided_reading,
            context_menu: self.context_menu.clone(),
            wrap_context_menu: self.wrap_context_menu,
            file_ref_items: self.file_ref_items.clone(),
            now: Instant::now(),
        }
    }

    fn next_key(&self) -> TextKey {
        let ordinal = self.next_ordinal.get();
        self.next_ordinal.set(ordinal + 1);
        TextKey::new(self.row.clone(), ordinal)
    }

    fn take_block_break(&self) -> bool {
        self.starts_block.replace(false)
    }

    /// The current copy margin — `Cell<Option<Rc>>` can't be read in place,
    /// so this swaps it out and back.
    fn copy_margin(&self) -> Option<Rc<str>> {
        let margin = self.copy_margin.take();
        self.copy_margin.set(margin.clone());
        margin
    }

    /// Extend the copy margin for a nested container, returning the previous
    /// value for the caller to restore when the container's blocks are done.
    fn push_copy_margin(&self, add: &str) -> Option<Rc<str>> {
        let previous = self.copy_margin.take();
        let margin = format!("{}{add}", previous.as_deref().unwrap_or(""));
        self.copy_margin.set(Some(Rc::from(margin)));
        previous
    }

    /// The element's copy spec: its own fragments and wrap, preceded by the
    /// container's markdown prefix — the one-shot `copy_lead` a list marker
    /// left, else the `copy_margin` — so a heading inside a quote copies as
    /// `> ## Title`.
    fn copy_spec(&self, flat: &Rc<FlatText>) -> Rc<CopySpec> {
        let container = self.copy_lead.take().or_else(|| self.copy_margin());
        match container {
            None => flat.copy.clone(),
            Some(prefix) => {
                let mut spec = (*flat.copy).clone();
                spec.prefix = Rc::from(format!("{prefix}{}", spec.prefix));
                Rc::new(spec)
            }
        }
    }

    /// Flatten through the cache when one is wired: a settled block reuses its
    /// string and `TextRun`s untouched, so an unchanged paragraph costs one
    /// `Rc` clone per frame instead of a fresh allocation.
    fn flat(&self, ordinal: usize, build: impl FnOnce() -> FlatText) -> Rc<FlatText> {
        self.flat_inner(ordinal, true, build)
    }

    /// [`Self::flat`] without reference detection — a code block's contents are
    /// quoted text, not the agent's voice, so citations and SHAs inside one get
    /// no transcript affordance.
    fn flat_undecorated(&self, ordinal: usize, build: impl FnOnce() -> FlatText) -> Rc<FlatText> {
        self.flat_inner(ordinal, false, build)
    }

    fn flat_inner(
        &self,
        ordinal: usize,
        detect_refs: bool,
        build: impl FnOnce() -> FlatText,
    ) -> Rc<FlatText> {
        let build = || {
            let mut flat = build();
            if let Some(guided) = self.guided_reading {
                apply_fixation(&mut flat, &self.families.code, &guided);
            }
            if detect_refs {
                if self.annotation_ref_labels > 0 {
                    flat.annotation_refs = annotation_references(&flat, self.annotation_ref_labels);
                }
                if self.commit_refs {
                    flat.commit_refs = commit_references(&flat);
                }
                if let Some(root) = &self.file_link_root {
                    let refs = file_references(&flat, root);
                    flat.file_refs = refs.iter().map(|(range, _)| range.clone()).collect();
                    flat.links.extend(refs);
                }
            }
            flat
        };
        match self.cache {
            Some(view) => view.flat(ordinal, build),
            None => Rc::new(build()),
        }
    }
}

// ── The shared text primitive ──────────────────────────────────────────────

/// One selectable, decorated text element.
///
/// The `canvas` underlay is an *earlier sibling* than the text, so GPUI paints
/// it first — underneath the glyphs — while the text's prepaint has already
/// filled in the shared [`TextLayout`]. That ordering is what lets a pure-paint
/// pass read real glyph geometry without a second layout pass.
fn text_element_with_selection(
    flat: &FlatText,
    runs: Vec<TextRun>,
    key: TextKey,
    selection: TranscriptSelection,
    search: Option<SearchHighlights>,
    link_handler: Option<LinkHandler>,
    file_menu: Option<(ContextMenuHandle, FileRefMenuItems)>,
    code_wash: Hsla,
    selection_wash: Hsla,
    search_match_wash: Hsla,
    active_search_match_wash: Hsla,
    annotation_wash: Hsla,
    ref_underline: Hsla,
    ref_underline_hovered: Hsla,
    block_break: bool,
    copy: Rc<CopySpec>,
) -> AnyElement {
    let styled = StyledText::new(flat.text.clone()).with_runs(runs);
    let layout = styled.layout().clone();

    let body: AnyElement = if flat.links.is_empty() {
        styled.into_any_element()
    } else {
        let (ranges, urls): (Vec<_>, Vec<_>) = flat.links.iter().cloned().unzip();
        let id = SharedString::from(format!("{}-t{}", key.row, key.index));
        InteractiveText::new(id, styled)
            .on_click(ranges, move |clicked, window, cx| {
                if let Some(url) = urls.get(clicked) {
                    if let Some(handler) = &link_handler {
                        handler(url, window, cx);
                    } else {
                        cx.open_url(url);
                    }
                }
            })
            .into_any_element()
    };

    let underlay = canvas(|_, _, _| (), {
        let text = flat.text.clone();
        let code_ranges = flat.code_ranges.clone();
        let annotation_refs = flat.annotation_refs.clone();
        let commit_refs = flat.commit_refs.clone();
        let file_refs = flat.file_refs.clone();
        let layout = layout.clone();
        let key = key.clone();
        move |_, _, window, _| {
            for range in &code_ranges {
                for rect in range_rects(&layout, range, CODE_WASH_PAD_X, CODE_WASH_INSET_Y) {
                    window.paint_quad(quad(
                        rect,
                        px(CODE_WASH_RADIUS),
                        code_wash,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            if let Some(search) = &search {
                let first = search
                    .matches
                    .partition_point(|found| found.ordinal < key.index);
                for found in search.matches[first..]
                    .iter()
                    .take_while(|found| found.ordinal == key.index)
                {
                    let color = if search.active.as_ref() == Some(found) {
                        active_search_match_wash
                    } else {
                        search_match_wash
                    };
                    for rect in range_rects(&layout, &found.range, 1.0, 1.0) {
                        window.paint_quad(quad(
                            rect,
                            px(2.0),
                            color,
                            px(0.0),
                            gpui::transparent_black(),
                            BorderStyle::default(),
                        ));
                    }
                }
            }
            // Commented passages carry a soft fill so they read as annotated
            // rather than selected. A span only paints while its snapshot
            // still matches the element's bytes at that offset — an edited
            // message would otherwise carry a highlight over different words.
            // Painted below the selection wash so selecting across an
            // annotation still looks like a selection.
            {
                let annotations = selection.annotations.borrow();
                for (span, emphasised) in annotations.wash_spans(&key) {
                    let end = span.range.end.min(text.len());
                    if span.range.start >= end
                        || end > span.text.len()
                        || span.text.as_bytes()[..end] != text.as_bytes()[..end]
                    {
                        continue;
                    }
                    let fill_color = if emphasised {
                        annotation_wash.opacity((annotation_wash.a * 1.75).min(1.0))
                    } else {
                        annotation_wash
                    };
                    for rect in range_rects(&layout, &(span.range.start..end), 0.0, 0.0) {
                        window.paint_quad(quad(
                            rect,
                            px(0.0),
                            fill_color,
                            px(0.0),
                            gpui::transparent_black(),
                            BorderStyle::default(),
                        ));
                    }
                }
            }
            // `Annotation N` citations wear a dotted underline — the
            // affordance for the hover tooltip that resolves the label back
            // to the submitted annotation's quote and comment.
            if !annotation_refs.is_empty() {
                let hovered_ref = selection.annotations.borrow().hovered_ref.clone();
                for (range, _) in &annotation_refs {
                    let emphasised =
                        hovered_ref
                            .as_ref()
                            .is_some_and(|(hover_key, hover_range)| {
                                *hover_key == key && *hover_range == *range
                            });
                    let color = if emphasised {
                        ref_underline_hovered
                    } else {
                        ref_underline
                    };
                    for rect in range_rects(&layout, range, 0.0, 0.0) {
                        paint_dotted_underline(window, rect, color);
                    }
                }
            }
            if !commit_refs.is_empty() {
                let hovered_ref = selection.hovered_commit.borrow().clone();
                // Candidates stay plain text until a lookup confirms the SHA;
                // hex-looking ids (UUIDs, content hashes) never underline.
                let resolved = selection.resolved_commits.borrow();
                for (range, sha) in &commit_refs {
                    if !resolved.contains(sha.as_str()) {
                        continue;
                    }
                    let emphasised =
                        hovered_ref
                            .as_ref()
                            .is_some_and(|(hover_key, hover_range)| {
                                *hover_key == key && *hover_range == *range
                            });
                    let color = if emphasised {
                        ref_underline_hovered
                    } else {
                        ref_underline
                    };
                    for rect in range_rects(&layout, range, 0.0, 0.0) {
                        paint_dotted_underline(window, rect, color);
                    }
                }
            }
            // `@`-mentions ride `links` for the click and pointer cursor but
            // carry no link-styled run — the dotted underline is their only
            // affordance.
            for range in &file_refs {
                for rect in range_rects(&layout, range, 0.0, 0.0) {
                    paint_dotted_underline(window, rect, ref_underline);
                }
            }
            if let Some(range) = selection.selection.borrow().wash_range(&key) {
                for rect in range_rects(&layout, &range, 0.0, 0.0) {
                    window.paint_quad(quad(
                        rect,
                        px(0.0),
                        selection_wash,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            // Paint order is document order, so simply appending here
            // rebuilds the frame's selection continuity.
            selection.registry.borrow_mut().push(RegisteredText {
                key: key.clone(),
                text: Rc::from(text.as_ref()),
                block_break,
                annotation_refs: annotation_refs.clone(),
                commit_refs: commit_refs.clone(),
                copy: copy.clone(),
                geometry: TextGeometry::Text(layout.clone()),
            });
        }
    })
    .absolute()
    .size_full();

    let mut element = div()
        .relative()
        .w_full()
        .min_w_0()
        .cursor(CursorStyle::IBeam);
    if !flat.file_refs.is_empty()
        && let Some((menu, items)) = file_menu
    {
        let geometry = TextGeometry::Text(layout.clone());
        let file_refs = flat.file_refs.clone();
        let links = flat.links.clone();
        element = element.on_mouse_down(MouseButton::Right, move |event, _, cx| {
            if let Some(path) = file_ref_at(&geometry, &file_refs, &links, event.position) {
                menu.set_context_items(items(&path, cx));
            }
            // Bubble to the row's own menu so it opens with these actions
            // ahead of its usual ones.
        });
    }
    element.child(underlay).child(body).into_any_element()
}

fn text_element(flat: &Rc<FlatText>, key: TextKey, ctx: &Ctx) -> AnyElement {
    if ctx.math_enabled && flat.math.is_some() {
        return math_text::element(flat.clone(), key, ctx);
    }
    let runs = match ctx
        .cache
        .filter(|view| ctx.animate_streaming && view.streaming.get())
    {
        Some(view) => {
            let spans = view
                .veil
                .borrow_mut()
                .advance(key.index, flat.text.as_ref(), ctx.now);
            apply_veil(flat.runs.clone(), &spans)
        }
        None => flat.runs.clone(),
    };
    text_element_with_selection(
        flat,
        runs,
        key,
        ctx.selection.clone(),
        ctx.search.clone(),
        ctx.link_handler.clone(),
        ctx.context_menu.clone().zip(ctx.file_ref_items.clone()),
        ctx.palette.code_wash,
        ctx.palette.selection,
        ctx.palette.search_match,
        ctx.palette.active_search_match,
        ctx.palette.annotation,
        ctx.palette.tertiary,
        ctx.palette.secondary,
        ctx.take_block_break(),
        ctx.copy_spec(flat),
    )
}

/// A selectable styled line outside the markdown block renderer.
///
/// Diff viewers and other virtualized code surfaces can share the transcript's
/// cross-element selection behavior without manufacturing a markdown tree.
/// The caller supplies a stable key in paint order and decides whether copying
/// across this element should insert a paragraph break or a single newline.
pub fn selectable_flat_text(
    flat: &FlatText,
    key: TextKey,
    selection: TranscriptSelection,
    code_wash: Hsla,
    selection_wash: Hsla,
    block_break: bool,
) -> AnyElement {
    text_element_with_selection(
        flat,
        flat.runs.clone(),
        key,
        selection,
        None,
        None,
        None,
        code_wash,
        selection_wash,
        gpui::transparent_black(),
        gpui::transparent_black(),
        gpui::transparent_black(),
        gpui::transparent_black(),
        gpui::transparent_black(),
        block_break,
        flat.copy.clone(),
    )
}

/// A selectable plain-text element: user messages, tool output, anything that
/// is not markdown but still takes part in transcript-wide selection.
pub fn plain_text(
    text: impl Into<SharedString>,
    family: impl Into<SharedString>,
    weight: FontWeight,
    color: Hsla,
    ctx: &Ctx,
) -> AnyElement {
    let key = ctx.next_key();
    let flat = ctx.flat(key.index, || flatten_plain(text, family, weight, color));
    text_element(&flat, key, ctx)
}

/// A zero-size canvas that clears the frame's registry. Paint it *before* any
/// transcript text so the registry holds exactly this frame's visible elements.
pub fn frame_reset(selection: TranscriptSelection) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, _, _| selection.registry.borrow_mut().clear(),
    )
    .absolute()
    .w(px(0.0))
    .h(px(0.0))
}

// ── Selection geometry and input ───────────────────────────────────────────

/// Wash boxes for one byte range: one box per visual row the range covers, in
/// window coordinates, from the laid-out text's own geometry. `pad_x` overhangs
/// horizontally (inline code) and `inset_y` shrinks vertically; a selection
/// wash passes zero for both so its boxes tile seamlessly across wrapped rows.
pub(crate) fn range_rects(
    layout: &TextLayout,
    range: &Range<usize>,
    pad_x: f32,
    inset_y: f32,
) -> Vec<Bounds<Pixels>> {
    let mut rects = Vec::new();
    if range.is_empty() || layout_missing(layout) {
        return rects;
    }

    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let mut row_top = bounds.top();
    let mut line_start = 0;

    // A soft-wrap boundary belongs to both adjacent rows, but GPUI's generic
    // `position_for_index` gives it the preceding row's caret affinity. Walking
    // with that API therefore has to jump beyond the boundary to make progress,
    // dropping the first glyph of every continuation row. Use the shaped wrap
    // boundaries directly, as Zed's Markdown renderer does, so adjacent visual
    // rows share the exact same byte boundary without a gap.
    for line in layout.line_layouts() {
        let line_end = line_start + line.len();
        let unwrapped = &line.unwrapped_layout;
        let row_ends = line
            .wrap_boundaries()
            .iter()
            .map(|boundary| {
                let glyph = &unwrapped.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
                (line_start + glyph.index, glyph.position.x)
            })
            .chain([(line_end, unwrapped.width)]);
        let mut row_start = line_start;
        let mut row_start_x = Pixels::ZERO;

        for (row_end, row_end_x) in row_ends {
            let selected_start = range.start.max(row_start);
            let selected_end = range.end.min(row_end);
            if selected_start < selected_end {
                let x_for_index =
                    |index| bounds.left() + unwrapped.x_for_index(index - line_start) - row_start_x;
                let start_x = x_for_index(selected_start);
                let end_x = x_for_index(selected_end);
                if end_x > start_x {
                    rects.push(Bounds::new(
                        point(start_x - px(pad_x), row_top + px(inset_y)),
                        size(
                            end_x - start_x + px(2.0 * pad_x),
                            line_height - px(2.0 * inset_y),
                        ),
                    ));
                }
            }

            row_start = row_end;
            row_start_x = row_end_x;
            row_top += line_height;
        }

        // `TextLayout` separates hard lines with one newline byte, which has
        // no glyph box of its own.
        line_start = line_end + 1;
        if line_start > range.end {
            break;
        }
    }
    rects
}

/// A dotted baseline mark under `rect` — the hover affordance of an
/// `Annotation N` citation, kept visually distinct from a link's solid
/// underline.
fn paint_dotted_underline(window: &mut Window, rect: Bounds<Pixels>, color: Hsla) {
    const DOT: f32 = 1.5;
    const PERIOD: f32 = 3.0;
    let y = rect.bottom() - px(DOT) - px(0.5);
    let mut x = rect.left();
    while x + px(DOT) <= rect.right() {
        window.paint_quad(quad(
            Bounds::new(point(x, y), size(px(DOT), px(DOT))),
            px(DOT / 2.0),
            color,
            px(0.0),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
        x += px(PERIOD);
    }
}

/// Painted glyph boxes for a byte range in a registered text element.
/// Find-in-page uses this after a virtualized row mounts to reveal the exact
/// wrapped line rather than stopping at the top of a long message.
pub fn text_range_bounds(layout: &TextGeometry, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
    match layout {
        TextGeometry::Text(layout) => range_rects(layout, range, 0.0, 0.0),
        TextGeometry::Math(layout) => layout.range_rects(range),
    }
}

/// The resolved path of the `@`-mention under `position`, when the point
/// lands inside one of the mention's glyph boxes. The index probe alone would
/// also claim clicks in the slack beside the token.
pub(super) fn file_ref_at(
    geometry: &TextGeometry,
    file_refs: &[Range<usize>],
    links: &[(Range<usize>, String)],
    position: Point<Pixels>,
) -> Option<String> {
    if file_refs.is_empty() || geometry.is_missing() {
        return None;
    }
    let index = geometry
        .index_for_position(position)
        .unwrap_or_else(|index| index);
    let range = file_refs.iter().find(|range| {
        range.contains(&index)
            && text_range_bounds(geometry, range)
                .iter()
                .any(|rect| rect.contains(&position))
    })?;
    links
        .iter()
        .find(|(link, _)| link == range)
        .map(|(_, path)| path.clone())
}

/// `TextLayout::bounds` panics before prepaint has run. A row that was spliced
/// this frame can reach paint with a fresh layout, so probe first.
fn layout_missing(layout: &TextLayout) -> bool {
    layout.line_layouts().is_empty()
}

/// The registry entry containing `position`, else the nearest by vertical
/// distance so a drag through a gutter or between blocks clamps sensibly.
fn registry_point(
    registry: &SelectionRegistry<TextGeometry>,
    position: Point<Pixels>,
) -> Option<(usize, usize)> {
    let mut best: Option<(usize, f32)> = None;
    for (index, entry) in registry.entries().iter().enumerate() {
        if entry.geometry.is_missing() {
            continue;
        }
        let bounds = entry.geometry.bounds();
        let distance = if position.y < bounds.top() {
            f32::from(bounds.top() - position.y)
        } else if position.y > bounds.bottom() {
            f32::from(position.y - bounds.bottom())
        } else {
            0.0
        };
        if best.is_none_or(|(_, best)| distance < best) {
            best = Some((index, distance));
        }
        if distance == 0.0 {
            break;
        }
    }
    let (index, _) = best?;
    let offset = match registry.entries()[index]
        .geometry
        .index_for_position(position)
    {
        Ok(offset) | Err(offset) => offset,
    };
    Some((index, offset))
}

/// Install the frame's selection mouse listeners.
///
/// These live once per frame at the transcript root rather than once per
/// painted text element: the registry already holds every element's geometry,
/// so three closures replace three-per-element and a mouse move costs one
/// registry scan instead of one dispatch per visible paragraph.
///
/// Window-level listeners bypass hitbox dispatch, so the registry's geometric
/// bounds check alone would let clicks through occluding surfaces — a double
/// click in the model picker would select the word beneath it. The caller
/// prepaints a Normal hitbox over the region each frame and passes its id as
/// `region`, which gates the handlers via `is_hovered` — false whenever a
/// `BlockMouse` or `BlockMouseExceptScroll` hitbox covers the point.
///
/// `alt_click` is the action an ⌥-press dispatches on mouse-up. The press
/// starts an ordinary drag but arms the pressed line as its release fallback,
/// so a click selects the line and a drag keeps its own spans — the transcript
/// passes ⌘L's `AddToChat`, making the gesture "select this and annotate it"
/// without the action handler seeing anything but an ordinary settled
/// selection. Surfaces that pass `None` keep plain click behavior under ⌥.
/// An ⌥-click landing on an existing annotation highlight never reaches the
/// dispatch: the annotation listeners run first and consume the armed
/// fallback, so the click reopens that comment's editor instead of stacking
/// a new annotation on top.
pub fn install_selection_input(
    region: HitboxId,
    window: &mut Window,
    state: &TranscriptSelection,
    alt_click: Option<Box<dyn Action>>,
) {
    let alt_click = Rc::new(alt_click);
    window.on_mouse_event({
        let state = state.clone();
        let alt_click = alt_click.clone();
        move |event: &MouseDownEvent, phase, window, _cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !region.is_hovered(window)
            {
                return;
            }
            let registry = state.registry.borrow();
            let hit = registry.entries().iter().enumerate().find(|(_, entry)| {
                !entry.geometry.is_missing() && entry.geometry.bounds().contains(&event.position)
            });
            let mut selection = state.selection.borrow_mut();
            match hit {
                Some((index, entry)) => {
                    let offset = match entry.geometry.index_for_position(event.position) {
                        Ok(offset) | Err(offset) => offset,
                    };
                    // ⌥-press starts an ordinary drag and arms the pressed
                    // line as its release fallback: mouse-up selects the line
                    // when nothing was dragged, then runs the caller's action
                    // against whatever the release settled.
                    if event.modifiers.alt && alt_click.is_some() {
                        selection.begin_with_fallback(
                            entry.key.clone(),
                            offset,
                            Span {
                                key: entry.key.clone(),
                                range: line_range(&entry.text, offset),
                                text: entry.text.clone(),
                                block_break: false,
                                copy: entry.copy.clone(),
                            },
                        );
                        drop(selection);
                        drop(registry);
                        window.refresh();
                        return;
                    }
                    // Shift-click grows a settled selection to the clicked
                    // character instead of anchoring a new drag.
                    let extended = event.modifiers.shift
                        && event.click_count == 1
                        && selection.extend_to(&registry, (index, offset));
                    if !extended {
                        match event.click_count {
                            2 => selection.begin_with_span(
                                entry.key.clone(),
                                entry.text.clone(),
                                word_range(&entry.text, offset),
                                entry.copy.clone(),
                            ),
                            count if count >= 3 => selection.begin_with_span(
                                entry.key.clone(),
                                entry.text.clone(),
                                line_range(&entry.text, offset),
                                entry.copy.clone(),
                            ),
                            _ => selection.begin(entry.key.clone(), offset),
                        }
                    }
                    drop(selection);
                    drop(registry);
                    window.refresh();
                }
                None => {
                    // A shift-click in a gutter or between blocks still
                    // extends, to the nearest text.
                    let extended = event.modifiers.shift
                        && event.click_count == 1
                        && registry_point(&registry, event.position)
                            .is_some_and(|head| selection.extend_to(&registry, head));
                    let had_selection = !selection.is_empty();
                    if !extended {
                        selection.clear();
                    }
                    drop(selection);
                    drop(registry);
                    if extended || had_selection {
                        window.refresh();
                    }
                }
            }
        }
    });

    window.on_mouse_event({
        let state = state.clone();
        move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble || !event.dragging() || !region.is_hovered(window) {
                return;
            }
            let registry = state.registry.borrow();
            let anchor = {
                let selection = state.selection.borrow();
                selection
                    .anchor()
                    .cloned()
                    .and_then(|key| selection.drag_anchor(&key).map(|offset| (key, offset)))
                    .and_then(|(key, offset)| registry.position(&key).map(|index| (index, offset)))
            };
            // The anchor scrolling out of the frame keeps the existing spans
            // rather than collapsing the selection.
            let Some((anchor_index, anchor_offset)) = anchor else {
                return;
            };
            let Some(head) = registry_point(&registry, event.position) else {
                return;
            };
            let spans = registry.resolve((anchor_index, anchor_offset), head);
            drop(registry);
            if state.selection.borrow_mut().set_spans(spans) {
                window.refresh();
            }
        }
    });

    window.on_mouse_event({
        let state = state.clone();
        let alt_click = alt_click.clone();
        move |_: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            if state.selection.borrow_mut().release()
                && let Some(action) = alt_click.as_ref()
            {
                window.dispatch_action(action.boxed_clone(), cx);
                window.refresh();
            }
        }
    });
}

// ── Blocks ─────────────────────────────────────────────────────────────────

/// Per-top-level-block ordinal stride: an element's ordinal is
/// `block_index << 16 | position_within_block`. Deriving keys from the
/// block's document index rather than a running document counter means a
/// walk that skips leading blocks ([`markdown_tail`]) hands every rendered
/// block exactly the flatten-cache and veil keys a full walk would, so the
/// two can alternate without thrashing either.
const BLOCK_ORDINAL_STRIDE_BITS: u32 = 16;

fn block_ordinal_base(block_ix: usize) -> usize {
    block_ix << BLOCK_ORDINAL_STRIDE_BITS
}

/// The top-level block an element ordinal was issued under — the inverse of
/// the `block_index << 16 | position_within_block` scheme described above.
pub fn block_index_of_ordinal(ordinal: usize) -> usize {
    ordinal >> BLOCK_ORDINAL_STRIDE_BITS
}

/// Find every non-empty regex match in the text elements produced by the
/// markdown renderer, in paint order. This deliberately walks the same block
/// shapes and advances the same element ordinals as [`render_block`], keeping
/// off-screen search results aligned with the exact glyph ranges that appear
/// once their virtualized transcript row mounts.
pub fn markdown_search_matches(
    source: &str,
    regex: &Regex,
    cap: usize,
) -> (Vec<TextSearchMatch>, bool) {
    let tree = super::parser::parse(source);
    let mut matches = Vec::new();
    for (block_ix, top) in tree.blocks.iter().enumerate() {
        let mut ordinal = block_ordinal_base(block_ix);
        if search_block(&top.block, &mut ordinal, regex, cap, &mut matches) {
            return (matches, true);
        }
    }
    (matches, false)
}

/// Find matches in one non-markdown text element.
pub fn plain_search_matches(
    text: &str,
    ordinal: usize,
    regex: &Regex,
    cap: usize,
) -> (Vec<TextSearchMatch>, bool) {
    let mut matches = Vec::new();
    let limited = search_text(text, ordinal, regex, cap, &mut matches);
    (matches, limited)
}

fn search_block(
    block: &Block,
    ordinal: &mut usize,
    regex: &Regex,
    cap: usize,
    matches: &mut Vec<TextSearchMatch>,
) -> bool {
    match block {
        Block::Paragraph { runs } | Block::Heading { runs, .. } => {
            let text = runs.iter().map(|run| run.text.as_str()).collect::<String>();
            let current = *ordinal;
            *ordinal += 1;
            search_text(&text, current, regex, cap, matches)
        }
        Block::CodeBlock { code, .. } | Block::DisplayMath { latex: code } => {
            let current = *ordinal;
            *ordinal += 1;
            search_text(code, current, regex, cap, matches)
        }
        Block::Image { .. } => {
            // The renderer consumes an ordinal for the image id, but its alt
            // caption is not a selectable/shaped text element and therefore
            // has no glyph geometry for a find highlight.
            *ordinal += 1;
            false
        }
        Block::BlockQuote { children } => children
            .iter()
            .any(|child| search_block(child, ordinal, regex, cap, matches)),
        Block::List { items, .. } => items.iter().any(|item| {
            item.blocks
                .iter()
                .any(|child| search_block(child, ordinal, regex, cap, matches))
        }),
        Block::Table { header, rows, .. } => header
            .iter()
            .chain(rows.iter().flat_map(|row| row.iter()))
            .any(|cell| {
                let text = cell.iter().map(|run| run.text.as_str()).collect::<String>();
                let current = *ordinal;
                *ordinal += 1;
                search_text(&text, current, regex, cap, matches)
            }),
        Block::Rule => false,
    }
}

fn search_text(
    text: &str,
    ordinal: usize,
    regex: &Regex,
    cap: usize,
    matches: &mut Vec<TextSearchMatch>,
) -> bool {
    for found in regex.find_iter(text).filter(|found| !found.is_empty()) {
        if matches.len() >= cap {
            return true;
        }
        matches.push(TextSearchMatch {
            ordinal,
            range: found.range(),
        });
    }
    false
}

/// Render a markdown body. Returns `None` when it has no content.
pub fn markdown<'a>(view: &'a MarkdownView, ctx: &Ctx<'a>) -> Option<AnyElement> {
    markdown_capped(view, ctx, usize::MAX)
}

/// Like [`markdown`], but builds only the trailing `max_blocks` top-level
/// blocks. The live reasoning peek shows a tail-pinned viewport while a
/// thought streams, and building the whole growing document every pulse tick
/// made a long think O(document) per frame; the cap makes it O(window).
pub fn markdown_tail<'a>(
    view: &'a MarkdownView,
    ctx: &Ctx<'a>,
    max_blocks: usize,
) -> Option<AnyElement> {
    markdown_capped(view, ctx, max_blocks.max(1))
}

fn markdown_capped<'a>(
    view: &'a MarkdownView,
    ctx: &Ctx<'a>,
    max_blocks: usize,
) -> Option<AnyElement> {
    let blocks = view.blocks().collect::<Vec<_>>();
    let Some((&last, leading)) = blocks.split_last() else {
        if ctx.animate_streaming && view.streaming.get() {
            let mut veil = view.veil.borrow_mut();
            veil.begin_frame();
            veil.finish_frame();
        }
        return None;
    };

    view.sync_style(ctx.palette, &ctx.metrics, &ctx.families, ctx.guided_reading);
    let ctx = ctx.with_cache(view);
    if ctx.animate_streaming && view.streaming.get() {
        view.veil.borrow_mut().begin_frame();
    }
    let first = blocks.len().saturating_sub(max_blocks);
    let mut children = Vec::with_capacity(blocks.len() - first);
    for (block_ix, block) in leading.iter().enumerate().skip(first) {
        ctx.next_ordinal.set(block_ordinal_base(block_ix));
        children.push(render_block(block, &ctx));
        debug_assert!(
            ctx.next_ordinal.get() - block_ordinal_base(block_ix) < 1 << BLOCK_ORDINAL_STRIDE_BITS,
            "a single block overflowed its ordinal stride"
        );
    }
    // Everything before the final block is settled, so its flattened elements
    // stay cacheable across appends.
    let last_base = block_ordinal_base(blocks.len() - 1);
    ctx.next_ordinal.set(last_base);
    view.volatile_from
        .set(block_ordinal_base(view.parser.display_tail_start()));
    children.push(render_block(last, &ctx));
    if ctx.animate_streaming && view.streaming.get() {
        // Every element visible on the attach pass has synchronously adopted
        // its baseline. Elements introduced by later appends should now fade.
        view.veil.borrow_mut().finish_frame();
    }

    let element = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(ctx.metrics.block_gap))
        .children(children);
    Some(
        if ctx.wrap_context_menu
            && (ctx.math_enabled || ctx.file_link_root.is_some())
            && let Some(menu) = &ctx.context_menu
        {
            context_menu(
                element,
                SharedString::from(format!("context-menu-{}", ctx.row)),
                menu,
                |_| Vec::new(),
            )
        } else {
            element.into_any_element()
        },
    )
}

fn render_block(block: &Block, ctx: &Ctx) -> AnyElement {
    ctx.starts_block.set(true);
    match block {
        Block::Paragraph { runs } => {
            let key = ctx.next_key();
            let flat = ctx.flat(key.index, || {
                flatten(
                    runs,
                    ctx.palette,
                    &ctx.families,
                    FontWeight::NORMAL,
                    ctx.palette.text,
                )
            });
            div()
                .w_full()
                .min_w_0()
                .text_size(px(ctx.metrics.text_size))
                .line_height(px(ctx.metrics.line_height))
                .child(text_element(&flat, key, ctx))
                .into_any_element()
        }
        Block::Heading { level, runs } => {
            let (size, line_height, weight) = heading_metrics(*level, &ctx.metrics);
            let key = ctx.next_key();
            let flat = ctx.flat(key.index, || {
                let mut flat = flatten(runs, ctx.palette, &ctx.families, weight, ctx.palette.text);
                flat.copy = Rc::new(CopySpec {
                    prefix: Rc::from(format!("{} ", "#".repeat(*level as usize))),
                    ..(*flat.copy).clone()
                });
                flat
            });
            div()
                .w_full()
                .min_w_0()
                .when(*level <= 2, |element| element.pt(px(4.0)))
                .text_size(px(size))
                .line_height(px(line_height))
                .child(text_element(&flat, key, ctx))
                .into_any_element()
        }
        Block::Image { url, alt } => render_image(url, alt, ctx),
        Block::DisplayMath { latex } => {
            let key = ctx.next_key();
            let flat = ctx.flat(key.index, || {
                let mut flat = flatten_plain(
                    latex.clone(),
                    ctx.families.code.clone(),
                    FontWeight::NORMAL,
                    ctx.palette.text,
                );
                flat.math = Some(Rc::new(math_text::MathData::new(vec![
                    math_text::MathSpan {
                        range: 0..latex.len(),
                        latex: std::sync::Arc::from(latex.as_str()),
                        display: true,
                    },
                ])));
                flat.copy = Rc::new(CopySpec {
                    prefix: Rc::from("$$"),
                    suffix: Rc::from("$$"),
                    fragments: Vec::new(),
                });
                flat
            });
            div()
                .w_full()
                .min_w_0()
                .text_size(px(ctx.metrics.text_size))
                .line_height(px(ctx.metrics.line_height))
                .child(text_element(&flat, key, ctx))
                .into_any_element()
        }
        Block::CodeBlock { language, code } => render_code_block(language.as_deref(), code, ctx),
        Block::BlockQuote { children } => {
            let margin = ctx.push_copy_margin("> ");
            let rendered = children
                .iter()
                .map(|child| render_block(child, ctx))
                .collect::<Vec<_>>();
            ctx.copy_margin.set(margin);
            div()
                .w_full()
                .min_w_0()
                .flex()
                .gap(px(10.0))
                .child(
                    div()
                        .w(px(2.0))
                        .flex_none()
                        .rounded_full()
                        .bg(ctx.palette.separator),
                )
                .child(
                    div()
                        // flex_auto, not flex_1: a zero flex-basis erases the
                        // content's intrinsic width, collapsing shrink-wrapped
                        // user bubbles to the quote bar.
                        .flex_auto()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(ctx.metrics.block_gap))
                        .children(rendered),
                )
                .into_any_element()
        }
        Block::List {
            ordered_start,
            items,
        } => render_list(*ordered_start, items, ctx),
        Block::Table {
            header,
            rows,
            align,
        } => render_table(header, rows, align, ctx),
        Block::Rule => {
            ctx.copy_lead.take();
            div()
                .w_full()
                .h(hairline())
                .my(px(4.0))
                .bg(ctx.palette.separator)
                .into_any_element()
        }
    }
}

fn render_list(ordered_start: Option<u64>, items: &[ListItem], ctx: &Ctx) -> AnyElement {
    let marker_width = if ordered_start.is_some() { 22.0 } else { 14.0 };
    let rendered = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = match (ordered_start, item.task) {
                (_, Some(checked)) => div()
                    .w(px(marker_width))
                    .flex_none()
                    .flex()
                    .justify_start()
                    .child(checkbox(checked, ctx))
                    .into_any_element(),
                (Some(start), None) => {
                    marker_text(format!("{}.", start + index as u64), marker_width, ctx)
                }
                (None, None) => marker_text("•".to_owned(), marker_width, ctx),
            };
            // Markdown copy: the item's first element gets the marker as a
            // one-shot lead; every element in the item carries the
            // continuation indent as margin.
            let lead = match (ordered_start, item.task) {
                (_, Some(checked)) => {
                    format!("- [{}] ", if checked { "x" } else { " " })
                }
                (Some(start), None) => format!("{}. ", start + index as u64),
                (None, None) => "- ".to_owned(),
            };
            let base = ctx.copy_margin();
            let margin = ctx.push_copy_margin(&" ".repeat(lead.len()));
            ctx.copy_lead.set(Some(Rc::from(format!(
                "{}{lead}",
                base.as_deref().unwrap_or("")
            ))));
            let blocks = item
                .blocks
                .iter()
                .map(|block| render_block(block, ctx))
                .collect::<Vec<_>>();
            ctx.copy_margin.set(margin);
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .child(marker)
                .child(
                    div()
                        // flex_auto, not flex_1: a zero flex-basis erases the
                        // content's intrinsic width, collapsing shrink-wrapped
                        // user bubbles to the marker column.
                        .flex_auto()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(ctx.metrics.block_gap * 0.6))
                        .children(blocks),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();

    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(ctx.metrics.block_gap * 0.5))
        .children(rendered)
        .into_any_element()
}

/// A list marker. Markers are not selectable: they are generated ornament, not
/// content the user typed, so they stay out of the selection registry.
fn marker_text(label: String, width: f32, ctx: &Ctx) -> AnyElement {
    div()
        .w(px(width))
        .flex_none()
        .text_size(px(ctx.metrics.text_size))
        .line_height(px(ctx.metrics.line_height))
        .text_color(ctx.palette.tertiary)
        .child(SharedString::from(label))
        .into_any_element()
}

fn checkbox(checked: bool, ctx: &Ctx) -> AnyElement {
    let box_size = (ctx.metrics.text_size * 0.92).round();
    div()
        .size(px(box_size))
        .my(px(((ctx.metrics.line_height - box_size) / 2.0).max(0.0)))
        .flex_none()
        .rounded(px(3.0))
        .border(hairline())
        .border_color(if checked {
            ctx.palette.accent
        } else {
            ctx.palette.border
        })
        .when(checked, |element| element.bg(ctx.palette.accent))
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |element| {
            element.child(crate::ui::icon(
                "icons/check.svg",
                box_size - 4.0,
                ctx.palette.inset,
            ))
        })
        .into_any_element()
}

/// An inline image. Data URLs decode in place; a cached local file wins over
/// the remote URL when the surface resolved one; a pending fetch paints the
/// surface's placeholder; anything else is handed to GPUI to load. The alt
/// text renders beneath as a caption when there is one, so a failed or slow
/// load still says what it was.
fn render_image(url: &str, alt: &str, ctx: &Ctx) -> AnyElement {
    const MAX_HEIGHT: f32 = 320.0;

    let caption = |element: Div| {
        element.when(!alt.trim().is_empty(), |element| {
            element.child(
                div()
                    .text_size(px((ctx.metrics.text_size - 2.0).max(12.5)))
                    .line_height(px(ctx.metrics.line_height - 4.0))
                    .text_color(ctx.palette.ghost)
                    .child(SharedString::from(alt.to_owned())),
            )
        })
    };

    // The key is consumed before branching so a placeholder → image swap does
    // not shift later blocks' ordinals.
    let key = ctx.next_key();
    // An image registers no selectable text; a list marker must not leak past
    // it into the next element's copy.
    ctx.copy_lead.take();
    let id = SharedString::from(format!("image-{}-{}", key.row, key.index));

    let decoded = decode_data_url(url);
    let resolved = if decoded.is_none() {
        ctx.image_resolver.as_ref().and_then(|resolve| resolve(url))
    } else {
        None
    };

    // A fetch in flight gets the surface's placeholder instead of asking GPUI
    // to race the same URL.
    if decoded.is_none()
        && resolved.is_none()
        && let Some(placeholder) = ctx
            .image_placeholder
            .as_ref()
            .and_then(|placeholder| placeholder(url))
    {
        return caption(div().w_full().min_w_0().flex().flex_col().gap(px(4.0)))
            .child(placeholder)
            .into_any_element();
    }

    let image = match (decoded, resolved) {
        (Some(decoded), _) => img(decoded).id(id),
        (None, Some(path)) => img(path).id(id),
        (None, None) => img(url.to_owned()).id(id),
    };
    caption(div().w_full().min_w_0().flex().flex_col().gap(px(4.0)))
        .child(
            image
                .max_w(relative(1.0))
                .max_h(px(MAX_HEIGHT))
                .rounded(px(8.0))
                .object_fit(gpui::ObjectFit::ScaleDown),
        )
        .into_any_element()
}

const CODE_COPY_FEEDBACK_DURATION: Duration = Duration::from_secs(3);
type CodeCopyFeedback = Rc<RefCell<HashMap<usize, u64>>>;

fn begin_code_copy_feedback(feedback: &CodeCopyFeedback, ordinal: usize) -> u64 {
    let mut feedback = feedback.borrow_mut();
    let generation = feedback
        .get(&ordinal)
        .copied()
        .unwrap_or_default()
        .wrapping_add(1);
    feedback.insert(ordinal, generation);
    generation
}

fn clear_code_copy_feedback(feedback: &CodeCopyFeedback, ordinal: usize, generation: u64) -> bool {
    let mut feedback = feedback.borrow_mut();
    if feedback.get(&ordinal) != Some(&generation) {
        return false;
    }
    feedback.remove(&ordinal);
    true
}

fn show_code_copied(feedback: CodeCopyFeedback, ordinal: usize, cx: &mut gpui::App) {
    let generation = begin_code_copy_feedback(&feedback, ordinal);
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(CODE_COPY_FEEDBACK_DURATION)
            .await;
        if clear_code_copy_feedback(&feedback, ordinal, generation) {
            cx.refresh();
        }
    })
    .detach();
}

/// Decode a `data:` image URL. Shared with the transcript's tool-output images.
pub fn decode_data_url(url: &str) -> Option<std::sync::Arc<gpui::Image>> {
    use base64::Engine as _;

    let (header, encoded) = url.split_once(',')?;
    let mime_type = header.strip_prefix("data:")?.split(';').next()?;
    let format = gpui::ImageFormat::from_mime_type(mime_type)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    (!bytes.is_empty()).then(|| std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
}

fn render_code_block(language: Option<&str>, code: &str, ctx: &Ctx) -> AnyElement {
    let key = ctx.next_key();
    // Tokenizing is the most expensive flatten in the document, so a settled
    // code block is exactly the case the cache exists for.
    let flat = ctx.flat_undecorated(key.index, || {
        let lang = language.and_then(highlight::lang_for_tag);
        let mut code_font = font(ctx.families.code.clone());
        code_font.weight = FontWeight::NORMAL;
        FlatText {
            text: SharedString::from(code.to_owned()),
            runs: code_runs(code, lang, &code_font, ctx.palette),
            links: Vec::new(),
            code_ranges: Vec::new(),
            annotation_refs: Vec::new(),
            commit_refs: Vec::new(),
            file_refs: Vec::new(),
            math: None,
            // The fence wraps only a whole-block grab; a partial selection
            // copies raw code.
            copy: Rc::new(CopySpec {
                prefix: Rc::from(format!("```{}\n", language.unwrap_or(""))),
                suffix: Rc::from("\n```"),
                fragments: Vec::new(),
            }),
        }
    });
    let label = language
        .filter(|language| !language.is_empty())
        .map(|language| language.to_ascii_lowercase());
    // Reuse the cached shaped string. Settled code blocks render every frame,
    // so cloning the whole source here would turn the copy affordance into a
    // permanent O(code length) render cost; allocate only when it is invoked.
    let copy_content = flat.text.clone();
    let keyboard_copy_content = copy_content.clone();
    let copy_feedback = ctx.cache.map(|view| view.copied_code_blocks.clone());
    let copied = copy_feedback
        .as_ref()
        .is_some_and(|feedback| feedback.borrow().contains_key(&key.index));
    let keyboard_copy_feedback = copy_feedback.clone();
    let ordinal = key.index;
    let copy_button = div()
        .id(SharedString::from(format!(
            "copy-code-{}-{}",
            key.row, key.index
        )))
        .tab_index(0)
        .size(px(24.0))
        .flex_none()
        .rounded(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_default()
        .focus_visible(|style| style.bg(ctx.palette.focus))
        .hover(|style| style.bg(ctx.palette.overlay))
        .child(crate::ui::icon(
            if copied {
                "icons/check.svg"
            } else {
                "icons/copy.svg"
            },
            11.0,
            ctx.palette.ghost,
        ))
        .tooltip(Tooltip::text(if copied {
            tr!("common.copied")
        } else {
            tr!("common.copy_code")
        }))
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copy_content.to_string()));
            if let Some(feedback) = copy_feedback.clone() {
                show_code_copied(feedback, ordinal, cx);
            }
        })
        .on_key_down(move |event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.write_to_clipboard(ClipboardItem::new_string(keyboard_copy_content.to_string()));
                if let Some(feedback) = keyboard_copy_feedback.clone() {
                    show_code_copied(feedback, ordinal, cx);
                }
                cx.stop_propagation();
            }
        });

    div()
        .id(SharedString::from(format!(
            "code-block-{}-{}",
            key.row, key.index
        )))
        .tab_group()
        .tab_stop(false)
        .w_full()
        .min_w_0()
        .rounded(px(10.0))
        .border(hairline())
        .border_color(ctx.palette.border_subtle)
        .bg(ctx.palette.inset)
        .overflow_hidden()
        .child(
            div()
                .w_full()
                .h(px(28.0))
                .pl(px(10.0))
                .pr(px(2.0))
                .flex()
                .items_center()
                .border_b(hairline())
                .border_color(ctx.palette.separator)
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(px(12.5))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(ctx.palette.ghost)
                        .when_some(label, |element, label| {
                            element.child(SharedString::from(label))
                        }),
                )
                .child(copy_button),
        )
        .child(
            div()
                .id(SharedString::from(format!(
                    "code-{}-{}",
                    key.row, key.index
                )))
                .w_full()
                .min_w_0()
                .px(px(10.0))
                .py(px(8.0))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(ctx.metrics.code_text_size))
                        .line_height(px(ctx.metrics.code_line_height))
                        .text_color(ctx.palette.secondary)
                        .child(text_element(&flat, key, ctx)),
                ),
        )
        .into_any_element()
}

/// `TextRun`s that tile `code` exactly, colored by the lexer. Every run shares
/// one font, so the shaped width of a line is identical with or without
/// highlighting — the property that makes coloring safe to defer.
fn code_runs(code: &str, lang: Option<Lang>, code_font: &Font, palette: &Palette) -> Vec<TextRun> {
    let plain = palette.secondary;
    let mut runs: Vec<TextRun> = Vec::new();
    let push = |runs: &mut Vec<TextRun>, len: usize, color: Hsla| {
        if len == 0 {
            return;
        }
        match runs.last_mut() {
            Some(last) if last.color == color => last.len += len,
            _ => runs.push(TextRun {
                len,
                font: code_font.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }),
        }
    };

    let tokenized = lang.map(|lang| highlight::tokenize(lang, code));
    let lines = code.split('\n').collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let tokens = tokenized
            .as_ref()
            .and_then(|lines| lines.get(index))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut cursor = 0;
        for token in tokens {
            push(&mut runs, token.range.start.saturating_sub(cursor), plain);
            push(&mut runs, token.range.len(), palette.token(token.class));
            cursor = token.range.end;
        }
        push(&mut runs, line.len().saturating_sub(cursor), plain);
        if index + 1 < lines.len() {
            // The '\n' separator must belong to a run or shaping rejects them.
            push(&mut runs, 1, plain);
        }
    }
    runs
}

fn render_table(
    header: &[Vec<InlineRun>],
    rows: &[Vec<Vec<InlineRun>>],
    align: &[TableAlign],
    ctx: &Ctx,
) -> AnyElement {
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return div().into_any_element();
    }
    // The table's base ordinal doubles as its resize-state key: stable across
    // re-renders, unique within the row.
    let table_id = ctx.next_ordinal.get();
    let resize = ctx.cache.map(|view| view.table_resize.clone());
    let widths = resize
        .as_ref()
        .and_then(|state| state.borrow().widths.get(&table_id).cloned())
        .filter(|stored| stored.len() == columns)
        .unwrap_or_else(|| column_widths(header, rows, columns));

    let mut table = div()
        .w_full()
        .min_w_0()
        .relative()
        .rounded(px(10.0))
        .border(hairline())
        .border_color(ctx.palette.border_subtle)
        .overflow_hidden()
        .flex()
        .flex_col();

    if !header.is_empty() {
        // `overflow_hidden` clips to a rectangle, so the header's background
        // needs its own radii to follow the parent's rounded corners.
        let inner_radius = px(10.0) - hairline();
        table = table.child(
            table_row(
                header,
                &widths,
                align,
                ctx,
                FontWeight::SEMIBOLD,
                !rows.is_empty(),
            )
            .bg(ctx.palette.overlay)
            .rounded_t(inner_radius)
            .when(rows.is_empty(), |row| row.rounded_b(inner_radius)),
        );
    }
    for (index, row) in rows.iter().enumerate() {
        table = table.child(table_row(
            row,
            &widths,
            align,
            ctx,
            FontWeight::NORMAL,
            index + 1 < rows.len(),
        ));
    }
    if let Some(state) = resize
        && columns > 1
    {
        let mut boundary = 0.0;
        for column in 0..columns - 1 {
            boundary += widths[column];
            table = table.child(table_resize_handle(
                table_id, column, boundary, &widths, &state, ctx,
            ));
        }
        table = table.child(table_resize_listeners(table_id, state));
    }
    table.into_any_element()
}

/// The pointer target over one column boundary: a 9px strip centered on the
/// edge, full table height, with a grip line that appears on hover, focus, and
/// while its boundary is being dragged. `boundary` is the cumulative fraction
/// of the table left of the edge.
fn table_resize_handle(
    table_id: usize,
    column: usize,
    boundary: f32,
    widths: &[f32],
    state: &Rc<RefCell<TableResize>>,
    ctx: &Ctx,
) -> impl IntoElement {
    let group = SharedString::from(format!("table-resize-grip-{table_id}-{column}"));
    let active = state
        .borrow()
        .drag
        .as_ref()
        .is_some_and(|drag| drag.table == table_id && drag.column == column);
    let pressed = widths.to_vec();
    let drag_state = state.clone();
    let key_state = state.clone();
    let key_widths = widths.to_vec();
    div()
        .id(SharedString::from(format!(
            "table-resize-{}-{table_id}-{column}",
            ctx.row
        )))
        .tab_index(0)
        .tab_stop(true)
        .group(group.clone())
        .absolute()
        .top_0()
        .bottom_0()
        .left(relative(boundary))
        .w(px(RESIZE_HANDLE_WIDTH))
        .ml(-px(RESIZE_HANDLE_WIDTH / 2.0))
        .cursor_col_resize()
        .flex()
        .justify_center()
        .focus_visible(|element| element.bg(ctx.palette.focus))
        .child(
            div()
                .w(px(1.5))
                .h_full()
                .rounded_full()
                .when(active, |element| element.bg(ctx.palette.accent))
                .group_hover(group, |element| element.bg(ctx.palette.accent)),
        )
        .on_mouse_down(MouseButton::Left, {
            move |event: &MouseDownEvent, window, cx| {
                drag_state.borrow_mut().drag = Some(TableDrag {
                    table: table_id,
                    column,
                    start_x: event.position.x,
                    original: pressed.clone(),
                });
                // Armed before the transcript's window-level selection
                // listeners see the press, so it must not begin a text
                // selection.
                cx.stop_propagation();
                window.refresh();
            }
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.modifiers.modified() {
                return;
            }
            let delta = match event.keystroke.key.as_str() {
                "left" => -RESIZE_KEY_STEP,
                "right" => RESIZE_KEY_STEP,
                _ => return,
            };
            let mut state = key_state.borrow_mut();
            let stored = state
                .widths
                .entry(table_id)
                .or_insert_with(|| key_widths.clone());
            if stored.len() != key_widths.len() {
                *stored = key_widths.clone();
            }
            if let Some((left, right)) = resized_pair(stored, column, delta) {
                stored[column] = left;
                stored[column + 1] = right;
            }
            drop(state);
            cx.stop_propagation();
            window.refresh();
        })
}

/// An invisible canvas covering the table that installs the drag's window-level
/// move/up listeners each paint — the same pattern as [`crate::ui::slider`].
/// Element-level move handlers would stop receiving events once the pointer
/// leaves the handle; these run for as long as a drag is armed.
fn table_resize_listeners(table_id: usize, state: Rc<RefCell<TableResize>>) -> impl IntoElement {
    canvas(|_, _, _| (), {
        move |bounds, _, window: &mut Window, _| {
            window.on_mouse_event({
                let state = state.clone();
                move |event: &MouseMoveEvent, phase, window, _| {
                    if phase != DispatchPhase::Bubble || !event.dragging() {
                        return;
                    }
                    let mut state = state.borrow_mut();
                    let Some(drag) = &state.drag else {
                        return;
                    };
                    if drag.table != table_id {
                        return;
                    }
                    let column = drag.column;
                    let original = drag.original.clone();
                    let delta = f32::from(event.position.x - drag.start_x)
                        / f32::from(bounds.size.width).max(1.0);
                    let stored = state
                        .widths
                        .entry(table_id)
                        .or_insert_with(|| original.clone());
                    if stored.len() != original.len() {
                        *stored = original.clone();
                    }
                    if resized_pair(&original, column, delta).is_some_and(|(left, right)| {
                        stored[column] = left;
                        stored[column + 1] = right;
                        true
                    }) {
                        drop(state);
                        window.refresh();
                    }
                }
            });
            window.on_mouse_event({
                move |_: &MouseUpEvent, phase, window, _| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    if state.borrow_mut().drag.take().is_some() {
                        window.refresh();
                    }
                }
            });
        }
    })
    .absolute()
    .inset_0()
}

fn table_row(
    cells: &[Vec<InlineRun>],
    widths: &[f32],
    align: &[TableAlign],
    ctx: &Ctx,
    weight: FontWeight,
    divider: bool,
) -> gpui::Div {
    let mut row = div()
        .w_full()
        .min_w_0()
        .flex()
        .items_start()
        .when(divider, |element| {
            element
                .border_b(hairline())
                .border_color(ctx.palette.separator)
        });
    for (index, cell) in cells.iter().enumerate() {
        let key = ctx.next_key();
        let flat = ctx.flat(key.index, || {
            flatten(cell, ctx.palette, &ctx.families, weight, ctx.palette.text)
        });
        let alignment = align.get(index).copied().unwrap_or_default();
        row = row.child(
            div()
                .w(relative(widths.get(index).copied().unwrap_or(0.0)))
                .min_w_0()
                .px(px(9.0))
                .py(px(6.0))
                .text_size(px((ctx.metrics.text_size - 0.5).max(12.5)))
                .line_height(px(ctx.metrics.line_height - 2.0))
                .map(|element| match alignment {
                    TableAlign::Left => element,
                    TableAlign::Center => element.items_center().text_center(),
                    TableAlign::Right => element.items_end().text_right(),
                })
                .child(text_element(&flat, key, ctx)),
        );
    }
    row
}

/// Content-proportional column widths as fractions of the table, floored so a
/// narrow column stays readable.
fn column_widths(
    header: &[Vec<InlineRun>],
    rows: &[Vec<Vec<InlineRun>>],
    columns: usize,
) -> Vec<f32> {
    const MIN_FRACTION_SCALE: f32 = 0.55;

    let mut content = vec![0.0f32; columns];
    let mut note = |index: usize, cell: &Vec<InlineRun>| {
        if let Some(slot) = content.get_mut(index) {
            let length = cell
                .iter()
                .map(|run| run.text.chars().count())
                .sum::<usize>();
            *slot = slot.max(length as f32);
        }
    };
    for (index, cell) in header.iter().enumerate() {
        note(index, cell);
    }
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            note(index, cell);
        }
    }

    let even = 1.0 / columns as f32;
    let floor = even * MIN_FRACTION_SCALE;
    if content.iter().sum::<f32>() <= 0.0 {
        return vec![even; columns];
    }

    // Water-fill rather than clamp-then-renormalise: renormalising after a
    // clamp erodes the very floor it just applied. Each pass pins whatever fell
    // under the floor at exactly the floor and shares the remaining budget
    // among the rest, so the fractions still sum to one and every column clears
    // the floor. Terminates in at most `columns` passes.
    let mut widths = vec![even; columns];
    let mut pinned = vec![false; columns];
    loop {
        let free = (0..columns).filter(|index| !pinned[*index]).count();
        if free == 0 {
            break;
        }
        let budget = 1.0 - floor * (columns - free) as f32;
        let free_content = (0..columns)
            .filter(|index| !pinned[*index])
            .map(|index| content[index])
            .sum::<f32>();
        let mut pinned_any = false;
        for index in 0..columns {
            if pinned[index] {
                continue;
            }
            widths[index] = if free_content > 0.0 {
                budget * content[index] / free_content
            } else {
                budget / free as f32
            };
            if widths[index] < floor {
                widths[index] = floor;
                pinned[index] = true;
                pinned_any = true;
            }
        }
        if !pinned_any {
            break;
        }
    }
    widths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::parser;
    use gpui::TestAppContext;

    fn palette() -> Palette {
        Palette::from_theme(&Theme::dark())
    }

    #[test]
    fn math_search_uses_the_same_source_ranges_and_ordinals_as_selection() {
        let source = "before $x^2$ after\n\n$$x^2$$\n\n| Value |\n| --- |\n| $x^2$ |";
        let (matches, limited) = markdown_search_matches(source, &Regex::new(r"x\^2").unwrap(), 20);
        assert!(!limited);
        assert_eq!(
            matches
                .iter()
                .map(|found| (found.ordinal, found.range.clone()))
                .collect::<Vec<_>>(),
            vec![(0, 7..10), (1 << 16, 0..3), ((2 << 16) + 1, 0..3)]
        );
    }

    #[test]
    fn block_source_range_maps_display_indices_back_to_source() {
        let source = "# Title\n\nsay hi\n";
        let mut view = MarkdownView::new();
        view.set_text(source, false);
        assert_eq!(
            view.block_source_range(0).map(|range| source[range].trim()),
            Some("# Title")
        );
        assert_eq!(
            view.block_source_range(1).map(|range| source[range].trim()),
            Some("say hi")
        );
        assert_eq!(view.block_source_range(2), None);
        // The same index an element ordinal encodes.
        assert_eq!(block_index_of_ordinal((1 << 16) + 3), 1);
    }

    fn runs_of(source: &str) -> Vec<InlineRun> {
        match &parser::parse(source).blocks[0].block {
            Block::Paragraph { runs } => runs.clone(),
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    /// `StyledText::with_runs` panics unless the runs tile the text exactly.
    fn assert_runs_tile(flat: &FlatText) {
        let total = flat.runs.iter().map(|run| run.len).sum::<usize>();
        assert_eq!(
            total,
            flat.text.len(),
            "runs must tile the text exactly: {:?}",
            flat.text
        );
    }

    #[test]
    fn flattened_runs_tile_the_text_and_carry_styles() {
        let flat = flatten(
            &runs_of("plain **bold** `code` [link](https://example.com) ~~gone~~"),
            &palette(),
            &Fonts::default(),
            FontWeight::NORMAL,
            palette().text,
        );
        assert_runs_tile(&flat);
        assert_eq!(flat.text.as_ref(), "plain bold code link gone");
        assert_eq!(flat.links.len(), 1);
        assert_eq!(&flat.text[flat.links[0].0.clone()], "link");
        assert_eq!(flat.links[0].1, "https://example.com");
        assert_eq!(flat.code_ranges.len(), 1);
        assert_eq!(&flat.text[flat.code_ranges[0].clone()], "code");
        assert!(
            flat.runs
                .iter()
                .any(|run| run.strikethrough.is_some() && run.len == 4)
        );
        assert!(flat.runs.iter().any(|run| run.underline.is_some()));
    }

    #[test]
    fn flatten_records_copy_fragments_for_styled_runs() {
        let flat = flatten(
            &runs_of("plain **bold** `code` [link](https://example.com) ~~gone~~"),
            &palette(),
            &Fonts::default(),
            FontWeight::NORMAL,
            palette().text,
        );
        let fragments = flat
            .copy
            .fragments
            .iter()
            .map(|(range, markdown)| (flat.text[range.clone()].to_owned(), markdown.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(
            fragments,
            vec![
                ("bold".to_owned(), "**bold**".to_owned()),
                ("code".to_owned(), "`code`".to_owned()),
                ("link".to_owned(), "[link](https://example.com)".to_owned()),
                ("gone".to_owned(), "~~gone~~".to_owned()),
            ]
        );
    }

    /// `(slice, weight)` per run, in order — how a fixated flat reads.
    fn run_pieces(flat: &FlatText) -> Vec<(String, FontWeight)> {
        let mut offset = 0;
        flat.runs
            .iter()
            .map(|run| {
                let slice = flat.text[offset..offset + run.len].to_owned();
                offset += run.len;
                (slice, run.font.weight)
            })
            .collect()
    }

    #[test]
    fn fixation_splits_words_and_still_tiles_the_text() {
        let mut flat = flatten(
            &runs_of("hello, world"),
            &palette(),
            &Fonts::default(),
            FontWeight::NORMAL,
            palette().text,
        );
        apply_fixation(&mut flat, &Fonts::default().code, &GuidedReading::default());
        assert_runs_tile(&flat);
        // "hello" anchors: five-letter words fixate their first two
        // graphemes. The 10-letter saccade jump then lands past "world",
        // which is emitted plain.
        assert_eq!(
            run_pieces(&flat),
            vec![
                ("he".to_owned(), FontWeight::SEMIBOLD),
                ("llo, world".to_owned(), FontWeight::NORMAL),
            ]
        );
    }

    #[test]
    fn fixation_skips_code_bold_and_math_but_keeps_link_styles() {
        let mut flat = flatten(
            &runs_of("see **bolden** `codex` [linked](https://example.com)"),
            &palette(),
            &Fonts::default(),
            FontWeight::NORMAL,
            palette().text,
        );
        apply_fixation(&mut flat, &Fonts::default().code, &GuidedReading::default());
        assert_runs_tile(&flat);
        let pieces = run_pieces(&flat);
        // Markdown bold is already semibold — never split.
        assert_eq!(
            pieces
                .iter()
                .filter(|(slice, _)| slice == "bolden")
                .collect::<Vec<_>>(),
            vec![&("bolden".to_owned(), FontWeight::SEMIBOLD)]
        );
        // Inline code keeps one normal-weight run in the code face.
        assert_eq!(
            pieces
                .iter()
                .filter(|(slice, _)| slice == "codex")
                .collect::<Vec<_>>(),
            vec![&("codex".to_owned(), FontWeight::NORMAL)]
        );
        // Linked words still fixate — and keep their underline.
        let linked = pieces
            .iter()
            .filter(|(slice, _)| slice == "lin" || slice == "ked")
            .map(|(slice, _)| slice.as_str())
            .collect::<Vec<_>>();
        assert_eq!(linked, vec!["lin", "ked"]);
        assert!(
            flat.runs
                .iter()
                .any(|run| run.underline.is_some() && run.font.weight == FontWeight::SEMIBOLD)
        );
    }

    #[test]
    fn fixation_leaves_joining_and_non_latin_scripts_untouched() {
        let mut flat = flatten_plain(
            "مرحبا 世界 test",
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        apply_fixation(&mut flat, &Fonts::default().code, &GuidedReading::default());
        assert_runs_tile(&flat);
        assert_eq!(
            run_pieces(&flat),
            vec![
                ("مرحبا 世界 ".to_owned(), FontWeight::NORMAL),
                ("te".to_owned(), FontWeight::SEMIBOLD),
                ("st".to_owned(), FontWeight::NORMAL),
            ]
        );
    }

    #[test]
    fn fixation_never_cuts_inside_a_grapheme_cluster() {
        // "cafe\u{301}" — decomposed é is one cluster of two chars.
        let mut flat = flatten_plain(
            "cafe\u{301} nave",
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        apply_fixation(&mut flat, &Fonts::default().code, &GuidedReading::default());
        assert_runs_tile(&flat);
        // "café" is four clusters → "ca" fixated; the accent stays with its
        // base in the trailing segment, which coalesces with the jumped-over
        // "nave".
        assert_eq!(
            run_pieces(&flat),
            vec![
                ("ca".to_owned(), FontWeight::SEMIBOLD),
                ("fe\u{301} nave".to_owned(), FontWeight::NORMAL),
            ]
        );
    }

    #[test]
    fn fixation_skips_monospace_runs() {
        let mut flat = flatten_plain(
            "verbatim output",
            crate::fonts::DEFAULT_CODE_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        apply_fixation(&mut flat, &Fonts::default().code, &GuidedReading::default());
        assert_eq!(flat.runs.len(), 1);
        assert_eq!(flat.runs[0].font.weight, FontWeight::NORMAL);
    }

    #[test]
    fn saccade_counts_letters_between_fixations() {
        // Saccade is a letter distance, not a word count: the word a
        // 10-letter jump lands on is anchored.
        let mut flat = flatten_plain(
            "one two three four",
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        apply_fixation(
            &mut flat,
            &Fonts::default().code,
            &GuidedReading {
                saccade: 10,
                ..GuidedReading::default()
            },
        );
        assert_runs_tile(&flat);
        assert_eq!(
            run_pieces(&flat),
            vec![
                ("on".to_owned(), FontWeight::SEMIBOLD),
                ("e two ".to_owned(), FontWeight::NORMAL),
                ("th".to_owned(), FontWeight::SEMIBOLD),
                ("ree four".to_owned(), FontWeight::NORMAL),
            ]
        );

        // A word longer than the whole jump still anchors — its length is
        // the distance's floor.
        let mut flat = flatten_plain(
            "a supercalifragilistic word",
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        apply_fixation(
            &mut flat,
            &Fonts::default().code,
            &GuidedReading {
                saccade: 10,
                ..GuidedReading::default()
            },
        );
        assert_runs_tile(&flat);
        assert_eq!(
            run_pieces(&flat),
            vec![
                ("a".to_owned(), FontWeight::SEMIBOLD),
                (" ".to_owned(), FontWeight::NORMAL),
                ("supercal".to_owned(), FontWeight::SEMIBOLD),
                ("ifragilistic word".to_owned(), FontWeight::NORMAL),
            ]
        );
    }

    #[test]
    fn fixation_level_scales_the_prefix() {
        let mut flat = flatten_plain(
            "characters",
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        // Level 5 → 60% of ten graphemes.
        apply_fixation(
            &mut flat,
            &Fonts::default().code,
            &GuidedReading {
                fixation: 5,
                ..GuidedReading::default()
            },
        );
        assert_eq!(
            run_pieces(&flat),
            vec![
                ("charac".to_owned(), FontWeight::SEMIBOLD),
                ("ters".to_owned(), FontWeight::NORMAL),
            ]
        );
    }

    #[test]
    fn opacity_fades_unemphasized_text_only() {
        let base = palette().text;
        let mut flat = flatten_plain(
            "tail end",
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            base,
        );
        apply_fixation(
            &mut flat,
            &Fonts::default().code,
            &GuidedReading {
                opacity: 50,
                ..GuidedReading::default()
            },
        );
        assert_runs_tile(&flat);
        for run in &flat.runs {
            if run.font.weight == FontWeight::SEMIBOLD {
                assert_eq!(run.color, base, "fixated text keeps full opacity");
            } else {
                assert!(
                    (run.color.a - base.a * 0.5).abs() < 0.01,
                    "unemphasized text fades to half opacity"
                );
            }
        }
    }

    #[test]
    fn a_streaming_link_is_styled_but_not_clickable() {
        let flat = flatten(
            &runs_of(&format!("see [docs]({PENDING_LINK_URL})")),
            &palette(),
            &Fonts::default(),
            FontWeight::NORMAL,
            palette().text,
        );
        assert_runs_tile(&flat);
        assert!(
            flat.links.is_empty(),
            "the pending sentinel must not register a clickable range"
        );
        assert!(
            flat.runs.iter().any(|run| run.underline.is_some()),
            "but it should still look like a link"
        );
    }

    #[test]
    fn adjacent_code_and_link_runs_merge_into_one_range() {
        let flat = flatten(
            &runs_of("[**a** `b`](https://x) tail"),
            &palette(),
            &Fonts::default(),
            FontWeight::NORMAL,
            palette().text,
        );
        assert_runs_tile(&flat);
        assert_eq!(flat.links.len(), 1, "one link, not one per styled run");
        assert_eq!(&flat.text[flat.links[0].0.clone()], "a b");
    }

    #[test]
    fn plain_flatten_tiles_and_handles_empty_text() {
        let flat = flatten_plain(
            "hello",
            crate::fonts::DEFAULT_CODE_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        assert_runs_tile(&flat);
        assert_eq!(flat.runs.len(), 1);

        let empty = flatten_plain(
            "",
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        assert_runs_tile(&empty);
        assert!(empty.runs.is_empty());
    }

    #[test]
    fn code_runs_tile_the_block_including_newlines() {
        let code = "fn main() {\n    let x = 1; // c\n}";
        let mut code_font = font(crate::fonts::DEFAULT_CODE_FAMILY);
        code_font.weight = FontWeight::NORMAL;
        let runs = code_runs(code, Some(Lang::Rust), &code_font, &palette());
        assert_eq!(
            runs.iter().map(|run| run.len).sum::<usize>(),
            code.len(),
            "code runs must tile the whole block, newlines included"
        );
        assert!(runs.len() > 1, "highlighting should produce several runs");

        // Without a language the block is one plain run of the same length.
        let plain = code_runs(code, None, &code_font, &palette());
        assert_eq!(plain.iter().map(|run| run.len).sum::<usize>(), code.len());
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn code_block_rendering_wraps_and_exposes_a_keyboard_copy_control() {
        let source = include_str!("render.rs");
        let start = source
            .find("\nfn render_code_block(")
            .expect("code block renderer");
        let body = &source[start + 1..];
        let end = body
            .find("\nfn code_runs(")
            .expect("code block renderer end");
        let body = &body[..end];

        assert!(body.contains(".whitespace_normal()"));
        assert!(!body.contains(".overflow_x_scroll()"));
        assert!(!body.contains(".whitespace_nowrap()"));
        assert!(body.contains("\"icons/copy.svg\""));
        assert!(body.contains("\"icons/check.svg\""));
        assert!(body.contains("ClipboardItem::new_string"));
        assert!(body.contains("show_code_copied"));
        assert!(body.contains(".tab_index(0)"));
        assert!(body.contains(".on_key_down"));
    }

    #[test]
    fn copied_code_feedback_resets_after_three_seconds_and_ignores_stale_timers() {
        assert_eq!(CODE_COPY_FEEDBACK_DURATION, Duration::from_secs(3));

        let feedback = Rc::new(RefCell::new(HashMap::new()));
        let first = begin_code_copy_feedback(&feedback, 4);
        let second = begin_code_copy_feedback(&feedback, 4);

        assert!(!clear_code_copy_feedback(&feedback, 4, first));
        assert!(feedback.borrow().contains_key(&4));
        assert!(clear_code_copy_feedback(&feedback, 4, second));
        assert!(!feedback.borrow().contains_key(&4));
    }

    /// A soft-wrap boundary has two caret affinities. GPUI's generic
    /// `position_for_index` resolves it to the preceding row, so selection
    /// geometry must use the wrapped rows themselves or it can skip the first
    /// glyph on every continuation row.
    #[gpui::test]
    fn wrapped_selection_starts_at_each_continuation_row_origin(cx: &mut TestAppContext) {
        struct TestWindow;

        impl gpui::Render for TestWindow {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                div()
            }
        }

        let (_, cx) = cx.add_window_view(|_, _| TestWindow);
        let text: SharedString =
            "one two three four five six seven eight nine ten eleven twelve".into();
        let styled = StyledText::new(text.clone());
        let layout = styled.layout().clone();

        cx.draw(Point::default(), size(px(96.0), px(400.0)), move |_, _| {
            div()
                .w(px(96.0))
                .text_size(px(14.0))
                .line_height(px(20.0))
                .child(styled)
        });

        let rects = range_rects(&layout, &(0..text.len()), 0.0, 0.0);
        assert!(rects.len() >= 3, "fixture must wrap across several rows");
        let left = layout.bounds().left();
        assert!(
            rects.iter().all(|rect| rect.left() == left),
            "a full selection must include each wrapped row's first glyph: {rects:?}"
        );
    }

    /// Highlighting must never change the shaped length of a code block, or a
    /// deferred colorize would reflow the row.
    #[test]
    fn highlighting_never_changes_run_lengths() {
        let code = "const a = `t ${b}`;\n// note\nlet n = 0x1F;";
        let mut code_font = font(crate::fonts::DEFAULT_CODE_FAMILY);
        code_font.weight = FontWeight::NORMAL;
        let highlighted = code_runs(code, Some(Lang::Script), &code_font, &palette());
        let plain = code_runs(code, None, &code_font, &palette());
        assert_eq!(
            highlighted.iter().map(|run| run.len).sum::<usize>(),
            plain.iter().map(|run| run.len).sum::<usize>()
        );
        assert!(highlighted.iter().all(|run| &run.font == &code_font));
    }

    #[test]
    fn markdown_view_reuses_settled_elements_across_appends() {
        fn stub(label: &str) -> FlatText {
            flatten_plain(
                label.to_owned(),
                crate::fonts::DEFAULT_UI_FAMILY,
                FontWeight::NORMAL,
                palette().text,
            )
        }

        let mut view = MarkdownView::new();
        view.set_text("First block.\n\nSecond bl", true);
        // Stand in for a render pass: two blocks, the second still streaming.
        let settled = view.flat(0, || stub("a"));
        let streaming = view.flat(1, || stub("b"));
        view.volatile_from.set(1);

        view.set_text("First block.\n\nSecond block.", true);

        // The settled block is reused by identity — no re-flatten, no alloc.
        let after = view.flat(0, || panic!("a settled block must not be rebuilt"));
        assert!(Rc::ptr_eq(&settled, &after));

        // The block that changed is rebuilt.
        let rebuilt = view.flat(1, || stub("c"));
        assert!(!Rc::ptr_eq(&streaming, &rebuilt));
        assert_eq!(rebuilt.text.as_ref(), "c");
    }

    /// Colors live inside `TextRun`s, so a theme switch has to drop the cache
    /// or the transcript keeps painting the previous palette.
    #[test]
    fn a_style_change_drops_cached_flats() {
        let view = MarkdownView::new();
        let dark = Palette::from_theme(&Theme::dark());
        let light = Palette::from_theme(&Theme::light());

        view.sync_style(&dark, &Metrics::BODY, &Fonts::default(), None);
        let cached = view.flat(0, || {
            flatten_plain(
                "a",
                crate::fonts::DEFAULT_UI_FAMILY,
                FontWeight::NORMAL,
                dark.text,
            )
        });

        view.sync_style(&dark, &Metrics::BODY, &Fonts::default(), None);
        assert!(
            Rc::ptr_eq(
                &cached,
                &view.flat(0, || panic!("an unchanged style must reuse the cache"))
            ),
            "re-syncing the same style must not invalidate"
        );

        view.sync_style(&light, &Metrics::BODY, &Fonts::default(), None);
        let relit = view.flat(0, || {
            flatten_plain(
                "a",
                crate::fonts::DEFAULT_UI_FAMILY,
                FontWeight::NORMAL,
                light.text,
            )
        });
        assert!(!Rc::ptr_eq(&cached, &relit));
        assert_eq!(relit.runs[0].color, light.text);
    }

    /// Reasoning text goes through the same view as a response, so a plain
    /// prose block must actually produce renderable blocks.
    #[test]
    fn a_view_over_plain_prose_yields_blocks() {
        let mut view = MarkdownView::new();
        view.set_text("Let me check the parser first.", false);
        assert_eq!(view.blocks().count(), 1);

        // Streaming (mended) content too.
        let mut streaming = MarkdownView::new();
        streaming.set_text("Let me check the **parser", true);
        assert_eq!(streaming.blocks().count(), 1);

        // Empty content has nothing to render, which is the only case where
        // the renderer legitimately produces no element.
        let mut empty = MarkdownView::new();
        empty.set_text("", false);
        assert_eq!(empty.blocks().count(), 0);
    }

    #[test]
    fn markdown_view_blocks_swap_in_the_mended_tail() {
        let mut view = MarkdownView::new();
        view.set_text("Settled.\n\nNow **bold", true);
        let bold = view.blocks().any(|block| match block {
            Block::Paragraph { runs } => runs.iter().any(|run| run.style.bold),
            _ => false,
        });
        assert!(bold, "streaming emphasis should be styled");

        // Settled rendering keeps the markers literal.
        view.set_text("Settled.\n\nNow **bold", false);
        let bold = view.blocks().any(|block| match block {
            Block::Paragraph { runs } => runs.iter().any(|run| run.style.bold),
            _ => false,
        });
        assert!(!bold, "a settled response must not invent a closer");
        assert_eq!(view.blocks().count(), 2);
    }

    #[test]
    fn column_widths_are_content_proportional_and_floored() {
        let header = vec![runs_of("id"), runs_of("a much longer description column")];
        let widths = column_widths(&header, &[], 2);
        assert!(widths[1] > widths[0], "wider content gets a wider column");
        // The floor survives the fill, and the fractions still sum to one.
        let floor = 0.55 / 2.0;
        assert!(
            widths.iter().all(|width| *width >= floor - 1e-6),
            "every column keeps its floor: {widths:?}"
        );
        assert!((widths.iter().sum::<f32>() - 1.0).abs() < 1e-4);

        // A column that is merely narrow, not starved, stays proportional.
        let balanced = column_widths(&[runs_of("aaaa"), runs_of("bbbbbb")], &[], 2);
        assert!((balanced[0] - 0.4).abs() < 1e-3, "{balanced:?}");

        // An empty table falls back to even columns.
        let even = column_widths(&[], &[], 3);
        assert!(even.iter().all(|width| (width - 1.0 / 3.0).abs() < 1e-6));
    }

    #[test]
    fn resized_pair_shifts_the_boundary_and_keeps_the_sum() {
        let widths = [0.5, 0.3, 0.2];
        let (left, right) = resized_pair(&widths, 0, 0.1).unwrap();
        assert!((left - 0.6).abs() < 1e-6);
        assert!((right - 0.2).abs() < 1e-6);
        assert!(((left + right) - 0.8).abs() < 1e-6);

        // The floor clamps both directions instead of passing the pair sum.
        let (left, right) = resized_pair(&widths, 1, 1.0).unwrap();
        assert!((right - MIN_COLUMN_FRACTION).abs() < 1e-6);
        assert!(((left + right) - 0.5).abs() < 1e-6);
        let (left, _) = resized_pair(&widths, 1, -1.0).unwrap();
        assert!((left - MIN_COLUMN_FRACTION).abs() < 1e-6);

        // Out-of-range boundaries and starved pairs are refused.
        assert!(resized_pair(&widths, 2, 0.1).is_none());
        assert!(resized_pair(&[0.05, 0.05], 0, 0.1).is_none());
    }

    fn refs(text: &str, limit: usize) -> Vec<(Range<usize>, usize)> {
        annotation_references(
            &flatten_plain(
                text.to_owned(),
                crate::fonts::DEFAULT_UI_FAMILY,
                FontWeight::NORMAL,
                palette().text,
            ),
            limit,
        )
    }

    #[test]
    fn annotation_references_match_word_bounded_labels() {
        assert_eq!(refs("see Annotation 1 for details", 2), vec![(4..16, 1)]);
        // The word is case-insensitive and several labels can appear.
        assert_eq!(
            refs("annotation 2 and Annotation 1", 2),
            vec![(0..12, 2), (17..29, 1)]
        );
        // Word boundaries on both sides: glued and plural forms are not
        // citations, and neither is a digit-glued label.
        assert!(refs("preAnnotation 1", 3).is_empty());
        assert!(refs("Annotations 1", 3).is_empty());
        assert!(refs("Annotation 1x", 3).is_empty());
        assert_eq!(refs("(Annotation 3).", 3), vec![(1..13, 3)]);
    }

    #[test]
    fn annotation_references_only_mark_resolvable_labels() {
        // Labels past the set's size — and "Annotation 0", which is no label —
        // stay plain text.
        assert_eq!(refs("Annotation 3", 2), Vec::new());
        assert_eq!(refs("Annotation 0", 2), Vec::new());
    }

    #[test]
    fn annotation_references_skip_code_and_links() {
        let text = "run `Annotation 1` then Annotation 2";
        let mut flat = flatten_plain(
            text.to_owned(),
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        flat.code_ranges.push(4..17);
        assert_eq!(annotation_references(&flat, 2), vec![(24..36, 2)]);
        flat.code_ranges.clear();
        flat.links.push((4..17, "https://example.com".to_owned()));
        assert_eq!(annotation_references(&flat, 2), vec![(24..36, 2)]);
    }

    fn commit_refs(text: &str) -> Vec<(Range<usize>, String)> {
        commit_references(&flatten_plain(
            text.to_owned(),
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        ))
    }

    #[test]
    fn commit_references_match_word_bounded_shas() {
        assert_eq!(
            commit_refs("fixed in 0123456789abcdef0123456789abcdef01234567"),
            vec![(9..49, "0123456789abcdef0123456789abcdef01234567".to_owned())]
        );
        assert_eq!(
            commit_refs("see ABCDEF1"),
            vec![(4..11, "abcdef1".to_owned())]
        );
        assert!(commit_refs("see abcdef").is_empty());
        assert!(commit_refs("x0123456 g123456").is_empty());
    }

    #[test]
    fn commit_references_skip_hyphenated_hex_tokens() {
        // Every UUID segment is word-bounded hex; none is a commit.
        assert!(commit_refs("id 3f8a2b1c-9d4e-4f5a-8b6c-7d8e9f0a1b2c done").is_empty());
        assert!(commit_refs("deadbeef-1234").is_empty());
        // A hyphenated token with non-hex letters is prose around a SHA.
        assert_eq!(
            commit_refs("the abcdef1-based fix"),
            vec![(4..11, "abcdef1".to_owned())]
        );
        // A bare hex run beside a hyphenated token still matches.
        assert_eq!(
            commit_refs("abc1234 then 3f8a2b1c-9d4e-4f5a-8b6c-7d8e9f0a1b2c"),
            vec![(0..7, "abc1234".to_owned())]
        );
    }

    #[test]
    fn commit_references_keep_inline_code_but_skip_links() {
        let mut flat = flatten_plain(
            "run `0123456` then abcdef1".to_owned(),
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        flat.code_ranges.push(4..13);
        assert_eq!(
            commit_references(&flat),
            vec![
                (5..12, "0123456".to_owned()),
                (19..26, "abcdef1".to_owned())
            ]
        );
        flat.code_ranges.clear();
        flat.links.push((4..13, "https://example.com".to_owned()));
        assert_eq!(
            commit_references(&flat),
            vec![(19..26, "abcdef1".to_owned())]
        );
    }

    fn file_refs(text: &str) -> Vec<(Range<usize>, String)> {
        file_references(
            &flatten_plain(
                text.to_owned(),
                crate::fonts::DEFAULT_UI_FAMILY,
                FontWeight::NORMAL,
                palette().text,
            ),
            Path::new("/repo"),
        )
    }

    #[test]
    fn file_references_match_mentions_and_resolve_against_the_workspace() {
        assert_eq!(
            file_refs("look at @src/app.rs please"),
            vec![(8..19, "/repo/src/app.rs".to_owned())]
        );
        assert_eq!(
            file_refs("@/abs/path.rs and @dir/"),
            vec![
                (0..13, "/abs/path.rs".to_owned()),
                (18..23, "/repo/dir/".to_owned()),
            ]
        );
        assert!(file_refs("mail user@host.com").is_empty());
        assert!(file_refs("no mention").is_empty());
    }

    #[test]
    fn file_references_trim_prose_punctuation() {
        assert_eq!(
            file_refs("see (@src/a.rs), then @b.md."),
            vec![
                (5..14, "/repo/src/a.rs".to_owned()),
                (22..27, "/repo/b.md".to_owned())
            ]
        );
    }

    #[test]
    fn file_references_skip_code_and_links() {
        let mut flat = flatten_plain(
            "run `@gen.sh` then @real.sh".to_owned(),
            crate::fonts::DEFAULT_UI_FAMILY,
            FontWeight::NORMAL,
            palette().text,
        );
        flat.code_ranges.push(4..13);
        assert_eq!(
            file_references(&flat, Path::new("/repo")),
            vec![(19..27, "/repo/real.sh".to_owned())]
        );
        flat.code_ranges.clear();
        flat.links.push((4..13, "https://example.com".to_owned()));
        assert_eq!(
            file_references(&flat, Path::new("/repo")),
            vec![(19..27, "/repo/real.sh".to_owned())]
        );
    }
}
