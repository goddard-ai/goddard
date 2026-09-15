//! The animated "still working" mark.
//!
//! The layers under `assets/icons/goddard-thinking-*.svg` split the website's
//! `goddard-thinking.svg` into one asset per animated group. GPUI renders
//! SVGs as static alpha masks — SMIL and CSS animations don't run — so the
//! source's sway, head toss, ear twitches, and blink are replayed here on the
//! shared pulse clock as per-layer `Transformation`s.
//!
//! The source's blink slides a pair of lid rects over each eye, clipped to
//! the eye shape. The eyes are holes in the face path and everything is one
//! color, so covering an eye reads identically to painting it: the eyes layer
//! fades in on the lids' timing instead. Under reduce-motion the clock holds
//! every track at its first frame — the mark sits still, eyes open.

use gpui::{AnyElement, Hsla, Svg, Transformation, div, point, prelude::*, px, radians, svg};

use crate::ui::motion;

/// Rendered size of the mark. The source viewBox is `-10 -10 160 125`; its
/// margin leaves room for the sway, so the drawn face fills ~84% of the box.
const WIDTH: f32 = 28.0;
const HEIGHT: f32 = WIDTH * 125.0 / 160.0;
const CENTER: (f32, f32) = (WIDTH / 2.0, HEIGHT / 2.0);

/// Element px per viewBox unit; the viewBox's (-10, -10) min puts user-space
/// point `u` at `(u + 10) * UNITS_TO_PX`.
const UNITS_TO_PX: f32 = WIDTH / 160.0;

/// The `sway` group's gentle oscillation — `rotate` about (103, 26), eased.
const SWAY: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.25, 1.8),
    (0.5, 0.0),
    (0.75, -1.8),
    (1.0, 0.0),
];
const SWAY_PERIOD: f32 = 6.0;
const SWAY_PIVOT: (f32, f32) = (103.0, 26.0);

/// The `rock` group's quicker nod — `rotate` about (114, 20), eased.
const ROCK: &[(f32, f32)] = &[(0.0, 0.0), (0.5, 3.2), (1.0, 0.0)];
const ROCK_PERIOD: f32 = 2.0;

/// The `head` group's occasional toss — `rotate` about (114, 20), linear.
const HEAD: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.01, 3.2),
    (0.0225, 3.2),
    (0.035, 0.0),
    (0.25, 0.0),
    (0.26, 3.2),
    (0.2725, 3.2),
    (0.285, 0.0),
    (0.5, 0.0),
    (0.51, 3.2),
    (0.5225, 3.2),
    (0.535, 0.0),
    (0.75, 0.0),
    (0.76, -3.2),
    (0.7725, -3.2),
    (0.785, 0.0),
    (0.8, 0.0),
    (0.81, -3.2),
    (0.8225, -3.2),
    (0.835, 0.0),
    (1.0, 0.0),
];
const HEAD_PERIOD: f32 = 34.0;
const HEAD_PIVOT: (f32, f32) = (114.0, 20.0);

/// The `ear` group's twitch — `rotate` about (112, 20), linear.
const EAR: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.0117, -6.0),
    (0.0233, 2.0),
    (0.0367, 0.0),
    (0.25, 0.0),
    (0.2617, -6.0),
    (0.2733, 2.0),
    (0.2867, 0.0),
    (0.5, 0.0),
    (0.5117, -6.0),
    (0.5233, 2.0),
    (0.5367, 0.0),
    (0.75, 0.0),
    (0.7617, -6.0),
    (0.7733, 2.0),
    (0.7867, 0.0),
    (0.8033, 0.0),
    (0.815, -6.0),
    (0.8267, 2.0),
    (0.84, 0.0),
    (1.0, 0.0),
];
const EAR_PIVOT: (f32, f32) = (112.0, 20.0);

/// The `other-ear-anim` group's counter-twitch — `rotate` about (58, 6).
const EAR_BACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.0117, 6.0),
    (0.0233, -2.0),
    (0.0367, 0.0),
    (0.25, 0.0),
    (0.2617, 6.0),
    (0.2733, -2.0),
    (0.2867, 0.0),
    (0.5, 0.0),
    (0.5117, 6.0),
    (0.5233, -2.0),
    (0.5367, 0.0),
    (0.75, 0.0),
    (0.7617, 6.0),
    (0.7733, -2.0),
    (0.7867, 0.0),
    (0.8033, 0.0),
    (0.815, 6.0),
    (0.8267, -2.0),
    (0.84, 0.0),
    (1.0, 0.0),
];
const EAR_BACK_PIVOT: (f32, f32) = (58.0, 6.0);
const EAR_PERIOD: f32 = 18.0;

