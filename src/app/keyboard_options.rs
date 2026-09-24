//! Centered option lists opened by keyboard shortcuts. Mouse-triggered menus
//! stay anchored to their controls; keyboard shortcuts use this surface so
//! the available choices are visible before one is applied.

use super::*;
use crate::ui::ActivationExt;
use std::sync::Arc;

const MODAL_WIDTH: f32 = 480.0;
const MODAL_RADIUS: f32 = 17.0;
const MODAL_INSET: f32 = 6.0;
const ITEM_HEIGHT: f32 = 52.0;
const SECTION_HEIGHT: f32 = 28.0;
const SEARCH_HEIGHT: f32 = 48.0;
const ROW_RADIUS: f32 = 10.0;

#[derive(Clone)]
pub(super) enum KeyboardOptionAction {
    Model {
        provider: ProviderKind,
        model: String,
        effort: Option<String>,
        fast: bool,
    },
    AutoRoute,
    ReasoningEffort(Option<String>),
    RuntimeMode(RuntimeMode),
    Environment(SessionEnvironment),
    Workspace(SessionWorkspace),
    Branch(String),
    CreateBranch,
}

#[derive(Clone)]
pub(super) struct KeyboardOptionChoice {
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<&'static str>,
    pub selected: bool,
    pub enabled: bool,
    pub action: KeyboardOptionAction,
}

impl KeyboardOptionChoice {
    pub(super) fn new(
        label: impl Into<String>,
        description: Option<String>,
        icon: Option<&'static str>,
        selected: bool,
        enabled: bool,
        action: KeyboardOptionAction,
    ) -> Self {
        Self {
            label: label.into(),
            description,
            icon,
            selected,
            enabled,
            action,
        }
    }
}

#[derive(Clone)]
pub(super) enum KeyboardOptionItem {
    Section(String),
    Choice(KeyboardOptionChoice),
    /// A row straight from the shared model picker's list — carries the
    /// full [`PickerRow`] so the ⌘/ modal can draw the same row chrome the
    /// anchored panel does instead of flattening it to label/description.
    Model {
        row: model_picker::PickerRow,
        selected: bool,
        enabled: bool,
    },
}

impl KeyboardOptionItem {
    /// Whether the highlight or a pick can land on this item — sections
    /// are labels, not choices.
    pub(super) fn selectable(&self) -> bool {
        !matches!(self, KeyboardOptionItem::Section(_))
    }

    /// Whether this item marks the current selection.
    pub(super) fn selected(&self) -> bool {
        match self {
            KeyboardOptionItem::Section(_) => false,
            KeyboardOptionItem::Choice(choice) => choice.selected,
            KeyboardOptionItem::Model { selected, .. } => *selected,
        }
    }

