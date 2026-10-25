//! Shared desktop project dropdown. Callers own eligibility and selection effects;
//! each open panel owns its search, keyboard cursor, and virtualized list.
use crate::input::{InputEvent, TextInput};
use crate::theme::{Theme, hairline, sp};
use crate::ui::{
    icon,
    menu::{
        ConfirmEntry, ContextMenuHandle, MenuAlign, MenuItem, SelectNextEntry, SelectNextTab,
        SelectPreviousEntry, SelectPreviousTab, popover,
    },
    tooltip::Tooltip,
};
use gpui::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, ListAlignment, ListState,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, WeakEntity, Window, actions, div, list, prelude::FluentBuilder, px,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
const PICKER_ROW_HEIGHT: f32 = 26.0;

const LIST_MAX_HEIGHT: f32 = 260.0;

actions!(project_picker, [FirstProject, LastProject]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNextEntry, Some("ProjectPicker > TextInput")),
        KeyBinding::new("up", SelectPreviousEntry, Some("ProjectPicker > TextInput")),
        KeyBinding::new("tab", SelectNextTab, Some("ProjectPicker > TextInput")),
        KeyBinding::new(
            "shift-tab",
            SelectPreviousTab,
            Some("ProjectPicker > TextInput"),
        ),
        KeyBinding::new("enter", ConfirmEntry, Some("ProjectPicker > TextInput")),
        KeyBinding::new("home", FirstProject, Some("ProjectPicker > TextInput")),
        KeyBinding::new("end", LastProject, Some("ProjectPicker > TextInput")),
    ]);
}

#[derive(Clone, Copy)]
pub enum ProjectPickerVariant {
    NewTask,
    TaskProject,
    ListFilter,
    ProjectSelection,
}

#[derive(Clone)]
pub struct ProjectPickerHandle {
    menu: ContextMenuHandle,
    panel: Rc<RefCell<Option<OpenProjectPicker>>>,
}

struct OpenProjectPicker {
    view: Entity<ProjectPicker>,
    previous_focus: Option<FocusHandle>,
}

impl ProjectPickerHandle {
    pub fn new(menu: ContextMenuHandle) -> Self {
        let panel: Rc<RefCell<Option<OpenProjectPicker>>> = Rc::new(RefCell::new(None));
        let close_panel = Rc::downgrade(&panel);
        let menu = menu.on_toggle(move |open, window, cx| {
            if !open && let Some(panel) = close_panel.upgrade() {
                let closed = panel.borrow_mut().take();
                if let Some(previous) = closed.and_then(|panel| panel.previous_focus) {
                    window.focus(&previous, cx);
                }
            }
        });
        Self { menu, panel }
    }

    pub fn menu(&self) -> &ContextMenuHandle {
        &self.menu
    }

    pub fn is_open(&self) -> bool {
        self.menu.is_open()
    }
}

#[derive(Clone)]
pub struct ProjectPickerOption {
    /// Stable, unique identity; keeps star focus on the project when rows reorder.
    pub key: SharedString,
    pub item: MenuItem,
    /// Lowercase searchable name and any context-specific path metadata.
    pub search_text: String,
    pub detail: Option<SharedString>,
}

impl From<MenuItem> for ProjectPickerOption {
    fn from(item: MenuItem) -> Self {
        let key = match &item {
            MenuItem::Entry { label, .. } => label.clone(),
            _ => SharedString::default(),
        };
        let search_text = key.to_lowercase();
        Self {
            key,
            item,
            search_text,
            detail: None,
        }
    }
}

pub struct ProjectPickerContent {
    pub projects: Rc<Vec<ProjectPickerOption>>,
    pub leading: Vec<ProjectPickerOption>,
    pub footer: Vec<ProjectPickerOption>,
}

