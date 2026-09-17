//! "Inspect elements" and "Inspect colors" — command-palette commands that put
//! the window into gpui's inspector pick mode. Hovering floats a label just
//! above the element (or below it when the window's top edge is too close),
//! the next click is intercepted by gpui's picking layer and copies the label
//! to the clipboard; Escape cancels.
//!
//! `Elements` shows the element's construction site (`file:line`, captured by
//! `#[track_caller]` inside gpui). `Colors` instead reads the element's
//! declared style colors and labels each with the theme token it matches — or
//! its hex value when no token matches.
//!
//! Debug builds only: gpui compiles the inspector out of release builds unless
//! its `inspector` feature is enabled, so `start` and `init` are stubs there.

#[cfg(not(debug_assertions))]
use gpui::{App, Window};

/// Which payload the inspector's floating label shows and its click copies.
/// Lives outside the debug-only module so the release stub shares the
/// signature.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum InspectorMode {
    /// `file:line` of the element's construction site.
    #[default]
    Elements,
    /// Theme tokens backing the element's style colors.
    Colors,
}

#[cfg(debug_assertions)]
mod implementation {
    use gpui::{
        AnyElement, App, ClipboardItem, Context, DivInspectorState, Empty, Global, Hsla, Inspector,
        InspectorElementId, IntoElement, Pixels, Rgba, SharedString, StyleRefinement, Window, div,
        prelude::*, px, rems,
    };

    use crate::fonts;
    use crate::theme::{Theme, hairline, sp};

    use super::InspectorMode;

    /// Inspector session state. `label` stashes the text the floating pill is
    /// currently showing: the click that ends picking is intercepted by gpui's
    /// picking layer before app handlers run, so the copy path in
    /// `render_inspector` can't reach back into the element's inspector state —
    /// it copies whatever the last picking frame displayed.
    #[derive(Default)]
    struct InspectorUi {
        mode: InspectorMode,
        label: Option<String>,
    }

    impl Global for InspectorUi {}

    /// Width gpui reserves on the window's right edge for inspector UI whenever
    /// the inspector entity exists (`Window::draw_roots`). The inspector root
    /// is laid out at that x offset, so absolute-positioned children translate
    /// window coordinates by subtracting it.
    fn panel_origin(window: &Window) -> Pixels {
        window.viewport_size().width - rems(30.0).to_pixels(window.rem_size())
    }

