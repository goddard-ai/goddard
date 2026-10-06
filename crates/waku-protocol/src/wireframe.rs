//! The Goddard wireframe format (v1) and its SVG renderer.
//!
//! A planning session emits a `.wireframe.json` companion beside a plan
//! document (`plans/<name>.wireframe.json` for `plans/<name>.md`). The
//! document is a set of named screens; each screen is a frame tree built
//! from a deliberately small primitive set — `frame`, `rect`, `text`, and
//! `divider` — over a fixed token palette and type scale. Low fidelity is
//! the point: the planner stays fast, the preview stays cheap, and the
//! result reads as "proposal, not product".
//!
//! The schema is a file format rather than a wire message, but every
//! consumer — in-app preview, the SVG/Figma export artifact, any future
//! validating op — must agree on one definition, which is what this crate
//! is for. `Wireframe::parse` is the single entry point: it rejects
//! unknown versions explicitly, names malformed input, and enforces a
//! defensive size bound so previews cannot be made to hang.
//!
//! `WireScreen::render_svg` is the renderer every consumer shares: the
//! in-app preview can show the output through the existing image surface,
//! and Figma imports the same SVG natively (its own text layout keeps
//! `<text>` elements editable). Rendering is pure — no I/O — which keeps
//! this crate's no-side-effects boundary intact.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The only schema version this build reads. The field is load-bearing:
/// the format will grow, and previews must reject unknown versions
/// explicitly rather than misrendering them.
pub const WIREFRAME_VERSION: u32 = 1;

/// Defensive bound on total nodes in one document. Screens are small by
/// design (~30–60 nodes); the cap exists so a malformed or adversarial
/// document cannot make a preview layout pass hang.
pub const MAX_WIREFRAME_NODES: usize = 2_000;

/// A wireframe document: named screens sharing one token vocabulary.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wireframe {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub screens: Vec<WireScreen>,
}

/// One named screen — a fixed-size frame containing a node tree. The
/// root node always spans the whole screen; its own sizing is ignored.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireScreen {
    pub name: String,
    pub width: f32,
    pub height: f32,
    pub root: WireNode,
}

/// The closed set of v1 primitives. Serde's internal tag writes the node
/// kind as `"type": "frame" | "rect" | "text" | "divider"`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireNode {
    Frame(WireFrame),
    Rect(WireRect),
    Text(WireText),
    Divider(WireDivider),
}

/// A layout container. Beyond direction/gap/padding/alignment it carries
/// optional surface styling (`fill`, `stroke`, `cornerRadius`) because a
/// `rect` cannot hold children — panels, chips, and segmented controls
/// need a paintable box that also lays out content.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireFrame {
    #[serde(default)]
    pub direction: WireDirection,
    #[serde(default)]
    pub gap: f32,
    #[serde(default)]
    pub padding: WirePadding,
    /// Cross-axis placement of children; `stretch` (the default) gives a
    /// column's children full width, a row's children full height.
    #[serde(default)]
    pub align: WireAlign,
    /// Main-axis placement of the children as a group when space is left
    /// over. `fill` children consume the space first.
    #[serde(default)]
    pub justify: WireJustify,
    /// Main- and cross-axis sizing; see [`WireSize`]. Both default to
    /// `hug` — a column's children still span its full width through
    /// `stretch` alignment, while a row's children stay snug unless they
    /// ask to share space with `fill`.
    #[serde(default)]
    pub width: WireSize,
    #[serde(default)]
    pub height: WireSize,
    #[serde(default)]
    pub fill: Option<WireTone>,
    #[serde(default)]
    pub stroke: Option<WireTone>,
    #[serde(default)]
    pub corner_radius: f32,
    /// A short note ("reuses Files working tree") the preview surfaces on
    /// select/hover. Wireframes carry intent, not just geometry.
    #[serde(default)]
    pub annotations: Option<String>,
    #[serde(default)]
    pub children: Vec<WireNode>,
}

/// A placeholder block — an image, avatar, chart, or input stand-in. An
/// optional `label` names what it stands in for; a labeled rect hugs its
/// label like a chip.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireRect {
    #[serde(default)]
    pub fill: Option<WireTone>,
    #[serde(default)]
    pub stroke: Option<WireTone>,
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub width: WireSize,
    #[serde(default)]
    pub height: WireSize,
    #[serde(default)]
    pub annotations: Option<String>,
}

