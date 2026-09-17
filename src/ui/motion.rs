//! Shared pulse clock for the repeating loaders.
//!
//! A repeating `with_animation` element requests a redraw every display frame
//! for as long as it is mounted — one working row pinned the whole window at
//! 120 Hz on a ProMotion panel. Loaders instead read their phase from one
//! shared clock: it ticks at up to 60 fps, notifies only views that painted a
//! loader recently, and parks itself once the last lease lapses, so a window
//! with no loader mounted schedules nothing at all. Every loader shares one
//! epoch, keeping multi-instance loaders phase-locked.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationElement, AnimationExt, AnyElement, App, Bounds, ContentMask, Element,
    ElementId, EntityId, Global, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Point, RenderOnce, Styled, Svg, Transformation, Window, ease_out_quint, percentage,
    point, px, size,
};

/// Repeat-tick interval, rounded up so spinner ticks never exceed 60 fps.
const PULSE_TICK: Duration = Duration::from_nanos(16_666_667);

/// Non-spinning pulses retain their ~30 fps cadence on the faster clock.
const PULSE_STRIDE: u32 = 2;

/// How long a view stays on the tick list after it last painted a loader. One
/// lease outlives a few missed frames; an unmounted loader stops renewing and
/// its view drops off, letting the clock park.
const PULSE_LEASE: Duration = Duration::from_millis(300);

/// The rotating `loader-circle` spinners' period.
const SPINNER_PERIOD: Duration = Duration::from_millis(900);

/// The `spin_slow` loaders' period: beside a session title or status row the
/// shared period reads as a chase, so one orbit takes twice as long.
const SPINNER_PERIOD_SLOW: Duration = Duration::from_millis(1_800);

struct Lease {
    until: Instant,
    /// Notify this view every `stride`-th tick. A view's whole subtree
    /// rebuilds per notify, so a loader on an expensive surface can trade
    /// animation granularity for a cheaper cadence.
    stride: u32,
}

struct PulseClock {
    epoch: Instant,
    leases: HashMap<EntityId, Lease>,
    ticks: u64,
    running: bool,
}

impl Global for PulseClock {}

impl Default for PulseClock {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            leases: HashMap::new(),
            ticks: 0,
            running: false,
        }
    }
}

/// Keep `view` re-rendering at ~30 fps until the lease lapses. A caller
/// that stops leasing stops being notified, and the clock parks once no
/// leases remain — quiescence needs no unsubscribe step.
pub fn pulse_lease(view: EntityId, cx: &mut App) {
    pulse_lease_with_stride(view, PULSE_STRIDE, cx);
}

/// [`pulse_lease`] at half rate (~15 fps), for animations whose view
/// is expensive to rebuild and whose motion survives the coarser step — a
/// notify re-renders the view's whole subtree, so cadence is priced per
/// tick, not per animation.
pub fn pulse_lease_slow(view: EntityId, cx: &mut App) {
    pulse_lease_with_stride(view, PULSE_STRIDE * 2, cx);
}

fn pulse_lease_with_stride(view: EntityId, stride: u32, cx: &mut App) {
    let clock = cx.default_global::<PulseClock>();
    let until = Instant::now() + PULSE_LEASE;
    // A view hosting both a full-rate and a strided loader keeps full rate.
    clock
        .leases
        .entry(view)
        .and_modify(|lease| {
            lease.until = until;
            lease.stride = lease.stride.min(stride);
        })
        .or_insert(Lease { until, stride });
    if clock.running {
        return;
    }
    clock.running = true;
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(PULSE_TICK).await;
            let parked = cx.update(|cx| {
                let clock = cx.default_global::<PulseClock>();
                let now = Instant::now();
                clock.ticks += 1;
                let ticks = clock.ticks;
                clock.leases.retain(|_, lease| lease.until > now);
                if clock.leases.is_empty() {
                    clock.running = false;
                    return true;
                }
                let due = clock
                    .leases
                    .iter_mut()
                    .filter(|(_, lease)| ticks % lease.stride.max(1) as u64 == 0)
                    .map(|(view, lease)| {
                        // Strides re-establish on the render this notify
                        // triggers; without the reset, one full-rate lease
                        // would drag its view's cadence down permanently.
                        lease.stride = u32::MAX;
                        *view
                    })
                    .collect::<Vec<_>>();
                for view in due {
                    cx.notify(view);
                }
                false
            });
            if parked {
                break;
            }
        }
    })
    .detach();
}