/// Lid coverage over the 20s blink cycle: 0 open, 1 closed. The source's four
/// single blinks plus the double-blink near the end, on its `ease-in-out`.
const BLINK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.184, 0.0),
    (0.188, 1.0),
    (0.1895, 1.0),
    (0.1945, 0.0),
    (0.384, 0.0),
    (0.388, 1.0),
    (0.3895, 1.0),
    (0.3945, 0.0),
    (0.584, 0.0),
    (0.588, 1.0),
    (0.5895, 1.0),
    (0.5945, 0.0),
    (0.784, 0.0),
    (0.788, 1.0),
    (0.7895, 1.0),
    (0.7945, 0.0),
    (0.949, 0.0),
    (0.953, 1.0),
    (0.9545, 1.0),
    (0.9595, 0.0),
    (0.9695, 0.0),
    (0.9735, 1.0),
    (0.975, 1.0),
    (0.98, 0.0),
    (1.0, 0.0),
];
const BLINK_PERIOD: f32 = 20.0;

/// The animated mark, tinted like the row's text. Mounted for the whole turn,
/// it stays on the pulse clock's ~30 fps cadence: the blink holds shut for
/// only ~100 ms, and a coarser stride could drop whole blinks between ticks.
pub fn goddard_thinking(color: Hsla) -> AnyElement {
    motion::pulse_elapsed(move |elapsed| {
        let to_px = |u: f32| (u + 10.0) * UNITS_TO_PX;
        let phase = |period: f32| (elapsed / period).fract();

        let sway = track(phase(SWAY_PERIOD), SWAY, true);
        // Rock and the head toss share their pivot, so they compose by
        // summing angles.
        let rock = track(phase(ROCK_PERIOD), ROCK, true) + track(phase(HEAD_PERIOD), HEAD, false);
        let ear = track(phase(EAR_PERIOD), EAR, false);
        let ear_back = track(phase(EAR_PERIOD), EAR_BACK, false);
        let blink = track(phase(BLINK_PERIOD), BLINK, true);

        let head = Affine::rotate_about((to_px(SWAY_PIVOT.0), to_px(SWAY_PIVOT.1)), sway).then(
            Affine::rotate_about((to_px(HEAD_PIVOT.0), to_px(HEAD_PIVOT.1)), rock),
        );
        let ear = head.then(Affine::rotate_about(
            (to_px(EAR_PIVOT.0), to_px(EAR_PIVOT.1)),
            ear,
        ));
        let ear_back = head.then(Affine::rotate_about(
            (to_px(EAR_BACK_PIVOT.0), to_px(EAR_BACK_PIVOT.1)),
            ear_back,
        ));

        div()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .flex_none()
            .relative()
            .child(layer("icons/goddard-thinking-face.svg", head, color))
            .child(layer("icons/goddard-thinking-eyes.svg", head, color).opacity(blink))
            .child(layer(
                "icons/goddard-thinking-ear-back.svg",
                ear_back,
                color,
            ))
            .child(layer("icons/goddard-thinking-ear.svg", ear, color))
            .into_any_element()
    })
    .into_any_element()
}

/// One full-canvas layer, stacked on its siblings and carrying the transform
/// its nested groups would apply.
fn layer(path: &'static str, transform: Affine, color: Hsla) -> Svg {
    svg()
        .path(path)
        .absolute()
        .top_0()
        .left_0()
        .w(px(WIDTH))
        .h(px(HEIGHT))
        .text_color(color)
        .with_transformation(transform.svg_transformation())
}