/// A text run. `role` picks from the fixed type scale — wireframes use
/// type roles, not raw font specs — and `tone` picks from the text
/// palette. Text truncates with an ellipsis when it overflows its box,
/// like real UI would.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireText {
    pub text: String,
    #[serde(default)]
    pub role: WireTextRole,
    #[serde(default)]
    pub tone: WireTextTone,
    /// Horizontal placement inside the text's own box: `start` (default),
    /// `center`, or `end`.
    #[serde(default)]
    pub align: WireTextAlign,
    #[serde(default)]
    pub width: WireSize,
    #[serde(default)]
    pub height: WireSize,
    #[serde(default)]
    pub annotations: Option<String>,
}

/// A hairline separator. Its orientation follows the parent frame: a
/// divider in a column is a horizontal rule; in a row, a vertical one.
/// The parent layout gives it 1px on the main axis and the full inner
/// span on the cross axis; it declares no sizing of its own.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireDivider {
    #[serde(default)]
    pub annotations: Option<String>,
}

/// A node's size along one axis: a share of the space remaining after
/// fixed and hug siblings, the node's content size, or an exact pixel
/// value. Serializes as `"fill"`, `"hug"`, or a number (`120`). The
/// serde impl is hand-written because untagged unit variants only match
/// `null`, never their names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WireSize {
    /// Equal share of the parent's remaining main-axis space; on the
    /// cross axis, `fill` takes the parent's full inner span.
    Fill,
    /// The node's content size — children plus padding for a frame,
    /// measured text for `text`, the label for a `rect`.
    Hug,
    /// An exact pixel size.
    Fixed(f32),
}

impl Default for WireSize {
    /// Both axes default to hugging content; `fill` is always an explicit
    /// ask for a share of the parent's remaining space.
    fn default() -> Self {
        Self::Hug
    }
}

impl Serialize for WireSize {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Fill => serializer.serialize_str("fill"),
            Self::Hug => serializer.serialize_str("hug"),
            Self::Fixed(v) => serializer.serialize_f32(*v),
        }
    }
}

impl<'de> Deserialize<'de> for WireSize {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SizeVisitor;
        impl serde::de::Visitor<'_> for SizeVisitor {
            type Value = WireSize;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#""fill", "hug", or a pixel number"#)
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "fill" => Ok(WireSize::Fill),
                    "hug" => Ok(WireSize::Hug),
                    other => Err(E::unknown_variant(other, &["fill", "hug"])),
                }
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(WireSize::Fixed(value as f32))
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(WireSize::Fixed(value as f32))
            }

            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(WireSize::Fixed(value as f32))
            }
        }
        deserializer.deserialize_any(SizeVisitor)
    }
}

/// Frame padding: one number for all sides (`"padding": 16`) or per-axis
/// (`"padding": {"x": 16, "y": 8}`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum WirePadding {
    /// One value applied to every side.
    Uniform(f32),
    /// Independent horizontal and vertical padding.
    Axes {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
    },
}

impl Default for WirePadding {
    fn default() -> Self {
        Self::Uniform(0.0)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireDirection {
    Row,
    #[default]
    Column,
}

/// Cross-axis placement of a frame's children.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireAlign {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

/// Main-axis placement of a frame's children as a group.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireJustify {
    #[default]
    Start,
    Center,
    End,
    /// First and last children pin to the ends; the rest space evenly.
    SpaceBetween,
}

/// The fixed gray-plus-accent palette. Every painted value in a
/// wireframe is one of these tones — no arbitrary colors in v1.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireTone {
    /// Raised card or selected-segment surface.
    Surface,
    /// Recessed well — input fields, segmented-control tracks.
    Inset,
    /// Mid emphasis — selected rows, icon and avatar placeholders.
    Overlay,
    /// Dark emphasis — primary buttons and active segments.
    Ink,
    /// The single accent, reserved for interactive and unread cues.
    Accent,
}

