//! Goddard's tooltip surface.
//!
//! GPUI already owns tooltip *behaviour* — hover timing, placement, dismissal —
//! through `InteractiveElement::tooltip`, which asks only for a view to render.
//! This is that view, and nothing more.

use gpui::{
    Action, AnyView, App, AppContext, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, prelude::FluentBuilder, px,
};

use crate::theme::{Theme, hairline, sp};
use crate::ui::shortcut::ShortcutHint;

/// A single-line hint, optionally with the action's shortcut rendered dim
/// after the label.
pub struct Tooltip {
    label: SharedString,
    shortcut: Option<ShortcutHint>,
}

impl Tooltip {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
        }
    }

    /// A shortcut hint rendered dim after the label.
    pub fn shortcut(mut self, hint: ShortcutHint) -> Self {
        self.shortcut = Some(hint);
        self
    }

    /// Resolve `action`'s binding against the live keymap and show it after
    /// the label. The hint drops out when the action is unbound where the
    /// pointer is hovering.
    pub fn action(self, action: &dyn Action) -> Self {
        self.shortcut(ShortcutHint::action(action))
    }

    /// Build the view GPUI's `.tooltip(..)` expects.
    pub fn build(self, _window: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }

    /// Shorthand for the overwhelmingly common case:
    /// `.tooltip(Tooltip::text("Copy message"))`.
    pub fn text(
        label: impl Into<SharedString>,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let label = label.into();
        move |window, cx| Tooltip::new(label.clone()).build(window, cx)
    }

    /// `text` plus the action's resolved shortcut:
    /// `.tooltip(Tooltip::text_with_action("Toggle Sidebar", &ToggleSidebar))`.
    pub fn text_with_action(
        label: impl Into<SharedString>,
        action: &dyn Action,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        Self::text_with_hint(label, ShortcutHint::action(action))
    }

    /// `text` plus a prebuilt hint — for chords that reach the action through
    /// a chord-sharing sibling, like
    /// `ShortcutHint::action(&NewSession).shadowed_by(&SwitchProjectForward)`.
    pub fn text_with_hint(
        label: impl Into<SharedString>,
        hint: ShortcutHint,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let label = label.into();
        move |window, cx| {
            Tooltip::new(label.clone())
                .shortcut(hint.clone())
                .build(window, cx)
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let shortcut = self
            .shortcut
            .as_ref()
            .and_then(|hint| hint.resolve(window, cx));
        // The outer wrapper is transparent and only offsets the card from the
        // cursor; the shadow needs a parent that does not clip it.
        div().pt(px(4.0)).pl(px(2.0)).child(
            div()
                .px(px(7.0))
                .py(px(4.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.raised)
                .shadow_md()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(sp(12.5))
                .line_height(sp(15.0))
                .text_color(theme.text_secondary)
                .child(self.label.clone())
                .when_some(shortcut, |element, label| {
                    element.child(
                        div()
                            .flex_none()
                            .text_color(theme.text_tertiary)
                            .child(label),
                    )
                }),
        )
    }
}
