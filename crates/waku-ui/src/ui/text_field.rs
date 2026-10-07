use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement, Interactivity, ParentElement, RenderOnce,
    Stateful, StyleRefinement, Styled, Window, div, prelude::*, px,
};

use crate::input::TextInput;
use crate::theme::{Theme, hairline, sp};

use super::icon;

/// A bordered field around a [`TextInput`], with an optional leading icon
/// and an accent border on keyboard-driven focus.
///
/// The default shell is fixed-height for a single-line input. Use
/// [`TextField::multiline`] with an auto-height input so wrapped content can
/// grow the shell instead of clipping inside it.
/// Construction stays at the call site because the entity needs the window;
/// this component owns everything visual.
#[derive(IntoElement)]
pub struct TextField {
    base: Stateful<Div>,
    input: gpui::Entity<TextInput>,
    icon: Option<(&'static str, f32)>,
    multiline: bool,
}

impl TextField {
    #[track_caller]
    pub fn new(id: impl Into<ElementId>, input: gpui::Entity<TextInput>) -> Self {
        Self {
            base: div().id(id),
            input,
            icon: None,
            multiline: false,
        }
    }

    /// Size the shell to an auto-height multi-line input.
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// Leading icon, tinted tertiary like the address bar's lock.
    pub fn icon(mut self, path: &'static str, size: f32) -> Self {
        self.icon = Some((path, size));
        self
    }
}

impl Styled for TextField {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for TextField {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl ParentElement for TextField {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let ring = self.input.read(cx).show_focus_ring(window);
        self.base
            .when(!self.multiline, |field| field.h(px(28.0)))
            .px(px(8.0))
            .when(self.multiline, |field| field.py(px(6.0)))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(if ring {
                theme.focus_highlight()
            } else {
                theme.inset
            })
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(sp(12.5))
            .line_height(sp(16.0))
            .when_some(self.icon, |element, (path, size)| {
                element.child(icon(path, size, theme.text_tertiary))
            })
            .child(div().min_w_0().flex_1().child(self.input))
    }
}
