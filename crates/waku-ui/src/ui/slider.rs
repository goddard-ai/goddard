//! A horizontal slider for a 0–`max` value.
//!
//! Painted as quads from the canvas's own bounds with the pointer handlers
//! registered during paint — the same single-element pattern as the overlay
//! scrollbar. The owner keeps the [`SliderState`] so an in-flight drag
//! survives the repaints each pointer move requests.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, DispatchPhase, Div, ElementId, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Stateful, Window, canvas, div, point, prelude::*, px,
    quad, size,
};

use crate::theme::Theme;

/// Track thickness; the hitbox is the control's full height.
const TRACK_HEIGHT: f32 = 4.0;
const THUMB_SIZE: f32 = 13.0;
/// Arrow keys step the value 5% of the range so a focused slider stays usable
/// without a long press.
const KEY_STEP: f32 = 0.05;

/// Cross-frame slider state. The owner holds one per slider.
#[derive(Debug, Default)]
pub struct SliderState {
    /// While the pointer holds the thumb, the value it implies. The stored
    /// setting only moves on release, so a mid-drag repaint draws from this
    /// cell instead.
    drag_value: Cell<Option<f32>>,
}

impl SliderState {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// The value a frame should draw: the in-flight drag value while one is
    /// held, otherwise the stored `value`.
    pub fn shown(&self, value: f32) -> f32 {
        self.drag_value.get().unwrap_or(value)
    }

    /// Drop a pending drag. Needed when the slider unmounts mid-gesture —
    /// its release handler dies with it, leaving a stale value behind.
    pub fn cancel(&self) {
        self.drag_value.set(None);
    }
}

/// The value a pointer x position implies over `bounds`' thumb travel. Pure,
/// so the pointer mapping is unit-testable.
fn value_at(bounds: Bounds<Pixels>, x: Pixels, max: f32) -> f32 {
    let travel = (bounds.size.width - px(THUMB_SIZE)).max(Pixels::ZERO);
    if travel <= Pixels::ZERO {
        return 0.0;
    }
    ((x - bounds.left() - px(THUMB_SIZE / 2.0)) / travel).clamp(0.0, 1.0) * max
}

