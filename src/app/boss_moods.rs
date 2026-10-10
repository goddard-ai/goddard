//! Local generation of DiceBear avatars.
//!
//! Boss avatars used to be fetched from `api.dicebear.com`, which cost a
//! network round-trip per employee, broke offline, and leaked avatar seeds.
//! This module ports the deterministic engine so a seed renders the same
//! face entirely locally.
//!
//! The style definitions are vendored verbatim from
//! `dicebear/styles@a63968226125e9c7f5d24865ea2ab4fdb3491ec2` (`src/*.json`;
//! each file's `meta` block carries its license). The selection semantics
//! below mirror the DiceBear 11.x core for the option set the app requests —
//! transparent background, no flip, animation off: a seeded FNV-1a +
//! Mulberry32 PRNG draws each value under a fixed key, variants are
//! weight-picked over name-sorted pools, palette colors come from a seeded
//! shuffle unless the pool declares `contrastTo` (then the strongest-contrast
//! value against the referenced color wins, unshuffled), and component
//! `translate`/`rotate`/`scale` ranges draw per component name. Component
//! references nest: a variant's elements may place other components, whose
//! picks are shared per seed. `dicebear_styles_match_api` pins the result
//! against captures from the live API.
//!
//! Not ported: tag filtering (`tags=` narrows nothing for these styles),
//! `notEqualTo` palette rules (unused by the vendored styles), text/style
//! element types, and CSS animations (the app rasterizes a single frame).

mod agent;
mod avvvatars;

use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::OnceLock;
use waku_protocol::boss::AvatarStyle;

const MOODS_JSON: &str = include_str!("boss_moods.json");
const GAZE_JSON: &str = include_str!("boss_moods/gaze.json");
const LINE_FACE_JSON: &str = include_str!("boss_moods/line-face.json");

/// The `size` the generated SVG is requested at — fixes the intrinsic
/// viewport so `avatar_scale` can target an exact raster size.
pub(super) const AVATAR_SOURCE_SIZE: f32 = 256.0;

#[derive(Deserialize)]
struct Style {
    canvas: Canvas,
    components: BTreeMap<String, Component>,
    colors: BTreeMap<String, ColorPool>,
}

#[derive(Deserialize)]
struct Canvas {
    width: f64,
    height: f64,
    elements: Vec<Element>,
}

#[derive(Deserialize)]
struct Element {
    name: Option<String>,
    /// `component` places a named component's picked variant; `element` is a
    /// literal SVG node. Every vendored element declares one explicitly.
    #[serde(rename = "type")]
    kind: Option<String>,
    attributes: Option<BTreeMap<String, AttrValue>>,
    children: Option<Vec<Element>>,
}

#[derive(Deserialize)]
struct Component {
    width: f64,
    height: f64,
    probability: Option<f64>,
    translate: Option<Translate>,
    rotate: Option<SteppedRange>,
    scale: Option<SteppedRange>,
    variants: BTreeMap<String, Variant>,
}

#[derive(Deserialize)]
struct Translate {
    x: Option<Range>,
    y: Option<Range>,
}

#[derive(Clone, Copy, Deserialize)]
struct Range {
    min: f64,
    max: f64,
}

#[derive(Clone, Copy, Deserialize)]
struct SteppedRange {
    min: f64,
    max: f64,
    step: Option<f64>,
}

#[derive(Deserialize)]
struct Variant {
    weight: Option<f64>,
    elements: Vec<Element>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AttrValue {
    Text(String),
    Color { name: String },
}

#[derive(Deserialize)]
struct ColorPool {
    values: Vec<String>,
    /// Name of another pool this one must contrast with; the picked value is
    /// the strongest-contrast candidate, with no shuffle.
    #[serde(rename = "contrastTo")]
    contrast_to: Option<String>,
}

fn dicebear_style(style: AvatarStyle) -> &'static Style {
    static MOODS: OnceLock<Style> = OnceLock::new();
    static GAZE: OnceLock<Style> = OnceLock::new();
    static LINE_FACE: OnceLock<Style> = OnceLock::new();
    let (lock, json) = match style {
        AvatarStyle::DiceBear => (&MOODS, MOODS_JSON),
        AvatarStyle::Gaze => (&GAZE, GAZE_JSON),
        AvatarStyle::LineFace => (&LINE_FACE, LINE_FACE_JSON),
        AvatarStyle::AgentAvatars | AvatarStyle::Avvvatars => {
            unreachable!("{style:?} is not a schema-driven DiceBear style")
        }
    };
    lock.get_or_init(|| {
        serde_json::from_str(json).expect("vendored DiceBear definitions must parse")
    })
}