    /// The item's height in the modal's content-height sum.
    fn height(&self) -> f32 {
        match self {
            KeyboardOptionItem::Section(_) => SECTION_HEIGHT,
            KeyboardOptionItem::Choice(_) => ITEM_HEIGHT,
            KeyboardOptionItem::Model { .. } => model_picker::MODEL_PICKER_ROW_HEIGHT.into(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum KeyboardOptionFocus {
    Modal,
    BranchCreate,
}

/// A picker opened by holding a modifier chord commits like the ⌘N project
/// switcher: tapping the chord again steps the highlight, and letting the
/// modifiers go picks whatever is highlighted. Which chord opened the modal
/// is what a repeated press cycles back through.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum KeyboardOptionsChord {
    /// ⌘⌥N — no search field, so a bare press-release should still mean
    /// something: the open lands one choice past the current one.
    Workspace,
    /// ⌘⌥⇧N — has a branch filter; a release before the first step leaves
    /// the picker open with the search field focused instead of committing.
    Branch,
    /// ⌘/ — has a model filter; like the branch picker, a release before
    /// the first step leaves the picker open with the search focused.
    Model,
}

impl KeyboardOptionsChord {
    /// The modifiers whose release ends the hold. ⇧ is direction in the
    /// branch chord, not part of the hold — that gesture ends when ⌘ or ⌥
    /// comes up. The model chord's hold is ⌘ alone.
    fn hold_down(self, secondary: bool, alt: bool) -> bool {
        match self {
            KeyboardOptionsChord::Workspace | KeyboardOptionsChord::Branch => secondary && alt,
            KeyboardOptionsChord::Model => secondary,
        }
    }
}

pub(super) struct KeyboardOptionsUi {
    open: bool,
    title: String,
    items: Arc<Vec<KeyboardOptionItem>>,
    content_height: f32,
    highlighted: Option<usize>,
    creating_branch: bool,
    /// Which chord picker the modal is showing — set whether the gesture or
    /// a menu click opened it, so a chord tap mid-modal knows what to step.
    chord: Option<KeyboardOptionsChord>,
    /// The chord's modifiers are held right now — a release commits. Only
    /// armed while the hold is real, so a menu-opened modal ignores stray
    /// modifier traffic.
    armed: bool,
    /// A chord tap already moved the highlight during this hold — the
    /// release that ends it commits rather than parking on the search field.
    cycled: bool,
    search: Option<Entity<TextInput>>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    list: ListState,
    generation: u64,
}

impl KeyboardOptionsUi {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            title: String::new(),
            items: Arc::new(Vec::new()),
            content_height: 0.0,
            highlighted: None,
            creating_branch: false,
            chord: None,
            armed: false,
            cycled: false,
            search: None,
            focus,
            previous_focus: None,
            list: ListState::new(0, ListAlignment::Top, px(ITEM_HEIGHT)),
            generation: 0,
        }
    }
}

impl Waku {
    pub(super) fn open_access_control_options(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self.composer_session();
        let selected_mode = session
            .map(|session| session.runtime_mode)
            .unwrap_or_default();
        let environment = session.map(AgentSession::environment).unwrap_or_default();
        let provider = session.map(|session| session.provider).unwrap_or_default();
        let sandbox_enabled = self.state.sandbox_experiment_enabled;
        let busy = session.is_some_and(AgentSession::is_busy);
        let started = session.is_some_and(AgentSession::has_started);
        let mut items = vec![KeyboardOptionItem::Section(
            tr!("keyboard_options.access_mode").to_string(),
        )];
        items.extend(RuntimeMode::ACCESS_OPTIONS.into_iter().map(|mode| {
            KeyboardOptionItem::Choice(KeyboardOptionChoice::new(
                mode.label(),
                Some(mode.description()),
                Some(mode.icon()),
                mode == selected_mode,
                !busy,
                KeyboardOptionAction::RuntimeMode(mode),
            ))
        }));
        if sandbox_enabled {
            items.push(KeyboardOptionItem::Section(
                tr!("sandbox.environment").to_string(),
            ));
            let mut environments = vec![
                (
                    SessionEnvironment::Local,
                    tr!("sandbox.this_mac").to_string(),
                    tr!("sandbox.this_mac_description").to_string(),
                ),
                (
                    SessionEnvironment::Sandbox,
                    tr!("sandbox.sandbox_vm").to_string(),
                    if provider.supports_sandbox() {
                        tr!("sandbox.sandbox_vm_description").to_string()
                    } else {
                        tr!(
                            "sandbox.sandbox_vm_unsupported",
                            provider = provider.display_name()
                        )
                        .to_string()
                    },
                ),
            ];
            if provider.supports_cloud() {
                environments.push((
                    SessionEnvironment::Cloud,
                    tr!("sandbox.provider_cloud", provider = provider.display_name()).to_string(),
                    tr!(
                        "sandbox.provider_cloud_description",
                        provider = provider.display_name()
                    )
                    .to_string(),
                ));
            }
            items.extend(environments.into_iter().map(|(value, label, description)| {
                KeyboardOptionItem::Choice(KeyboardOptionChoice::new(
                    label,
                    Some(description),
                    Some(value.icon()),
                    value == environment,
                    !started,
                    KeyboardOptionAction::Environment(value),
                ))
            }));
        }
        let highlighted = items.iter().position(|item| {
            matches!(
                item,
                KeyboardOptionItem::Choice(choice) if choice.selected
            )
        });
        self.open_keyboard_options(
            tr!("keyboard_options.choose_access_control").to_string(),
            items,
            highlighted,
            KeyboardOptionFocus::Modal,
            None,
            window,
            cx,
        );
    }

