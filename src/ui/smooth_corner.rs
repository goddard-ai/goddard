//! Figma-style smooth corners ("squircles") painted as GPUI paths.
//!
//! Port of the geometry layer of [Lisse](https://github.com/JaceThings/Lisse)
//! (`@lisse/core`), which implements the corner-smoothing algorithm from
//! Figma's "Desperately seeking squircles" post. Where GPUI's `corner_radii`
//! paints plain circular arcs, a smoothed corner transitions curvature
//! gradually: a central arc flanked by two cubic shoulders.
//!
//! Two caveats follow from painting a `Path` instead of styling a quad:
//! children are not clipped to the curve (`ContentMask` is rectangular), and
//! the path carries no border or shadow. Borders can be approximated with a
//! second, stroked copy of the outline via [`SmoothCorner::border_path`].

// Not yet adopted by any view; the tests below are the only callers.
#![allow(dead_code)]

use gpui::{
    Background, Bounds, Canvas, Corners, Path, PathBuilder, Pixels, Point, Window, canvas, point,
    px,
};

/// Figma's labeled "iOS" smoothing preset — design-handoff parity value.
pub const FIGMA_SMOOTHING: f32 = 0.6;
/// Closest Figma-curve match to Apple's continuous corners.
pub const APPLE_SMOOTHING: f32 = 0.65;
/// Lisse's default smoothing — same value as [`APPLE_SMOOTHING`].
pub const DEFAULT_SMOOTHING: f32 = APPLE_SMOOTHING;

/// Slack allowed when deciding a corner's radius has reached the cap radius.
const CAP_EPS: f32 = 1e-9;
const BAND_EPS: f32 = 1e-9;

/// Per-corner radii plus smoothing — the analogue of Lisse's
/// `SmoothCornerOptions`. Radii are distributed exactly like CSS
/// `border-radius`: oversized corners shrink, larger corners winning
/// contested edges first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothCorner {
    radii: Corners<Pixels>,
    smoothing: f32,
    preserve_smoothing: bool,
}

impl Default for SmoothCorner {
    fn default() -> Self {
        Self {
            radii: Corners::default(),
            smoothing: DEFAULT_SMOOTHING,
            preserve_smoothing: true,
        }
    }
}

impl SmoothCorner {
    pub fn new() -> Self {
        Self::default()
    }