/// Styles that clip to a circle instead of the app's rounded-square frame —
/// the raster is cropped in the SVG so every surface paints the same shape.
pub(super) fn circular_frame(style: AvatarStyle) -> bool {
    matches!(style, AvatarStyle::LineFace)
}

/// FNV-1a over UTF-16 code units, matching DiceBear's `Fnv1a.hash`.
fn fnv1a(input: &str) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for unit in input.encode_utf16() {
        hash ^= unit as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

struct Mulberry32(u32);

impl Mulberry32 {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x6d2b79f5);
        let z = self.0;
        let mut t = (z ^ (z >> 15)).wrapping_mul(z | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    fn next_float(&mut self) -> f64 {
        self.next() as f64 / 4294967296.0
    }
}

/// One deterministic draw in `[0, 1)` for `seed:key` — DiceBear's keyed PRNG
/// makes every pick independent of call order.
fn value(seed: &str, key: &str) -> f64 {
    Mulberry32(fnv1a(&format!("{seed}:{key}"))).next_float()
}

/// JavaScript `Math.round`: halves go toward positive infinity.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn bool_at(seed: &str, key: &str, likelihood: f64) -> bool {
    value(seed, key) * 100.0 < likelihood
}

fn float_at(seed: &str, key: &str, range: Range) -> f64 {
    let (min, max) = (range.min.min(range.max), range.min.max(range.max));
    js_round((min + value(seed, key) * (max - min)) * 10000.0) / 10000.0
}

/// DiceBear's `Prng.float` for `rotate`/`scale`: a positive `step` draws
/// uniformly from `min + i*step` buckets; otherwise the range is continuous.
fn stepped_float_at(seed: &str, key: &str, range: SteppedRange) -> f64 {
    let (min, max) = (range.min.min(range.max), range.min.max(range.max));
    let step = range.step.unwrap_or(0.0);
    let drawn = if step > 0.0 {
        let buckets = ((max - min) / step).floor() + 1.0;
        min + (value(seed, key) * buckets).floor() * step
    } else {
        min + value(seed, key) * (max - min)
    };
    js_round(drawn * 10000.0) / 10000.0
}

/// DiceBear's `weightedPick` over a name-sorted pool. An all-zero pool falls
/// back to an unweighted pick, matching upstream.
fn weighted_pick<'a>(seed: &str, key: &str, weights: &[(&'a str, f64)]) -> Option<&'a str> {
    if let [(name, _)] = weights {
        return Some(name);
    }
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        let index = (value(seed, key) * weights.len() as f64) as usize;
        return weights.get(index).map(|(name, _)| *name);
    }
    let threshold = value(seed, key) * total;
    let mut cumulative = 0.0;
    for (name, weight) in weights {
        cumulative += weight;
        if threshold < cumulative {
            return Some(name);
        }
    }
    weights.last().map(|(name, _)| *name)
}

/// DiceBear's `shuffle`: Fisher-Yates over deduplicated, codepoint-sorted
/// items with a chained Mulberry32. Callers take the front of the result.
fn shuffled<'a>(seed: &str, key: &str, items: &'a [String]) -> Vec<&'a str> {
    let mut items: Vec<&str> = items.iter().map(String::as_str).collect();
    items.sort_unstable();
    items.dedup();
    let mut prng = Mulberry32(fnv1a(&format!("{seed}:{key}")));
    for i in (1..items.len()).rev() {
        let j = (prng.next_float() * (i + 1) as f64) as usize;
        items.swap(i, j);
    }
    items
}

/// `#[#rrggbb]`/`#rgb`/`#rrggbbaa` → `(r, g, b)`; alpha is dropped like
/// DiceBear's `toRgbHex`. Non-hex values yield `None`.
fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.strip_prefix('#')?;
    let expand = |h: &str| -> Option<(u8, u8, u8)> {
        Some((
            u8::from_str_radix(h.get(0..2)?, 16).ok()?,
            u8::from_str_radix(h.get(2..4)?, 16).ok()?,
            u8::from_str_radix(h.get(4..6)?, 16).ok()?,
        ))
    };
    match h.len() {
        3 | 4 => {
            let doubled: String = h.chars().take(3).flat_map(|c| [c, c]).collect();
            expand(&doubled.to_lowercase())
        }
        6 | 8 => expand(&h.to_lowercase()),
        _ => None,
    }
}