    pub(super) fn open_keyboard_options(
        &mut self,
        title: String,
        items: Vec<KeyboardOptionItem>,
        highlighted: Option<usize>,
        focus_target: KeyboardOptionFocus,
        chord: Option<KeyboardOptionsChord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if items.is_empty() {
            return;
        }

        let open_menus = self
            .menus
            .borrow()
            .values()
            .filter(|menu| menu.is_open())
            .cloned()
            .collect::<Vec<_>>();
        let has_competing_overlay = self.keyboard_options.open
            || self.project_switcher.is_open()
            || self.task_switcher.is_open()
            || self.command_palette.is_open()
            || !open_menus.is_empty();
        let previous_focus = if has_competing_overlay {
            Some(self.composer_focus(cx))
        } else {
            window.focused(cx)
        };

        if self.project_switcher.is_open() {
            self.cancel_project_switcher(window, cx);
        }
        if self.task_switcher.is_open() {
            self.cancel_task_switcher(window, cx);
        }
        if self.command_palette.is_open() {
            self.toggle_command_palette_action(&ToggleCommandPalette, window, cx);
        }

        let content_height = items.iter().map(KeyboardOptionItem::height).sum();
        let first_choice = items.iter().position(KeyboardOptionItem::selectable);
        let highlighted = highlighted
            .filter(|index| {
                items
                    .get(*index)
                    .is_some_and(KeyboardOptionItem::selectable)
            })
            .or(first_choice);
        self.keyboard_options.open = true;
        self.keyboard_options.title = title;
        self.keyboard_options.items = Arc::new(items);
        self.keyboard_options.content_height = content_height;
        self.keyboard_options.highlighted = highlighted;
        self.keyboard_options.creating_branch = false;
        self.keyboard_options.chord = chord;
        // The hold counts only if the chord's modifiers are actually down —
        // a menu dispatch arrives with none held and must not commit on the
        // next stray release.
        self.keyboard_options.armed = chord.is_some_and(|chord| {
            chord.hold_down(window.modifiers().secondary(), window.modifiers().alt)
        });
        self.keyboard_options.cycled = false;
        self.keyboard_options.search = match chord {
            Some(KeyboardOptionsChord::Branch) => Some(self.branch_search.clone()),
            Some(KeyboardOptionsChord::Model) => Some(self.model_picker.search.clone()),
            _ => None,
        };
        self.keyboard_options.previous_focus = previous_focus;
        self.keyboard_options.list = ListState::new(
            self.keyboard_options.items.len(),
            ListAlignment::Top,
            px(ITEM_HEIGHT),
        );
        if let Some(index) = highlighted {
            self.keyboard_options.list.scroll_to_reveal_item(index);
        }
        self.keyboard_options.generation = self.keyboard_options.generation.wrapping_add(1);
        let generation = self.keyboard_options.generation;
        let weak = cx.entity().downgrade();
        let focus = match focus_target {
            KeyboardOptionFocus::Modal => self.keyboard_options_modal_focus(cx),
            KeyboardOptionFocus::BranchCreate => self.branch_create_input.read(cx).focus_handle(cx),
        };

        if !open_menus.is_empty() {
            window.defer(cx, move |window, cx| {
                for menu in open_menus {
                    menu.close(window, cx);
                }
            });
        }
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let should_focus = weak
                    .update(cx, |this, _| {
                        this.keyboard_options.open && this.keyboard_options.generation == generation
                    })
                    .unwrap_or(false);
                if should_focus {
                    window.focus(&focus, cx);
                }
            });
        });
        // The workspace picker has no search to land in, so a bare
        // press-release should still change something — open one choice
        // past the current one.
        if self.keyboard_options.armed && matches!(chord, Some(KeyboardOptionsChord::Workspace)) {
            self.move_keyboard_option_highlight(1, cx);
        }
        cx.notify();
    }

    pub(super) fn render_keyboard_options(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.keyboard_options.open {
            return None;
        }
        let theme = Theme::current(cx);
        let focus = self.keyboard_options.focus.clone();
        let title = if self.keyboard_options.creating_branch {
            tr!("keyboard_options.create_branch").to_string()
        } else {
            self.keyboard_options.title.clone()
        };
        let items = self.keyboard_options.items.clone();
        let content_height = self.keyboard_options.content_height;
        let highlighted = self.keyboard_options.highlighted;
        let creating_branch = self.keyboard_options.creating_branch;
        let search = self.keyboard_options.search.clone();
        let list_state = self.keyboard_options.list.clone();
        let weak = cx.entity().downgrade();
        let row_theme = theme.clone();
        let jev_credential_missing = self.jev_credential_missing();

        let content = if creating_branch {
            div()
                .w_full()
                .p(px(14.0))
                .child(
                    div()
                        .h(px(38.0))
                        .px(px(10.0))
                        .rounded(px(11.0))
                        .border(hairline())
                        .border_color(theme.border_strong)
                        .bg(theme.surface)
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.branch_create_input.clone()),
                        ),
                )
                .child(
                    div()
                        .mt(px(9.0))
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(tr!("branches.create_hint")),
                )
                .into_any_element()
        } else {
            let viewport_height = f32::from(window.viewport_size().height);
            let list_height = content_height.min((viewport_height - 190.0).clamp(120.0, 440.0));
            let items_empty = items.is_empty();
            let rows = if items_empty {
                div()
                    .id("keyboard-options-list-empty")
                    .h(px(64.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(sp(12.5))
                    .text_color(theme.text_ghost)
                    .child(tr!("branches.none_found"))
                    .into_any_element()
            } else {
                list(list_state, move |index, window, cx| {
                    let Some(item) = items.get(index).cloned() else {
                        return div().into_any_element();
                    };
                    match item {
                        KeyboardOptionItem::Section(label) => div()
                            .id(SharedString::from(format!(
                                "keyboard-options-section-{index}"
                            )))
                            .h(px(SECTION_HEIGHT))
                            .w_full()
                            .flex_none()
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .text_size(sp(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(row_theme.text_tertiary)
                            .child(label)
                            .into_any_element(),
                        KeyboardOptionItem::Choice(choice) => {
                            let is_highlighted = highlighted == Some(index);
                            let move_weak = weak.clone();
                            let select_weak = weak.clone();
                            div()
                                .id(SharedString::from(format!("keyboard-options-row-{index}")))
                                .h(px(ITEM_HEIGHT))
                                .w_full()
                                .flex_none()
                                .px(px(10.0))
                                .rounded(px(ROW_RADIUS))
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .when(is_highlighted, |row| row.bg(row_theme.overlay_strong))
                                .when(choice.enabled, |row| {
                                    row.cursor_pointer().hover(|row| row.bg(row_theme.overlay))
                                })
                                .on_mouse_move(move |_, _, cx| {
                                    let _ = move_weak.update(cx, |this, cx| {
                                        if this.keyboard_options.highlighted != Some(index) {
                                            this.keyboard_options.highlighted = Some(index);
                                            this.keyboard_options.list.scroll_to_reveal_item(index);
                                            cx.notify();
                                        }
                                    });
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(move |_, window, cx| {
                                    let _ = select_weak.update(cx, |this, cx| {
                                        this.select_keyboard_option(index, window, cx);
                                    });
                                    cx.stop_propagation();
                                })
                                .when_some(choice.icon, |row, path| {
                                    row.child(icon(path, 14.0, row_theme.text_tertiary))
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .w_full()
                                                .truncate()
                                                .text_size(sp(13.0))
                                                .font_weight(if choice.selected {
                                                    FontWeight::SEMIBOLD
                                                } else {
                                                    FontWeight::MEDIUM
                                                })
                                                .text_color(if choice.enabled {
                                                    row_theme.text
                                                } else {
                                                    row_theme.text_tertiary
                                                })
                                                .child(choice.label),
                                        )
                                        .when_some(choice.description, |description, detail| {
                                            description.child(
                                                div()
                                                    .w_full()
                                                    .truncate()
                                                    .mt(px(2.0))
                                                    .text_size(sp(11.5))
                                                    .text_color(row_theme.text_tertiary)
                                                    .child(detail),
                                            )
                                        }),
                                )
                                .when(choice.selected, |row| {
                                    row.child(icon(
                                        "icons/check.svg",
                                        12.0,
                                        row_theme.text_secondary,
                                    ))
                                })
                                .into_any_element()
                        }
                        KeyboardOptionItem::Model {
                            row,
                            selected,
                            enabled,
                        } => model_option_row(
                            index,
                            &row,
                            selected,
                            enabled,
                            highlighted == Some(index),
                            jev_credential_missing,
                            &weak,
                            window,
                            cx,
                        ),
                    }
                })
                // A `list()` lays out as a childless node — without an explicit
                // size it lands zero high inside the fixed-height wrapper and
                // paints no items at all.
                .size_full()
                .into_any_element()
            };
            let body = div()
                .id("keyboard-options-list")
                .w_full()
                .h(px(if items_empty { 64.0 } else { list_height }))
                .flex_none()
                .child(rows);
            if let Some(search) = search {
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(SEARCH_HEIGHT))
                            .px(px(10.0))
                            .pb(px(6.0))
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
                                    .child(div().flex_1().min_w_0().child(search)),
                            ),
                    )
                    .child(body)
                    .into_any_element()
            } else {
                body.into_any_element()
            }
        };

        let card = div()
            .id("keyboard-options-card")
            .key_context("KeyboardOptions")
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::keyboard_options_key_down))
            // With the search field focused, the arrows and Enter reach it
            // as bound actions before this card's on_key_down could see the
            // keystroke — the "KeyboardOptions > TextInput" bindings turn
            // them back into entries on the list.
            .on_action(cx.listener(|this, _: &SelectNextEntry, _, cx| {
                this.move_keyboard_option_highlight(1, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousEntry, _, cx| {
                this.move_keyboard_option_highlight(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &ConfirmEntry, window, cx| {
                if this.keyboard_options.creating_branch {
                    if this.confirm_branch_creation(cx) {
                        this.dismiss_keyboard_options(true, window, cx);
                    }
                } else if let Some(index) = this.keyboard_options.highlighted {
                    this.select_keyboard_option(index, window, cx);
                }
            }))
            .w(px(MODAL_WIDTH))
            .max_w(px(MODAL_WIDTH))
            .p(px(MODAL_INSET))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(MODAL_RADIUS))
            .border(hairline())
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_xl()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(32.0))
                    .flex_none()
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(title),
            )
            .child(content)
            .child(
                div()
                    .h(px(28.0))
                    .flex_none()
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .text_size(sp(11.0))
                    .text_color(theme.text_ghost)
                    .child(tr!("keyboard_options.keyboard_hint")),
            );
        let layer = div()
            .id("keyboard-options-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.dismiss_keyboard_options(true, window, cx);
                }),
            )
            .child(motion::modal_enter("keyboard-options-card-enter", card));
        Some(gpui::deferred(layer).with_priority(8).into_any_element())
    }

    fn keyboard_options_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.keyboard_options.open {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" | "arrowup" => self.move_keyboard_option_highlight(-1, cx),
            "down" | "arrowdown" => self.move_keyboard_option_highlight(1, cx),
            "enter" => {
                if self.keyboard_options.creating_branch {
                    if self.confirm_branch_creation(cx) {
                        self.dismiss_keyboard_options(true, window, cx);
                    }
                } else if let Some(index) = self.keyboard_options.highlighted {
                    self.select_keyboard_option(index, window, cx);
                }
            }
            "escape" => {
                if self.keyboard_options.creating_branch {
                    self.keyboard_options.creating_branch = false;
                    self.branch_picker_mode = BranchPickerMode::Browse;
                    let focus = self.keyboard_options_modal_focus(cx);
                    window.focus(&focus, cx);
                    cx.notify();
                } else {
                    self.dismiss_keyboard_options(true, window, cx);
                }
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    /// Swap the open modal's rows after a search edit: a non-empty query
    /// lands the highlight on the first hit, clearing back to empty returns
    /// it to the current choice.
    pub(super) fn replace_keyboard_options_items(
        &mut self,
        items: Vec<KeyboardOptionItem>,
        highlighted: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if !self.keyboard_options.open {
            return;
        }
        self.keyboard_options.content_height = items.iter().map(KeyboardOptionItem::height).sum();
        self.keyboard_options.list =
            ListState::new(items.len(), ListAlignment::Top, px(ITEM_HEIGHT));
        self.keyboard_options.items = Arc::new(items);
        self.keyboard_options.highlighted = highlighted;
        if let Some(index) = highlighted {
            self.keyboard_options.list.scroll_to_reveal_item(index);
        }
        cx.notify();
    }

    /// Whether the centered options modal is up — sibling overlays fold it
    /// into their competing-overlay focus handling before opening.
    pub(super) fn keyboard_options_is_open(&self) -> bool {
        self.keyboard_options.open
    }

    /// The chord picker the modal is currently showing, if it's open.
    pub(super) fn open_keyboard_options_chord(&self) -> Option<KeyboardOptionsChord> {
        if self.keyboard_options.open {
            self.keyboard_options.chord
        } else {
            None
        }
    }

    /// A chord's repeated press steps the open picker instead of reopening
    /// it, and marks the hold so the release that ends it commits. Returns
    /// false while the modal is closed or owned by another picker.
    pub(super) fn cycle_keyboard_options_chord(
        &mut self,
        chord: KeyboardOptionsChord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.keyboard_options.open
            || self.keyboard_options.chord != Some(chord)
            || self.keyboard_options.creating_branch
        {
            return false;
        }
        self.keyboard_options.cycled = true;
        // A chord tap while the modal is open starts the hold even when a
        // menu click opened it — the release that follows commits.
        self.keyboard_options.armed =
            chord.hold_down(window.modifiers().secondary(), window.modifiers().alt);
        self.move_keyboard_option_highlight(1, cx);
        true
    }

    /// Releasing the chord's held modifiers ends the gesture. A picker with
    /// no search field commits the highlight outright; a searchable picker
    /// that was never stepped stays open and parks the search field for
    /// typing instead.
    pub(super) fn keyboard_options_modifiers_changed(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.keyboard_options.open
            || self.keyboard_options.creating_branch
            || !self.keyboard_options.armed
        {
            return;
        }
        let Some(chord) = self.keyboard_options.chord else {
            return;
        };
        if chord.hold_down(event.secondary(), event.alt) {
            return;
        }
        match chord {
            // A searchable picker that was never stepped stays open — the
            // release parks focus in its filter field for typing instead
            // of committing the highlighted row.
            KeyboardOptionsChord::Branch | KeyboardOptionsChord::Model
                if !self.keyboard_options.cycled =>
            {
                let focus = self.keyboard_options_modal_focus(cx);
                window.focus(&focus, cx);
            }
            _ => {
                if let Some(index) = self.keyboard_options.highlighted {
                    self.select_keyboard_option(index, window, cx);
                }
            }
        }
    }

    fn move_keyboard_option_highlight(&mut self, direction: isize, cx: &mut Context<Self>) {
        let len = self.keyboard_options.items.len();
        if len == 0 || self.keyboard_options.creating_branch {
            return;
        }
        let choices = self
            .keyboard_options
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.selectable().then_some(index))
            .collect::<Vec<_>>();
        if choices.is_empty() {
            return;
        }
        let current = self
            .keyboard_options
            .highlighted
            .and_then(|index| choices.iter().position(|choice| *choice == index));
        let next = match (direction, current) {
            (-1, Some(0)) | (-1, None) => choices.len() - 1,
            (-1, Some(index)) => index - 1,
            (_, Some(index)) => (index + 1) % choices.len(),
            (_, None) => 0,
        };
        let index = choices[next];
        self.keyboard_options.highlighted = Some(index);
        self.keyboard_options.list.scroll_to_reveal_item(index);
        cx.notify();
    }

    fn select_keyboard_option(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.keyboard_options.items.get(index).cloned() else {
            return;
        };
        let (action, enabled) = match item {
            KeyboardOptionItem::Section(_) => return,
            KeyboardOptionItem::Choice(choice) => (choice.action, choice.enabled),
            KeyboardOptionItem::Model { row, enabled, .. } => {
                let action = match row {
                    model_picker::PickerRow::Policy(model_picker::PolicyRowId::Auto) => {
                        KeyboardOptionAction::AutoRoute
                    }
                    model_picker::PickerRow::Combo(row) => KeyboardOptionAction::Model {
                        provider: row.provider,
                        model: row.model.id,
                        effort: row.effort,
                        fast: row.fast,
                    },
                    // The composer spec produces neither of these rows.
                    _ => return,
                };
                (action, enabled)
            }
        };
        if !enabled {
            return;
        }
        if matches!(&action, KeyboardOptionAction::CreateBranch) {
            self.keyboard_options.creating_branch = true;
            self.begin_branch_creation(window, cx);
            self.focus_keyboard_options_target(KeyboardOptionFocus::BranchCreate, window, cx);
            cx.notify();
            return;
        }

        let preserve_next_dialog_focus = matches!(
            &action,
            KeyboardOptionAction::RuntimeMode(RuntimeMode::FullAccess)
        ) && !self.state.full_access_acknowledged;
        self.dismiss_keyboard_options(!preserve_next_dialog_focus, window, cx);
        match action {
            KeyboardOptionAction::Model {
                provider,
                model,
                effort,
                fast,
            } => self.choose_model(provider, model, effort, fast, cx),
            KeyboardOptionAction::AutoRoute => self.choose_auto_route(cx),
            KeyboardOptionAction::ReasoningEffort(Some(effort)) => {
                self.set_reasoning_effort(effort, cx)
            }
            KeyboardOptionAction::ReasoningEffort(None) => self.clear_reasoning_effort(cx),
            KeyboardOptionAction::RuntimeMode(mode) => self.set_runtime_mode(mode, window, cx),
            KeyboardOptionAction::Environment(environment) => self.set_environment(environment, cx),
            KeyboardOptionAction::Workspace(workspace) => {
                if let Some(session_id) = self.ensure_workspace_subject_session(cx) {
                    self.select_workspace_for(session_id, workspace, cx);
                }
            }
            KeyboardOptionAction::Branch(branch) => {
                self.choose_workspace_branch(branch, cx);
            }
            KeyboardOptionAction::CreateBranch => unreachable!(),
        }
    }

    /// Where the modal's own focus lands: its search field when the picker
    /// has one, otherwise the card's handle.
    fn keyboard_options_modal_focus(&self, cx: &App) -> FocusHandle {
        self.keyboard_options
            .search
            .as_ref()
            .map(|search| search.read(cx).focus_handle(cx))
            .unwrap_or_else(|| self.keyboard_options.focus.clone())
    }

    fn focus_keyboard_options_target(
        &self,
        target: KeyboardOptionFocus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = match target {
            KeyboardOptionFocus::Modal => self.keyboard_options_modal_focus(cx),
            KeyboardOptionFocus::BranchCreate => self.branch_create_input.read(cx).focus_handle(cx),
        };
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
    }

    pub(super) fn dismiss_keyboard_options(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.keyboard_options.open {
            return;
        }
        let chord = self.keyboard_options.chord;
        self.keyboard_options.open = false;
        self.keyboard_options.creating_branch = false;
        self.keyboard_options.chord = None;
        self.keyboard_options.armed = false;
        self.keyboard_options.cycled = false;
        self.keyboard_options.search = None;
        self.keyboard_options.items = Arc::new(Vec::new());
        self.keyboard_options.content_height = 0.0;
        self.keyboard_options.highlighted = None;
        if self.branch_picker_mode == BranchPickerMode::Create {
            self.branch_picker_mode = BranchPickerMode::Browse;
        }
        // Parked unstars hold their favorites-block slots only while a model
        // picker is open — the modal closing releases them like the anchored
        // panel hiding does.
        if chord == Some(KeyboardOptionsChord::Model) {
            self.pinned_unfavorites.clear();
        }
        let previous_focus = self.keyboard_options.previous_focus.take();
        if restore_focus && let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        }
        cx.notify();
    }
}