/// Value of a `(keyTime, value)` track at `phase ∈ [0,1)` — the keyframes the
/// source SVG declares per group. `eased` applies its cubic-bezier
/// (0.42, 0, 0.58, 1) timing to the segment; the twitch tracks run linear.
fn track(phase: f32, keys: &[(f32, f32)], eased: bool) -> f32 {
    for segment in keys.windows(2) {
        let (t0, v0) = segment[0];
        let (t1, v1) = segment[1];
        if phase <= t1 {
            let progress = ((phase - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let progress = if eased {
                ease_in_out(progress)
            } else {
                progress
            };
            return v0 + (v1 - v0) * progress;
        }
    }
    keys.last().map_or(0.0, |key| key.1)
}

/// cubic-bezier(0.42, 0, 0.58, 1) — CSS `ease-in-out`.
fn ease_in_out(x: f32) -> f32 {
    // Invert the curve's x(t) by bisection, then read y(t).
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..20 {
        let t = (lo + hi) * 0.5;
        let xt = 3.0 * (1.0 - t) * t * (0.42 * (1.0 - t) + 0.58 * t) + t.powi(3);
        if xt < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    let t = (lo + hi) * 0.5;
    t * t * (3.0 - 2.0 * t)
}

/// A rigid 2D transform in element pixels —
/// `(x, y) ↦ (xx·x + xy·y + tx, yx·x + yy·y + ty)`. Only rotations occur
/// here, so the linear part stays orthonormal.
#[derive(Clone, Copy)]
struct Affine {
    xx: f32,
    xy: f32,
    yx: f32,
    yy: f32,
    tx: f32,
    ty: f32,
}

impl Affine {
    /// `degrees` of rotation about `pivot`, like SVG `rotate(a, px, py)`.
    fn rotate_about(pivot: (f32, f32), degrees: f32) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (px, py) = pivot;
        Self {
            xx: cos,
            xy: -sin,
            yx: sin,
            yy: cos,
            tx: px - px * cos + py * sin,
            ty: py - px * sin - py * cos,
        }
    }

    /// `self ∘ other`: `other` applies first, like an inner SVG group nested
    /// under the group carrying `self`.
    fn then(self, other: Affine) -> Self {
        Self {
            xx: self.xx * other.xx + self.xy * other.yx,
            xy: self.xx * other.xy + self.xy * other.yy,
            yx: self.yx * other.xx + self.yy * other.yx,
            yy: self.yx * other.xy + self.yy * other.yy,
            tx: self.xx * other.tx + self.xy * other.ty + self.tx,
            ty: self.yx * other.tx + self.yy * other.ty + self.ty,
        }
    }

    /// The `Transformation` painting an svg element with this transform.
    /// GPUI rotates about the element's center and translates after, so the
    /// difference between the implied center and the real one folds into the
    /// translation.
    fn svg_transformation(self) -> Transformation {
        let theta = self.yx.atan2(self.xx);
        let (sin, cos) = theta.sin_cos();
        Transformation::default()
            .with_rotation(radians(theta))
            .with_translation(point(
                px(self.tx + CENTER.0 * (cos - 1.0) - CENTER.1 * sin),
                px(self.ty + CENTER.0 * sin + CENTER.1 * (cos - 1.0)),
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pivot_survives_its_own_rotation() {
        let m = Affine::rotate_about((7.0, 3.0), 42.0);
        let x = m.xx * 7.0 + m.xy * 3.0 + m.tx;
        let y = m.yx * 7.0 + m.yy * 3.0 + m.ty;
        assert!((x - 7.0).abs() < 1e-4 && (y - 3.0).abs() < 1e-4);
    }

    #[test]
    fn same_pivot_rotations_sum() {
        let pivot = (114.0, 20.0);
        let composed = Affine::rotate_about(pivot, 3.2).then(Affine::rotate_about(pivot, -1.8));
        let summed = Affine::rotate_about(pivot, 1.4);
        for (x, y) in [(0.0, 0.0), (50.0, 80.0), (112.0, 20.0)] {
            let a = (
                composed.xx * x + composed.xy * y + composed.tx,
                composed.yx * x + composed.yy * y + composed.ty,
            );
            let b = (
                summed.xx * x + summed.xy * y + summed.tx,
                summed.yx * x + summed.yy * y + summed.ty,
            );
            assert!((a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3);
        }
    }

    #[test]
    fn a_track_holds_and_interpolates() {
        let keys = &[(0.0, 0.0), (0.5, 4.0), (1.0, 0.0)];
        assert_eq!(track(0.0, keys, false), 0.0);
        assert_eq!(track(0.5, keys, false), 4.0);
        assert_eq!(track(0.25, keys, false), 2.0);
        assert!((track(0.999, keys, false) - 0.008).abs() < 1e-4);
    }
}