/// WCAG 2.1 relative luminance with sRGB linearization — DiceBear's
/// `Color.luminance`.
fn luminance(hex: &str) -> f64 {
    let Some((r, g, b)) = rgb(hex) else {
        return 0.0;
    };
    let linearize = |channel: u8| {
        let s = f64::from(channel) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
}

/// The WCAG contrast ratio between two colors, `>= 1`.
fn contrast_ratio(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Seeded palette picks for one render, memoized per pool name. `contrastTo`
/// pools resolve deterministically against the referenced pick and skip the
/// shuffle, mirroring DiceBear's `resolveColor`.
struct Palette<'a> {
    style: &'a Style,
    seed: &'a str,
    picked: HashMap<String, &'a str>,
    resolving: Vec<String>,
}

impl<'a> Palette<'a> {
    const FALLBACK: &'static str = "#1e293b";

    fn new(style: &'a Style, seed: &'a str) -> Self {
        Self {
            style,
            seed,
            picked: HashMap::new(),
            resolving: Vec::new(),
        }
    }

    fn pick(&mut self, name: &str) -> &'a str {
        if let Some(&color) = self.picked.get(name) {
            return color;
        }
        let color = self.resolve(name);
        self.picked.insert(name.to_owned(), color);
        color
    }

    fn resolve(&mut self, name: &str) -> &'a str {
        let Some(pool) = self.style.colors.get(name) else {
            return Self::FALLBACK;
        };
        let Some(reference) = pool.contrast_to.as_deref() else {
            return shuffled(self.seed, &format!("{name}Color"), &pool.values)
                .first()
                .copied()
                .unwrap_or(Self::FALLBACK);
        };
        // Guard against contrastTo cycles the same way DiceBear errors on
        // them — vendored pools are acyclic, so the fallback never fires.
        if self.resolving.iter().any(|resolving| resolving == name) {
            return pool.values.first().map_or(Self::FALLBACK, String::as_str);
        }
        self.resolving.push(name.to_owned());
        let reference = self.pick(reference);
        self.resolving.pop();
        let mut candidates: Vec<&str> = pool.values.iter().map(String::as_str).collect();
        candidates.sort_by(|a, b| {
            contrast_ratio(b, reference)
                .partial_cmp(&contrast_ratio(a, reference))
                .unwrap_or(Ordering::Equal)
        });
        candidates.first().copied().unwrap_or(Self::FALLBACK)
    }
}

/// DiceBear's `Number.format`: round to five decimals, trim trailing zeros.
fn num(value: f64) -> String {
    let scaled = js_round(value * 100000.0) as i64;
    let sign = if scaled < 0 { "-" } else { "" };
    let magnitude = scaled.unsigned_abs();
    let fraction = format!("{:05}", magnitude % 100000);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{sign}{}", magnitude / 100000)
    } else {
        format!("{sign}{}.{fraction}", magnitude / 100000)
    }
}

fn write_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// One component's seeded picks for a render: the chosen variant (`None`
/// when its probability roll fails) and its translate/rotate/scale draws —
/// pixels for translate, degrees for rotate, a factor for scale.
#[derive(Clone, Copy, Default)]
struct Pick<'a> {
    variant: Option<&'a str>,
    translate: (f64, f64),
    rotate: f64,
    scale: f64,
}

fn resolve<'a>(style: &'a Style, seed: &str) -> HashMap<&'a str, Pick<'a>> {
    style
        .components
        .iter()
        .map(|(name, component)| {
            let variant = bool_at(
                seed,
                &format!("{name}Probability"),
                component.probability.unwrap_or(100.0),
            )
            .then(|| {
                let weights: Vec<(&str, f64)> = component
                    .variants
                    .iter()
                    .map(|(name, variant)| (name.as_str(), variant.weight.unwrap_or(1.0)))
                    .collect();
                weighted_pick(seed, &format!("{name}Variant"), &weights)
            })
            .flatten();
            let translate = (
                component
                    .translate
                    .as_ref()
                    .and_then(|t| t.x)
                    .map(|range| float_at(seed, &format!("{name}TranslateX"), range))
                    .unwrap_or(0.0)
                    * component.width
                    / 100.0,
                component
                    .translate
                    .as_ref()
                    .and_then(|t| t.y)
                    .map(|range| float_at(seed, &format!("{name}TranslateY"), range))
                    .unwrap_or(0.0)
                    * component.height
                    / 100.0,
            );
            let rotate = component
                .rotate
                .map(|range| stepped_float_at(seed, &format!("{name}Rotate"), range))
                .unwrap_or(0.0);
            let scale = component
                .scale
                .map(|range| stepped_float_at(seed, &format!("{name}Scale"), range))
                .unwrap_or(1.0);
            (
                name.as_str(),
                Pick {
                    variant,
                    translate,
                    rotate,
                    scale,
                },
            )
        })
        .collect()
}

