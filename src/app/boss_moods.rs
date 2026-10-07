//! Local generation of DiceBear "moods" avatars.
//!
//! Boss avatars used to be fetched from `api.dicebear.com`, which cost a
//! network round-trip per employee, broke offline, and leaked avatar seeds.
//! This module ports the deterministic engine so a seed renders the same
//! face entirely locally.
//!
//! [`STYLE_JSON`] is the upstream style definition vendored verbatim from
//! `dicebear/styles@a63968226125e9c7f5d24865ea2ab4fdb3491ec2`
//! (`src/moods.json`, CC0 1.0 — the license rides inside the file's `meta`
//! block). The selection semantics below mirror the DiceBear 11.x core for
//! the option set the app requests — transparent background, no flip,
//! animation off: a seeded FNV-1a + Mulberry32 PRNG draws each value under a
//! fixed key, variants are weight-picked over name-sorted pools, palette
//! colors come from a seeded shuffle, and component `translate` ranges are
//! percentages of the component's own size. `variant_picks_match_dicebear`
//! pins the result against captures from the live API.
//!
//! Not ported: tag filtering (`tags=animation` narrows nothing in this
//! style), `contrastTo`/`notEqualTo` palette rules (unused by moods), and
//! CSS animations (the app rasterizes a single frame).

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::OnceLock;

const STYLE_JSON: &str = include_str!("boss_moods.json");

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
    elements: Vec<CanvasElement>,
}

#[derive(Deserialize)]
struct CanvasElement {
    name: String,
    attributes: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct Component {
    width: f64,
    height: f64,
    probability: Option<f64>,
    translate: Option<Translate>,
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

#[derive(Deserialize)]
struct Variant {
    weight: Option<f64>,
    elements: Vec<Element>,
}

#[derive(Deserialize)]
struct Element {
    name: Option<String>,
    attributes: Option<BTreeMap<String, AttrValue>>,
    children: Option<Vec<Element>>,
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
}

fn style() -> &'static Style {
    static STYLE: OnceLock<Style> = OnceLock::new();
    STYLE.get_or_init(|| serde_json::from_str(STYLE_JSON).expect("vendored moods.json must parse"))
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

/// DiceBear's `weightedPick` over a name-sorted pool.
fn weighted_pick<'a>(seed: &str, key: &str, weights: &[(&'a str, f64)]) -> &'a str {
    if let [(name, _)] = weights {
        return name;
    }
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    let threshold = value(seed, key) * total;
    let mut cumulative = 0.0;
    for (name, weight) in weights {
        cumulative += weight;
        if threshold < cumulative {
            return name;
        }
    }
    weights[weights.len() - 1].0
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

/// The first solid-fill stop of a named palette, picked the way DiceBear's
/// `random` color order draws it.
fn palette_pick<'a>(style: &'a Style, seed: &str, name: &str) -> &'a str {
    static FALLBACK: &str = "#1e293b";
    let Some(pool) = style.colors.get(name) else {
        return FALLBACK;
    };
    shuffled(seed, &format!("{name}Color"), &pool.values)
        .first()
        .copied()
        .unwrap_or(FALLBACK)
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

/// The picks one seed produces, before SVG assembly.
struct Placement<'a> {
    component: &'a str,
    variant: &'a str,
    base_transform: &'a str,
    jitter: (f64, f64),
}

fn resolve<'a>(style: &'a Style, seed: &str) -> Vec<Placement<'a>> {
    let mut placements = Vec::new();
    for element in &style.canvas.elements {
        let Some(component) = style.components.get(&element.name) else {
            continue;
        };
        if !bool_at(
            seed,
            &format!("{}Probability", element.name),
            component.probability.unwrap_or(100.0),
        ) {
            continue;
        }
        let weights: Vec<(&str, f64)> = component
            .variants
            .iter()
            .map(|(name, variant)| (name.as_str(), variant.weight.unwrap_or(1.0)))
            .collect();
        let variant = weighted_pick(seed, &format!("{}Variant", element.name), &weights);
        let base_transform = element
            .attributes
            .as_ref()
            .and_then(|attrs| attrs.get("transform"))
            .map(String::as_str)
            .unwrap_or_default();
        let jitter = (
            component
                .translate
                .as_ref()
                .and_then(|t| t.x)
                .map(|range| float_at(seed, &format!("{}TranslateX", element.name), range))
                .unwrap_or(0.0)
                * component.width
                / 100.0,
            component
                .translate
                .as_ref()
                .and_then(|t| t.y)
                .map(|range| float_at(seed, &format!("{}TranslateY", element.name), range))
                .unwrap_or(0.0)
                * component.height
                / 100.0,
        );
        placements.push(Placement {
            component: &element.name,
            variant,
            base_transform,
            jitter,
        });
    }
    placements
}