    /// The same radius on all four corners.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radii = Corners::all(radius);
        self
    }

    /// Per-corner radii.
    pub fn radii(mut self, radii: Corners<Pixels>) -> Self {
        self.radii = radii;
        self
    }

    /// Corner smoothing, 0.0–1.0. `0.0` degenerates to the circular arc
    /// `.rounded()` already paints; `FIGMA_SMOOTHING` (0.6) is Figma's iOS
    /// preset and `APPLE_SMOOTHING` (0.65, the default) is the closest match
    /// to Apple's continuous corners.
    pub fn smoothing(mut self, smoothing: f32) -> Self {
        self.smoothing = smoothing;
        self
    }

    /// When a corner's radius is capped by the edge length, whether to keep
    /// the smoothing ratio (`true`, the default, matching Lisse) or let
    /// smoothing collapse toward a circular arc.
    pub fn preserve_smoothing(mut self, preserve: bool) -> Self {
        self.preserve_smoothing = preserve;
        self
    }

    /// The filled outline for `bounds`, or `None` when the shape is a plain
    /// rectangle (every radius <= 0) so callers can fall back to `.bg()`.
    pub fn path(&self, bounds: Bounds<Pixels>) -> Option<Path<Pixels>> {
        self.build(bounds, PathBuilder::fill())
    }

    /// The same outline stroked `width` pixels wide, centered on the edge —
    /// the way Lisse draws borders, since a `Path` has no border channel.
    pub fn border_path(&self, bounds: Bounds<Pixels>, width: Pixels) -> Option<Path<Pixels>> {
        self.build(bounds, PathBuilder::stroke(width))
    }

    /// Fill-paint helper for `canvas` paint callbacks.
    pub fn paint(&self, bounds: Bounds<Pixels>, color: impl Into<Background>, window: &mut Window) {
        if let Some(path) = self.path(bounds) {
            window.paint_path(path, color);
        }
    }

    /// A `canvas` element painting the smoothed fill over its bounds.
    /// Position it with `.absolute().inset_0()` as the first child of a
    /// relative parent; descendants paint above it but are only clipped to
    /// the parent's rectangle, not to the curve.
    pub fn layer(self, color: impl Into<Background>) -> Canvas<()> {
        let color = color.into();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| self.paint(bounds, color, window),
        )
    }

    fn build(&self, bounds: Bounds<Pixels>, mut builder: PathBuilder) -> Option<Path<Pixels>> {
        let outline = self.outline(f32::from(bounds.size.width), f32::from(bounds.size.height))?;
        builder.translate(bounds.origin);
        for cmd in outline.cmds {
            match cmd {
                OutlineCmd::MoveTo(to) => builder.move_to(to),
                OutlineCmd::LineTo(to) => builder.line_to(to),
                OutlineCmd::CubicTo { ctrl_a, ctrl_b, to } => {
                    builder.cubic_bezier_to(ctrl_a, ctrl_b, to)
                }
                OutlineCmd::Arc { radius, delta } => {
                    builder.relative_arc_to(point(radius, radius), px(0.0), false, true, delta)
                }
                OutlineCmd::Close => builder.close(),
            }
        }
        builder.build().ok()
    }

    /// Local-coordinate outline of the smoothed rectangle, clockwise from the
    /// top edge — the `generatePath` template with SVG strings swapped for
    /// commands.
    fn outline(&self, width: f32, height: f32) -> Option<Outline> {
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let radii = [
            f32::from(self.radii.top_left),
            f32::from(self.radii.top_right),
            f32::from(self.radii.bottom_right),
            f32::from(self.radii.bottom_left),
        ];
        if radii.iter().all(|&r| r <= 0.0) {
            return None;
        }

        let normalized = distribute_and_normalize(radii, width, height);
        let s = self.smoothing;
        let preserve = self.preserve_smoothing;
        let uniform = radii[0] == radii[1] && radii[1] == radii[2] && radii[2] == radii[3];

        // In the band 2R < short side < 2(1+s)R the classic template pops
        // while resizing toward a capsule; per-edge smoothing blends it out.
        if uniform {
            let blend_r = radii[0].min(width / 2.0).min(height / 2.0);
            let short_half = width.min(height) / 2.0;
            if blend_r > 0.0
                && short_half > blend_r + BAND_EPS
                && short_half < (1.0 + s) * blend_r - BAND_EPS
            {
                return Some(blend_outline(width, height, blend_r, s, preserve));
            }
        }

        // A fully-rounded end becomes one continuous cap segment. Each end is
        // independent, so half-pills work.
        let horizontal = width >= height;
        let cap_r = if horizontal {
            height / 2.0
        } else {
            width / 2.0
        };
        let is_cap = |a: NormalizedCorner, b: NormalizedCorner| {
            (a.radius - cap_r).abs() < CAP_EPS && (b.radius - cap_r).abs() < CAP_EPS
        };
        let corner = |n: NormalizedCorner| corner_path_params(n.radius, s, preserve, n.budget);
        // Index order here and below: TL, TR, BR, BL.
        let [tl, tr, br, bl] = normalized;

        if horizontal {
            let right_cap = is_cap(tr, br);
            let left_cap = is_cap(tl, bl);
            if right_cap || left_cap {
                let long_half = width / 2.0;
                let cap_right = right_cap.then(|| cap_params(cap_r, s, preserve, long_half));
                let cap_left = left_cap.then(|| cap_params(cap_r, s, preserve, long_half));

                let mut out = Outline::new();
                out.move_to(point(px(cap_left.map_or(corner(tl).p, |c| c.p)), px(0.0)));
                out.line_to(point(
                    px(width - cap_right.map_or(corner(tr).p, |c| c.p)),
                    px(0.0),
                ));
                if let Some(cap) = cap_right {
                    out.cap_right(cap);
                } else {
                    let o_tr = corner(tr);
                    out.squircle_corner(Orient::TopRight, &o_tr);
                    let o_br = corner(br);
                    out.line_to(point(px(width), px(o_br.p)));
                    out.line_to(point(px(width), px(height - o_br.p)));
                    out.squircle_corner(Orient::BottomRight, &o_br);
                }
                if let Some(cap) = cap_left {
                    out.line_to(point(px(cap.p), px(height)));
                    out.cap_left(cap);
                } else {
                    let o_bl = corner(bl);
                    out.line_to(point(px(width - o_bl.p), px(height)));
                    out.line_to(point(px(o_bl.p), px(height)));
                    out.squircle_corner(Orient::BottomLeft, &o_bl);
                    let o_tl = corner(tl);
                    out.line_to(point(px(0.0), px(height - o_tl.p)));
                    out.line_to(point(px(0.0), px(o_tl.p)));
                    out.squircle_corner(Orient::TopLeft, &o_tl);
                }
                return Some(out.closed());
            }
        } else {
            let top_cap = is_cap(tl, tr);
            let bottom_cap = is_cap(bl, br);
            if top_cap || bottom_cap {
                let long_half = height / 2.0;
                let cap_top = top_cap.then(|| cap_params(cap_r, s, preserve, long_half));
                let cap_bottom = bottom_cap.then(|| cap_params(cap_r, s, preserve, long_half));

                let mut out = Outline::new();
                if let Some(cap) = cap_top {
                    out.move_to(point(px(0.0), px(cap.p)));
                    out.cap_top(cap);
                } else {
                    out.move_to(point(px(corner(tl).p), px(0.0)));
                    let o_tr = corner(tr);
                    out.line_to(point(px(width - o_tr.p), px(0.0)));
                    out.squircle_corner(Orient::TopRight, &o_tr);
                }
                out.line_to(point(
                    px(width),
                    px(height - cap_bottom.map_or(corner(br).p, |c| c.p)),
                ));
                if let Some(cap) = cap_bottom {
                    out.cap_bottom(cap);
                } else {
                    let o_br = corner(br);
                    out.squircle_corner(Orient::BottomRight, &o_br);
                    let o_bl = corner(bl);
                    out.line_to(point(px(o_bl.p), px(height)));
                    out.squircle_corner(Orient::BottomLeft, &o_bl);
                }
                if let Some(cap) = cap_top {
                    out.line_to(point(px(0.0), px(cap.p)));
                } else {
                    let o_tl = corner(tl);
                    out.line_to(point(px(0.0), px(height - o_tl.p)));
                    out.line_to(point(px(0.0), px(o_tl.p)));
                    out.squircle_corner(Orient::TopLeft, &o_tl);
                }
                return Some(out.closed());
            }
        }

        let o_tl = corner(tl);
        let o_tr = corner(tr);
        let o_br = corner(br);
        let o_bl = corner(bl);

        let mut out = Outline::new();
        out.move_to(point(px(o_tl.p), px(0.0)));
        out.line_to(point(px(width - o_tr.p), px(0.0)));
        out.squircle_corner(Orient::TopRight, &o_tr);
        out.line_to(point(px(width), px(o_br.p)));
        out.line_to(point(px(width), px(height - o_br.p)));
        out.squircle_corner(Orient::BottomRight, &o_br);
        out.line_to(point(px(width - o_bl.p), px(height)));
        out.line_to(point(px(o_bl.p), px(height)));
        out.squircle_corner(Orient::BottomLeft, &o_bl);
        out.line_to(point(px(0.0), px(height - o_tl.p)));
        out.line_to(point(px(0.0), px(o_tl.p)));
        out.squircle_corner(Orient::TopLeft, &o_tl);
        Some(out.closed())
    }
}