/// How much of a Gaze face one render emits. The style's canvas is a single
/// `shape` component whose variants pair the body element with the `spacing`
/// component that seats the eye pair, so the animatable eye group splits off
/// at the element/`component` boundary inside `shape`. Non-Gaze renders
/// always use [`GazeSplit::Full`].
#[derive(Clone, Copy, PartialEq)]
enum GazeSplit {
    /// The whole face — the cached still.
    Full,
    /// The body without its eye group.
    Body,
    /// The eye group alone, kept at its seated spot inside the body's
    /// transform chain. `squash` flattens each eye to the `blink` track's
    /// mid-close scaleY so the app can swap this raster in for blink frames.
    Eyes { squash: bool },
}

/// Assembles one SVG document. Component bodies land in `<defs>` once and
/// every reference emits a `<use>`; `defs` elements nested in variants hoist
/// their children (gradients) into the shared section keyed by `id`.
struct Renderer<'a> {
    style: &'a Style,
    picks: HashMap<&'a str, Pick<'a>>,
    palette: Palette<'a>,
    split: GazeSplit,
    defs: Vec<String>,
    defined: HashSet<&'a str>,
}

impl<'a> Renderer<'a> {
    fn new(style: &'a Style, seed: &'a str, split: GazeSplit) -> Self {
        Self {
            style,
            picks: resolve(style, seed),
            palette: Palette::new(style, seed),
            split,
            defs: Vec::new(),
            defined: HashSet::new(),
        }
    }

    fn write_attribute(&mut self, out: &mut String, key: &str, attr: &AttrValue) {
        let _ = write!(out, " {key}=\"");
        match attr {
            AttrValue::Text(text) => write_escaped(out, text),
            AttrValue::Color { name } => {
                write_escaped(out, self.palette.pick(name.as_str()));
            }
        }
        out.push('"');
    }

    fn write_elements(&mut self, out: &mut String, elements: &'a [Element]) {
        for element in elements {
            self.write_element(out, element);
        }
    }

    fn write_element(&mut self, out: &mut String, element: &'a Element) {
        let Some(name) = element.name.as_deref() else {
            return;
        };
        if element.kind.as_deref() == Some("component") {
            return self.write_component(out, name, element);
        }
        if name == "defs" {
            // `defs` children are referencable document-wide; hoist them next
            // to the component bodies instead of nesting them in output.
            if let Some(children) = &element.children {
                for child in children {
                    let mut rendered = String::new();
                    self.write_element(&mut rendered, child);
                    if !rendered.is_empty() {
                        self.defs.push(rendered);
                    }
                }
            }
            return;
        }
        let mut children = String::new();
        if let Some(children_elements) = &element.children {
            self.write_elements(&mut children, children_elements);
        }
        // A wrapper whose children all rendered empty (an optional component
        // stayed home) has no content left to group; an id keeps it alive so
        // references still resolve.
        let keeps_id = element
            .attributes
            .as_ref()
            .is_some_and(|attributes| attributes.contains_key("id"));
        let had_children = element
            .children
            .as_ref()
            .is_some_and(|children| !children.is_empty());
        if children.is_empty() && had_children && !keeps_id {
            return;
        }
        let _ = write!(out, "<{name}");
        if let Some(attributes) = &element.attributes {
            for (key, attr) in attributes {
                self.write_attribute(out, key, attr);
            }
        }
        if children.is_empty() {
            out.push_str("/>");
        } else {
            let _ = write!(out, ">{children}</{name}>");
        }
    }

