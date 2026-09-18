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

use crate::theme::{Theme, hairline};

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
        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
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

#[cfg(test)]
mod tests {
    use super::*;

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