/// Seconds since the shared clock's epoch, plus a lease keeping `view`
/// re-rendering while its loader stays mounted. Under reduce-motion this is a
/// constant 0 — every animation's first frame, matching what a repeating
/// `with_animation` held — and nothing is scheduled.
fn pulse_elapsed_secs(stride: u32, view: EntityId, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.0;
    }
    let clock = cx.default_global::<PulseClock>();
    let elapsed = clock.epoch.elapsed().as_secs_f32();
    pulse_lease_with_stride(view, stride, cx);
    elapsed
}

/// A loader element styled from the shared clock's phase. Resolving the phase
/// is deferred to render, where the owning view is known, so call sites need
/// neither a `Window` nor an `EntityId` in scope.
pub fn pulse(period: Duration, render: impl FnOnce(f32) -> AnyElement + 'static) -> Pulse {
    let period = period.as_secs_f32();
    pulse_elapsed(move |elapsed| render((elapsed / period).fract()))
}

/// A loader element styled from the shared clock's elapsed seconds — for
/// animations layering several periods that a single phase can't express.
pub fn pulse_elapsed(render: impl FnOnce(f32) -> AnyElement + 'static) -> Pulse {
    Pulse {
        stride: PULSE_STRIDE,
        render: Box::new(render),
    }
}

/// A rotating loader icon riding the shared clock at up to 60 fps.
pub fn spin(icon: Svg) -> AnyElement {
    spin_with(icon, SPINNER_PERIOD, 1)
}

/// A rotating loader at every second tick (~30 fps) on a slower period.
/// For loaders on expensive surfaces: the
/// sidebar rebuilds its whole subtree per notify, and a session row's working
/// spinner is not worth pricing that at full rate.
pub fn spin_slow(icon: Svg) -> AnyElement {
    spin_with(icon, SPINNER_PERIOD_SLOW, 2)
}

fn spin_with(icon: Svg, period: Duration, stride: u32) -> AnyElement {
    let mut pulse = pulse(period, move |phase| {
        icon.with_transformation(Transformation::rotate(percentage(phase)))
            .into_any_element()
    });
    pulse.stride = stride;
    pulse.into_any_element()
}

#[derive(IntoElement)]
pub struct Pulse {
    stride: u32,
    render: Box<dyn FnOnce(f32) -> AnyElement>,
}

impl Pulse {
    /// Tick every `stride`-th ~30 fps pulse instead of every one. A view's whole
    /// subtree rebuilds per notify — the pane ticks at the fastest of its
    /// lessees — so a loader mounted for a whole turn on an expensive
    /// surface should ride the coarser cadence.
    pub fn every(mut self, stride: u32) -> Self {
        self.stride = stride.max(1).saturating_mul(PULSE_STRIDE);
        self
    }
}

impl RenderOnce for Pulse {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let elapsed = pulse_elapsed_secs(self.stride, window.current_view(), cx);
        (self.render)(elapsed)
    }
}

/// How long a side panel takes to slide open or shut. 200ms is long enough to
/// read as travel rather than a jump cut, short enough that the layout is
/// settled before the pointer arrives anywhere else.
pub const PANEL_SLIDE: Duration = Duration::from_millis(200);

/// A one-shot width slide, evaluated from `render` instead of wrapped around
/// an element.
///
/// `with_animation` cannot drive this. The width feeds the flex layout of the
/// panel's *siblings* — the transcript column takes whatever the panels leave
/// — and gpui keys an animation element by its element-id path, so a wrapper
/// that remounts would replay the slide from zero. Evaluating by hand keeps
/// the element tree's shape constant: a finished or dropped tween is exactly
/// the steady state.
#[derive(Clone, Copy, Debug)]
pub struct WidthTween {
    from: f32,
    started: Instant,
}

impl WidthTween {
    /// Start a slide from the width the panel currently occupies, so a toggle
    /// mid-slide reverses from where the edge actually is instead of jumping
    /// back to the far end.
    pub fn new(from: f32) -> Self {
        Self {
            from,
            started: Instant::now(),
        }
    }

    /// Eased width on the way to `target`, or `None` once the slide is over —
    /// the caller then drops the tween and reads `target` directly, which is
    /// also what retires a closed panel from the element tree.
    pub fn width_toward(&self, target: f32) -> Option<f32> {
        width_at(self.from, target, self.started.elapsed())
    }
}

fn width_at(from: f32, target: f32, elapsed: Duration) -> Option<f32> {
    let progress = elapsed.as_secs_f32() / PANEL_SLIDE.as_secs_f32();
    (progress < 1.0).then(|| from + (target - from) * ease_out_quint()(progress.max(0.0)))
}