/// One outline command in local coordinates — arcs keep their relative delta
/// because `PathBuilder::relative_arc_to` wants it that way; everything else
/// is already absolute.
#[derive(Clone, Copy, Debug)]
enum OutlineCmd {
    MoveTo(Point<Pixels>),
    LineTo(Point<Pixels>),
    CubicTo {
        ctrl_a: Point<Pixels>,
        ctrl_b: Point<Pixels>,
        to: Point<Pixels>,
    },
    Arc {
        radius: Pixels,
        delta: Point<Pixels>,
    },
    Close,
}

/// Corner identity under clockwise traversal; a canonical (entry → exit)
/// delta is rotated into place by [`rotate`].
#[derive(Clone, Copy)]
enum Orient {
    TopRight,
    BottomRight,
    BottomLeft,
    TopLeft,
}

/// Rotate a canonical (x = entry direction, y = exit direction) delta into
/// the oriented (dx, dy):
///
///   TR (x, y)   BR (-y, x)   BL (-x, -y)   TL (y, -x)
fn rotate(orient: Orient, x: f32, y: f32) -> (f32, f32) {
    match orient {
        Orient::TopRight => (x, y),
        Orient::BottomRight => (-y, x),
        Orient::BottomLeft => (-x, -y),
        Orient::TopLeft => (y, -x),
    }
}

struct Outline {
    cmds: Vec<OutlineCmd>,
    pen: Point<Pixels>,
}

impl Outline {
    fn new() -> Self {
        Self {
            cmds: Vec::with_capacity(32),
            pen: point(px(0.0), px(0.0)),
        }
    }

    fn move_to(&mut self, to: Point<Pixels>) {
        self.pen = to;
        self.cmds.push(OutlineCmd::MoveTo(to));
    }

    fn line_to(&mut self, to: Point<Pixels>) {
        self.pen = to;
        self.cmds.push(OutlineCmd::LineTo(to));
    }

    /// Relative cubic: control points and endpoint are deltas from the pen.
    fn cubic_rel(&mut self, ctrl_a: (f32, f32), ctrl_b: (f32, f32), to: (f32, f32)) {
        let pen = self.pen;
        let to = point(pen.x + px(to.0), pen.y + px(to.1));
        self.pen = to;
        self.cmds.push(OutlineCmd::CubicTo {
            ctrl_a: point(pen.x + px(ctrl_a.0), pen.y + px(ctrl_a.1)),
            ctrl_b: point(pen.x + px(ctrl_b.0), pen.y + px(ctrl_b.1)),
            to,
        });
    }

    /// Relative small-arc sweep (`a r r 0 0 1 dx dy` in SVG terms).
    fn arc_rel(&mut self, radius: f32, delta: (f32, f32)) {
        self.pen = point(self.pen.x + px(delta.0), self.pen.y + px(delta.1));
        self.cmds.push(OutlineCmd::Arc {
            radius: px(radius),
            delta: point(px(delta.0), px(delta.1)),
        });
    }

