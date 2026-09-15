//! "Inspect elements" — a command-palette command that puts the window into
//! gpui's inspector pick mode. Hovering floats the element's construction site
//! (`file:line:col`, captured by `#[track_caller]` inside gpui) beside the
//! cursor; the next click is intercepted by gpui's picking layer and copies the
//! label to the clipboard; Escape cancels.
//!
//! Debug builds only: gpui compiles the inspector out of release builds unless
//! its `inspector` feature is enabled, so `start` and `init` are stubs there.

#[cfg(not(debug_assertions))]
use gpui::{App, Window};

#[cfg(debug_assertions)]
mod implementation {
    use gpui::{
        AnyElement, App, ClipboardItem, Context, Empty, Inspector, InspectorElementId, IntoElement,
        Pixels, Window, div, prelude::*, px, rems,
    };

    use crate::fonts;
    use crate::theme::{Theme, sp};

    /// Width gpui reserves on the window's right edge for inspector UI whenever
    /// the inspector entity exists (`Window::prepaint`). The label escapes that
    /// strip through absolute positioning so it floats at the cursor instead.
    fn panel_origin(window: &Window) -> Pixels {
        window.viewport_size().width - rems(30.0).to_pixels(window.rem_size())
    }

    pub fn init(cx: &mut App) {
        cx.set_inspector_renderer(Box::new(render_inspector));
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

        let Some(inspector_id) = inspector.active_element_id() else {
            return Empty.into_any_element();
        };

        let theme = Theme::current(cx);
        let location_label = inspector_element_label(inspector_id);
        let global_id = inspector_id.path.global_id.to_string();

        // Estimated pill width keeps it inside the window near the edges;
        // JetBrains Mono at this size is roughly 6.8px per glyph.
        let approx_width = px(location_label.chars().count() as f32 * 6.8 + 20.0);
        let mouse = window.mouse_position();
        let viewport = window.viewport_size();
        let mut label_x = mouse.x + px(14.0);
        if label_x + approx_width > viewport.width - px(8.0) {
            label_x = (mouse.x - approx_width - px(6.0)).max(px(8.0));
        }
        let mut label_y = mouse.y + px(18.0);
        if label_y + px(26.0) > viewport.height - px(8.0) {
            label_y = (mouse.y - px(32.0)).max(px(8.0));
        }

        div()
            .size_full()
            .child(
                div()
                    .absolute()
                    .left(label_x - panel_origin(window))
                    .top(label_y)
                    .px(px(7.0))
                    .py(px(3.0))
                    .rounded(px(5.0))
                    .bg(theme.overlay_strong)
                    .border_1()
                    .border_color(theme.border_strong)
                    .shadow_lg()
                    .font_family(fonts::current(cx).code)
                    .text_size(sp(11.0))
                    .text_color(theme.text)
                    .child(location_label)
                    .when(!global_id.is_empty(), |this| {
                        this.child(
                            div()
                                .pt(px(1.0))
                                .text_color(theme.text_tertiary)
                                .child(global_id),
                        )
                    }),
            )
            .into_any_element()
    }

    /// `file:line:col` of the element's construction site — the label that
    /// identifies it in code. Workspace sources are already relative; absolute
    /// paths (dependency elements) lose the workspace prefix when they have it.
    fn inspector_element_label(id: &InspectorElementId) -> String {
        let location = id.path.source_location;
        let file = location
            .file()
            .strip_prefix(concat!(env!("CARGO_MANIFEST_DIR"), "/"))
            .unwrap_or_else(|| location.file());
        format!("{file}:{}:{}", location.line(), location.column())
    }
}

#[cfg(debug_assertions)]
pub use implementation::{init, start};

#[cfg(not(debug_assertions))]
pub fn init(_: &mut App) {}

#[cfg(not(debug_assertions))]
pub fn start(_: &mut Window, _: &mut App) {}