/// How long a menu or popover takes to grow in from its anchor — short enough
/// to read as instant, long enough to see where it came from.
pub const SURFACE_ENTER: Duration = Duration::from_millis(140);

/// The window-modal equivalent: the card is larger and grows from its own
/// center, so the reveal runs a touch longer.
pub const MODAL_ENTER: Duration = Duration::from_millis(160);

/// The scrim behind a modal fades on its own, quicker clock.
pub const SCRIM_ENTER: Duration = Duration::from_millis(120);

/// How far a surface drifts toward its anchor while the reveal opens, as a
/// fraction of the anchor's distance from the card's center — a few px for a
/// menu, nothing for a card anchored at its own center.
const ENTER_DRIFT: f32 = 0.05;

/// The extra downward settle for a center-anchored (modal) entrance.
const MODAL_SETTLE: Pixels = px(5.0);

/// How far past a surface's bounds its drop shadow can reach — `shadow_xl`'s
/// offset plus a 3× blur tail. The reveal targets the dilated bounds so the
/// shadow lands inside the clip and is revealed with the card; a clip that
/// stops at the card's edge hides it until the last frame, where it pops in.
const SHADOW_BLEED: Pixels = px(100.0);

/// A one-shot entrance for a floating surface — menu, popover, or dialog.
///
/// GPUI transforms only SVG subtrees, so the "grow" is painted as a clip: the
/// visible region eases from a zero-size rect at the anchor out to the
/// shadow-dilated bounds while the child drifts a few px toward the anchor. At
/// these durations it reads as a small scale. Drive it with `with_animation`;
/// under reduce-motion the oneshot delta is 1 and it renders settled.
///
/// The child stays opaque through the reveal rather than fading. GPUI applies
/// opacity per primitive — there is no group compositing — and the drop-shadow
/// silhouette fills the card's interior beneath the fill, so a translucent
/// card shows it as a dark cast that vanishes on the last frame: a dark-to-
/// light sweep on light themes. The opaque card occludes the silhouette for
/// the whole animation and the clip reveals card and shadow together.
pub struct SurfaceReveal<E> {
    child: Option<E>,
    /// The window-space point the surface grows out of — the click for a
    /// context menu, the trigger's attach corner for a dropdown. `None`
    /// grows from the element's own center while settling downward.
    anchor: Option<Point<Pixels>>,
    progress: f32,
}

impl<E: Styled + IntoElement + 'static> SurfaceReveal<E> {
    /// The eased animation delta for this frame, supplied by `with_animation`.
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = progress;
        self
    }
}

/// The shared entrance for a surface anchored to a point: the reveal grows
/// out of `anchor` — a context menu's click point, a dropdown's attach
/// corner on its trigger.
pub fn surface_enter<E>(
    id: impl Into<ElementId>,
    child: E,
    anchor: Point<Pixels>,
) -> AnimationElement<SurfaceReveal<E>>
where
    E: Styled + IntoElement + 'static,
{
    SurfaceReveal {
        child: Some(child),
        anchor: Some(anchor),
        progress: 0.0,
    }
    .with_animation(
        id,
        Animation::new(SURFACE_ENTER).with_easing(ease_out_quint()),
        |element, delta| element.progress(delta),
    )
}

/// The window-modal entrance: grow from the card's own center, settle a few
/// px downward.
pub fn modal_enter<E>(id: impl Into<ElementId>, child: E) -> AnimationElement<SurfaceReveal<E>>
where
    E: Styled + IntoElement + 'static,
{
    SurfaceReveal {
        child: Some(child),
        anchor: None,
        progress: 0.0,
    }
    .with_animation(
        id,
        Animation::new(MODAL_ENTER).with_easing(ease_out_quint()),
        |element, delta| element.progress(delta),
    )
}

/// A bare one-shot fade, for the scrim under a modal.
pub fn fade_in<E>(id: impl Into<ElementId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(id, Animation::new(SCRIM_ENTER), |element, delta| {
        element.opacity(delta)
    })
}

/// The clip rect for `progress`: a zero-size rect at `anchor` growing out to
/// `bounds`, keeping the anchor fixed so near-anchor content appears first.
fn reveal_bounds(anchor: Point<Pixels>, bounds: Bounds<Pixels>, progress: f32) -> Bounds<Pixels> {
    Bounds::new(
        anchor + (bounds.origin - anchor) * progress,
        size(bounds.size.width * progress, bounds.size.height * progress),
    )
}

fn clamp_into(position: Point<Pixels>, bounds: Bounds<Pixels>) -> Point<Pixels> {
    Point::new(
        position.x.max(bounds.left()).min(bounds.right()),
        position.y.max(bounds.top()).min(bounds.bottom()),
    )
}