    /// A squircle corner: cubic shoulder, central arc, mirrored shoulder.
    /// Emitted relative so the same code serves all four orients. Corners
    /// with no radius emit nothing, matching Lisse's empty segment.
    fn squircle_corner(&mut self, orient: Orient, params: &CornerPathParams) {
        if params.corner_radius <= 0.0 {
            return;
        }
        let ab = params.a + params.b;
        let abc = ab + params.c;
        let bc = params.b + params.c;
        let c1 = rotate(orient, params.a, 0.0);
        let c2 = rotate(orient, ab, 0.0);
        let to = rotate(orient, abc, params.d);
        self.cubic_rel(c1, c2, to);
        let delta = rotate(orient, params.arc_section_length, params.arc_section_length);
        self.arc_rel(params.corner_radius, delta);
        let c1 = rotate(orient, params.d, params.c);
        let c2 = rotate(orient, params.d, bc);
        let to = rotate(orient, params.d, abc);
        self.cubic_rel(c1, c2, to);
    }

    /// Right cap: (width−p, 0) → (width−p, height).
    fn cap_right(&mut self, cap: CapParams) {
        self.cubic_rel((cap.a, 0.0), (cap.a + cap.b, 0.0), (cap.e, cap.d));
        self.arc_rel(cap.r, (cap.ax, cap.ay));
        self.arc_rel(cap.r, (-cap.ax, cap.ay));
        self.cubic_rel((-cap.c, cap.d), (-(cap.b + cap.c), cap.d), (-cap.e, cap.d));
    }

    /// Left cap: (p, height) → (p, 0).
    fn cap_left(&mut self, cap: CapParams) {
        self.cubic_rel((-cap.a, 0.0), (-(cap.a + cap.b), 0.0), (-cap.e, -cap.d));
        self.arc_rel(cap.r, (-cap.ax, -cap.ay));
        self.arc_rel(cap.r, (cap.ax, -cap.ay));
        self.cubic_rel((cap.c, -cap.d), (cap.b + cap.c, -cap.d), (cap.e, -cap.d));
    }

    /// Top cap: (0, p) → (width, p).
    fn cap_top(&mut self, cap: CapParams) {
        self.cubic_rel((0.0, -cap.a), (0.0, -(cap.a + cap.b)), (cap.d, -cap.e));
        self.arc_rel(cap.r, (cap.ay, -cap.ax));
        self.arc_rel(cap.r, (cap.ay, cap.ax));
        self.cubic_rel((cap.d, cap.c), (cap.d, cap.b + cap.c), (cap.d, cap.e));
    }

    /// Bottom cap: (width, height−p) → (0, height−p).
    fn cap_bottom(&mut self, cap: CapParams) {
        self.cubic_rel((0.0, cap.a), (0.0, cap.a + cap.b), (-cap.d, cap.e));
        self.arc_rel(cap.r, (-cap.ay, cap.ax));
        self.arc_rel(cap.r, (-cap.ay, -cap.ax));
        self.cubic_rel(
            (-cap.d, -cap.c),
            (-cap.d, -(cap.b + cap.c)),
            (-cap.d, -cap.e),
        );
    }

    fn closed(mut self) -> Self {
        self.cmds.push(OutlineCmd::Close);
        self
    }
}

/// Figure 11.1/12.2 parameters of one Figma squircle corner: `a`/`b`/`c`/`d`
/// shape the two cubic shoulders, `p` is the tangency distance from the
/// sharp vertex, and `arc_section_length` is the central arc's chord.
#[derive(Clone, Copy, Debug, Default)]
struct CornerPathParams {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    p: f32,
    arc_section_length: f32,
    corner_radius: f32,
}

/// `getPathParamsForCorner` — `rounding_and_smoothing_budget` is how much of
/// the adjacent edges this corner may consume, already resolved by
/// [`distribute_and_normalize`].
fn corner_path_params(
    corner_radius: f32,
    mut corner_smoothing: f32,
    preserve_smoothing: bool,
    rounding_and_smoothing_budget: f32,
) -> CornerPathParams {
    if corner_radius <= 0.0 {
        return CornerPathParams::default();
    }

    // p = (1 + cornerSmoothing) * q, where q = R at a 90° corner.
    let mut p = (1.0 + corner_smoothing) * corner_radius;

    if !preserve_smoothing {
        corner_smoothing =
            corner_smoothing.min(rounding_and_smoothing_budget / corner_radius - 1.0);
        p = p.min(rounding_and_smoothing_budget);
    }

    // Arc measure shrinks as smoothing increases.
    let arc_measure = 90.0 * (1.0 - corner_smoothing);
    let arc_section_length =
        (arc_measure / 2.0).to_radians().sin() * corner_radius * f32::sqrt(2.0);

    let angle_alpha = (90.0 - arc_measure) / 2.0;
    let p3_to_p4_distance = corner_radius * (angle_alpha / 2.0).to_radians().tan();

    let angle_beta = 45.0 * corner_smoothing;
    let c = p3_to_p4_distance * angle_beta.to_radians().cos();
    let d = c * angle_beta.to_radians().tan();

    let mut b = (p - arc_section_length - c - d) / 3.0;
    let mut a = 2.0 * b;

    if preserve_smoothing && p > rounding_and_smoothing_budget {
        let p1_to_p3_max_distance = rounding_and_smoothing_budget - d - arc_section_length - c;
        let min_a = p1_to_p3_max_distance / 6.0;
        let max_b = p1_to_p3_max_distance - min_a;
        b = b.min(max_b);
        a = p1_to_p3_max_distance - b;
        p = p.min(rounding_and_smoothing_budget);
    }

    CornerPathParams {
        a,
        b,
        c,
        d,
        p,
        arc_section_length,
        corner_radius,
    }
}

