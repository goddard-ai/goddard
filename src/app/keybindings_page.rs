//! The Keybinding Manager page: a fixed keyboard stage above a searchable,
//! virtualized command table. Read-only browsing and highlighting land
//! first; the capture editor hooks the right-side chips next.
//!
//! Render reads only the snapshot and `filtered` — no keymap traversal or
//! I/O on the frame path.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{
    AnyElement, App, Context, Entity, ListAlignment, ListState, SharedString, Window, div, list, px,
    prelude::*,
};

use crate::input::{InputEvent, TextInput};
use crate::keybindings::layout::{KeyboardLayout, layout_by_id};
use crate::keybindings::{
    BindingSource, CommandRow, KeybindingService, KeymapSnapshot, LayoutId, LayoutSource,
    default_path, detect_layout,
};
use crate::theme::{Theme, sp};

const ROW_HEIGHT: f32 = 34.0;
const KEY_UNIT: f32 = 30.0;
const KEY_GAP: f32 = 3.0;

/// The manager's state: the persistence service, the last resolved
/// snapshot, and view state. `filtered` holds positions into
/// `snapshot.rows`.
pub(super) struct KeybindingsUi {
    pub search: Entity<TextInput>,
    service: KeybindingService,
    snapshot: KeymapSnapshot,
    /// Row indices passing the current search, in catalog order.
    filtered: Vec<usize>,
    list_state: ListState,
    hovered: Option<usize>,
    selected: Option<usize>,
    stage_collapsed: bool,
    layout: LayoutId,
    layout_source: LayoutSource,
    _search_subscription: gpui::Subscription,
}

impl KeybindingsUi {
    pub fn new(window: &mut Window, cx: &mut Context<super::Waku>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("keybind.search"))
                .placeholder(tr!("keybind.search_placeholder"))
        });
        let subscription = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Edited) {
                this.keybindings_refilter(cx);
            }
        });

        let service = default_path()
            .map(KeybindingService::load)
            .unwrap_or_else(|| KeybindingService::load(PathBuf::new()));
        let snapshot = service.snapshot();

        // Auto-detect from the OS input source; unmatched layouts fall back
        // to US ANSI with an honest `Unrecognized` marker.
        let (layout, layout_source) = detect_layout(cx.keyboard_layout().id());

        let filtered = (0..snapshot.rows.len()).collect();
        Self {
            search,
            service,
            snapshot,
            filtered,
            list_state: ListState::new(0, ListAlignment::Top, px(ROW_HEIGHT)),
            hovered: None,
            selected: None,
            stage_collapsed: false,
            layout,
            layout_source,
            _search_subscription: subscription,
        }
    }

    /// Rebuild `filtered` from the search field. Runs on `Edited` only —
    /// TextInput emits it once per real content change.
    fn refilter(&mut self, cx: &App) {
        let query = self.search.read(cx).content().trim().to_lowercase();
        self.filtered = self
            .snapshot
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row_matches(row, &query))
            .map(|(index, _)| index)
            .collect();
        self.list_state.reset(self.filtered.len());
        self.hovered = None;
        self.selected = None;
    }
}

fn row_matches(row: &CommandRow, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    row.descriptor.title().to_lowercase().contains(query)
        || row.descriptor.id.contains(query)
        || row
            .bindings
            .iter()
            .any(|binding| binding.sequence.to_lowercase().contains(query))
        || row
            .descriptor
            .builtin_label
            .is_some_and(|label| label.to_lowercase().contains(query))
}

/// Logical modifier name → the physical cap code it highlights.
fn modifier_codes(modifiers: gpui::Modifiers) -> impl Iterator<Item = &'static str> {
    [
        modifiers.control.then_some("ControlLeft"),
        modifiers.alt.then_some("AltLeft"),
        modifiers.platform.then_some("MetaLeft"),
        modifiers.shift.then_some("ShiftLeft"),
    ]
    .into_iter()
    .flatten()
}

/// Physical cap codes a binding sequence highlights.
fn highlight_codes(sequence: &str, layout: &KeyboardLayout) -> HashSet<&'static str> {
    let mut codes = HashSet::new();
    for stroke in sequence.split_whitespace() {
        if let Ok(keystroke) = gpui::Keystroke::parse(stroke) {
            codes.extend(modifier_codes(keystroke.modifiers));
            if let Some(code) = layout.physical_for_logical(&keystroke.key) {
                codes.insert(code);
            }
        }
    }
    codes
}

impl super::Waku {
    pub(super) fn keybindings_refilter(&mut self, cx: &mut Context<Self>) {
        if let Some(ui) = self.keybindings.as_mut() {
            ui.refilter(cx);
        }
        cx.notify();
    }