/// Existing menu sites keep their own typed callbacks and choice sets. Filter
/// variants pin their reset choice above the scroll area, independent of search.
#[track_caller]
pub fn project_dropdown_menu<E>(
    trigger: E,
    handle: &ProjectPickerHandle,
    align: MenuAlign,
    variant: ProjectPickerVariant,
    items: impl Fn(&mut App) -> Vec<MenuItem> + 'static,
) -> AnyElement
where
    E: ParentElement + Styled + InteractiveElement + IntoElement + 'static,
{
    project_popover(trigger, handle, align, move |cx| {
        let mut projects = items(cx)
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let mut option = ProjectPickerOption::from(item);
                option.key = format!("option-{index}").into();
                option
            })
            .collect::<Vec<_>>();
        let leading = if matches!(variant, ProjectPickerVariant::ListFilter) && !projects.is_empty()
        {
            vec![projects.remove(0)]
        } else {
            Vec::new()
        };
        ProjectPickerContent {
            projects: Rc::new(projects),
            leading,
            footer: Vec::new(),
        }
    })
}

#[track_caller]
pub fn project_popover<E>(
    trigger: E,
    handle: &ProjectPickerHandle,
    align: MenuAlign,
    content: impl Fn(&mut App) -> ProjectPickerContent + 'static,
) -> AnyElement
where
    E: ParentElement + Styled + InteractiveElement + IntoElement + 'static,
{
    let panel = handle.panel.clone();
    popover(trigger, &handle.menu, align, move |menu, window, cx| {
        let content = content(cx);
        let mut panel = panel.borrow_mut();
        let view = if let Some(view) = panel.as_ref() {
            view.view
                .update(cx, |this, cx| this.set_content(content, cx));
            view.view.clone()
        } else {
            let previous_focus = window.focused(cx);
            let view = cx.new(|cx| ProjectPicker::new(content, menu.clone(), window, cx));
            *panel = Some(OpenProjectPicker {
                view: view.clone(),
                previous_focus,
            });
            view
        };
        view.into_any_element()
    })
}

struct ProjectPicker {
    search: Entity<TextInput>,
    content: ProjectPickerContent,
    matches: Rc<Vec<usize>>,
    highlight: Option<usize>,
    list: ListState,
    menu: ContextMenuHandle,
    star_focus: Rc<HashMap<SharedString, FocusHandle>>,
    _search_subscription: Subscription,
}