/// The fixed type scale. Wireframes use these roles exclusively.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireTextRole {
    Title,
    #[default]
    Body,
    /// Quiet metadata — timestamps, counts, breadcrumbs.
    Caption,
    /// Emphasized short text — section labels, button text.
    Label,
}

/// The text palette, quieter than the fill palette by design.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireTextTone {
    #[default]
    Primary,
    Secondary,
    Tertiary,
    Accent,
}

/// Horizontal placement of text inside its box.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireTextAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Why a document failed to load. Preview surfaces show these directly,
/// so each variant names the problem rather than blanking.
#[derive(Clone, Debug, PartialEq)]
pub enum WireframeError {
    /// Not well-formed wireframe JSON — a parse failure, a wrong shape,
    /// or an unknown node type. Carries serde's message.
    Malformed(String),
    /// `version` names a schema this build does not read.
    UnsupportedVersion(u64),
    /// The document exceeds the defensive node bound.
    TooManyNodes { count: usize, max: usize },
    /// A screen's dimensions are non-finite or non-positive.
    InvalidScreen { name: String, width: f32, height: f32 },
}

impl fmt::Display for WireframeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(reason) => write!(f, "malformed wireframe document: {reason}"),
            Self::UnsupportedVersion(version) => write!(
                f,
                "unsupported wireframe version {version} — this build reads \
                 version {WIREFRAME_VERSION}"
            ),
            Self::TooManyNodes { count, max } => write!(
                f,
                "wireframe document has {count} nodes, over the {max}-node limit"
            ),
            Self::InvalidScreen {
                name,
                width,
                height,
            } => write!(
                f,
                "wireframe screen \"{name}\" has invalid dimensions {width}×{height}"
            ),
        }
    }
}

impl std::error::Error for WireframeError {}

impl Wireframe {
    /// The single load path for a `.wireframe.json` document. The version
    /// is probed before the full parse so a newer document reports
    /// [`WireframeError::UnsupportedVersion`] instead of a confusing
    /// shape error.
    pub fn parse(json: &str) -> Result<Self, WireframeError> {
        #[derive(Deserialize)]
        struct VersionProbe {
            version: Option<serde_json::Value>,
        }
        let probe: VersionProbe = serde_json::from_str(json)
            .map_err(|error| WireframeError::Malformed(error.to_string()))?;
        match probe.version {
            None => {
                return Err(WireframeError::Malformed(
                    "missing required `version`".to_owned(),
                ));
            }
            Some(value) => match value.as_u64() {
                Some(v) if v == u64::from(WIREFRAME_VERSION) => {}
                Some(other) => return Err(WireframeError::UnsupportedVersion(other)),
                None => {
                    return Err(WireframeError::Malformed(
                        "`version` must be a positive integer".to_owned(),
                    ));
                }
            },
        }
        let wireframe: Self = serde_json::from_str(json)
            .map_err(|error| WireframeError::Malformed(error.to_string()))?;
        wireframe.validate()?;
        Ok(wireframe)
    }

    /// Structural invariants beyond shape: positive screen sizes and the
    /// node bound. Runs inside [`Wireframe::parse`]; callers that build a
    /// document in memory may invoke it directly.
    pub fn validate(&self) -> Result<(), WireframeError> {
        let mut nodes = 0usize;
        for screen in &self.screens {
            if !screen.width.is_finite()
                || !screen.height.is_finite()
                || screen.width <= 0.0
                || screen.height <= 0.0
            {
                return Err(WireframeError::InvalidScreen {
                    name: screen.name.clone(),
                    width: screen.width,
                    height: screen.height,
                });
            }
            count_nodes(&screen.root, &mut nodes);
            if nodes > MAX_WIREFRAME_NODES {
                return Err(WireframeError::TooManyNodes {
                    count: nodes,
                    max: MAX_WIREFRAME_NODES,
                });
            }
        }
        Ok(())
    }
}

fn count_nodes(node: &WireNode, count: &mut usize) {
    *count += 1;
    if let WireNode::Frame(frame) = node {
        for child in &frame.children {
            count_nodes(child, count);
        }
    }
}

// ── SVG rendering ─────────────────────────────────────────────────────