/// A model row in the ⌘/ modal, drawn with the anchored picker's own
/// chrome: the same shell, two-line body, star, and favorite chord hint —
/// only the rail, the drag-reorder, and the popover's own close are absent.
fn model_option_row(
    index: usize,
    row: &model_picker::PickerRow,
    selected: bool,
    enabled: bool,
    is_highlighted: bool,
    jev_credential_missing: bool,
    weak: &WeakEntity<Waku>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::current(cx);
    let move_weak = weak.clone();
    let select_weak = weak.clone();
    let shell = |id: SharedString| {
        model_picker::model_picker_row_shell(id, selected, is_highlighted, &theme)
            .when(!enabled, |row| row.opacity(0.45))
            .on_mouse_move(move |_, _, cx| {
                let _ = move_weak.update(cx, |this, cx| {
                    if this.keyboard_options.highlighted != Some(index) {
                        this.keyboard_options.highlighted = Some(index);
                        this.keyboard_options.list.scroll_to_reveal_item(index);
                        cx.notify();
                    }
                });
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(move |_, window, cx| {
                let _ = select_weak.update(cx, |this, cx| {
                    this.select_keyboard_option(index, window, cx);
                });
                cx.stop_propagation();
            })
    };
    if let model_picker::PickerRow::Policy(policy) = row {
        // The router row carries no star — the settings shortcut that
        // configures the backend sits where a model row's star would.
        let auto = matches!(policy, model_picker::PolicyRowId::Auto);
        let (mark, title, subtitle) = model_picker::policy_row_parts(*policy, &theme);
        let settings_weak = weak.clone();
        return shell(SharedString::from(format!("model-row-policy-{policy:?}")))
            .child(model_picker::model_picker_row_body(
                title,
                auto.then(|| {
                    div()
                        .flex_none()
                        .truncate()
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(SharedString::from(tr!("models.auto_hint")))
                        .into_any_element()
                }),
                mark,
                subtitle,
                &theme,
            ))
            .when(jev_credential_missing && auto, |element| {
                element.child(
                    div()
                        .id("jev-credential-warning")
                        .tab_index(0)
                        .flex()
                        .items_center()
                        .justify_center()
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .tooltip(Tooltip::text(tr!("models.auto_missing_credential")))
                        .child(icon("icons/alert.svg", 13.0, theme.warning)),
                )
            })
            .child(
                div()
                    .id("jev-settings")
                    .tab_index(0)
                    .w(px(28.0))
                    .h(px(28.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|element| element.bg(theme.overlay_strong))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .tooltip(Tooltip::text(tr!("settings.jev")))
                    .child(icon("icons/settings.svg", 14.0, theme.text_ghost))
                    .on_activation_app(move |window, cx| {
                        let _ = settings_weak.update(cx, |this, cx| {
                            this.dismiss_keyboard_options(true, window, cx);
                            this.open_settings_action(&OpenSettings, window, cx);
                            this.open_settings_page(SettingsPage::Jev, window, cx);
                        });
                    }),
            )
            .into_any_element();
    }
    let model_picker::PickerRow::Combo(row) = row else {
        // The composer spec produces no provider-default rows.
        return div().into_any_element();
    };
    let kind = row.provider;
    let model = &row.model;
    let effort_label = row.effort.as_deref().and_then(|effort| {
        model
            .reasoning_efforts
            .iter()
            .find(|option| option.id == effort)
            .map(|option| {
                option
                    .label_i18n
                    .as_ref()
                    .map(waku_client::WireTranslation::render)
                    .unwrap_or_else(|| option.label.clone())
            })
    });
    let sub_provider = model
        .sub_provider
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let detail = model_picker::model_picker_subtitle(kind, sub_provider);
    // ⌘⌥1 is reserved for Auto; ⌘⌥2–⌘⌥9 ride on the first eight starred
    // rows — the same hint the anchored panel draws.
    let shortcut_hint = row
        .favorite_index
        .filter(|index| (1..=8).contains(index))
        .map(|index| {
            crate::ui::shortcut::ShortcutHint::action(&SelectFavoriteModel { index })
                .resolve(window, cx)
                .unwrap_or_else(|| {
                    crate::ui::shortcut::sequence_label(&format!("secondary-alt-{}", index + 1))
                })
        });
    let is_favorite = row.favorite_index.is_some();
    let model_id = model.id.clone();
    let effort = row.effort.clone();
    let fast = row.fast;
    let favorite_weak = weak.clone();
    let favorite_model_id = model.id.clone();
    let favorite_effort = row.effort.clone();
    shell(SharedString::from(format!(
        "model-row-{}-{}-{}-{}",
        kind.id(),
        model.id,
        row.effort.as_deref().unwrap_or("base"),
        row.fast
    )))
    .child(
        div()
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(SharedString::from(
                                model
                                    .name_i18n
                                    .as_ref()
                                    .map(waku_client::WireTranslation::render)
                                    .unwrap_or_else(|| model.name.clone()),
                            )),
                    )
                    .when_some(effort_label, |element, label| {
                        element.child(
                            div()
                                .flex_none()
                                .truncate()
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(SharedString::from(label)),
                        )
                    })
                    .when(row.fast, |element| {
                        element.child(icon("icons/zap.svg", 11.5, theme.text_tertiary))
                    }),
            )
            .child(
                div()
                    .mt(px(4.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(provider_mark(&theme, kind, 12.0, theme.text_tertiary))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(detail)),
                    ),
            ),
    )
    .when_some(shortcut_hint, |element, hint| {
        element.child(
            div()
                .flex_none()
                .text_size(sp(11.0))
                .text_color(theme.text_ghost)
                .child(SharedString::from(hint)),
        )
    })
    .child(
        div()
            .id(SharedString::from(format!(
                "favorite-model-{}-{}-{}-{}",
                kind.id(),
                model.id,
                row.effort.as_deref().unwrap_or("base"),
                row.fast
            )))
            .w(px(28.0))
            .h(px(28.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .hover(|element| element.bg(theme.overlay_strong))
            .child(icon(
                if is_favorite {
                    "icons/star-filled.svg"
                } else {
                    "icons/star.svg"
                },
                14.0,
                if is_favorite {
                    theme.favorite
                } else {
                    theme.text_ghost
                },
            ))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let _ = favorite_weak.update(cx, |this, cx| {
                    this.toggle_favorite_model(
                        kind,
                        favorite_model_id.clone(),
                        favorite_effort.clone(),
                        fast,
                        cx,
                    );
                    // Rebuild so stars and block order catch up; the
                    // highlight lands back on this combo's new slot.
                    this.refilter_model_keyboard_options(
                        Some((kind, model_id.clone(), effort.clone(), fast)),
                        cx,
                    );
                });
            }),
    )
    .into_any_element()
}