/// A focusable slider. Pointer drags move the drawn thumb continuously and
/// `commit` fires once per gesture, on release, and once per arrow/Home/End
/// key — so `commit` is where the value is stored, persisted, and previewed.
/// The owning `Window` is passed so commits can touch native window state.
#[track_caller]
pub fn slider<E>(
    id: impl Into<ElementId>,
    state: &Rc<SliderState>,
    max: f32,
    value: f32,
    cx: &mut Context<E>,
    commit: impl Fn(&mut E, f32, &mut Window, &mut Context<E>) + 'static,
) -> Stateful<Div>
where
    E: 'static,
{
    let theme = Theme::current(cx);
    let value = value.clamp(0.0, max);
    let weak = cx.entity().downgrade();
    let commit = Rc::new(commit);

    div()
        .id(id)
        .tab_index(0)
        .h(px(20.0))
        .rounded(px(4.0))
        .cursor_default()
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .child(
            canvas(|_, _, _| (), {
                let state = state.clone();
                let weak = weak.clone();
                let commit = commit.clone();
                move |bounds, _, window: &mut Window, cx: &mut App| {
                    let theme = Theme::current(cx);
                    let shown = state.shown(value).clamp(0.0, max) / max;
                    let thumb_center = bounds.left()
                        + px(THUMB_SIZE / 2.0)
                        + (bounds.size.width - px(THUMB_SIZE)) * shown;

                    window.paint_quad(quad(
                        Bounds::new(
                            point(bounds.left(), bounds.center().y - px(TRACK_HEIGHT / 2.0)),
                            size(bounds.size.width, px(TRACK_HEIGHT)),
                        ),
                        px(TRACK_HEIGHT / 2.0),
                        theme.inset,
                        px(0.0),
                        gpui::transparent_black(),
                        gpui::BorderStyle::default(),
                    ));
                    window.paint_quad(quad(
                        Bounds::new(
                            point(bounds.left(), bounds.center().y - px(TRACK_HEIGHT / 2.0)),
                            size(
                                (thumb_center - bounds.left()).max(Pixels::ZERO),
                                px(TRACK_HEIGHT),
                            ),
                        ),
                        px(TRACK_HEIGHT / 2.0),
                        theme.accent,
                        px(0.0),
                        gpui::transparent_black(),
                        gpui::BorderStyle::default(),
                    ));
                    window.paint_quad(quad(
                        Bounds::new(
                            point(
                                thumb_center - px(THUMB_SIZE / 2.0),
                                bounds.center().y - px(THUMB_SIZE / 2.0),
                            ),
                            size(px(THUMB_SIZE), px(THUMB_SIZE)),
                        ),
                        px(THUMB_SIZE / 2.0),
                        theme.inverse,
                        px(0.0),
                        gpui::transparent_black(),
                        gpui::BorderStyle::default(),
                    ));

                    window.on_mouse_event({
                        let state = state.clone();
                        move |event: &MouseDownEvent, phase, window, _| {
                            if phase != DispatchPhase::Bubble
                                || event.button != MouseButton::Left
                                || !bounds.contains(&event.position)
                            {
                                return;
                            }
                            state
                                .drag_value
                                .set(Some(value_at(bounds, event.position.x, max)));
                            window.refresh();
                        }
                    });
                    window.on_mouse_event({
                        let state = state.clone();
                        move |event: &MouseMoveEvent, phase, window, _| {
                            if phase != DispatchPhase::Bubble || state.drag_value.get().is_none() {
                                return;
                            }
                            state
                                .drag_value
                                .set(Some(value_at(bounds, event.position.x, max)));
                            window.refresh();
                        }
                    });
                    window.on_mouse_event({
                        let state = state.clone();
                        let weak = weak.clone();
                        let commit = commit.clone();
                        move |_: &MouseUpEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let Some(value) = state.drag_value.take() else {
                                return;
                            };
                            let _ = weak.update(cx, |this, cx| commit(this, value, window, cx));
                            window.refresh();
                        }
                    });
                }
            })
            .size_full(),
        )
        .on_key_down(cx.listener({
            let commit = commit.clone();
            move |this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let next = match event.keystroke.key.as_str() {
                    "left" | "down" => Some(value - KEY_STEP * max),
                    "right" | "up" => Some(value + KEY_STEP * max),
                    "home" => Some(0.0),
                    "end" => Some(max),
                    _ => None,
                };
                if let Some(next) = next {
                    commit(this, next.clamp(0.0, max), window, cx);
                    cx.stop_propagation();
                }
            }
        }))
}

/// Two independently focusable endpoints on a daily/cyclic range. Crossing
/// endpoints wraps the highlighted interval rather than swapping their roles.
pub struct RangeSliderState {
    drag: Cell<Option<(usize, f32)>>,
    focus: [gpui::FocusHandle; 2],
}

impl RangeSliderState {
    pub fn new(cx: &mut App) -> Rc<Self> {
        Rc::new(Self {
            drag: Cell::new(None),
            focus: [cx.focus_handle(), cx.focus_handle()],
        })
    }

    pub fn shown(&self, mut values: [f32; 2]) -> [f32; 2] {
        if let Some((index, value)) = self.drag.get() {
            values[index] = value;
        }
        values
    }

    pub fn cancel(&self) {
        self.drag.set(None);
    }
}