const SVG_FONT_STACK: &str = "-apple-system, 'Segoe UI', system-ui, sans-serif";
/// The canvas every screen paints on. Wireframes carry their own light
/// background so the same SVG reads on the app's dark or light theme and
/// on Figma's canvas.
const CANVAS_HEX: &str = "#ffffff";

impl WireTone {
    /// The tone used as a shape fill.
    fn fill_hex(self) -> &'static str {
        match self {
            Self::Surface => "#fafafa",
            Self::Inset => "#f4f4f5",
            Self::Overlay => "#e4e4e7",
            Self::Ink => "#3f3f46",
            Self::Accent => "#e2795b",
        }
    }

    /// The same tone used as a hairline stroke — a step darker than its
    /// fill so borders read at 1px.
    fn line_hex(self) -> &'static str {
        match self {
            Self::Surface => "#e4e4e7",
            Self::Inset => "#d4d4d8",
            Self::Overlay => "#a1a1aa",
            Self::Ink => "#27272a",
            Self::Accent => "#b8522e",
        }
    }
}

impl WireTextTone {
    fn hex(self) -> &'static str {
        match self {
            Self::Primary => "#27272a",
            Self::Secondary => "#52525b",
            Self::Tertiary => "#a1a1aa",
            Self::Accent => "#b8522e",
        }
    }
}

/// Role → (size, weight, line height) — the whole v1 type scale.
fn role_metrics(role: WireTextRole) -> (f32, u16, f32) {
    match role {
        WireTextRole::Title => (16.0, 600, 22.0),
        WireTextRole::Body => (13.0, 400, 18.0),
        WireTextRole::Caption => (11.0, 400, 15.0),
        WireTextRole::Label => (12.0, 500, 16.0),
    }
}

/// Rough average advance width for Latin UI text at a given size. The
/// estimate exists only for hug sizing and ellipsizing — wireframe
/// fidelity does not need real font metrics.
fn char_width(role: WireTextRole) -> f32 {
    role_metrics(role).0 * 0.55
}

fn text_measure(text: &str, role: WireTextRole) -> Vec2 {
    let (_, _, line_height) = role_metrics(role);
    Vec2 {
        x: text.chars().count() as f32 * char_width(role),
        y: line_height,
    }
}

#[derive(Clone, Copy)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl WireNode {
    /// The node's content size — what `hug` resolves to before the
    /// parent's stretch or fill distribution. `Fixed` sizing is resolved
    /// by the caller, which knows the axis it applies to.
    fn intrinsic(&self) -> Vec2 {
        match self {
            Self::Frame(frame) => {
                let padding = frame.padding_vec();
                let mut main = 0.0f32;
                let mut cross = 0.0f32;
                for child in &frame.children {
                    let size = child.intrinsic();
                    let (child_main, child_cross) = match frame.direction {
                        WireDirection::Row => (size.x, size.y),
                        WireDirection::Column => (size.y, size.x),
                    };
                    main += child_main;
                    cross = cross.max(child_cross);
                }
                if frame.children.len() > 1 {
                    main += frame.gap * (frame.children.len() - 1) as f32;
                }
                match frame.direction {
                    WireDirection::Row => Vec2 {
                        x: main + padding.x * 2.0,
                        y: cross + padding.y * 2.0,
                    },
                    WireDirection::Column => Vec2 {
                        x: cross + padding.x * 2.0,
                        y: main + padding.y * 2.0,
                    },
                }
            }
            Self::Rect(rect) => match &rect.label {
                // A labeled rect hugs like a chip: label text plus
                // breathing room on both axes.
                Some(label) => {
                    let size = text_measure(label, WireTextRole::Label);
                    Vec2 {
                        x: size.x + 20.0,
                        y: size.y + 10.0,
                    }
                }
                None => Vec2::ZERO,
            },
            Self::Text(text) => text_measure(&text.text, text.role),
            Self::Divider(_) => Vec2 { x: 1.0, y: 1.0 },
        }
    }

    fn sizes(&self) -> (WireSize, WireSize) {
        match self {
            Self::Frame(frame) => (frame.width, frame.height),
            Self::Rect(rect) => (rect.width, rect.height),
            Self::Text(text) => (text.width, text.height),
            // Overridden by the parent layout — a divider is always 1px
            // on the main axis and fills the cross span.
            Self::Divider(_) => (WireSize::Hug, WireSize::Hug),
        }
    }

    fn annotations(&self) -> Option<&str> {
        match self {
            Self::Frame(frame) => frame.annotations.as_deref(),
            Self::Rect(rect) => rect.annotations.as_deref(),
            Self::Text(text) => text.annotations.as_deref(),
            Self::Divider(divider) => divider.annotations.as_deref(),
        }
    }
}