fn write_elements(
    out: &mut String,
    elements: &[Element],
    style: &Style,
    seed: &str,
    colors: &mut HashMap<String, String>,
) {
    for element in elements {
        let Some(name) = &element.name else {
            continue;
        };
        let _ = write!(out, "<{name}");
        if let Some(attributes) = &element.attributes {
            for (key, attr) in attributes {
                let _ = write!(out, " {key}=\"");
                match attr {
                    AttrValue::Text(text) => write_escaped(out, text),
                    AttrValue::Color { name } => {
                        let color = colors
                            .entry(name.clone())
                            .or_insert_with(|| palette_pick(style, seed, name).to_owned());
                        out.push_str(color);
                    }
                }
                out.push('"');
            }
        }
        out.push('>');
        if let Some(children) = &element.children {
            write_elements(out, children, style, seed, colors);
        }
        let _ = write!(out, "</{name}>");
    }
}

/// The standalone avatar SVG for `seed` — the same face DiceBear's API
/// returns for it, generated without any network or I/O.
pub(super) fn avatar_svg(seed: &str) -> Vec<u8> {
    let style = style();
    let placements = resolve(style, seed);
    let mut colors = HashMap::new();
    let mut out = String::with_capacity(4096);
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\" fill=\"none\" shape-rendering=\"auto\"");
    let _ = write!(
        out,
        " width=\"{}\" height=\"{}\"><defs>",
        num(AVATAR_SOURCE_SIZE as f64),
        num(AVATAR_SOURCE_SIZE as f64)
    );
    for placement in &placements {
        let _ = write!(out, "<g id=\"{}\">", placement.component);
        if let Some(variant) = style.components[placement.component]
            .variants
            .get(placement.variant)
        {
            write_elements(&mut out, &variant.elements, style, seed, &mut colors);
        }
        out.push_str("</g>");
    }
    out.push_str("<clipPath id=\"clip\"><rect width=\"100\" height=\"100\" rx=\"0\" ry=\"0\"/></clipPath></defs>");
    out.push_str("<g clip-path=\"url(#clip)\">");
    for placement in &placements {
        let _ = write!(out, "<use transform=\"{}", placement.base_transform);
        if placement.jitter.0 != 0.0 || placement.jitter.1 != 0.0 {
            let _ = write!(
                out,
                " translate({}, {})",
                num(placement.jitter.0),
                num(placement.jitter.1)
            );
        }
        let _ = write!(out, "\" href=\"#{}\"></use>", placement.component);
    }
    out.push_str("</g></svg>");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let style = style();
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
            let placements = resolve(style, seed);
            let get = |name: &str| placements.iter().find(|p| p.component == name);
            let face_p = get("face").unwrap_or_else(|| panic!("{seed}: no face"));
            assert_eq!(face_p.variant, *face, "{seed} face");
            assert_eq!(
                palette_pick(style, seed, "face"),
                *face_color,
                "{seed} face color"
            );
            assert_eq!(
                get("cheeks").map(|p| p.variant),
                cheeks.then_some("blush"),
                "{seed} cheeks"
            );
            let eyes_p = get("eyes").unwrap_or_else(|| panic!("{seed}: no eyes"));
            assert_eq!(eyes_p.variant, *eyes, "{seed} eyes");
            assert_eq!(
                format!("{}, {}", num(eyes_p.jitter.0), num(eyes_p.jitter.1)),
                *eyes_jitter,
                "{seed} eyes jitter"
            );
            let mouth_p = get("mouth").unwrap_or_else(|| panic!("{seed}: no mouth"));
            assert_eq!(mouth_p.variant, *mouth, "{seed} mouth");
            assert_eq!(
                format!("{}, {}", num(mouth_p.jitter.0), num(mouth_p.jitter.1)),
                *mouth_jitter,
                "{seed} mouth jitter"
            );
        }
    }

    #[test]
    fn generated_svg_is_stable_and_balanced() {
        let a = avatar_svg("test");
        assert_eq!(a, avatar_svg("test"));
        let text = String::from_utf8(a).unwrap();
        assert!(text.starts_with("<svg"));
        assert!(text.ends_with("</svg>"));
        // Every color reference the style emits must land on a real palette.
        for name in ["face", "ink"] {
            assert!(style().colors.contains_key(name), "missing {name} palette");
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
            let svg = avatar_svg(seed);
            assert_eq!(svg, avatar_svg(seed), "unstable seed {seed:?}");
            for bucket in [16, 24, 56] {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    renderer.render_single_frame(&svg, bucket as f32 / AVATAR_SOURCE_SIZE)
                }));
                match result {
                    Ok(Ok(image)) => {
                        let frame = image.size(0);
                        assert_eq!((frame.width.0, frame.height.0), (bucket * 2, bucket * 2));
                    }
                    Ok(Err(error)) => {
                        failures.push(format!("seed {seed:?}, bucket {bucket}: {error:#}"))
                    }
                    Err(_) => {
                        failures.push(format!("seed {seed:?}, bucket {bucket}: renderer panicked"))
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
