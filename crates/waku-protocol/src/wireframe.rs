//! The Goddard wireframe format (v1).
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
//! consumer — the in-app preview, which maps the tree onto themed flex
//! elements, and any future validating op — must agree on one definition,
//! which is what this crate is for. `Wireframe::parse` is the single entry
//! point: it rejects unknown versions explicitly, names malformed input,
//! and enforces a defensive size bound so previews cannot be made to hang.
//! Rendering lives with the consumer: this crate holds no drawing code,
//! keeping its no-side-effects boundary intact.

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
    InvalidScreen {
        name: String,
        width: f32,
        height: f32,
    },
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

impl WireScreen {
    /// Total nodes in this screen's tree — the summary the
    /// `wireframe_check` bin prints.
    pub fn node_count(&self) -> usize {
        let mut count = 0;
        count_nodes(&self.root, &mut count);
        count
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
        let children =
            std::iter::repeat_n(r#"{"type":"rect","height":10}"#, MAX_WIREFRAME_NODES + 1)
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
    fn sizing_and_alignment_fields_parse() {
        let doc = Wireframe::parse(
            r#"{"version":1,"name":"x","screens":[{"name":"s","width":100,"height":40,
               "root":{"type":"frame","direction":"row","padding":{"x":10,"y":4},"gap":5,
               "align":"center","justify":"space_between","children":[
                 {"type":"rect","width":20},
                 {"type":"rect","width":"fill"},
                 {"type":"text","text":"Hi","role":"title","tone":"accent","align":"end"},
                 {"type":"divider","annotations":"splits the toolbar"}
               ]}}]}"#,
        )
        .unwrap();
        let WireNode::Frame(root) = &doc.screens[0].root else {
            panic!("root is a frame");
        };
        assert_eq!(root.direction, WireDirection::Row);
        assert_eq!(root.padding, WirePadding::Axes { x: 10.0, y: 4.0 });
        assert_eq!(root.align, WireAlign::Center);
        assert_eq!(root.justify, WireJustify::SpaceBetween);
        let WireNode::Rect(fixed) = &root.children[0] else {
            panic!("first child is a rect");
        };
        assert_eq!(fixed.width, WireSize::Fixed(20.0));
        let WireNode::Rect(filled) = &root.children[1] else {
            panic!("second child is a rect");
        };
        assert_eq!(filled.width, WireSize::Fill);
        assert_eq!(filled.height, WireSize::Hug);
        let WireNode::Text(text) = &root.children[2] else {
            panic!("third child is text");
        };
        assert_eq!(text.role, WireTextRole::Title);
        assert_eq!(text.tone, WireTextTone::Accent);
        assert_eq!(text.align, WireTextAlign::End);
        let WireNode::Divider(divider) = &root.children[3] else {
            panic!("fourth child is a divider");
        };
        assert_eq!(divider.annotations.as_deref(), Some("splits the toolbar"));
    }
}