/// A corner after radius distribution with the edge budget it resolved to.
#[derive(Clone, Copy, Debug, Default)]
struct NormalizedCorner {
    radius: f32,
    budget: f32,
}

/// One corner's rounding-and-smoothing budget: the minimum over its two
/// edge-sharing neighbours. A negative `n_*b` marks a neighbour not yet
/// processed. `distributeAndNormalize`'s `cornerBudget`.
fn corner_budget(
    r: f32,
    n_hr: f32,
    n_hb: f32,
    width: f32,
    n_vr: f32,
    n_vb: f32,
    height: f32,
) -> f32 {
    let term_h = if r == 0.0 && n_hr == 0.0 {
        0.0
    } else if n_hb >= 0.0 {
        width - n_hb
    } else {
        r / (r + n_hr) * width
    };
    let term_v = if r == 0.0 && n_vr == 0.0 {
        0.0
    } else if n_vb >= 0.0 {
        height - n_vb
    } else {
        r / (r + n_vr) * height
    };
    term_h.min(term_v)
}

/// `distributeAndNormalize` — clamp per-corner radii to the rectangle, the
/// same contest CSS runs: corners take their budget in descending-radius
/// order, ties broken TL → TR → BL → BR. Input is `[TL, TR, BR, BL]`
/// (matching `Corners`); internally neighbours are addressed as
/// `[TL, TR, BL, BR]` to mirror the source's insertion order.
fn distribute_and_normalize(radii: [f32; 4], width: f32, height: f32) -> [NormalizedCorner; 4] {
    let [otl, otr, obr, obl] = radii;

    // Uniform positive radius on a positive box resolves in closed form.
    if otl == otr && otr == obr && obr == obl && otl > 0.0 && width > 0.0 && height > 0.0 {
        let budget = width.min(height) / 2.0;
        let corner = NormalizedCorner {
            radius: otl.min(budget),
            budget,
        };
        return [corner; 4];
    }

    // Descending-radius processing rank; `>=` on earlier corners gives them
    // the tiebreak, matching the source's stable sort.
    let rank = [
        (otr > otl) as usize + (obl > otl) as usize + (obr > otl) as usize, // TL
        (otl >= otr) as usize + (obl > otr) as usize + (obr > otr) as usize, // TR
        (otl >= obl) as usize + (otr > obl) as usize + (obr > obl) as usize, // BL
        (otl >= obr) as usize + (otr >= obr) as usize + (obl >= obr) as usize, // BR
    ];

    // Original and current (clamped) radii/budgets indexed [TL, TR, BL, BR]
    // like the source's insertion order.
    let original = [otl, otr, obl, obr];
    let mut current = original;
    let mut budget = [-1.0f32; 4];

    for step in 0..4 {
        match rank.iter().position(|&r| r == step).unwrap() {
            // topLeft: H neighbour topRight, V neighbour bottomLeft
            0 => {
                budget[0] = corner_budget(
                    original[0],
                    current[1],
                    budget[1],
                    width,
                    current[2],
                    budget[2],
                    height,
                );
                current[0] = original[0].min(budget[0]);
            }
            // topRight: H neighbour topLeft, V neighbour bottomRight
            1 => {
                budget[1] = corner_budget(
                    original[1],
                    current[0],
                    budget[0],
                    width,
                    current[3],
                    budget[3],
                    height,
                );
                current[1] = original[1].min(budget[1]);
            }
            // bottomLeft: H neighbour bottomRight, V neighbour topLeft
            2 => {
                budget[2] = corner_budget(
                    original[2],
                    current[3],
                    budget[3],
                    width,
                    current[0],
                    budget[0],
                    height,
                );
                current[2] = original[2].min(budget[2]);
            }
            // bottomRight: H neighbour bottomLeft, V neighbour topRight
            _ => {
                budget[3] = corner_budget(
                    original[3],
                    current[2],
                    budget[2],
                    width,
                    current[1],
                    budget[1],
                    height,
                );
                current[3] = original[3].min(budget[3]);
            }
        }
    }

    [
        NormalizedCorner {
            radius: current[0],
            budget: budget[0],
        },
        NormalizedCorner {
            radius: current[1],
            budget: budget[1],
        },
        NormalizedCorner {
            radius: current[3],
            budget: budget[3],
        },
        NormalizedCorner {
            radius: current[2],
            budget: budget[2],
        },
    ]
}

/// Parameters of one capsule end cap: the squircle shoulder applied on the
/// flat-edge side only, with the circular arc carried to the cap midline.
/// `capsuleEndParams` — `long_half` is each end's share of the long axis.
#[derive(Clone, Copy, Debug)]
struct CapParams {
    p: f32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    /// Arc chord along the long axis, `p − e`.
    ax: f32,
    /// Arc chord toward the midline, `R − d`.
    ay: f32,
    r: f32,
}