impl WireFrame {
    fn padding_vec(&self) -> Vec2 {
        match self.padding {
            WirePadding::Uniform(v) => Vec2 { x: v, y: v },
            WirePadding::Axes { x, y } => Vec2 { x, y },
        }
    }
}

impl WireScreen {
    /// The whole screen as a standalone SVG document — the in-app
    /// preview asset and the Figma export artifact in one.
    pub fn render_svg(&self) -> String {
        let mut svg = String::with_capacity(4_096);
        svg.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" \
             viewBox=\"0 0 {} {}\" font-family=\"{}\">",
            fmt_num(self.width),
            fmt_num(self.height),
            fmt_num(self.width),
            fmt_num(self.height),
            SVG_FONT_STACK,
        ));
        svg.push_str(&format!(
            "<rect width=\"{}\" height=\"{}\" fill=\"{CANVAS_HEX}\"/>",
            fmt_num(self.width),
            fmt_num(self.height),
        ));
        layout(
            &self.root,
            Rect {
                x: 0.0,
                y: 0.0,
                w: self.width.max(0.0),
                h: self.height.max(0.0),
            },
            WireDirection::Column,
            &mut svg,
        );
        svg.push_str("</svg>");
        svg
    }
}

/// Assign `node` its resolved box and emit its SVG. `parent_direction` is
/// how a divider learns its orientation.
fn layout(node: &WireNode, rect: Rect, parent_direction: WireDirection, svg: &mut String) {
    match node {
        WireNode::Frame(frame) => layout_frame(node, frame, rect, svg),
        WireNode::Rect(_) | WireNode::Text(_) | WireNode::Divider(_) => {
            emit_leaf(node, rect, parent_direction, svg);
        }
    }
}

/// A child's resolved slot inside a frame: main-axis size, cross-axis
/// size, whether the cross was stretched by alignment, and whether the
/// main size still waits for a share of leftover space.
#[derive(Clone, Copy)]
struct Slot {
    main: f32,
    cross: f32,
    stretched: bool,
    pending_fill: bool,
}