impl ProjectPicker {
    fn new(
        content: ProjectPickerContent,
        menu: ContextMenuHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("input.search_projects"))
                .placeholder(tr!("input.search_projects"))
        });
        let focus = search.read(cx).focus_handle(cx);
        let weak = cx.entity().downgrade();
        let focus_menu = menu.clone();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                if focus_menu.is_open() && weak.upgrade().is_some() {
                    window.focus(&focus, cx);
                }
            });
        });
        let subscription = cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Edited) {
                this.refilter(cx);
                this.highlight = (!this.matches.is_empty()).then_some(this.content.leading.len());
                this.list.scroll_to_reveal_item(0);
                cx.notify();
            }
        });
        let mut this = Self {
            search,
            content,
            matches: Rc::new(Vec::new()),
            highlight: None,
            list: ListState::new(0, ListAlignment::Top, px(152.0)),
            menu,
            star_focus: Rc::new(HashMap::new()),
            _search_subscription: subscription,
        };
        this.refilter(cx);
        this
    }

    fn set_content(&mut self, content: ProjectPickerContent, cx: &mut Context<Self>) {
        let changed = self.content.projects.len() != content.projects.len()
            || self
                .content
                .projects
                .iter()
                .zip(content.projects.iter())
                .any(|(old, new)| old.key != new.key || old.search_text != new.search_text);
        self.content = content;
        if changed {
            self.refilter(cx);
        }
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).content().to_lowercase();
        let tokens = query.split_whitespace().collect::<Vec<_>>();
        let matches = matching_projects(&self.content.projects, &tokens);
        if matches != *self.matches {
            self.matches = Rc::new(matches);
            self.list
                .reset_with_uniform_height(self.matches.len(), px(PICKER_ROW_HEIGHT));
        }
        let focuses = Rc::make_mut(&mut self.star_focus);
        let keys = self
            .content
            .projects
            .iter()
            .map(|project| &project.key)
            .collect::<HashSet<_>>();
        focuses.retain(|key, _| keys.contains(key));
        for project in self.content.projects.iter() {
            if matches!(project.item, MenuItem::Entry { star: Some(_), .. }) {
                focuses
                    .entry(project.key.clone())
                    .or_insert_with(|| cx.focus_handle());
            }
        }
        if self.highlight.is_some_and(|index| index >= self.len()) {
            self.highlight = None;
        }
    }

    fn len(&self) -> usize {
        self.content.leading.len() + self.matches.len() + self.content.footer.len()
    }

    fn option(&self, index: usize) -> Option<&ProjectPickerOption> {
        let leading = self.content.leading.len();
        if index < leading {
            return self.content.leading.get(index);
        }
        let index = index - leading;
        if let Some(source) = self.matches.get(index) {
            return self.content.projects.get(*source);
        }
        self.content
            .footer
            .get(index.checked_sub(self.matches.len())?)
    }

    fn move_highlight(&mut self, reverse: bool, cx: &mut Context<Self>) {
        let len = self.len();
        if len == 0 {
            return;
        }
        self.highlight = Some(match (self.highlight, reverse) {
            (Some(0) | None, true) => len - 1,
            (Some(index), true) => index - 1,
            (Some(index), false) => (index + 1) % len,
            (None, false) => 0,
        });
        self.reveal(cx);
    }

    fn reveal(&self, cx: &mut Context<Self>) {
        if let Some(index) = self
            .highlight
            .and_then(|index| index.checked_sub(self.content.leading.len()))
            && index < self.matches.len()
        {
            self.list.scroll_to_reveal_item(index);
        }
        cx.notify();
    }

    fn confirm(&self, window: &mut Window, cx: &mut App) {
        let index = self.highlight.unwrap_or(0);
        if let Some(ProjectPickerOption {
            item:
                MenuItem::Entry {
                    on_click,
                    disabled: false,
                    ..
                },
            ..
        }) = self.option(index)
        {
            let on_click = on_click.clone();
            self.menu.close(window, cx);
            on_click(window, cx);
            window.refresh();
        }
    }

    fn focus_star(&self, reverse: bool, window: &mut Window, cx: &mut App) {
        let source = if reverse {
            self.matches
                .iter()
                .rev()
                .find(|source| self.has_star(**source))
        } else {
            self.matches.iter().find(|source| self.has_star(**source))
        };
        if let Some(source) = source {
            window.focus(&self.star_focus[&self.content.projects[*source].key], cx);
            if let Some(index) = self
                .matches
                .iter()
                .position(|candidate| candidate == source)
            {
                self.list.scroll_to_reveal_item(index);
            }
        }
    }

    fn has_star(&self, source: usize) -> bool {
        matches!(
            self.content.projects[source].item,
            MenuItem::Entry { star: Some(_), .. }
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        option: ProjectPickerOption,
        source: Option<(usize, FocusHandle)>,
        highlighted: bool,
        menu: ContextMenuHandle,
        weak: WeakEntity<Self>,
        window: &Window,
        cx: &mut App,
    ) -> AnyElement {
        let MenuItem::Entry {
            label,
            icon: glyph,
            selected,
            disabled,
            shortcut,
            star,
            on_click,
            ..
        } = option.item
        else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let mut row = div()
            .id(SharedString::from(format!(
                "project-picker-row-{}",
                option.key
            )))
            .w_full()
            .h(px(PICKER_ROW_HEIGHT))
            .px(px(8.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .text_size(sp(12.5))
            .line_height(sp(15.0))
            .text_color(theme.text)
            .when(highlighted, |row| row.bg(theme.overlay_strong))
            .hover(|row| row.bg(theme.overlay))
            .when(disabled, |row| row.opacity(0.5))
            .child(icon(glyph.unwrap_or("icons/folder.svg"), 12.0, theme.text))
            .child(div().min_w_0().flex_1().truncate().child(label))
            .when_some(option.detail, |row, detail| {
                row.child(div().text_color(theme.text_tertiary).child(detail))
            })
            .when(selected, |row| {
                row.child(icon("icons/check.svg", 11.0, theme.text_secondary))
            })
            .when_some(shortcut, |row, shortcut| {
                row.when_some(shortcut.resolve(window, cx), |row, label| {
                    row.child(div().flex_none().text_color(theme.text_ghost).child(label))
                })
            })
            .on_click(move |_, window, cx| {
                if disabled {
                    return;
                }
                menu.close(window, cx);
                on_click(window, cx);
                window.refresh();
            });
        if let (Some(star), Some((source, focus))) = (star, source) {
            let toggle = star.on_toggle.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!(
                        "project-picker-star-{}",
                        option.key
                    )))
                    .flex_none()
                    .size(px(20.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .track_focus(&focus)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .hover(|style| style.bg(theme.overlay_strong))
                    .tooltip(Tooltip::text(if star.starred {
                        tr!("common.unstar")
                    } else {
                        tr!("common.star")
                    }))
                    .child(icon(
                        if star.starred {
                            "icons/star-filled.svg"
                        } else {
                            "icons/star.svg"
                        },
                        12.0,
                        if star.starred {
                            theme.favorite
                        } else {
                            theme.text_ghost
                        },
                    ))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        toggle(window, cx);
                    })
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        let _ = weak.update(cx, |this, cx| match event.keystroke.key.as_str() {
                            "enter" | "space" => {
                                cx.stop_propagation();
                                (star.on_toggle)(window, cx);
                            }
                            "tab" => {
                                cx.stop_propagation();
                                let position = this
                                    .matches
                                    .iter()
                                    .position(|candidate| *candidate == source);
                                let next = position
                                    .and_then(|position| {
                                        if event.keystroke.modifiers.shift {
                                            this.matches[..position]
                                                .iter()
                                                .rev()
                                                .find(|source| this.has_star(**source))
                                        } else {
                                            this.matches[position + 1..]
                                                .iter()
                                                .find(|source| this.has_star(**source))
                                        }
                                    })
                                    .copied();
                                if let Some(next) = next {
                                    window.focus(
                                        &this.star_focus[&this.content.projects[next].key],
                                        cx,
                                    );
                                    if let Some(index) =
                                        this.matches.iter().position(|source| *source == next)
                                    {
                                        this.list.scroll_to_reveal_item(index);
                                    }
                                } else {
                                    window.focus(&this.search.read(cx).focus_handle(cx), cx);
                                }
                            }
                            "up" | "down" | "left" | "right" => {
                                cx.stop_propagation();
                                window.focus(&this.search.read(cx).focus_handle(cx), cx);
                            }
                            _ => {}
                        });
                    }),
            );
        }
        row.into_any_element()
    }
}