fn cap_params(r: f32, smoothing: f32, preserve_smoothing: bool, long_half: f32) -> CapParams {
    // The flat edge absorbs all smoothing; when it has no room the smoothing
    // collapses so the cap stays a true circle.
    let s_eff = smoothing.min(long_half / r - 1.0);
    let params = corner_path_params(r, s_eff, preserve_smoothing, long_half);
    let e = params.a + params.b + params.c;
    CapParams {
        p: params.p,
        a: params.a,
        b: params.b,
        c: params.c,
        d: params.d,
        e,
        ax: params.p - e,
        ay: r - params.d,
        r,
    }
}

/// One cubic shoulder in the blend regime — the same figure-11.1 formula as
/// a squircle corner, with the shoulder→arc tangent angle precomputed.
#[derive(Clone, Copy)]
struct Shoulder {
    a: f32,
    b: f32,
    p: f32,
    sin: f32,
    cos: f32,
}

fn shoulder(r: f32, s_edge: f32, preserve: bool, room: f32) -> Shoulder {
    let params = corner_path_params(r, s_edge, preserve, room);
    let beta = (45.0 * s_edge).to_radians();
    Shoulder {
        a: params.a,
        b: params.b,
        p: params.p,
        sin: beta.sin(),
        cos: beta.cos(),
    }
}

fn clamp_edge(room: f32, r: f32, s: f32) -> f32 {
    (room / r - 1.0).min(s).max(0.0)
}