fn layout_frame(node: &WireNode, frame: &WireFrame, rect: Rect, svg: &mut String) {
    // The frame's surface paints only when styled; an annotation-only
    // frame still wraps in a `<g>` so the note has a home.
    let grouped = frame.fill.is_some()
        || frame.stroke.is_some()
        || node.annotations().is_some();
    if grouped {
        svg.push_str("<g>");
        if let Some(note) = node.annotations() {
            svg.push_str(&format!("<title>{}</title>", escape_xml(note)));
        }
        if frame.fill.is_some() || frame.stroke.is_some() {
            svg.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{}/>",
                fmt_num(rect.x),
                fmt_num(rect.y),
                fmt_num(rect.w),
                fmt_num(rect.h),
                frame
                    .fill
                    .map(|tone| format!(" fill=\"{}\"", tone.fill_hex()))
                    .unwrap_or_else(|| " fill=\"none\"".to_owned()),
                stroke_attrs(frame.stroke, frame.corner_radius),
            ));
        }
    }

    let padding = frame.padding_vec();
    let inner = Rect {
        x: rect.x + padding.x,
        y: rect.y + padding.y,
        w: (rect.w - padding.x * 2.0).max(0.0),
        h: (rect.h - padding.y * 2.0).max(0.0),
    };
    let horizontal = frame.direction == WireDirection::Row;
    let (inner_main, inner_cross) = if horizontal {
        (inner.w, inner.h)
    } else {
        (inner.h, inner.w)
    };

    // Resolve each child's slot. A divider is always 1px on the main
    // axis and fills the cross axis, whatever its siblings do.
    let count = frame.children.len();
    let gap_total = if count > 1 {
        frame.gap * (count - 1) as f32
    } else {
        0.0
    };
    let mut fills = 0usize;
    let mut reserved = 0.0f32;
    let mut slots = Vec::with_capacity(count);
    for child in &frame.children {
        let (width, height) = child.sizes();
        let (mut main_sizing, mut cross_sizing) = if horizontal {
            (width, height)
        } else {
            (height, width)
        };
        if matches!(child, WireNode::Divider(_)) {
            main_sizing = WireSize::Fixed(1.0);
            cross_sizing = WireSize::Fill;
        }
        let measured = child.intrinsic();
        let measured_main = if horizontal { measured.x } else { measured.y };
        let measured_cross = if horizontal { measured.y } else { measured.x };
        let (main, pending_fill) = match main_sizing {
            WireSize::Fixed(v) => (v.max(0.0), false),
            WireSize::Hug => (measured_main, false),
            WireSize::Fill => {
                fills += 1;
                (0.0, true)
            }
        };
        reserved += main;
        // Cross: fill — or stretch alignment over a non-fixed size —
        // takes the inner span; fixed and un-stretched hug keep their
        // own size for start/center/end placement.
        let (cross, stretched) = match cross_sizing {
            WireSize::Fill => (inner_cross, true),
            WireSize::Fixed(v) => (v.max(0.0).min(inner_cross), false),
            WireSize::Hug => {
                if frame.align == WireAlign::Stretch {
                    (inner_cross, true)
                } else {
                    (measured_cross.min(inner_cross), false)
                }
            }
        };
        slots.push(Slot {
            main,
            cross,
            stretched,
            pending_fill,
        });
    }
    let leftover = (inner_main - gap_total - reserved).max(0.0);
    if fills > 0 {
        let share = leftover / fills as f32;
        for slot in &mut slots {
            if slot.pending_fill {
                slot.main = share;
            }
        }
    }

    // Where the group starts along the main axis, and how leftover
    // distributes between children when nothing fills it.
    let (offset, between) = match frame.justify {
        _ if fills > 0 => (0.0, frame.gap),
        WireJustify::Start => (0.0, frame.gap),
        WireJustify::Center => (leftover / 2.0, frame.gap),
        WireJustify::End => (leftover, frame.gap),
        WireJustify::SpaceBetween if count > 1 => {
            (0.0, frame.gap + leftover / (count - 1) as f32)
        }
        WireJustify::SpaceBetween => (0.0, frame.gap),
    };
    let mut cursor = offset + if horizontal { inner.x } else { inner.y };

    for (index, (child, slot)) in frame.children.iter().zip(slots.iter()).enumerate() {
        let cross_offset = if slot.stretched {
            0.0
        } else {
            match frame.align {
                WireAlign::Start | WireAlign::Stretch => 0.0,
                WireAlign::Center => (inner_cross - slot.cross).max(0.0) / 2.0,
                WireAlign::End => (inner_cross - slot.cross).max(0.0),
            }
        };
        let child_rect = if horizontal {
            Rect {
                x: cursor,
                y: inner.y + cross_offset,
                w: slot.main,
                h: slot.cross,
            }
        } else {
            Rect {
                x: inner.x + cross_offset,
                y: cursor,
                w: slot.cross,
                h: slot.main,
            }
        };
        layout(child, child_rect, frame.direction, svg);
        cursor += slot.main + if index + 1 < count { between } else { 0.0 };
    }

    if grouped {
        svg.push_str("</g>");
    }
}