fn matching_projects(projects: &[ProjectPickerOption], tokens: &[&str]) -> Vec<usize> {
    projects
        .iter()
        .enumerate()
        .filter_map(|(index, project)| {
            tokens
                .iter()
                .all(|token| project.search_text.contains(token))
                .then_some(index)
        })
        .collect()
}

impl Render for ProjectPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let leading = self
            .content
            .leading
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, option)| {
                Self::render_row(
                    option,
                    None,
                    self.highlight == Some(index),
                    self.menu.clone(),
                    cx.entity().downgrade(),
                    window,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let offset = self.content.leading.len() + self.matches.len();
        let footer = self
            .content
            .footer
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, option)| {
                Self::render_row(
                    option,
                    None,
                    self.highlight == Some(offset + index),
                    self.menu.clone(),
                    cx.entity().downgrade(),
                    window,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let rows = if self.matches.is_empty() {
            div()
                .id("project-picker-list-empty")
                .h(px(64.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(sp(12.5))
                .text_color(theme.text_ghost)
                .child(tr!("projects.none_found"))
                .into_any_element()
        } else {
            let weak = cx.entity().downgrade();
            let projects = self.content.projects.clone();
            let matches = self.matches.clone();
            let star_focus = self.star_focus.clone();
            let leading = self.content.leading.len();
            let highlight = self.highlight;
            let menu = self.menu.clone();
            div()
                .id("project-picker-list")
                .debug_selector(|| "project-picker-list".into())
                .h(px(
                    (self.matches.len() as f32 * PICKER_ROW_HEIGHT).min(LIST_MAX_HEIGHT)
                ))
                .flex_none()
                .px(px(4.0))
                .child(
                    list(self.list.clone(), move |index, window, cx| {
                        let Some(source) = matches.get(index).copied() else {
                            return div().into_any_element();
                        };
                        Self::render_row(
                            projects[source].clone(),
                            star_focus
                                .get(&projects[source].key)
                                .cloned()
                                .map(|focus| (source, focus)),
                            highlight == Some(leading + index),
                            menu.clone(),
                            weak.clone(),
                            window,
                            cx,
                        )
                    })
                    .size_full(),
                )
                .into_any_element()
        };
        div()
            .key_context("ProjectPicker")
            .w(px(360.0))
            .max_h(px(420.0))
            .rounded(px(16.0))
            .overflow_hidden()
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.raised)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_action(
                cx.listener(|this, _: &SelectNextEntry, _, cx| this.move_highlight(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectPreviousEntry, _, cx| this.move_highlight(true, cx)),
            )
            .on_action(cx.listener(|this, _: &ConfirmEntry, window, cx| this.confirm(window, cx)))
            .on_action(
                cx.listener(|this, _: &SelectNextTab, window, cx| {
                    this.focus_star(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SelectPreviousTab, window, cx| {
                this.focus_star(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FirstProject, _, cx| {
                this.highlight = (this.len() > 0).then_some(0);
                this.reveal(cx);
            }))
            .on_action(cx.listener(|this, _: &LastProject, _, cx| {
                this.highlight = this.len().checked_sub(1);
                this.reveal(cx);
            }))
            .child(
                div()
                    .h(px(52.0))
                    .px(px(12.0))
                    .pt(px(10.0))
                    .pb(px(8.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .h(px(34.0))
                            .px(px(10.0))
                            .rounded(px(11.0))
                            .bg(theme.surface)
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("icons/search.svg", 15.0, theme.text_secondary))
                            .child(div().flex_1().min_w_0().child(self.search.clone())),
                    ),
            )
            .child(
                div()
                    .px(px(14.0))
                    .pt(px(3.0))
                    .pb(px(7.0))
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(tr!("projects.title")),
            )
            .children(leading)
            .child(rows)
            .when(!footer.is_empty(), |card| {
                card.child(
                    div()
                        .mx(px(6.0))
                        .my(px(4.0))
                        .h(hairline())
                        .bg(theme.separator),
                )
            })
            .children(footer)
            .child(div().h(px(4.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, point};
    use std::cell::Cell;

    struct Harness {
        handle: ProjectPickerHandle,
        projects: Vec<MenuItem>,
        task: bool,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let trigger = div().id("project-trigger").w(px(120.0)).h(px(26.0));
            let projects = self.projects.clone();
            if self.task {
                project_popover(trigger, &self.handle, MenuAlign::BelowLeft, move |_| {
                    let mut projects = projects
                        .iter()
                        .cloned()
                        .map(ProjectPickerOption::from)
                        .collect::<Vec<_>>();
                    let footer = projects.split_off(projects.len() - 2);
                    ProjectPickerContent {
                        projects: Rc::new(projects),
                        leading: Vec::new(),
                        footer,
                    }
                })
            } else {
                project_dropdown_menu(
                    trigger,
                    &self.handle,
                    MenuAlign::BelowLeft,
                    ProjectPickerVariant::ListFilter,
                    move |_| projects.clone(),
                )
            }
        }
    }

    fn settle(cx: &mut gpui::VisualTestContext) {
        cx.run_until_parked();
        for _ in 0..2 {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
            });
            cx.run_until_parked();
        }
    }

    fn setup(cx: &mut TestAppContext) -> ProjectPickerHandle {
        cx.update(|cx| {
            crate::input::init(cx);
            crate::ui::menu::init(cx);
            init(cx);
            cx.set_reduce_motion(true);
            ProjectPickerHandle::new(ContextMenuHandle::new(cx))
        })
    }

    fn item(label: &str, selected: Rc<RefCell<Vec<String>>>) -> MenuItem {
        let value = label.to_owned();
        MenuItem::new(value.clone(), move |_, _| {
            selected.borrow_mut().push(value.clone())
        })
    }

    #[gpui::test]
    fn filter_search_selects_matching_project_and_resets_on_reopen(cx: &mut TestAppContext) {
        let handle = setup(cx);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let mut projects = vec![
            item("All projects", selected.clone()),
            item("Beta remote", selected.clone()),
        ];
        projects.extend((0..40).map(|index| item(&format!("Project {index}"), selected.clone())));
        let (_harness, cx) = cx.add_window_view(|_, _| Harness {
            handle: handle.clone(),
            projects,
            task: false,
        });
        cx.update(|window, cx| window.focus(handle.menu.trigger_focus_handle(), cx));
        cx.simulate_keystrokes("enter");
        settle(cx);
        let panel = handle.panel.borrow().as_ref().unwrap().view.clone();
        let focus = cx.read_entity(&panel, |panel, cx| panel.search.read(cx).focus_handle(cx));
        cx.update(|window, _| assert!(focus.is_focused(window), "opening focuses search"));
        assert_eq!(
            cx.debug_bounds("project-picker-list").unwrap().size.height,
            px(LIST_MAX_HEIGHT)
        );

        cx.simulate_input("BETA remote");
        cx.simulate_keystrokes("enter");
        assert_eq!(&*selected.borrow(), &["Beta remote"]);
        assert!(!handle.is_open());
        cx.update(|window, _| assert!(handle.menu.trigger_focus_handle().is_focused(window)));

        cx.simulate_keystrokes("enter");
        settle(cx);
        let panel = handle.panel.borrow().as_ref().unwrap().view.clone();
        cx.read_entity(&panel, |panel, cx| {
            assert!(panel.search.read(cx).content().is_empty())
        });
        cx.simulate_keystrokes("end enter");
        assert_eq!(selected.borrow().last().unwrap(), "Project 39");

        cx.simulate_keystrokes("enter");
        settle(cx);
        cx.simulate_input("does not exist");
        cx.simulate_keystrokes("home enter");
        assert_eq!(
            selected.borrow().last().unwrap(),
            "All projects",
            "reset stays available with no matches"
        );
    }

    #[gpui::test]
    fn task_stars_do_not_select_and_footer_remains_keyboard_reachable(cx: &mut TestAppContext) {
        let handle = setup(cx);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let toggled = Rc::new(Cell::new(0));
        let on_toggle = toggled.clone();
        let projects = vec![
            item("Alpha", selected.clone())
                .star(false, move |_, _| on_toggle.set(on_toggle.get() + 1)),
            item("Beta", selected.clone()).star(false, |_, _| {
                panic!("focus must stay with Alpha after reordering")
            }),
            item("New project", selected.clone()),
            item("No project", selected.clone()),
        ];
        let (harness, cx) = cx.add_window_view(|_, _| Harness {
            handle: handle.clone(),
            projects,
            task: true,
        });
        cx.simulate_mouse_down(
            point(px(10.0), px(10.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        settle(cx);
        cx.simulate_keystrokes("tab space");
        assert_eq!(toggled.get(), 1);
        assert!(selected.borrow().is_empty());
        assert!(handle.is_open());
        let focused = cx.update(|window, cx| window.focused(cx));
        cx.update(|_, cx| {
            harness.update(cx, |harness, cx| {
                harness.projects.swap(0, 1);
                cx.notify();
            })
        });
        settle(cx);
        cx.update(|window, cx| assert_eq!(window.focused(cx), focused));
        cx.simulate_keystrokes("space");
        assert_eq!(toggled.get(), 2);

        cx.simulate_keystrokes("tab");
        cx.simulate_input("unmatched");
        cx.simulate_keystrokes("down down enter");
        assert_eq!(&*selected.borrow(), &["No project"]);
        assert!(!handle.is_open());
    }
}