impl<E> Element for SurfaceReveal<E>
where
    E: Styled + IntoElement + 'static,
{
    type RequestLayoutState = AnyElement;
    type PrepaintState = Option<ContentMask<Pixels>>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = self
            .child
            .take()
            .expect("request_layout runs once per frame")
            .into_any_element();
        let layout_id = child.request_layout(window, cx);
        (layout_id, child)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let progress = self.progress.clamp(0.0, 1.0);
        if progress >= 1.0 {
            child.prepaint(window, cx);
            return None;
        }
        let anchor = self
            .anchor
            .map(|anchor| clamp_into(anchor, bounds))
            .unwrap_or_else(|| bounds.center());
        // Drift toward the anchor as the clip opens — the arrival direction a
        // real scale would imply. A center-anchored card has no direction, so
        // it only settles downward.
        let drift = if self.anchor.is_some() {
            (anchor - bounds.center()) * ENTER_DRIFT * (1.0 - progress)
        } else {
            point(px(0.0), -MODAL_SETTLE * (1.0 - progress))
        };
        // Reveal toward the shadow-dilated bounds: scaling the dilation with
        // progress keeps the grow-from-anchor read while keeping the shadow
        // inside the clip, so it sweeps out with the card instead of popping
        // in on the last frame.
        let mask = ContentMask {
            bounds: reveal_bounds(anchor, bounds.dilate(SHADOW_BLEED), progress),
        };
        window.with_element_offset(drift, |window| {
            window.with_content_mask(Some(mask), |window| child.prepaint(window, cx));
        });
        Some(mask)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        mask: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(*mask, |window| child.paint(window, cx));
    }
}

impl<E> IntoElement for SurfaceReveal<E>
where
    E: Styled + IntoElement + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slide_eases_out_and_then_retires() {
        let start = width_at(0.0, 260.0, Duration::ZERO).expect("a fresh slide is in flight");
        assert!(start.abs() < 0.01, "the slide opens from its start width");

        let half = width_at(0.0, 260.0, PANEL_SLIDE / 2).expect("halfway is in flight");
        assert!(
            half > 130.0,
            "ease-out covers most of the distance early, got {half}"
        );

        assert_eq!(
            width_at(0.0, 260.0, PANEL_SLIDE),
            None,
            "an elapsed slide reports no width so the caller settles on the target"
        );
    }

    #[test]
    fn a_slide_reversed_mid_flight_leaves_from_where_it_is() {
        let interrupted = width_at(0.0, 260.0, PANEL_SLIDE / 4).expect("in flight");
        let reversed = width_at(interrupted, 0.0, Duration::ZERO).expect("in flight");
        assert!(
            (reversed - interrupted).abs() < 0.01,
            "the reversed slide starts at the interrupted width"
        );
    }

    #[test]
    fn a_reveal_grows_from_the_anchor_out_to_the_full_bounds() {
        let bounds = Bounds::new(point(px(100.0), px(50.0)), size(px(200.0), px(100.0)));
        let anchor = point(px(100.0), px(50.0));

        let start = reveal_bounds(anchor, bounds, 0.0);
        assert_eq!(start.size, size(px(0.0), px(0.0)));
        assert_eq!(
            start.origin, anchor,
            "a fresh reveal is a point at the anchor"
        );

        let half = reveal_bounds(anchor, bounds, 0.5);
        assert_eq!(half.size, size(px(100.0), px(50.0)));
        assert_eq!(half.origin, anchor, "the corner anchor stays pinned");

        assert_eq!(reveal_bounds(anchor, bounds, 1.0), bounds);
    }

    #[test]
    fn a_reveal_from_center_grows_symmetrically() {
        let bounds = Bounds::new(point(px(100.0), px(50.0)), size(px(200.0), px(100.0)));
        let half = reveal_bounds(bounds.center(), bounds, 0.5);
        assert_eq!(half.origin, point(px(150.0), px(75.0)));
        assert_eq!(half.size, size(px(100.0), px(50.0)));
    }

    #[test]
    fn an_out_of_bounds_anchor_is_clamped_into_the_card() {
        // A context menu snapped back inside the window leaves the click point
        // outside the card; the reveal grows from the nearest edge instead.
        let bounds = Bounds::new(point(px(100.0), px(50.0)), size(px(200.0), px(100.0)));
        let anchor = clamp_into(point(px(400.0), px(70.0)), bounds);
        assert_eq!(anchor, point(px(300.0), px(70.0)));
    }
}
