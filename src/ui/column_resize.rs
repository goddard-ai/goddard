//! A drag handle for resizing a fixed-width table column.
//!
//! Two pieces per table, same split as the slider: the handle is a real
//! element — hitbox, cursor, keyboard focus — while the drag's move/up
//! listeners live on a `canvas` the table covers, so the gesture keeps
//! tracking after the pointer leaves the strip. The owner keeps the
//! [`ColumnResize`] so an in-flight drag survives the repaints it requests.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Context, DispatchPhase, Div, ElementId, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Stateful, Window, canvas, div, prelude::*,
    px,
};

use crate::theme::Theme;

/// Width of the handle's hit region, centered on the column's right edge.
const HANDLE_WIDTH: f32 = 9.0;
/// Smallest width a column accepts from either pointer or keys.
const MIN_COLUMN_WIDTH: f32 = 40.0;
/// Arrow-key step for a focused handle.
const KEY_STEP: f32 = 16.0;

/// Cross-frame resize state, one per resizable table. The owner holds it so
/// the drag survives the repaints each pointer move requests.
#[derive(Debug, Default)]
pub struct ColumnResize {
    /// The armed drag: boundary index, press x, and the column's width at
    /// press time — one delta from the press, so the gesture doesn't
    /// accumulate error across repaints.
    drag: Cell<Option<ColumnDrag>>,
}

#[derive(Clone, Copy, Debug)]
struct ColumnDrag {
    column: usize,
    start_x: Pixels,
    start_width: f32,
}

impl ColumnResize {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Whether `column`'s boundary is currently held — the grip's active
    /// styling reads this.
    fn dragging(&self, column: usize) -> bool {
        self.drag
            .get()
            .is_some_and(|drag| drag.column == column)
    }
}

/// The boundary strip at a fixed-width column's right edge. Place it inside
/// a `.relative()` header cell; it centers itself on the cell's trailing
/// edge, overflowing into the column gap when the cell doesn't clip it.
///
/// `width` is the column's current width and `set` stores a new one — it
/// runs continuously through the drag and once per arrow key, so it should
/// write the entity's width field and `cx.notify()`.
pub fn column_resize_handle<E>(
    id: impl Into<ElementId>,
    state: &Rc<ColumnResize>,
    column: usize,
    width: f32,
    theme: &Theme,
    cx: &mut Context<E>,
    set: impl Fn(&mut E, usize, f32, &mut Context<E>) + 'static,
) -> Stateful<Div>
where
    E: 'static,
{
    let id = id.into();
    let group = SharedString::from(format!("column-resize-grip-{id:?}"));
    let active = state.dragging(column);
    let weak = cx.entity().downgrade();
    let set = Rc::new(set);

    div()
        .id(id)
        .tab_index(0)
        .tab_stop(true)
        .group(group.clone())
        .absolute()
        .top_0()
        .bottom_0()
        .right(-px(HANDLE_WIDTH / 2.0))
        .w(px(HANDLE_WIDTH))
        .cursor_col_resize()
        .flex()
        .justify_center()
        .focus_visible(|element| element.bg(theme.focus_highlight()))
        .child(
            div()
                .w(px(1.5))
                .h_full()
                .rounded_full()
                .when(active, |element| element.bg(theme.accent))
                .group_hover(group, |element| element.bg(theme.accent)),
        )
        .on_mouse_down(MouseButton::Left, {
            let state = state.clone();
            move |event: &MouseDownEvent, window, cx| {
                state.drag.set(Some(ColumnDrag {
                    column,
                    start_x: event.position.x,
                    start_width: width,
                }));
                // Rows and headers under the strip keep their own press
                // handlers; the boundary grab must not fall through to them.
                cx.stop_propagation();
                window.refresh();
            }
        })
        .on_key_down({
            let set = set.clone();
            let weak = weak.clone();
            move |event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let step = match event.keystroke.key.as_str() {
                    "left" => -KEY_STEP,
                    "right" => KEY_STEP,
                    _ => return,
                };
                let _ = weak.update(cx, |this, cx| {
                    set(this, column, (width + step).max(MIN_COLUMN_WIDTH), cx);
                });
                cx.stop_propagation();
            }
        })
}

/// An invisible canvas the table covers; it installs the drag's move/up
/// listeners each paint. Element-level move handlers stop at the handle's
/// bounds, but these run for as long as a drag is armed — `set` fires
/// continuously so the column tracks the pointer.
pub fn column_resize_listeners<E>(
    state: &Rc<ColumnResize>,
    cx: &mut Context<E>,
    set: impl Fn(&mut E, usize, f32, &mut Context<E>) + 'static,
) -> impl IntoElement
where
    E: 'static,
{
    let weak = cx.entity().downgrade();
    let set = Rc::new(set);
    canvas(|_, _, _| (), {
        let state = state.clone();
        move |_, _, window: &mut Window, _| {
            window.on_mouse_event({
                let state = state.clone();
                let weak = weak.clone();
                let set = set.clone();
                move |event: &MouseMoveEvent, phase, window, cx: &mut App| {
                    if phase != DispatchPhase::Bubble || !event.dragging() {
                        return;
                    }
                    let Some(drag) = state.drag.get() else {
                        return;
                    };
                    let width = (drag.start_width
                        + f32::from(event.position.x - drag.start_x))
                    .max(MIN_COLUMN_WIDTH);
                    let _ = weak.update(cx, |this, cx| set(this, drag.column, width, cx));
                    window.refresh();
                }
            });
            window.on_mouse_event({
                let state = state.clone();
                move |_: &MouseUpEvent, phase, window, _| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    if state.drag.take().is_some() {
                        window.refresh();
                    }
                }
            });
        }
    })
    .absolute()
    .inset_0()
}