    fn write_component(&mut self, out: &mut String, name: &'a str, element: &'a Element) {
        let (Some(component), Some(pick)) = (
            self.style.components.get(name),
            self.picks.get(name).copied(),
        ) else {
            return;
        };
        let Some(variant) = pick.variant else {
            return;
        };
        if self.defined.insert(name) {
            let mut body = String::new();
            if let Some(variant) = component.variants.get(variant) {
                let elements: Vec<&Element> = variant
                    .elements
                    .iter()
                    .filter(|element| {
                        // Only the canvas-level `shape` body splits: `Body`
                        // keeps its literal elements, `Eyes` keeps the nested
                        // `spacing` component that seats the eye pair.
                        name != "shape"
                            || match self.split {
                                GazeSplit::Full => true,
                                GazeSplit::Body => {
                                    element.kind.as_deref() != Some("component")
                                }
                                GazeSplit::Eyes { .. } => {
                                    element.kind.as_deref() == Some("component")
                                }
                            }
                    })
                    .collect();
                for element in elements {
                    self.write_element(&mut body, element);
                }
            }
            // `blink` scales each eye about its own center; every eye variant
            // is drawn in the shared 16×16 box, so wrapping the def body once
            // squashes both placed eyes around (8, 8).
            if name == "eyes" && self.split == (GazeSplit::Eyes { squash: true }) {
                body = format!(
                    "<g transform=\"translate(8 8) scale(1 0.06) translate(-8 -8)\">{body}</g>"
                );
            }
            self.defs.push(format!("<g id=\"{name}\">{body}</g>"));
        }
        // The element's own transform lands first, then the component's
        // translate/rotate/scale draws — DiceBear's `buildTransforms` order.
        let mut parts: Vec<String> = element
            .attributes
            .as_ref()
            .and_then(|attributes| attributes.get("transform"))
            .and_then(|attr| match attr {
                AttrValue::Text(text) if !text.is_empty() => Some(text.clone()),
                _ => None,
            })
            .into_iter()
            .collect();
        let (cx, cy) = (component.width / 2.0, component.height / 2.0);
        if pick.translate.0 != 0.0 || pick.translate.1 != 0.0 {
            parts.push(format!(
                "translate({}, {})",
                num(pick.translate.0),
                num(pick.translate.1)
            ));
        }
        if pick.rotate != 0.0 {
            parts.push(format!(
                "rotate({}, {}, {})",
                num(pick.rotate),
                num(cx),
                num(cy)
            ));
        }
        if pick.scale != 1.0 {
            parts.push(format!(
                "translate({}, {}) scale({}) translate({}, {})",
                num(cx),
                num(cy),
                num(pick.scale),
                num(-cx),
                num(-cy)
            ));
        }
        let transform = parts.join(" ");
        out.push_str("<use");
        let mut wrote_transform = false;
        if let Some(attributes) = &element.attributes {
            for (key, attr) in attributes {
                if key == "transform" {
                    self.write_attribute(out, key, &AttrValue::Text(transform.clone()));
                    wrote_transform = true;
                } else {
                    self.write_attribute(out, key, attr);
                }
            }
        }
        if !wrote_transform && !transform.is_empty() {
            self.write_attribute(out, "transform", &AttrValue::Text(transform));
        }
        let _ = write!(out, " href=\"#{name}\"></use>");
    }
}

/// Dispatch the human-selected generator at the requested logical bucket.
pub(super) fn avatar_svg_for_style(seed: &str, style: AvatarStyle, bucket: u32) -> Vec<u8> {
    match style {
        AvatarStyle::DiceBear | AvatarStyle::Gaze | AvatarStyle::LineFace => {
            dicebear_svg(style, seed)
        }
        AvatarStyle::AgentAvatars => agent::avatar_svg(seed),
        AvatarStyle::Avvvatars => avvvatars::avatar_svg(seed, bucket),
    }
}

/// A Gaze face split into the layers the animated composition needs: the
/// body alone, the eye group at its seated spot, and the eye group mid-blink.
/// Every layer is a full-canvas document, so the app overlays them at the
/// avatar's rendered size with no coordinate mapping.
pub(super) struct GazeLayers {
    pub body: Vec<u8>,
    pub eyes: Vec<u8>,
    pub eyes_closed: Vec<u8>,
}

pub(super) fn gaze_layers(seed: &str) -> GazeLayers {
    GazeLayers {
        body: dicebear_svg_split(AvatarStyle::Gaze, seed, GazeSplit::Body),
        eyes: dicebear_svg_split(AvatarStyle::Gaze, seed, GazeSplit::Eyes { squash: false }),
        eyes_closed: dicebear_svg_split(
            AvatarStyle::Gaze,
            seed,
            GazeSplit::Eyes { squash: true },
        ),
    }
}

/// The same face DiceBear's API returns, generated without network or I/O.
fn dicebear_svg(style: AvatarStyle, seed: &str) -> Vec<u8> {
    dicebear_svg_split(style, seed, GazeSplit::Full)
}