/// `drawBlendPath` — a uniform squircle in the band where its short side sits
/// strictly between 2R and 2(1+s)R. Each edge keeps as much smoothing as it
/// has room for, so the outline moves continuously toward the capsule limit.
fn blend_outline(width: f32, height: f32, r: f32, smoothing: f32, preserve: bool) -> Outline {
    let h = shoulder(
        r,
        clamp_edge(width / 2.0, r, smoothing),
        preserve,
        width / 2.0,
    );
    let v = shoulder(
        r,
        clamp_edge(height / 2.0, r, smoothing),
        preserve,
        height / 2.0,
    );

    let mut out = Outline::new();
    out.move_to(point(px(h.p), px(0.0)));

    // One corner, oriented by unit axes: u points from the corner back along
    // the arrival edge, v along the departure edge.
    fn seg(
        out: &mut Outline,
        h: Shoulder,
        v: Shoulder,
        r: f32,
        cx: f32,
        cy: f32,
        ux: f32,
        uy: f32,
        vx: f32,
        vy: f32,
    ) {
        let s1 = if uy == 0.0 { h } else { v };
        let s2 = if vy == 0.0 { h } else { v };
        let ox = cx + (ux + vx) * r;
        let oy = cy + (uy + vy) * r;
        let j1x = ox - vx * r * s1.cos - ux * r * s1.sin;
        let j1y = oy - vy * r * s1.cos - uy * r * s1.sin;
        let j2x = ox - ux * r * s2.cos - vx * r * s2.sin;
        let j2y = oy - uy * r * s2.cos - vy * r * s2.sin;
        let p0x = cx + ux * s1.p;
        let p0y = cy + uy * s1.p;
        let arced = (j2x - j1x).hypot(j2y - j1y) > 1e-6;
        let ex = if arced { j2x } else { j1x };
        let ey = if arced { j2y } else { j1y };
        let p3x = cx + vx * s2.p;
        let p3y = cy + vy * s2.p;
        out.line_to(point(px(p0x), px(p0y)));
        out.cubic_rel(
            (-ux * s1.a, -uy * s1.a),
            (-ux * (s1.a + s1.b), -uy * (s1.a + s1.b)),
            (j1x - p0x, j1y - p0y),
        );
        if arced {
            out.arc_rel(r, (j2x - j1x, j2y - j1y));
        }
        out.cubic_rel(
            (p3x - vx * (s2.a + s2.b) - ex, p3y - vy * (s2.a + s2.b) - ey),
            (p3x - vx * s2.a - ex, p3y - vy * s2.a - ey),
            (p3x - ex, p3y - ey),
        );
    }

    // Corners clockwise from top-right; the path opened on the top edge.
    seg(&mut out, h, v, r, width, 0.0, -1.0, 0.0, 0.0, 1.0);
    seg(&mut out, h, v, r, width, height, 0.0, -1.0, -1.0, 0.0);
    seg(&mut out, h, v, r, 0.0, height, 1.0, 0.0, 0.0, -1.0);
    seg(&mut out, h, v, r, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0);
    out.closed()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_point_near(actual: Point<Pixels>, expected_x: f32, expected_y: f32) {
        let (x, y) = (f32::from(actual.x), f32::from(actual.y));
        assert!(
            (x - expected_x).abs() < 0.001 && (y - expected_y).abs() < 0.001,
            "expected ({expected_x}, {expected_y}), got ({x}, {y})"
        );
    }

    /// Endpoint of each emitted command — the outline's landmarks.
    fn endpoints(outline: &Outline) -> Vec<Point<Pixels>> {
        let mut pen = point(px(0.0), px(0.0));
        outline
            .cmds
            .iter()
            .map(|cmd| match *cmd {
                OutlineCmd::MoveTo(to)
                | OutlineCmd::LineTo(to)
                | OutlineCmd::CubicTo { to, .. } => {
                    pen = to;
                    to
                }
                OutlineCmd::Arc { delta, .. } => {
                    pen = point(pen.x + delta.x, pen.y + delta.y);
                    pen
                }
                OutlineCmd::Close => pen,
            })
            .collect()
    }

    #[test]
    fn plain_rectangle_returns_none() {
        assert!(SmoothCorner::new().outline(100.0, 50.0).is_none());
        assert!(
            SmoothCorner::new()
                .radius(px(10.0))
                .outline(0.0, 50.0)
                .is_none()
        );
    }

    /// Reference: generatePath(200, 100, {radius: 16, smoothing: 0.6}).
    #[test]
    fn uniform_squircle_matches_lisse() {
        let outline = SmoothCorner::new()
            .radius(px(16.0))
            .smoothing(FIGMA_SMOOTHING)
            .outline(200.0, 100.0)
            .unwrap();
        let pts = endpoints(&outline);
        // M 25.6 0 · L 174.4 0 · c→(191.2638, 1.7439) · a→(198.2561, 8.7362)
        // c→(200, 25.6) · L 200 25.6 · L 200 74.4 · c→(198.2561, 91.2638)
        // a→(191.2638, 98.2562) · c→(174.4, 100) · L 174.4 100 · L 25.6 100 ·
        // c→(8.7362, 98.2561) · a→(1.7439, 91.2638) · c→(0, 74.4) ·
        // L 0 74.4 · L 0 25.6 · c→(1.7439, 16.8638) · a→(8.7362, 1.7439) ·
        // c→(25.6, 0) · Z
        assert_point_near(pts[0], 25.6, 0.0);
        assert_point_near(pts[1], 174.4, 0.0);
        assert_point_near(pts[2], 191.2638, 1.7439);
        assert_point_near(pts[3], 198.2561, 8.7362);
        assert_point_near(pts[4], 200.0, 25.6);
        assert_point_near(pts[6], 200.0, 74.4);
        assert_point_near(pts[9], 174.4, 100.0);
        assert_point_near(pts[11], 25.6, 100.0);
        assert_point_near(pts[14], 0.0, 74.4);
        assert_point_near(pts[16], 0.0, 25.6);
        assert_point_near(pts[19], 25.6, 0.0);
    }

    /// Reference: generatePath(200, 100, {radius: 50, smoothing: 0.65}) — a
    /// full capsule emits two cap segments.
    #[test]
    fn capsule_matches_lisse() {
        let outline = SmoothCorner::new()
            .radius(px(50.0))
            .smoothing(APPLE_SMOOTHING)
            .outline(200.0, 100.0)
            .unwrap();
        let pts = endpoints(&outline);
        // M 82.5 0 · L 117.5 0 · cap: c→(174.4311, 6.3752) a→(200, 50)
        // a→(174.4311, 93.6248) c→(117.5, 100) · L 82.5 100 ·
        // c→(25.5689, 93.6248) a→(0, 50) a→(25.5689, 6.3752) c→(82.5, 0) · Z
        assert_point_near(pts[0], 82.5, 0.0);
        assert_point_near(pts[1], 117.5, 0.0);
        assert_point_near(pts[2], 174.4311, 6.3752);
        assert_point_near(pts[3], 200.0, 50.0);
        assert_point_near(pts[4], 174.4311, 93.6248);
        assert_point_near(pts[5], 117.5, 100.0);
        assert_point_near(pts[6], 82.5, 100.0);
        assert_point_near(pts[8], 0.0, 50.0);
        assert_point_near(pts[10], 82.5, 0.0);
    }

    /// Reference: generatePath(100, 40, {topLeft: 20, bottomLeft: 20,
    /// smoothing: 0.65}) — only the rounded end caps.
    #[test]
    fn half_pill_matches_lisse() {
        let radii = Corners {
            top_left: px(20.0),
            top_right: px(0.0),
            bottom_right: px(0.0),
            bottom_left: px(20.0),
        };
        let outline = SmoothCorner::new()
            .radii(radii)
            .smoothing(APPLE_SMOOTHING)
            .outline(100.0, 40.0)
            .unwrap();
        let pts = endpoints(&outline);
        // M 33 0 · L 100 0 · L 100 0 · L 100 40 · L 33 40 · left cap:
        // c→(10.2276, 37.4499) a→(0, 20) a→(10.2276, 2.5501) c→(33, 0) · Z
        assert_point_near(pts[0], 33.0, 0.0);
        assert_point_near(pts[3], 100.0, 40.0);
        assert_point_near(pts[4], 33.0, 40.0);
        assert_point_near(pts[5], 10.2276, 37.4499);
        assert_point_near(pts[6], 0.0, 20.0);
        assert_point_near(pts[7], 10.2276, 2.5501);
        assert_point_near(pts[8], 33.0, 0.0);
    }

    /// Reference: generatePath(120, 80, {radius: 35, smoothing: 0.8}) — the
    /// blend band between classic squircle and capsule.
    #[test]
    fn blend_band_matches_lisse() {
        let outline = SmoothCorner::new()
            .radius(px(35.0))
            .smoothing(0.8)
            .outline(120.0, 80.0)
            .unwrap();
        let pts = endpoints(&outline);
        // M 60 0 · L 60 0 · c→(103.6211, 5.3647) a→(119.7799, 31.0813)
        // c→(120, 40) · L 120 40 · c→(119.7799, 48.9188) a→(103.6211, 74.6353)
        // c→(60, 80) · L 60 80 · c→(16.3789, 74.6353) a→(0.2201, 48.9188)
        // c→(0, 40) · L 0 40 · c→(0.2201, 31.0813) a→(16.3789, 5.3647)
        // c→(60, 0) · Z
        assert_point_near(pts[0], 60.0, 0.0);
        assert_point_near(pts[2], 103.6211, 5.3647);
        assert_point_near(pts[3], 119.7799, 31.0813);
        assert_point_near(pts[4], 120.0, 40.0);
        assert_point_near(pts[7], 103.6211, 74.6353);
        assert_point_near(pts[8], 60.0, 80.0);
        assert_point_near(pts[11], 0.2201, 48.9188);
        assert_point_near(pts[12], 0.0, 40.0);
        assert_point_near(pts[15], 16.3789, 5.3647);
        assert_point_near(pts[16], 60.0, 0.0);
    }

    /// Reference: generatePath(200, 100, {topLeft: 8, topRight: 24,
    /// bottomRight: 40, bottomLeft: 0, smoothing: 0.6}) — contested edges
    /// shrink the larger corner's budget first.
    #[test]
    fn contested_corners_match_lisse() {
        let radii = Corners {
            top_left: px(8.0),
            top_right: px(24.0),
            bottom_right: px(40.0),
            bottom_left: px(0.0),
        };
        let outline = SmoothCorner::new()
            .radii(radii)
            .smoothing(FIGMA_SMOOTHING)
            .outline(200.0, 100.0)
            .unwrap();
        let pts = endpoints(&outline);
        // M 12.8 0 · L 162.5 0 · c→(186.8958, 2.6158) a→(197.3842, 13.1042)
        // c→(200, 37.5) · L 200 62.5 · L 200 37.5 · c→(195.6403, 78.1596)
        // a→(178.1597, 95.6403) · c→(137.5, 100) · L 200 100 · L 0 100 ·
        // L 0 87.2 · L 0 12.8 · c→(0.8719, 4.3681) a→(4.368, 0.872)
        // c→(12.8, 0) · Z
        assert_point_near(pts[0], 12.8, 0.0);
        assert_point_near(pts[1], 162.5, 0.0);
        assert_point_near(pts[2], 186.8958, 2.6158);
        assert_point_near(pts[4], 200.0, 37.5);
        assert_point_near(pts[5], 200.0, 62.5);
        assert_point_near(pts[6], 200.0, 37.5);
        assert_point_near(pts[9], 137.5, 100.0);
        // BL radius 0 → no corner emitted.
        assert_point_near(pts[12], 0.0, 87.2);
        assert_point_near(pts[13], 0.0, 12.8);
        assert_point_near(pts[16], 12.8, 0.0);
    }

    /// smoothing = 0 degenerates to quarter-circle arcs — the same outline
    /// `.rounded()` paints.
    #[test]
    fn zero_smoothing_is_a_rounded_rect() {
        let outline = SmoothCorner::new()
            .radius(px(12.0))
            .smoothing(0.0)
            .outline(100.0, 60.0)
            .unwrap();
        let pts = endpoints(&outline);
        assert_point_near(pts[0], 12.0, 0.0);
        assert_point_near(pts[1], 88.0, 0.0);
        assert_point_near(pts[2], 88.0, 0.0); // degenerate entry cubic
        assert_point_near(pts[3], 100.0, 12.0); // quarter arc
        assert_point_near(pts[4], 100.0, 12.0); // degenerate exit cubic
        assert_point_near(pts[6], 100.0, 48.0);
    }

    #[test]
    fn built_path_covers_bounds() {
        let spec = SmoothCorner::new().radius(px(20.0));
        let bounds = Bounds {
            origin: point(px(10.0), px(20.0)),
            size: gpui::size(px(200.0), px(100.0)),
        };
        let path = spec.path(bounds).unwrap();
        let (px_, py_) = (
            f32::from(path.bounds.origin.x),
            f32::from(path.bounds.origin.y),
        );
        let (pw, ph) = (
            f32::from(path.bounds.size.width),
            f32::from(path.bounds.size.height),
        );
        assert!(px_ <= 10.001);
        assert!(py_ <= 20.001);
        assert!(px_ + pw >= 209.999, "path right edge {}", px_ + pw);
        assert!(py_ + ph >= 119.999);
    }
}