fn emit_leaf(node: &WireNode, rect: Rect, parent_direction: WireDirection, svg: &mut String) {
    let annotated = node.annotations().is_some();
    if annotated {
        svg.push_str(&format!(
            "<g><title>{}</title>",
            escape_xml(node.annotations().unwrap_or_default())
        ));
    }
    match node {
        WireNode::Rect(placeholder) => {
            svg.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{}/>",
                fmt_num(rect.x),
                fmt_num(rect.y),
                fmt_num(rect.w),
                fmt_num(rect.h),
                placeholder
                    .fill
                    .map(|tone| format!(" fill=\"{}\"", tone.fill_hex()))
                    .unwrap_or_else(|| " fill=\"none\"".to_owned()),
                stroke_attrs(placeholder.stroke, placeholder.corner_radius),
            ));
            if let Some(label) = &placeholder.label {
                // Dark fills get a light label; anything else reads best
                // in the quiet tertiary tone.
                let color = match placeholder.fill {
                    Some(WireTone::Ink) | Some(WireTone::Accent) => "#ffffff",
                    _ => WireTextTone::Tertiary.hex(),
                };
                emit_text_line(svg, label, rect, WireTextRole::Label, color, WireTextAlign::Center);
            }
        }
        WireNode::Text(text) => emit_text_line(
            svg,
            &fit_text(&text.text, rect.w, text.role),
            rect,
            text.role,
            text.tone.hex(),
            text.align,
        ),
        WireNode::Divider(_) => {
            // In a column the divider is a horizontal rule; in a row it
            // runs vertically.
            let (x1, y1, x2, y2) = match parent_direction {
                WireDirection::Column => {
                    let y = rect.y + rect.h / 2.0;
                    (rect.x, y, rect.x + rect.w, y)
                }
                WireDirection::Row => {
                    let x = rect.x + rect.w / 2.0;
                    (x, rect.y, x, rect.y + rect.h)
                }
            };
            svg.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" \
                 stroke=\"#d4d4d8\" stroke-width=\"1\"/>",
                fmt_num(x1),
                fmt_num(y1),
                fmt_num(x2),
                fmt_num(y2),
            ));
        }
        WireNode::Frame(_) => {}
    }
    if annotated {
        svg.push_str("</g>");
    }
}

/// Emit one line of text inside `rect`: horizontally per `align`,
/// vertically centered by an ascent estimate.
fn emit_text_line(
    svg: &mut String,
    text: &str,
    rect: Rect,
    role: WireTextRole,
    color: &str,
    align: WireTextAlign,
) {
    if text.is_empty() {
        return;
    }
    let (size, weight, _line_height) = role_metrics(role);
    let (x, anchor) = match align {
        WireTextAlign::Start => (rect.x, "start"),
        WireTextAlign::Center => (rect.x + rect.w / 2.0, "middle"),
        WireTextAlign::End => (rect.x + rect.w, "end"),
    };
    // Baseline estimate: the visual center of the box sits ~0.35em above
    // the font's baseline.
    let y = rect.y + rect.h / 2.0 + size * 0.35;
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"{}\" font-weight=\"{weight}\" \
         fill=\"{color}\" text-anchor=\"{anchor}\">{}</text>",
        fmt_num(x),
        fmt_num(y),
        fmt_num(size),
        escape_xml(text),
    ));
}

/// Truncate `text` to fit `width` at its role's estimated advance width,
/// adding an ellipsis when clipped — wireframes ellipsize like real UI.
/// Exact fits get a half-pixel of slack so float noise can't truncate a
/// string that genuinely fits.
fn fit_text(text: &str, width: f32, role: WireTextRole) -> String {
    if text_measure(text, role).x <= width + 0.5 {
        return text.to_owned();
    }
    let capacity = (width / char_width(role)).floor() as usize;
    let mut fitted: String = text.chars().take(capacity.saturating_sub(1)).collect();
    fitted.push('…');
    fitted
}

fn stroke_attrs(stroke: Option<WireTone>, corner_radius: f32) -> String {
    let mut attrs = String::new();
    if corner_radius > 0.0 {
        attrs.push_str(&format!(" rx=\"{}\"", fmt_num(corner_radius)));
    }
    if let Some(tone) = stroke {
        attrs.push_str(&format!(" stroke=\"{}\" stroke-width=\"1\"", tone.line_hex()));
    }
    attrs
}