/// A two-thumb range slider. Each thumb is a Tab stop; arrows move by `step`,
/// Home/End reach the boundaries, and pointer changes commit on release.
#[track_caller]
pub fn range_slider<E: 'static>(
    id: impl Into<ElementId>,
    state: &Rc<RangeSliderState>,
    max: f32,
    step: f32,
    values: [f32; 2],
    cx: &mut Context<E>,
    commit: impl Fn(&mut E, usize, f32, &mut Window, &mut Context<E>) + 'static,
) -> Stateful<Div> {
    let values = values.map(|value| value.clamp(0.0, max));
    let shown = state.shown(values).map(|value| value.clamp(0.0, max));
    let weak = cx.entity().downgrade();
    let commit = Rc::new(commit);
    let theme = Theme::current(cx);
    let mut control = div().id(id).relative().h(px(24.0)).cursor_default().child(
        canvas(|_, _, _| (), {
            let state = state.clone();
            let commit = commit.clone();
            move |bounds, _, window: &mut Window, cx: &mut App| {
                let theme = Theme::current(cx);
                let shown = state.shown(values).map(|value| value.clamp(0.0, max) / max);
                let travel = (bounds.size.width - px(THUMB_SIZE)).max(Pixels::ZERO);
                let centers =
                    shown.map(|value| bounds.left() + px(THUMB_SIZE / 2.0) + travel * value);
                let mut segment = |left: Pixels, right: Pixels, color| {
                    window.paint_quad(quad(
                        Bounds::new(
                            point(left, bounds.center().y - px(TRACK_HEIGHT / 2.0)),
                            size((right - left).max(Pixels::ZERO), px(TRACK_HEIGHT)),
                        ),
                        px(TRACK_HEIGHT / 2.0),
                        color,
                        px(0.0),
                        gpui::transparent_black(),
                        gpui::BorderStyle::default(),
                    ));
                };
                segment(bounds.left(), bounds.right(), theme.inset);
                if shown[0] < shown[1] {
                    segment(centers[0], centers[1], theme.accent);
                } else if shown[0] > shown[1] {
                    segment(bounds.left(), centers[1], theme.accent);
                    segment(centers[0], bounds.right(), theme.accent);
                }
                for center in centers {
                    window.paint_quad(quad(
                        Bounds::new(
                            point(
                                center - px(THUMB_SIZE / 2.0),
                                bounds.center().y - px(THUMB_SIZE / 2.0),
                            ),
                            size(px(THUMB_SIZE), px(THUMB_SIZE)),
                        ),
                        px(THUMB_SIZE / 2.0),
                        theme.inverse,
                        px(0.0),
                        gpui::transparent_black(),
                        gpui::BorderStyle::default(),
                    ));
                }
                window.on_mouse_event({
                    let state = state.clone();
                    move |event: &MouseDownEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble
                            || event.button != MouseButton::Left
                            || !bounds.contains(&event.position)
                        {
                            return;
                        }
                        let index = usize::from(
                            (event.position.x - centers[1]).abs()
                                < (event.position.x - centers[0]).abs(),
                        );
                        window.focus(&state.focus[index], cx);
                        state
                            .drag
                            .set(Some((index, value_at(bounds, event.position.x, max))));
                        window.refresh();
                    }
                });
                window.on_mouse_event({
                    let state = state.clone();
                    move |event: &MouseMoveEvent, phase, window, _| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }
                        if let Some((index, _)) = state.drag.get() {
                            state
                                .drag
                                .set(Some((index, value_at(bounds, event.position.x, max))));
                            window.refresh();
                        }
                    }
                });
                window.on_mouse_event({
                    let state = state.clone();
                    let weak = weak.clone();
                    let commit = commit.clone();
                    move |event: &MouseUpEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                            return;
                        }
                        if let Some((index, value)) = state.drag.take() {
                            let _ =
                                weak.update(cx, |this, cx| commit(this, index, value, window, cx));
                            window.refresh();
                        }
                    }
                });
            }
        })
        .size_full(),
    );
    for index in 0..2 {
        let commit = commit.clone();
        control = control.child(
            div()
                .id(("range-thumb", index))
                .absolute()
                .left(gpui::relative(shown[index] / max))
                .ml(px(-THUMB_SIZE * shown[index] / max))
                .w(px(THUMB_SIZE))
                .h_full()
                .rounded(px(4.0))
                .track_focus(&state.focus[index])
                .tab_index(0)
                .focus_visible(|style| {
                    style
                        .bg(theme.focus_highlight())
                        .border_1()
                        .border_color(theme.accent)
                })
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    let next = match event.keystroke.key.as_str() {
                        "left" | "down" => values[index] - step,
                        "right" | "up" => values[index] + step,
                        "home" => 0.0,
                        "end" => max,
                        _ => return,
                    };
                    commit(this, index, next.clamp(0.0, max), window, cx);
                    cx.stop_propagation();
                })),
        );
    }
    control
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RangeHarness {
        state: Rc<RangeSliderState>,
        values: Rc<Cell<[f32; 2]>>,
    }

    impl gpui::Render for RangeHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            range_slider(
                "sleep-range",
                &self.state,
                1440.0,
                15.0,
                self.values.get(),
                cx,
                |this, endpoint, value, _, cx| {
                    let mut values = this.values.get();
                    values[endpoint] = value;
                    this.values.set(values);
                    cx.notify();
                },
            )
            .w(px(280.0))
        }
    }

    #[gpui::test]
    fn range_endpoints_are_independently_keyboard_operable(cx: &mut gpui::TestAppContext) {
        let state = cx.update(RangeSliderState::new);
        let values = Rc::new(Cell::new([1320.0, 420.0]));
        let (_, cx) = cx.add_window_view(|_, _| RangeHarness {
            state: state.clone(),
            values: values.clone(),
        });
        cx.update(|window, cx| window.focus(&state.focus[0], cx));
        cx.simulate_keystrokes("right");
        assert_eq!(values.get(), [1335.0, 420.0]);
        cx.update(|window, cx| window.focus(&state.focus[1], cx));
        cx.simulate_keystrokes("left");
        assert_eq!(values.get(), [1335.0, 405.0]);
        cx.simulate_keystrokes("home");
        assert_eq!(values.get(), [1335.0, 0.0]);
        cx.simulate_keystrokes("end");
        assert_eq!(values.get(), [1335.0, 1440.0]);
    }

    #[gpui::test]
    fn range_drag_commits_only_the_dragged_endpoint_on_release(cx: &mut gpui::TestAppContext) {
        let state = cx.update(RangeSliderState::new);
        let values = Rc::new(Cell::new([1320.0, 420.0]));
        let (_, cx) = cx.add_window_view(|_, _| RangeHarness {
            state: state.clone(),
            values: values.clone(),
        });
        // A 280px track has 267px of thumb travel; this is the start thumb.
        cx.simulate_mouse_down(
            point(px(251.25), px(12.0)),
            MouseButton::Left,
            gpui::Modifiers::none(),
        );
        cx.simulate_mouse_move(
            point(px(140.0), px(12.0)),
            MouseButton::Left,
            gpui::Modifiers::none(),
        );
        assert_eq!(
            values.get(),
            [1320.0, 420.0],
            "dragging must not persist intermediate values"
        );
        assert_eq!(state.shown(values.get()), [720.0, 420.0]);
        cx.simulate_mouse_up(
            point(px(140.0), px(12.0)),
            MouseButton::Left,
            gpui::Modifiers::none(),
        );
        assert_eq!(values.get(), [720.0, 420.0]);
    }

    fn track() -> Bounds<Pixels> {
        Bounds::new(point(px(10.0), px(0.0)), size(px(113.0), px(20.0)))
    }

    #[test]
    fn pointer_positions_map_across_the_thumb_travel() {
        // The thumb center travels from one thumb-radius in to one short of
        // the far edge: 10 + 6.5 .. 10 + 113 - 6.5.
        assert_eq!(value_at(track(), px(16.5), 1.0), 0.0);
        assert_eq!(value_at(track(), px(66.5), 1.0), 0.5);
        assert_eq!(value_at(track(), px(116.5), 1.0), 1.0);
        // `max` scales the same positions into the slider's own units.
        assert_eq!(value_at(track(), px(66.5), 2.0), 1.0);
        assert_eq!(value_at(track(), px(116.5), 2.0), 2.0);
    }

    #[test]
    fn pointer_positions_clamp_at_both_ends() {
        assert_eq!(value_at(track(), px(-500.0), 1.0), 0.0);
        assert_eq!(value_at(track(), px(9_999.0), 1.0), 1.0);
    }
}
