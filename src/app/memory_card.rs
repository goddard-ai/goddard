use super::*;
use crate::ui::ActivationExt;

/// State belongs to the record, not its current virtualized row or position.
#[derive(Default)]
pub(super) struct MemoryCardState {
    pub expanded: bool,
    pub viewport: UserMessageScrollViewport,
    pub markdown: MarkdownView,
}

#[derive(IntoElement)]
pub(super) struct MemoryCard {
    pub id: String,
    pub bucket: String,
    pub removed: bool,
    pub expanded: bool,
    pub viewport: UserMessageScrollViewport,
    pub body: Option<AnyElement>,
    pub selection: TranscriptSelection,
    pub focus: FocusHandle,
    pub disclosure_focus: FocusHandle,
    pub undo_focus: FocusHandle,
    pub theme: Theme,
    pub owner: EntityId,
    pub toggle: Rc<dyn Fn(&mut Window, &mut App)>,
    pub undo: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl RenderOnce for MemoryCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let expandable = self.expanded || self.viewport.overflowing.get();
        let surface = theme.surface.blend(theme.overlay.opacity(0.7));
        let toggle = self.toggle.clone();
        let selection = self.selection;
        let expanded = self.expanded;
        let scroll = self.viewport.scroll_handle.clone();
        let overflowing = self.viewport.overflowing.clone();
        let owner = self.owner;
        let label = format!("Memory recorded in {}", self.bucket);
        let card_selector = self.id.clone();
        let undo_selector = format!("{}-undo", self.id);
        let disclosure_selector = format!("{}-disclosure", self.id);
        let mut normal = font(crate::fonts::current(cx).ui.clone());
        normal.weight = FontWeight::NORMAL;
        let mut bold = normal.clone();
        bold.weight = FontWeight::BOLD;
        let header = gpui::StyledText::new(label.clone()).with_runs(vec![
            TextRun {
                len: "Memory recorded in ".len(),
                font: normal,
                color: theme.text,
                background_color: None,
                underline: None,
                strikethrough: None,
            },
            TextRun {
                len: self.bucket.len(),
                font: bold,
                color: theme.text,
                background_color: None,
                underline: None,
                strikethrough: None,
            },
        ]);
        let action = if self.removed {
            div()
                .text_size(sp(11.0))
                .text_color(theme.text_tertiary)
                .child(tr!("boss.memory_removed"))
                .into_any_element()
        } else {
            boss::boss_button(format!("{}-undo", self.id), tr!("boss.memory_undo"), &theme)
                .debug_selector(move || undo_selector.clone().into())
                .track_focus(&self.undo_focus)
                .tab_stop(true)
                .aria_label(tr!("boss.memory_undo"))
                .child(icon("icons/undo.svg", 12.0, theme.text_secondary))
                .child(tr!("boss.memory_undo"))
                .on_activation_app(move |window, cx| (self.undo)(window, cx))
                .into_any_element()
        };
        div()
            .id(SharedString::from(self.id.clone()))
            .debug_selector(move || card_selector.clone().into())
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .rounded(px(11.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(surface)
            .px(px(10.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when(expandable, |card| {
                card.track_focus(&self.focus)
                    .tab_index(0)
                    .tab_stop(true)
                    .cursor_pointer()
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .hover(|style| style.bg(theme.surface.blend(theme.overlay)))
                    .aria_label(SharedString::from(format!(
                        "{label}; {}",
                        if expanded { "Show less" } else { "Show more" }
                    )))
                    .on_click(move |event, window, cx| {
                        if !memory_card_selection_click(event, &selection) {
                            toggle(window, cx);
                        }
                        cx.stop_propagation();
                    })
                    .on_key_down({
                        let toggle = self.toggle.clone();
                        move |event: &KeyDownEvent, window, cx| {
                            if !event.keystroke.modifiers.modified()
                                && matches!(event.keystroke.key.as_str(), "enter" | "space")
                            {
                                toggle(window, cx);
                                cx.stop_propagation();
                            }
                        }
                    })
            })
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(icon("icons/brain.svg", 11.0, theme.text))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(11.0))
                            .line_height(sp(14.0))
                            .text_color(theme.text)
                            .child(header),
                    )
                    .child(action),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .relative()
                    .child(
                        div()
                            .id(SharedString::from(format!("{}-text", self.id)))
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .when(!expanded, |body| body.max_h(sp(30.0)))
                            .track_scroll(&self.viewport.scroll_handle)
                            .children(self.body),
                    )
                    .child(
                        canvas(
                            move |_, window, _| {
                                // Preserve the collapsed measurement while expanded. Re-measure
                                // on collapse and resize, just as capped user bubbles do.
                                if !expanded {
                                    let clipped = scroll.max_offset().y > px(0.5);
                                    if overflowing.replace(clipped) != clipped {
                                        // GPUI ignores view invalidation while drawing.
                                        // Defer until the frame is complete so an idle
                                        // transcript also reveals its disclosure.
                                        window.on_next_frame(move |_, cx| cx.notify(owner));
                                    }
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(1.0)),
                    ),
            )
            .when(expandable, |card| {
                card.child(
                    div()
                        .id(SharedString::from(format!("{}-disclosure", self.id)))
                        .debug_selector(move || disclosure_selector.clone().into())
                        .track_focus(&self.disclosure_focus)
                        .tab_index(0)
                        .tab_stop(true)
                        .self_start()
                        .h(px(24.0))
                        .px(px(4.0))
                        .ml(-px(4.0))
                        .rounded(px(4.0))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .hover(|style| style.text_color(theme.text))
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .aria_label(if expanded { "Show less" } else { "Show more" })
                        .child(if expanded { "Show less" } else { "Show more" })
                        .on_activation_app(move |window, cx| (self.toggle)(window, cx)),
                )
            })
    }
}

fn memory_card_selection_click(event: &ClickEvent, selection: &TranscriptSelection) -> bool {
    let selected = selection.selection.borrow();
    if selected.is_dragging() || !selected.is_empty() {
        return true;
    }
    match event {
        ClickEvent::Mouse(event) => {
            let moved = event.up.position - event.down.position;
            event.down.click_count > 1 || moved.x.abs() > px(4.0) || moved.y.abs() > px(4.0)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    struct Harness {
        state: MemoryCardState,
        selection: TranscriptSelection,
        focus: FocusHandle,
        disclosure_focus: FocusHandle,
        undo_focus: FocusHandle,
        toggles: usize,
        undos: usize,
        links: usize,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = Theme::current(cx);
            let palette = MarkdownPalette::from_theme(&theme);
            let link_owner = cx.weak_entity();
            let ctx = MarkdownCtx::new(
                "fixture-memory",
                &palette,
                MarkdownMetrics::COMPACT,
                self.selection.clone(),
            )
            .with_link_handler(Rc::new(move |_, _, cx| {
                let _ = link_owner.update(cx, |this, _| this.links += 1);
                cx.stop_propagation();
            }));
            let body = md::render::markdown(&self.state.markdown, &ctx);
            let toggle_owner = cx.weak_entity();
            let undo_owner = cx.weak_entity();
            let selection = self.selection.clone();
            div()
                .w(px(360.0))
                .flex()
                .flex_col()
                .child(md::render::frame_reset(self.selection.clone()))
                .child(MemoryCard {
                    id: "fixture".into(),
                    bucket: "Project facts".into(),
                    removed: false,
                    expanded: self.state.expanded,
                    viewport: self.state.viewport.clone(),
                    body,
                    selection: self.selection.clone(),
                    focus: self.focus.clone(),
                    disclosure_focus: self.disclosure_focus.clone(),
                    undo_focus: self.undo_focus.clone(),
                    theme,
                    owner: cx.entity_id(),
                    toggle: Rc::new(move |_, cx| {
                        let _ = toggle_owner.update(cx, |this, cx| {
                            this.state.expanded = !this.state.expanded;
                            this.state
                                .viewport
                                .scroll_handle
                                .set_offset(gpui::Point::default());
                            this.toggles += 1;
                            cx.notify();
                        });
                    }),
                    undo: Rc::new(move |_, cx| {
                        let _ = undo_owner.update(cx, |this, cx| {
                            this.undos += 1;
                            cx.notify();
                        });
                    }),
                })
                .child(
                    canvas(
                        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
                        move |_, hitbox, window, _| {
                            md::render::install_selection_input(hitbox, window, &selection, None)
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
        }
    }

    fn harness<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
    ) -> (Entity<Harness>, &'a mut VisualTestContext) {
        cx.add_window_view(|_, cx| {
            let mut state = MemoryCardState::default();
            state
                .markdown
                .set_text_with_soft_breaks_as_newlines(text, false, true);
            Harness {
                state,
                selection: TranscriptSelection::default(),
                focus: cx.focus_handle(),
                disclosure_focus: cx.focus_handle(),
                undo_focus: cx.focus_handle(),
                toggles: 0,
                undos: 0,
                links: 0,
            }
        })
    }

    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, id: &'static str) {
        let bounds = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("{id} is painted"));
        cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::none());
        settle(cx);
    }

    #[gpui::test]
    fn memory_card_measures_overflow_and_toggles_once_without_undo_bubbling(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = harness(cx, "First line\nSecond line\nThird line\nFourth line");
        settle(cx);
        assert!(view.read_with(cx, |view, _| view.state.viewport.overflowing.get()));
        let collapsed_height = cx.debug_bounds("fixture").unwrap().size.height;
        click(cx, "fixture-disclosure");
        assert!(cx.debug_bounds("fixture").unwrap().size.height > collapsed_height);
        assert!(view.read_with(cx, |view, _| view.state.expanded));
        assert_eq!(view.read_with(cx, |view, _| view.toggles), 1);
        click(cx, "fixture-undo");
        assert_eq!(
            view.read_with(cx, |view, _| (view.undos, view.toggles)),
            (1, 1)
        );
        let focus = view.read_with(cx, |view, _| view.disclosure_focus.clone());
        cx.update(|window, app| focus.focus(window, app));
        cx.simulate_keystrokes("space");
        settle(cx);
        assert!(!view.read_with(cx, |view, _| view.state.expanded));
        assert_eq!(
            cx.debug_bounds("fixture").unwrap().size.height,
            collapsed_height
        );
        assert_eq!(view.read_with(cx, |view, _| view.toggles), 2);
        let focus = view.read_with(cx, |view, _| view.undo_focus.clone());
        cx.update(|window, app| focus.focus(window, app));
        cx.simulate_keystrokes("enter");
        settle(cx);
        assert_eq!(
            view.read_with(cx, |view, _| (view.undos, view.toggles)),
            (2, 2)
        );
        // Click the card header, outside text selection and either child control.
        let bounds = cx.debug_bounds("fixture").unwrap();
        let point = point(bounds.left() + px(12.0), bounds.top() + px(12.0));
        cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::none());
        settle(cx);
        assert_eq!(view.read_with(cx, |view, _| view.toggles), 3);
        assert!(view.read_with(cx, |view, _| view.state.expanded));
    }

    #[gpui::test]
    fn memory_card_short_text_has_no_disclosure_and_selection_keeps_full_content(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = harness(cx, "Short memory.");
        settle(cx);
        assert!(!view.read_with(cx, |view, _| view.state.viewport.overflowing.get()));
        assert!(cx.debug_bounds("fixture-disclosure").is_none());
        click(cx, "fixture");
        assert_eq!(view.read_with(cx, |view, _| view.toggles), 0);
        view.update(cx, |view, cx| {
            view.state.markdown.set_text_with_soft_breaks_as_newlines(
                "First line\nSecond line\nThird line — 東京\n[Full link](https://example.com)",
                false,
                true,
            );
            cx.notify();
        });
        settle(cx);
        click(cx, "fixture-disclosure");
        view.read_with(cx, |view, _| {
            let registry = view.selection.registry.borrow();
            let entries = registry.entries();
            let spans = registry.resolve(
                (0, 0),
                (entries.len() - 1, entries.last().unwrap().text.len()),
            );
            let mut selection = md::selection::Selection::default();
            selection.set_spans(spans);
            assert_eq!(
                selection.markdown(),
                "First line\nSecond line\nThird line — 東京\n[Full link](https://example.com)"
            );
        });
        let (start, end) = view.read_with(cx, |view, _| {
            let registry = view.selection.registry.borrow();
            let entry = &registry.entries()[0];
            let bounds = md::render::text_range_bounds(&entry.geometry, &(0..entry.text.len()))[0];
            (
                point(bounds.left() + px(4.0), bounds.top() + px(8.0)),
                point(bounds.left() + px(65.0), bounds.top() + px(8.0)),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        settle(cx);
        assert_eq!(view.read_with(cx, |view, _| view.toggles), 1);
        assert!(!view.read_with(cx, |view, _| view.selection.selection.borrow().is_empty()));
        let link = view.read_with(cx, |view, _| {
            let registry = view.selection.registry.borrow();
            let entry = registry.entries().last().unwrap();
            let start = entry.text.find("Full link").unwrap();
            md::render::text_range_bounds(&entry.geometry, &(start..start + "Full link".len()))[0]
                .center()
        });
        cx.simulate_mouse_down(link, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(link, MouseButton::Left, Modifiers::none());
        settle(cx);
        assert_eq!(
            view.read_with(cx, |view, _| (view.links, view.toggles)),
            (1, 1)
        );
    }
}