    pub fn init(cx: &mut App) {
        cx.set_global(InspectorUi::default());
        cx.set_inspector_renderer(Box::new(render_inspector));
        // Only divs register inspector hitboxes, so the active element always
        // carries a `DivInspectorState` — which is where gpui records the
        // element's bounds and declared style each frame. Rendering the label
        // as the state's inspector element anchors it to those bounds.
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

    pub fn start(mode: InspectorMode, window: &mut Window, cx: &mut App) {
        {
            let ui = cx.global_mut::<InspectorUi>();
            ui.mode = mode;
            ui.label = None;
        }
        // `toggle_inspector` both enables and disables — leave an active pick
        // alone so switching modes mid-flight doesn't tear the session down.
        if !window.is_inspector_picking(cx) {
            window.toggle_inspector(cx);
        }
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
            let label = cx.global_mut::<InspectorUi>().label.take();
            let toast_key = match cx.global::<InspectorUi>().mode {
                InspectorMode::Elements => "element_inspector.copied",
                InspectorMode::Colors => "element_inspector.colors_copied",
            };
            let window_handle = window.window_handle();
            cx.defer(move |cx| {
                window_handle
                    .update(cx, |root, window, cx| {
                        if let Some(label) = label {
                            cx.write_to_clipboard(ClipboardItem::new_string(label));
                            if let Ok(waku) = root.downcast::<crate::app::Waku>() {
                                waku.update(cx, |waku, cx| {
                                    waku.show_success_toast(tr!(toast_key));
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

    /// The floating label for the hovered element, anchored to the bounds gpui
    /// recorded into `DivInspectorState` during prepaint. In colors mode an
    /// element declaring no colors gets no label at all — and nothing to copy.
    fn inspector_label(
        inspector_id: InspectorElementId,
        state: &DivInspectorState,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let mode = cx.global::<InspectorUi>().mode;
        let lines = match mode {
            InspectorMode::Elements => vec![inspector_element_label(&inspector_id)],
            InspectorMode::Colors => color_lines(&state.base_style, &theme),
        };
        cx.global_mut::<InspectorUi>().label = (!lines.is_empty()).then(|| lines.join("\n"));
        if lines.is_empty() {
            return Empty.into_any_element();
        }

        let bounds = state.bounds;
        let viewport = window.viewport_size();

        // Estimated pill size: 11sp text (~15px line) plus padding and border
        // vertically, and JetBrains Mono runs roughly 6.8px per glyph at this
        // size.
        let max_chars = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as f32;
        let pill_height = px(lines.len() as f32 * 15.0 + 8.0);
        let approx_width = px(max_chars * 6.8 + 16.0);
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
            .flex()
            .flex_col()
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
            // The pill lives inside a zero-sized container, so without nowrap
            // the text wraps at zero width — one character per line.
            .whitespace_nowrap()
            .children(
                lines
                    .into_iter()
                    .map(|line| div().child(SharedString::from(line))),
            )
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

    /// The colors an element declares, labeled by the style property that set
    /// them.
    fn color_lines(style: &StyleRefinement, theme: &Theme) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(background) = style.background.as_ref().and_then(|fill| fill.color()) {
            match background.as_solid() {
                Some(color) => {
                    lines.push(format!("Background: {}", describe_color(color, theme)));
                }
                None => lines.push("Background: gradient".to_string()),
            }
        }
        if let Some(border) = style.border_color {
            lines.push(format!("Border: {}", describe_color(border, theme)));
        }
        if let Some(text) = style.text.color {
            lines.push(format!("Text: {}", describe_color(text, theme)));
        }
        if let Some(shadows) = &style.box_shadow {
            for shadow in shadows {
                lines.push(format!("Shadow: {}", describe_color(shadow.color, theme)));
            }
        }
        lines
    }

    /// `theme.<field>` for every token whose color equals `color`, or the
    /// color's hex value when it came from a literal or a computed value.
    fn describe_color(color: Hsla, theme: &Theme) -> String {
        let matches: Vec<&'static str> = theme_tokens(theme)
            .into_iter()
            .filter(|(_, token)| *token == color)
            .map(|(name, _)| name)
            .collect();
        if matches.is_empty() {
            hex_color(color)
        } else {
            matches.join(", ")
        }
    }

    fn hex_color(color: Hsla) -> String {
        let rgba = Rgba::from(color);
        let channel = |value: f32| (value * 255.0).round() as u8;
        let (r, g, b, a) = (
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b),
            channel(rgba.a),
        );
        if a == u8::MAX {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    /// Every `Hsla` field on `Theme`, named the way code refers to it. Kept
    /// next to its only caller; `ansi` is omitted since it backs terminal
    /// colors, not element styles.
    fn theme_tokens(theme: &Theme) -> Vec<(&'static str, Hsla)> {
        vec![
            ("theme.canvas", theme.canvas),
            ("theme.sidebar", theme.sidebar),
            (
                "theme.sidebar_drag_background",
                theme.sidebar_drag_background,
            ),
            (
                "theme.sidebar_item_background",
                theme.sidebar_item_background,
            ),
            ("theme.surface", theme.surface),
            ("theme.raised", theme.raised),
            ("theme.composer", theme.composer),
            ("theme.inset", theme.inset),
            ("theme.terminal", theme.terminal),
            ("theme.overlay", theme.overlay),
            ("theme.overlay_strong", theme.overlay_strong),
            ("theme.border", theme.border),
            ("theme.border_strong", theme.border_strong),
            ("theme.separator", theme.separator),
            ("theme.sidebar_border", theme.sidebar_border),
            ("theme.text", theme.text),
            ("theme.text_secondary", theme.text_secondary),
            ("theme.text_tertiary", theme.text_tertiary),
            ("theme.text_ghost", theme.text_ghost),
            ("theme.accent", theme.accent),
            ("theme.resize_handle", theme.resize_handle),
            ("theme.gauge", theme.gauge),
            ("theme.selection", theme.selection),
            ("theme.code_text", theme.code_text),
            ("theme.code_wash", theme.code_wash),
            ("theme.inverse", theme.inverse),
            ("theme.on_inverse", theme.on_inverse),
            ("theme.info", theme.info),
            ("theme.warning", theme.warning),
            ("theme.success", theme.success),
            ("theme.favorite", theme.favorite),
            ("theme.danger", theme.danger),
            ("theme.danger_soft", theme.danger_soft),
            ("theme.syntax.keyword", theme.syntax.keyword),
            ("theme.syntax.literal", theme.syntax.literal),
            ("theme.syntax.string", theme.syntax.string),
            ("theme.syntax.comment", theme.syntax.comment),
            ("theme.syntax.number", theme.syntax.number),
            ("theme.syntax.ty", theme.syntax.ty),
            ("theme.syntax.function", theme.syntax.function),
            ("theme.syntax.meta", theme.syntax.meta),
        ]
    }
}

#[cfg(debug_assertions)]
pub use implementation::{init, start};

#[cfg(not(debug_assertions))]
pub fn init(_: &mut App) {}

#[cfg(not(debug_assertions))]
pub fn start(_: InspectorMode, _: &mut Window, _: &mut App) {}