    pub(super) fn open_keybindings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.keybindings.is_none() {
            self.keybindings = Some(KeybindingsUi::new(window, cx));
        }
        self.settings_page = Some(super::SettingsPage::Keybindings);
        if let Some(ui) = &self.keybindings {
            ui.search.read(cx).focus().focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn render_keybindings_page(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(ui) = &self.keybindings else {
            return div().into_any_element();
        };
        let layout = layout_by_id(ui.layout);

        // The chord the stage shows: selected > hover > none (capture joins
        // this precedence chain in the editing phase).
        let preview_row = ui
            .selected
            .or(ui.hovered)
            .and_then(|index| ui.filtered.get(index))
            .and_then(|row| ui.snapshot.rows.get(*row));
        let mut codes = HashSet::new();
        if let Some(row) = preview_row {
            for binding in &row.bindings {
                codes.extend(highlight_codes(&binding.sequence, layout));
            }
        }

        let modified = ui
            .snapshot
            .rows
            .iter()
            .filter(|row| {
                row.bindings
                    .iter()
                    .any(|binding| binding.source == BindingSource::User)
            })
            .count();

        let source_label = match ui.layout_source {
            LayoutSource::Auto => tr!("keybind.layout.auto"),
            LayoutSource::Manual => tr!("keybind.layout.manual"),
            LayoutSource::Unrecognized => tr!("keybind.layout.unrecognized"),
        };

        // `list`'s item closure gets `&mut App`, not the entity — hand it a
        // snapshot clone so rows render without touching `self`.
        let rows = ui.snapshot.rows.clone();
        let filtered = ui.filtered.clone();
        let selected = ui.selected;

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                // Header: title, search, layout readout.
                div()
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_size(sp(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(tr!("keybind.title")),
                    )
                    .child(div().w(px(320.0)).child(ui.search.clone()))
                    .child(
                        div()
                            .ml_auto()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(format!("{source_label} · {}", layout.name)),
                    ),
            )
            .child(render_keyboard_stage(ui, layout, &codes, preview_row, theme))
            .child(
                // Counts strip between stage and table.
                div()
                    .px(px(20.0))
                    .py(px(6.0))
                    .text_size(sp(11.5))
                    .text_color(theme.text_tertiary)
                    .child(format!(
                        "{} · {}",
                        tr!("keybind.counts", n = ui.filtered.len()),
                        tr!("keybind.modified", n = modified)
                    )),
            )
            .child(
                div().flex_1().min_h_0().px(px(20.0)).child(
                    list(ui.list_state.clone(), move |index, _window, cx| {
                        let theme = Theme::current(cx);
                        let row_index = match filtered.get(index) {
                            Some(row_index) => *row_index,
                            None => return div().into_any_element(),
                        };
                        let Some(row) = rows.get(row_index) else {
                            return div().into_any_element();
                        };
                        render_row(row, selected == Some(index), theme)
                    })
                    .size_full(),
                ),
            )
            .into_any_element()
    }
}

fn render_keyboard_stage(
    ui: &KeybindingsUi,
    layout: &KeyboardLayout,
    codes: &HashSet<&'static str>,
    preview_row: Option<&CommandRow>,
    theme: Theme,
) -> gpui::Div {
    if ui.stage_collapsed {
        let summary = preview_row
            .and_then(|row| row.bindings.first())
            .map(|binding| binding.sequence.clone())
            .or_else(|| {
                preview_row
                    .and_then(|row| row.descriptor.builtin_label)
                    .map(str::to_string)
            });
        return div()
            .h(px(40.0))
            .px(px(20.0))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(summary.unwrap_or_else(|| tr!("keybind.stage.collapsed"))),
            );
    }

    let mut stage = div()
        .px(px(20.0))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(KEY_GAP))
        .border_b_1()
        .border_color(theme.border);
    for row in layout.rows {
        let mut line = div().flex().gap(px(KEY_GAP));
        for capdef in *row {
            let active = codes.contains(capdef.code);
            line = line.child(
                div()
                    .w(px(capdef.width * KEY_UNIT + (capdef.width - 1.0) * KEY_GAP))
                    .h(px(KEY_UNIT))
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(sp(10.5))
                    .border_1()
                    .border_color(if active { theme.text } else { theme.border })
                    .bg(if active {
                        theme.sidebar_item_background
                    } else {
                        theme.canvas
                    })
                    .text_color(if active {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .child(capdef.unshifted.to_string()),
            );
        }
        stage = stage.child(line);
    }
    stage
}

fn render_row(row: &CommandRow, selected: bool, theme: Theme) -> AnyElement {
    let category = tr!(row.descriptor.category.title_key());
    let title = row.descriptor.title();
    div()
        .id(SharedString::from(row.descriptor.id))
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .when(selected, |element| element.bg(theme.sidebar_item_background))
        .child(
            div()
                .w(px(110.0))
                .text_size(sp(11.0))
                .text_color(theme.text_tertiary)
                .child(category),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(sp(12.5))
                .text_color(theme.text)
                .child(title),
        )
        .child(
            div()
                .w(px(160.0))
                .text_size(sp(11.0))
                .text_color(theme.text_secondary)
                .child(
                    row.bindings
                        .first()
                        .and_then(|binding| binding.context.clone())
                        .unwrap_or_default(),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(4.0))
                .children(row.bindings.iter().map(|binding| {
                    div()
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(theme.border)
                        .text_size(sp(11.0))
                        .text_color(theme.text)
                        .child(crate::ui::shortcut::sequence_label(&binding.sequence))
                        .into_any_element()
                }))
                .children(row.descriptor.builtin_label.iter().map(|label| {
                    div()
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(format!("{label} · built in"))
                        .into_any_element()
                })),
        )
        .into_any_element()
}