fn dicebear_svg_split(style: AvatarStyle, seed: &str, split: GazeSplit) -> Vec<u8> {
    let style_def = dicebear_style(style);
    let mut renderer = Renderer::new(style_def, seed, split);
    let mut body = String::with_capacity(4096);
    for element in &style_def.canvas.elements {
        renderer.write_element(&mut body, element);
    }
    let (width, height) = (num(style_def.canvas.width), num(style_def.canvas.height));
    let clip = if circular_frame(style) {
        let radius = num(style_def.canvas.width.min(style_def.canvas.height) / 2.0);
        format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{radius}\"/>",
            num(style_def.canvas.width / 2.0),
            num(style_def.canvas.height / 2.0)
        )
    } else {
        format!("<rect width=\"{width}\" height=\"{height}\" rx=\"0\" ry=\"0\"/>")
    };
    let mut out = String::with_capacity(body.len() + 4096);
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    let _ = write!(
        out,
        " viewBox=\"0 0 {width} {height}\" fill=\"none\" shape-rendering=\"auto\" width=\"{}\" height=\"{}\"><defs>",
        num(AVATAR_SOURCE_SIZE as f64),
        num(AVATAR_SOURCE_SIZE as f64)
    );
    for def in &renderer.defs {
        out.push_str(def);
    }
    let _ = write!(out, "<clipPath id=\"clip\">{clip}</clipPath></defs>");
    let _ = write!(out, "<g clip-path=\"url(#clip)\">{body}</g></svg>");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(style: &Style, seed: &str, name: &str) -> String {
        Palette::new(style, seed).pick(name).to_owned()
    }

    #[test]
    fn fnv1a_utf16_matches_reference() {
        assert_eq!(fnv1a("test"), 0xafd071e5);
        assert_eq!(fnv1a(""), 0x811c9dc5);
    }

    /// Golden vectors captured from `api.dicebear.com/10.x/moods` with the
    /// app's request parameters — the port must reproduce the service's
    /// variant, color, and jitter picks seed-for-seed.
    #[test]
    fn variant_picks_match_dicebear() {
        let style = dicebear_style(AvatarStyle::DiceBear);
        // (seed, face, face color, eyes, mouth, cheeks shown,
        //  eyes jitter, mouth jitter) — jitter as the API formats it.
        let cases: &[(&str, &str, &str, &str, &str, bool, &str, &str)] = &[
            (
                "test",
                "blob",
                "#fdba74",
                "sparkle",
                "gasp",
                false,
                "-0.2101, 0.10581",
                "0.70009, -0.95211",
            ),
            (
                "7c9e6679-7425-40de-944b-e07fc1f90ae7",
                "hexagon",
                "#a5b4fc",
                "squint",
                "laugh",
                false,
                "0.2602, -0.66711",
                "-0.88218, -0.84444",
            ),
            (
                "petra",
                "wide",
                "#a5b4fc",
                "wink",
                "smile",
                false,
                "-0.46765, 0.15001",
                "-0.84064, 0.48177",
            ),
            (
                "",
                "tall",
                "#fdba74",
                "wink",
                "gasp",
                true,
                "0.1545, 0.89095",
                "-0.29596, 0.46062",
            ),
            (
                "a",
                "squircle",
                "#5eead4",
                "wink",
                "smirk",
                true,
                "-0.03705, 0.65067",
                "-1.43946, 1.61748",
            ),
            (
                "zz top",
                "softSquare",
                "#fca5a5",
                "small",
                "smile",
                true,
                "0.845, 0.8508",
                "0.74696, -0.08391",
            ),
            (
                "seed with spaces & <special>",
                "hexagon",
                "#fdba74",
                "wink",
                "grin",
                true,
                "0.81245, 0.39311",
                "-0.05458, -0.98841",
            ),
            (
                "0f3d2b1a-1111-4222-8333-abcdefabcdef",
                "blob",
                "#fdba74",
                "uneven",
                "open",
                true,
                "0.79525, 0.71953",
                "-0.05908, -1.61811",
            ),
            (
                "employee-42",
                "squircle",
                "#c4b5fd",
                "bigPupils",
                "bigSmile",
                false,
                "-0.51605, -0.58195",
                "0.39247, -1.36102",
            ),
            (
                "\u{1f600}emoji",
                "bean",
                "#fda4af",
                "happy",
                "tongue",
                true,
                "-0.80165, -0.95433",
                "0.04446, -0.35412",
            ),
            (
                "seed2",
                "squircle",
                "#fdba74",
                "closed",
                "tongue",
                true,
                "0.138, 0.48351",
                "-1.26734, 0.08906",
            ),
            (
                "xyz",
                "circle",
                "#f0abfc",
                "calm",
                "bigSmile",
                false,
                "-0.5732, 0.92127",
                "-0.20693, -1.13076",
            ),
        ];
        for (seed, face, face_color, eyes, mouth, cheeks, eyes_jitter, mouth_jitter) in cases {
            let picks = resolve(style, seed);
            let get = |name: &str| picks.get(name);
            let face_p = get("face").unwrap_or_else(|| panic!("{seed}: no face"));
            assert_eq!(face_p.variant, Some(*face), "{seed} face");
            assert_eq!(color(style, seed, "face"), *face_color, "{seed} face color");
            assert_eq!(
                get("cheeks").and_then(|pick| pick.variant),
                cheeks.then_some("blush"),
                "{seed} cheeks"
            );
            let eyes_p = get("eyes").unwrap_or_else(|| panic!("{seed}: no eyes"));
            assert_eq!(eyes_p.variant, Some(*eyes), "{seed} eyes");
            assert_eq!(
                format!("{}, {}", num(eyes_p.translate.0), num(eyes_p.translate.1)),
                *eyes_jitter,
                "{seed} eyes jitter"
            );
            let mouth_p = get("mouth").unwrap_or_else(|| panic!("{seed}: no mouth"));
            assert_eq!(mouth_p.variant, Some(*mouth), "{seed} mouth");
            assert_eq!(
                format!("{}, {}", num(mouth_p.translate.0), num(mouth_p.translate.1)),
                *mouth_jitter,
                "{seed} mouth jitter"
            );
        }
    }

    /// Golden vectors captured from `api.dicebear.com/11.x` for Gaze and Line
    /// Face, covering nested components, canvas-level elements, contrast
    /// palettes, and stepped rotation.
    #[test]
    fn dicebear_styles_match_api() {
        let gaze = dicebear_style(AvatarStyle::Gaze);
        // (seed, shape, spacing, eyes, body, ink, shapeRotate, shapeScale,
        //  eyesScale)
        let gaze_cases: &[(&str, &str, &str, &str, &str, &str, f64, f64, f64)] = &[
            (
                "test", "egg", "wide", "small", "#9bd78d", "#0f172a", 0.2514, 0.985, 0.9352,
            ),
            (
                "", "octagon", "close", "wide", "#9cc8fb", "#0f172a", -0.7415, 0.9886, 1.0895,
            ),
            (
                "petra", "hexagon", "wide", "wide", "#fbabaf", "#0f172a", 7.2778, 1.0337, 0.9949,
            ),
            (
                "a", "triangle", "snug", "wide", "#52dcd8", "#0f172a", -7.8354, 0.9877, 0.9638,
            ),
            (
                "xyz", "egg", "normal", "beans", "#52dcd8", "#0f172a", -0.862, 1.0293, 1.0794,
            ),
            (
                "zz top", "pill", "normal", "small", "#52dcd8", "#0f172a", 0.6528, 0.9937, 1.0576,
            ),
            (
                "employee-42",
                "hexagon",
                "close",
                "beans",
                "#9bd78d",
                "#0f172a",
                -2.9499,
                0.9662,
                0.9921,
            ),
        ];
        for (seed, shape, spacing, eyes, body, ink, rotate, scale, eyes_scale) in gaze_cases {
            let picks = resolve(gaze, seed);
            assert_eq!(picks["shape"].variant, Some(*shape), "{seed} shape");
            assert_eq!(picks["spacing"].variant, Some(*spacing), "{seed} spacing");
            assert_eq!(picks["eyes"].variant, Some(*eyes), "{seed} eyes");
            assert_eq!(color(gaze, seed, "body"), *body, "{seed} body");
            assert_eq!(color(gaze, seed, "ink"), *ink, "{seed} ink");
            assert_eq!(picks["shape"].rotate, *rotate, "{seed} shape rotate");
            assert_eq!(picks["shape"].scale, *scale, "{seed} shape scale");
            assert_eq!(picks["eyes"].scale, *eyes_scale, "{seed} eyes scale");
        }

        let line_face = dicebear_style(AvatarStyle::LineFace);
        // (seed, eyes, nose, mouth, ink, mouthRotate — stepped to ±6)
        let line_face_cases: &[(&str, &str, &str, &str, &str, f64)] = &[
            ("test", "unevenDots", "curve", "line", "#2e2218", 0.0),
            ("", "winkRight", "curve", "line", "#1f2c40", 0.0),
            ("petra", "winkRight", "angular", "smile", "#1f2c40", 6.0),
            ("a", "winkRight", "hook", "smirk", "#181818", 6.0),
            ("xyz", "dots", "long", "frown", "#2e2218", -6.0),
            ("zz top", "unevenDots", "curve", "shy", "#181818", -6.0),
            ("employee-42", "closed", "bow", "frown", "#1f2c40", 6.0),
        ];
        for (seed, eyes, nose, mouth, ink, rotate) in line_face_cases {
            let picks = resolve(line_face, seed);
            assert_eq!(picks["eyes"].variant, Some(*eyes), "{seed} eyes");
            assert_eq!(picks["nose"].variant, Some(*nose), "{seed} nose");
            assert_eq!(picks["mouth"].variant, Some(*mouth), "{seed} mouth");
            assert_eq!(color(line_face, seed, "ink"), *ink, "{seed} ink");
            assert_eq!(picks["mouth"].rotate, *rotate, "{seed} mouth rotate");
        }
    }

    #[test]
    fn generated_style_svgs_are_pinned() {
        for (style, expected) in [
            (AvatarStyle::DiceBear, 0x7564c8be),
            (AvatarStyle::Gaze, 0x00eaf0f7),
            (AvatarStyle::LineFace, 0xc6990950),
        ] {
            let svg = avatar_svg_for_style("test", style, 16);
            assert_eq!(
                fnv1a(std::str::from_utf8(&svg).unwrap()),
                expected,
                "{style:?}"
            );
            assert_ne!(svg, avatar_svg_for_style("another", style, 16));
        }
        for (style, expected) in [
            (AvatarStyle::AgentAvatars, 0x3e13591c),
            (AvatarStyle::Avvvatars, 0xe2ba1b6b),
        ] {
            let svg = avatar_svg_for_style("test", style, 16);
            assert_eq!(fnv1a(std::str::from_utf8(&svg).unwrap()), expected);
            assert_ne!(svg, avatar_svg_for_style("another", style, 16));
        }
    }

    #[test]
    fn generated_svg_is_stable_and_balanced() {
        for style in [
            AvatarStyle::DiceBear,
            AvatarStyle::Gaze,
            AvatarStyle::LineFace,
        ] {
            let a = dicebear_svg(style, "test");
            assert_eq!(a, dicebear_svg(style, "test"), "{style:?} unstable");
            let text = String::from_utf8(a).unwrap();
            assert!(text.starts_with("<svg"), "{style:?}");
            assert!(text.ends_with("</svg>"), "{style:?}");
        }
    }

    /// Circular styles crop the raster to a disc so every painted surface —
    /// sidebar, chips, mentions — shows the same round face.
    #[test]
    fn circular_styles_clip_to_a_circle() {
        for style in [AvatarStyle::LineFace] {
            let svg = String::from_utf8(dicebear_svg(style, "test")).unwrap();
            assert!(svg.contains("<circle"), "{style:?} lacks a circular clip");
            assert!(circular_frame(style));
        }
        for style in [AvatarStyle::DiceBear, AvatarStyle::Gaze] {
            let svg = String::from_utf8(dicebear_svg(style, "test")).unwrap();
            assert!(
                svg.contains("<rect"),
                "{style:?} should keep the square clip"
            );
            assert!(!circular_frame(style));
        }
    }

    /// The generated markup has to rasterize through GPUI's resvg pipeline —
    /// this catches SVG features (masks, `<use>` hrefs) the renderer rejects.
    #[gpui::test]
    fn generated_svg_rasters(cx: &mut gpui::TestAppContext) {
        let renderer = cx.update(|cx| cx.svg_renderer());
        // Empty and UUID seeds have both been suspected of leaving employees
        // on their letter placeholder. Exercise the same scales as the queue.
        let mut failures = Vec::new();
        for seed in [
            "",
            "test",
            "b8efc99d-28f9-4dcd-872a-afc315bccb91",
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ] {
            for style in [
                AvatarStyle::DiceBear,
                AvatarStyle::Gaze,
                AvatarStyle::LineFace,
                AvatarStyle::AgentAvatars,
                AvatarStyle::Avvvatars,
            ] {
                for bucket in [16, 24, 56] {
                    let svg = avatar_svg_for_style(seed, style, bucket);
                    assert_eq!(
                        svg,
                        avatar_svg_for_style(seed, style, bucket),
                        "unstable seed {seed:?}"
                    );
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        renderer.render_single_frame(&svg, bucket as f32 / AVATAR_SOURCE_SIZE)
                    }));
                    match result {
                        Ok(Ok(image)) => {
                            let frame = image.size(0);
                            assert_eq!(
                                (frame.width.0, frame.height.0),
                                (bucket as i32 * 2, bucket as i32 * 2)
                            );
                        }
                        Ok(Err(error)) => {
                            failures.push(format!("seed {seed:?}, bucket {bucket}: {error:#}"))
                        }
                        Err(_) => failures
                            .push(format!("seed {seed:?}, bucket {bucket}: renderer panicked")),
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
