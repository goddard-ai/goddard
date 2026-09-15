//! "Inspect elements" — a command-palette command that puts the window into
//! gpui's inspector pick mode. Hovering floats the element's construction site
//! (`file:line`, captured by `#[track_caller]` inside gpui) just above the
//! element, or below it when the window's top edge is too close; the next
//! click is intercepted by gpui's picking layer and copies the label to the
//! clipboard; Escape cancels.
//!
//! Debug builds only: gpui compiles the inspector out of release builds unless
//! its `inspector` feature is enabled, so `start` and `init` are stubs there.

#[cfg(not(debug_assertions))]
use gpui::{App, Window};

#[cfg(debug_assertions)]
mod implementation {
    use gpui::{
        AnyElement, App, ClipboardItem, Context, DivInspectorState, Empty, Inspector,
        InspectorElementId, IntoElement, Pixels, Window, div, prelude::*, px, rems,
    };

    use crate::fonts;
    use crate::theme::{Theme, hairline, sp};

    /// Width gpui reserves on the window's right edge for inspector UI whenever
    /// the inspector entity exists (`Window::draw_roots`). The inspector root
    /// is laid out at that x offset, so absolute-positioned children translate
    /// window coordinates by subtracting it.
    fn panel_origin(window: &Window) -> Pixels {
        window.viewport_size().width - rems(30.0).to_pixels(window.rem_size())
    }

    pub fn init(cx: &mut App) {
        cx.set_inspector_renderer(Box::new(render_inspector));
        // Only divs register inspector hitboxes, so the active element always
        // carries a `DivInspectorState` — which is where gpui records the
        // element's bounds each frame. Rendering the label as the state's
        // inspector element anchors it to those bounds.
        cx.register_inspector_element(inspector_label);
        // Picking bypasses element mouse handlers but not key dispatch, so
        // cancelling needs an interceptor to outrank a focused field's own
        // Escape binding.
        cx.intercept_keystrokes(|event, window, cx| {
            if event.keystroke.key == "escape" && window.is_inspector_picking(cx) {
                window.toggle_inspector(cx);
                cx.stop_propagation();
            }
        })
        .detach();
    }

    pub fn start(window: &mut Window, cx: &mut App) {
        window.toggle_inspector(cx);
    }

    fn render_inspector(
        inspector: &mut Inspector,
        window: &mut Window,
        cx: &mut Context<Inspector>,
    ) -> AnyElement {
        // The selecting click ends picking without reaching the app. Copy the
        // resolved label, then tear the mode down so the next invocation
        // starts a fresh pick.
        if !inspector.is_picking() {
            let label = inspector.active_element_id().map(inspector_element_label);
            let window_handle = window.window_handle();
            cx.defer(move |cx| {
                window_handle
                    .update(cx, |root, window, cx| {
                        if let Some(label) = label {
                            cx.write_to_clipboard(ClipboardItem::new_string(label));
                            if let Ok(waku) = root.downcast::<crate::app::Waku>() {
                                waku.update(cx, |waku, cx| {
                                    waku.show_success_toast(tr!("element_inspector.copied"));
                                    cx.notify();
                                });
                            }
                        }
                        window.toggle_inspector(cx);
                    })
                    .ok();
            });
            return Empty.into_any_element();
        }

        let labels = inspector.render_inspector_states(window, cx);
        // Zero size: while picking every div claims a hitbox, and a container
        // covering gpui's reserved strip would swallow picks meant for the app
        // elements painted underneath it.
        div().size(px(0.0)).children(labels).into_any_element()
    }

    /// The floating `file:line` label for the hovered element, anchored to the
    /// bounds gpui recorded into `DivInspectorState` during prepaint.
    fn inspector_label(
        inspector_id: InspectorElementId,
        state: &DivInspectorState,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let location_label = inspector_element_label(&inspector_id);
        let bounds = state.bounds;
        let viewport = window.viewport_size();

        // Estimated pill size: 11sp text plus padding and border vertically,
        // and JetBrains Mono runs roughly 6.8px per glyph at this size.
        let pill_height = px(22.0);
        let approx_width = px(location_label.chars().count() as f32 * 6.8 + 16.0);
        let gap = px(4.0);
        let margin = px(8.0);

        let above = bounds.top() - gap - pill_height;
        let below = bounds.bottom() + gap;
        let label_y = if above >= margin {
            above
        } else if below + pill_height <= viewport.height - margin {
            below
        } else {
            // The element is taller than the room on either side, so overlay
            // the edge farther from the cursor. The pill carries its own
            // inspector hitbox, and landing under the cursor would make the
            // next hover pick the pill itself.
            let inside_top = (bounds.top() + margin).max(margin);
            let inside_bottom = (bounds.bottom() - pill_height - margin)
                .min(viewport.height - pill_height - margin)
                .max(margin);
            let mouse_y = window.mouse_position().y;
            if mouse_y - bounds.top() > bounds.bottom() - mouse_y {
                inside_top
            } else {
                inside_bottom
            }
        };
        // Align with the element's left edge, kept inside the window.
        let label_x = bounds
            .left()
            .max(margin)
            .min((viewport.width - approx_width - margin).max(margin));

        div()
            .absolute()
            .left(label_x - panel_origin(window))
            .top(label_y)
            .px(px(7.0))
            .py(px(3.0))
            .rounded(px(5.0))
            .bg(theme.raised)
            .border(hairline())
            .border_color(theme.border_strong)
            .shadow_lg()
            .font_family(fonts::current(cx).code)
            .text_size(sp(11.0))
            .text_color(theme.text)
            .child(location_label)
            .into_any_element()
    }

    /// `file:line` of the element's construction site — the label that
    /// identifies it in code. Workspace sources are already relative; absolute
    /// paths (dependency elements) lose the workspace prefix when they have it.
    fn inspector_element_label(id: &InspectorElementId) -> String {
        let location = id.path.source_location;
        let file = location
            .file()
            .strip_prefix(concat!(env!("CARGO_MANIFEST_DIR"), "/"))
            .unwrap_or_else(|| location.file());
        format!("{file}:{}", location.line())
    }
}

#[cfg(debug_assertions)]
pub use implementation::{init, start};

#[cfg(not(debug_assertions))]
pub fn init(_: &mut App) {}

#[cfg(not(debug_assertions))]
pub fn start(_: &mut Window, _: &mut App) {}