/// Compact float formatting — `12` rather than `12.0` keeps the SVG
/// clean for Figma import.
fn fmt_num(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e9 {
        format!("{}", value as i64)
    } else {
        format!("{value:.2}")
    }
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_and_unknown_versions() {
        assert!(matches!(
            Wireframe::parse(r#"{"name":"x","screens":[]}"#),
            Err(WireframeError::Malformed(_))
        ));
        assert_eq!(
            Wireframe::parse(r#"{"version":2,"name":"x","screens":[]}"#),
            Err(WireframeError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn names_unknown_node_types() {
        let error = Wireframe::parse(
            r#"{"version":1,"name":"x","screens":[{"name":"s","width":100,"height":100,
               "root":{"type":"ellipse"}}]}"#,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, WireframeError::Malformed(_)));
        assert!(message.contains("ellipse"), "{message}");
    }

    #[test]
    fn enforces_the_node_bound() {
        let children = std::iter::repeat_n(r#"{"type":"rect","height":10}"#, MAX_WIREFRAME_NODES + 1)
            .collect::<Vec<_>>()
            .join(",");
        let doc = format!(
            r#"{{"version":1,"name":"x","screens":[{{"name":"s","width":100,
               "height":100,"root":{{"type":"frame","children":[{children}]}}}}]}}"#
        );
        assert!(matches!(
            Wireframe::parse(&doc),
            Err(WireframeError::TooManyNodes { .. })
        ));
    }

    #[test]
    fn distributes_fill_space_and_stretches_cross_axis() {
        let doc = Wireframe::parse(
            r#"{"version":1,"name":"x","screens":[{"name":"s","width":100,"height":40,
               "root":{"type":"frame","direction":"row","padding":10,"gap":5,"children":[
                 {"type":"rect","width":20},
                 {"type":"rect","width":"fill"},
                 {"type":"rect","width":"fill"},
                 {"type":"rect"}
               ]}}]}"#,
        )
        .unwrap();
        // Inner width 80, gap total 15, fixed child 20 → each fill = 22.5.
        // The last child keeps the default hug sizing → an empty rect
        // measures 0. Stretch alignment gives all of them the 20px inner
        // height.
        let svg = doc.screens[0].render_svg();
        assert!(svg.contains("x=\"10\" y=\"10\" width=\"20\" height=\"20\""));
        assert!(svg.contains("x=\"35\" y=\"10\" width=\"22.50\" height=\"20\""));
        assert!(svg.contains("x=\"62.50\" y=\"10\" width=\"22.50\" height=\"20\""));
        assert!(svg.contains("x=\"90\" y=\"10\" width=\"0\" height=\"20\""));
    }

    #[test]
    fn renders_text_rect_and_divider_primitives() {
        let doc = Wireframe::parse(
            r#"{"version":1,"name":"x","screens":[{"name":"s","width":100,"height":60,
               "root":{"type":"frame","children":[
                 {"type":"text","text":"Hello","role":"title"},
                 {"type":"divider"},
                 {"type":"rect","label":"avatar","fill":"overlay","height":30}
               ]}}]}"#,
        )
        .unwrap();
        let svg = doc.screens[0].render_svg();
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains(">Hello</text>"));
        assert!(svg.contains("<line"));
        assert!(svg.contains("fill=\"#e4e4e7\""));
        assert!(svg.contains(">avatar</text>"));
    }

    #[test]
    fn ellipsizes_overlong_text_and_escapes_xml() {
        let doc = Wireframe::parse(
            r#"{"version":1,"name":"x","screens":[{"name":"s","width":400,"height":60,
               "root":{"type":"frame","children":[
                 {"type":"text","text":"A very long label that overruns its box and must be clipped with an ellipsis"},
                 {"type":"text","text":"<b> & <co>"}
               ]}}]}"#,
        )
        .unwrap();
        let svg = doc.screens[0].render_svg();
        assert!(svg.contains("…"));
        assert!(!svg.contains("<b>"));
        assert!(svg.contains("&lt;b&gt; &amp; &lt;co&gt;"));
    }

    #[test]
    fn annotations_become_svg_titles() {
        let doc = Wireframe::parse(
            r#"{"version":1,"name":"x","screens":[{"name":"s","width":100,"height":40,
               "root":{"type":"frame","children":[
                 {"type":"rect","height":20,"annotations":"reuses Files working tree"}
               ]}}]}"#,
        )
        .unwrap();
        let svg = doc.screens[0].render_svg();
        assert!(svg.contains("<title>reuses Files working tree</title>"));
    }
}
