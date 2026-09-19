//! The Keybinding Manager page: a fixed keyboard stage above a searchable,
//! virtualized command table. Read-only browsing and highlighting land
//! first; the capture editor hooks the right-side chips next.
//!
//! Render reads only the snapshot and `filtered` — no keymap traversal or
//! I/O on the frame path.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, ListAlignment, ListState, MouseButton,
    SharedString, Task, Window, div, list, px, prelude::*,
};

use crate::input::{InputEvent, TextInput};
use crate::keybindings::{
    BindingFact, BindingOperation, BindingSource, CommandId, CommandRow, Conflict, ConflictKind,
    Editability, KeyboardLayout, KeybindingService, KeymapSnapshot, LayoutId, LayoutSource,
    PlatformSet, UserOverride, analyze_conflicts, default_path, detect_layout, layout_by_id,
    snapshot_key_bindings,
};
use std::collections::HashMap;
use std::rc::Rc;

use crate::theme::{Theme, hairline, sp};
use crate::ui::tooltip::Tooltip;
use crate::ui::{column_resize, icon, motion};

const ROW_HEIGHT: f32 = 34.0;
/// Table columns (VS Code layout): Command | Keybinding | When | Category.
const KEYBINDING_COL: f32 = 260.0;
const WHEN_COL: f32 = 160.0;
const CATEGORY_COL: f32 = 110.0;
/// Geometry the column header and the rows must share so every label sits
/// over its column: the table's outer inset, each row's own padding, and
/// the gap between cells.
const TABLE_INSET: f32 = 20.0;
const ROW_PAD: f32 = 8.0;
const COL_GAP: f32 = 12.0;
const KEY_UNIT: f32 = 30.0;
const KEY_GAP: f32 = 3.0;
/// How long capture waits for the next stroke before committing — long
/// enough for multi-stroke chords, short enough to feel immediate.
const CAPTURE_TIMEOUT: Duration = Duration::from_millis(1500);
const MAX_CAPTURE_STROKES: usize = 3;

/// In-progress chord recording against one command's binding slot.
/// `binding` is the index into the row's bindings being replaced, or
/// `None` when recording a new binding to add.
struct Capture {
    command: CommandId,
    binding: Option<usize>,
    /// Each stroke as GPUI canonical parts (`["cmd","shift","p"]`).
    strokes: Vec<Vec<String>>,
    /// Guards the timeout task: a superseded generation means a newer
    /// keystroke already replaced it.
    generation: usize,
    /// Set after the user acknowledged a conflict warning — Enter commits
    /// without re-checking.
    confirmed: bool,
    /// Conflict banner text shown while the capture waits for confirmation.
    warning: Option<String>,
}

/// `Keystroke` → the parts vector `UserOverride::sequence` stores, using the
/// same modifier spellings `Keystroke::parse` accepts.
fn stroke_parts(keystroke: &gpui::Keystroke) -> Vec<String> {
    let modifiers = keystroke.modifiers;
    let mut parts = Vec::new();
    if modifiers.function {
        parts.push("fn".to_string());
    }
    if modifiers.control {
        parts.push("ctrl".to_string());
    }
    if modifiers.alt {
        parts.push("alt".to_string());
    }
    if modifiers.platform {
        parts.push(platform_modifier_name().to_string());
    }
    if modifiers.shift {
        parts.push("shift".to_string());
    }
    parts.push(keystroke.key.clone());
    parts
}

fn platform_modifier_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd"
    }
    #[cfg(target_os = "windows")]
    {
        "win"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "super"
    }
}

/// The `platforms` value overrides written on this build carry.
fn current_platform_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "linux"
    }
}

/// Whether this command has a hard conflict — the same chord under the
/// same context, where registration order decides silently and one binding
/// is unreachable. Shadowed and partial overlaps are not conflicts the
/// table marks.
fn has_hard_conflict(conflicts: &HashMap<String, Vec<Conflict>>, command: &str) -> bool {
    conflicts.get(command).is_some_and(|conflicts| {
        conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::Hard)
    })
}

/// Worst-collision analysis over the resolved keymap, keyed by command.
fn compute_conflicts(snapshot: &KeymapSnapshot) -> HashMap<String, Vec<Conflict>> {
    let facts: Vec<BindingFact> = snapshot
        .bindings
        .iter()
        .map(|binding| BindingFact {
            command: binding.command,
            sequence: &binding.sequence,
            context: binding.context.as_deref(),
            platform: binding.platform,
        })
        .collect();
    analyze_conflicts(&facts, PlatformSet::CURRENT)
        .into_iter()
        .map(|(command, conflicts)| (command.to_string(), conflicts))
        .collect()
}

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
    capture: Option<Capture>,
    capture_focus: FocusHandle,
    capture_intercept: Option<gpui::Subscription>,
    capture_timeout: Option<Task<()>>,
    /// Focus handle for the command table so arrow-key navigation works
    /// without leaving the keyboard.
    table_focus: FocusHandle,
    /// Drag-resized widths for the Keybinding/When/Category columns —
    /// Command stays flexible and absorbs the difference.
    col_widths: [f32; 3],
    col_resize: Rc<column_resize::ColumnResize>,
    stage_collapsed: bool,
    layout: LayoutId,
    layout_source: LayoutSource,
    /// Last rejected write (stale revision, validation failure), shown in
    /// the header strip until the next successful commit clears it.
    commit_error: Option<String>,
    /// Conflicts across the effective keymap, recomputed with the snapshot.
    conflicts: HashMap<String, Vec<Conflict>>,
    /// Cycling position for the conflicts label: the next conflicting row a
    /// click reveals.
    conflict_cursor: usize,
    conflicts_focus: FocusHandle,
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

        // A manually picked layout wins; otherwise auto-detect from the OS
        // input source, falling back to US ANSI when unrecognized.
        let (layout, layout_source) = service
            .saved_layout()
            .and_then(LayoutId::parse)
            .map(|id| (id, LayoutSource::Manual))
            .unwrap_or_else(|| detect_layout(cx.keyboard_layout().id()));

        let filtered = (0..snapshot.rows.len()).collect();
        let row_count = snapshot.rows.len();
        let conflicts = compute_conflicts(&snapshot);
        Self {
            search,
            service,
            snapshot,
            filtered,
            list_state: ListState::new(row_count, ListAlignment::Top, px(ROW_HEIGHT)),
            hovered: None,
            selected: None,
            capture: None,
            capture_focus: cx.focus_handle(),
            capture_intercept: None,
            capture_timeout: None,
            table_focus: cx.focus_handle(),
            col_widths: [KEYBINDING_COL, WHEN_COL, CATEGORY_COL],
            col_resize: column_resize::ColumnResize::new(),
            stage_collapsed: false,
            layout,
            layout_source,
            commit_error: None,
            conflicts,
            conflict_cursor: 0,
            conflicts_focus: cx.focus_handle(),
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
        // Settings-family pages close the inbox on open; the early return
        // in `open_settings_page` skips its clear, so match it here.
        self.notifications.open = false;
        // The footer's hover zone unmounts without firing hover-off; only the
        // dock's own hover may keep it alive across the swap.
        self.sidebar_dock_zone_hovered = false;
        if let Some(ui) = &self.keybindings {
            ui.search.read(cx).focus().focus(window, cx);
        }
        cx.notify();
    }

    /// Start recording a chord for `command`. `binding` selects which
    /// existing binding gets replaced; `None` adds a new one.
    pub(super) fn keybindings_begin_capture(
        &mut self,
        command: CommandId,
        binding: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        ui.capture = Some(Capture {
            command,
            binding,
            strokes: Vec::new(),
            generation: 0,
            confirmed: false,
            warning: None,
        });
        let this = cx.weak_entity();
        // Interception is scoped to the capture's lifetime: the subscription
        // drops when recording ends, and stop_propagation inside the handler
        // keeps captured keys from dispatching their commands.
        ui.capture_intercept = Some(cx.intercept_keystrokes(move |event, window, app| {
            let _ = this.update(app, |this, cx| {
                this.keybindings_on_capture_keystroke(&event.keystroke, window, cx);
            });
        }));
        ui.capture_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn keybindings_cancel_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        ui.capture = None;
        ui.capture_intercept = None;
        ui.capture_timeout = None;
        ui.table_focus.focus(window, cx);
        cx.notify();
    }

    /// One captured key event: Escape cancels, Backspace undoes the last
    /// stroke, Enter commits early, anything else records a stroke.
    fn keybindings_on_capture_keystroke(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.keybindings.as_ref().is_none_or(|ui| ui.capture.is_none()) {
            return;
        }
        cx.stop_propagation();
        match keystroke.key.as_str() {
            "escape" => {
                self.keybindings_cancel_capture(window, cx);
                return;
            }
            "backspace" => {
                if let Some(capture) = self
                    .keybindings
                    .as_mut()
                    .and_then(|ui| ui.capture.as_mut())
                {
                    capture.strokes.pop();
                }
                cx.notify();
                return;
            }
            "enter" => {
                // Enter while a conflict warning is showing confirms the
                // write; otherwise it finishes early.
                if let Some(capture) = self
                    .keybindings
                    .as_mut()
                    .and_then(|ui| ui.capture.as_mut())
                {
                    if capture.warning.is_some() {
                        capture.confirmed = true;
                    }
                }
                self.keybindings_finish_capture(cx);
                return;
            }
            _ => {}
        }

        let mapper = cx.keyboard_mapper().clone();
        let normalized = gpui::KeybindingKeystroke::new_with_mapper(
            keystroke.clone(),
            false,
            mapper.as_ref(),
        );
        let stroke = stroke_parts(normalized.inner());

        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        let Some(capture) = ui.capture.as_mut() else {
            return;
        };
        // A new stroke after a warning edits the chord — the stale warning
        // and its confirmation no longer apply.
        capture.warning = None;
        capture.confirmed = false;
        capture.strokes.push(stroke);
        if capture.strokes.len() >= MAX_CAPTURE_STROKES {
            self.keybindings_finish_capture(cx);
            return;
        }

        // Reset the commit timeout: a keystroke that just landed must outlive
        // the task scheduled for the previous one.
        capture.generation += 1;
        let generation = capture.generation;
        let this = cx.weak_entity();
        ui.capture_timeout = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(CAPTURE_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                let still_current = this
                    .keybindings
                    .as_ref()
                    .and_then(|ui| ui.capture.as_ref())
                    .is_some_and(|capture| capture.generation == generation);
                if still_current {
                    this.keybindings_finish_capture(cx);
                }
            });
        }));
        cx.notify();
    }

    /// Commit the captured chord through the service, then swap the live
    /// keymap so the new binding works immediately. A colliding chord first
    /// surfaces its conflicts and waits for an explicit Enter.
    fn keybindings_finish_capture(&mut self, cx: &mut Context<Self>) {
        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        let Some(capture) = ui.capture.take() else {
            return;
        };
        ui.capture_timeout = None;

        // Empty capture means the user backspaced everything or pressed
        // Enter before typing — close without writing. Unbinding is the
        // row's explicit remove control, not a capture outcome.
        if capture.strokes.is_empty() {
            ui.capture_intercept = None;
            cx.notify();
            return;
        }

        let row = ui
            .snapshot
            .rows
            .iter()
            .find(|row| row.descriptor.id == capture.command);
        let context = capture
            .binding
            .and_then(|index| row.and_then(|row| row.bindings.get(index)))
            .and_then(|binding| binding.context.clone());

        let override_record = UserOverride {
            command_id: capture.command.to_string(),
            platforms: vec![current_platform_label().to_string()],
            context,
            operation: match capture.binding {
                Some(_) => BindingOperation::Replace,
                None => BindingOperation::Add,
            },
            semantics: "logical".to_string(),
            sequence: Some(capture.strokes.clone()),
            extra: Default::default(),
        };

        // Conflict gate: preview the write and, if the new chord collides
        // with another command, warn instead of committing. Enter while the
        // warning shows confirms; anything else can still edit or cancel.
        if !capture.confirmed {
            if let Ok(preview) = ui.service.preview(&[override_record.clone()]) {
                let facts: Vec<BindingFact> = preview
                    .iter()
                    .map(|binding| BindingFact {
                        command: binding.command,
                        sequence: &binding.sequence,
                        context: binding.context.as_deref(),
                        platform: binding.platform,
                    })
                    .collect();
                let conflicts = analyze_conflicts(&facts, PlatformSet::CURRENT);
                let names: Vec<String> = conflicts
                    .iter()
                    .filter(|(command, _)| *command == capture.command)
                    .flat_map(|(_, conflicts)| conflicts.iter())
                    .filter(|conflict| conflict.kind != ConflictKind::Duplicate)
                    .filter_map(|conflict| {
                        crate::keybindings::command(&conflict.other)
                            .map(|descriptor| descriptor.title().to_string())
                    })
                    .collect();
                if !names.is_empty() {
                    let mut capture = capture;
                    capture.warning = Some(format!(
                        "{} · {}",
                        tr!("keybind.conflict.banner", other = names.join(", ")),
                        tr!("keybind.conflict.confirm")
                    ));
                    ui.capture = Some(capture);
                    cx.notify();
                    return;
                }
            }
        }

        ui.capture_intercept = None;
        let revision = ui.snapshot.revision;
        match ui.service.commit(revision, vec![override_record]) {
            Ok(_) => {
                ui.snapshot = ui.service.snapshot();
                ui.conflicts = compute_conflicts(&ui.snapshot);
                ui.commit_error = None;
                // Live rebind: the new effective map replaces the keymap
                // wholesale, preserving precedence order.
                let bindings = snapshot_key_bindings(&ui.snapshot);
                cx.clear_key_bindings();
                cx.bind_keys(bindings);
                ui.refilter(cx);
            }
            Err(error) => {
                ui.commit_error = Some(format!("{error:?}"));
            }
        }
        cx.notify();
    }

    /// Restore a command's default bindings (drops its overrides).
    pub(super) fn keybindings_reset(&mut self, command: CommandId, cx: &mut Context<Self>) {
        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        if ui.service.reset(command).is_ok() {
            ui.snapshot = ui.service.snapshot();
            ui.conflicts = compute_conflicts(&ui.snapshot);
            ui.commit_error = None;
            let bindings = snapshot_key_bindings(&ui.snapshot);
            cx.clear_key_bindings();
            cx.bind_keys(bindings);
            ui.refilter(cx);
        }
        cx.notify();
    }

    fn keybindings_select_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        if ui.filtered.is_empty() {
            return;
        }
        let index = index.min(ui.filtered.len() - 1);
        ui.selected = Some(index);
        ui.list_state.scroll_to_reveal_item(index);
        cx.notify();
    }

    /// Enter on a selected row starts capture on its first binding (or adds
    /// one when the command has none).
    fn keybindings_edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = self.keybindings.as_ref() else {
            return;
        };
        let Some(row_index) = ui.selected.and_then(|index| ui.filtered.get(index)) else {
            return;
        };
        let Some(row) = ui.snapshot.rows.get(*row_index) else {
            return;
        };
        if !matches!(row.descriptor.editability, Editability::Editable) {
            return;
        }
        let binding = (!row.bindings.is_empty()).then_some(0);
        self.keybindings_begin_capture(row.descriptor.id, binding, window, cx);
    }

    /// Reveal the next hard-conflicting row, cycling back to the first.
    /// The row is marked the way a pointer hover marks it, so the keyboard
    /// stage previews the colliding chord too. Only rows the current search
    /// leaves visible can be revealed.
    fn keybindings_cycle_conflicts(&mut self, cx: &mut Context<Self>) {
        let Some(ui) = self.keybindings.as_ref() else {
            return;
        };
        // Positions in the filtered list, in table order.
        let targets: Vec<usize> = ui
            .filtered
            .iter()
            .enumerate()
            .filter(|(_, row_index)| {
                ui.snapshot
                    .rows
                    .get(**row_index)
                    .is_some_and(|row| has_hard_conflict(&ui.conflicts, row.descriptor.id))
            })
            .map(|(index, _)| index)
            .collect();
        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        if targets.is_empty() {
            return;
        }
        let cursor = ui.conflict_cursor % targets.len();
        ui.conflict_cursor = (cursor + 1) % targets.len();
        if let Some(&index) = targets.get(cursor) {
            ui.hovered = Some(index);
            ui.list_state.scroll_to_reveal_item(index);
        }
        cx.notify();
    }

    /// Arrow-key navigation over the filtered table.
    fn keybindings_move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(ui) = self.keybindings.as_mut() else {
            return;
        };
        if ui.filtered.is_empty() {
            return;
        }
        let current = ui.selected.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, ui.filtered.len() as isize - 1) as usize;
        ui.selected = Some(next);
        ui.list_state.scroll_to_reveal_item(next);
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

        // The chord the stage shows: an in-progress capture wins over
        // selected, which wins over hover.
        let preview_row = ui
            .capture
            .as_ref()
            .and_then(|capture| {
                ui.snapshot
                    .rows
                    .iter()
                    .find(|row| row.descriptor.id == capture.command)
            })
            .or_else(|| {
                // The pointer leads the keyboard preview; selection is the
                // fallback once the pointer leaves the table.
                ui.hovered
                    .or(ui.selected)
                    .and_then(|index| ui.filtered.get(index))
                    .and_then(|row| ui.snapshot.rows.get(*row))
            });
        let mut codes = HashSet::new();
        if let Some(capture) = &ui.capture {
            for stroke in &capture.strokes {
                codes.extend(highlight_codes(&stroke.join("-"), layout));
            }
        } else if let Some(binding) = preview_row.and_then(|row| row.bindings.first()) {
            // The table shows one binding per command, so the keyboard
            // lights that same chord rather than a union of all of them.
            codes.extend(highlight_codes(&binding.sequence, layout));
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
        // snapshot clone plus a weak handle back to `Waku` so hover and
        // click can still reach the page state.
        let rows = ui.snapshot.rows.clone();
        let filtered = ui.filtered.clone();
        let selected = ui.selected;
        let hovered = ui.hovered;
        let col_widths = ui.col_widths;
        let this = cx.weak_entity();
        // Hard conflicts only — same chord under the same context, so one
        // binding is silently unreachable. Shadowed and partial overlaps
        // are how a contextual keymap normally looks and are not marked.
        // The value names the colliding commands for the row's tooltip.
        let hard_conflicts: HashMap<String, SharedString> = ui
            .conflicts
            .iter()
            .filter_map(|(command, conflicts)| {
                let mut others: Vec<String> = conflicts
                    .iter()
                    .filter(|conflict| conflict.kind == ConflictKind::Hard)
                    .filter_map(|conflict| {
                        crate::keybindings::command(&conflict.other)
                            .map(|descriptor| descriptor.title().to_string())
                    })
                    .collect();
                others.sort();
                others.dedup();
                (!others.is_empty()).then(|| {
                    (
                        command.clone(),
                        SharedString::from(tr!(
                            "keybind.conflict.banner",
                            other = others.join(", ")
                        )),
                    )
                })
            })
            .collect();
        let hard_conflict_count = hard_conflicts.len();

        let mut page = div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&ui.table_focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if let Some(ui) = this.keybindings.as_ref() {
                    if ui.capture.is_some() || ui.search.read(cx).focus().is_focused(window) {
                        return;
                    }
                }
                match event.keystroke.key.as_str() {
                    "down" => this.keybindings_move_selection(1, cx),
                    "up" => this.keybindings_move_selection(-1, cx),
                    "pagedown" => this.keybindings_move_selection(20, cx),
                    "pageup" => this.keybindings_move_selection(-20, cx),
                    "home" => this.keybindings_select_row(0, cx),
                    "end" => this.keybindings_select_row(usize::MAX, cx),
                    "enter" => {
                        this.keybindings_edit_selected(window, cx);
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }));

        page = page.child(
                // Header: title, search, layout readout.
                div()
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .border_b(hairline())
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
                        // Click cycles the bundled layouts and pins the pick
                        // in keybindings.json (manual mode).
                        div()
                            .id("keybindings-layout")
                            .ml_auto()
                            .px(px(6.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .cursor_pointer()
                            .hover(|element| element.bg(theme.overlay))
                            .child(format!("{source_label} · {}", layout.name))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                if let Some(ui) = this.keybindings.as_mut() {
                                    let layouts = crate::keybindings::bundled_layouts();
                                    let next = layouts
                                        .iter()
                                        .position(|layout| layout.id == ui.layout)
                                        .map(|index| (index + 1) % layouts.len())
                                        .unwrap_or(0);
                                    if let Some(layout) = layouts.get(next) {
                                        ui.layout = layout.id;
                                        ui.layout_source = LayoutSource::Manual;
                                        if ui
                                            .service
                                            .set_layout(layout.id.as_str(), true)
                                            .is_err()
                                        {
                                            ui.commit_error =
                                                Some("could not save layout".into());
                                        }
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            );

        page = page.child(render_keyboard_stage(ui, layout, &codes, preview_row, theme));

        if let Some(error) = &ui.commit_error {
            page = page.child(
                div()
                    .px(px(20.0))
                    .py(px(6.0))
                    .text_size(sp(11.5))
                    .text_color(theme.danger)
                    .child(format!("{error}")),
            );
        }

        // Counts strip between stage and table. The conflict count is a
        // control: each activation reveals the next conflicting row.
        let mut strip = div()
            .px(px(TABLE_INSET))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(5.0))
            .text_size(sp(11.5))
            .text_color(theme.text_tertiary)
            .child(format!(
                "{} · {}",
                tr!("keybind.counts", n = ui.filtered.len()),
                tr!("keybind.modified", n = modified),
            ));
        if hard_conflict_count > 0 {
            strip = strip.child("·").child(
                div()
                    .id("keybindings-conflicts")
                    .track_focus(&ui.conflicts_focus)
                    .tab_index(0)
                    .px(px(4.0))
                    .rounded(px(4.0))
                    .text_color(theme.danger)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.overlay))
                    .focus_visible(|style| {
                        style.border(hairline()).border_color(theme.accent)
                    })
                    .tooltip(Tooltip::text(tr!("keybind.conflicts.cycle")))
                    .child(tr!("keybind.conflicts", n = hard_conflict_count))
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.keybindings_cycle_conflicts(cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.keybindings_cycle_conflicts(cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }

        page = page
            .child(strip)
            .child(render_column_header(ui, theme, cx))
            .child(
                div().flex_1().min_h_0().px(px(TABLE_INSET)).child(
                    list(ui.list_state.clone(), move |index, _window, cx| {
                        let theme = Theme::current(cx);
                        let row_index = match filtered.get(index) {
                            Some(row_index) => *row_index,
                            None => return div().into_any_element(),
                        };
                        let Some(row) = rows.get(row_index) else {
                            return div().into_any_element();
                        };
                        render_row(
                            row,
                            index,
                            selected == Some(index),
                            hovered == Some(index),
                            hard_conflicts.get(row.descriptor.id).cloned(),
                            this.clone(),
                            col_widths,
                            theme,
                        )
                    })
                    .size_full(),
                ),
            );

        // While recording, a VS Code-style modal holds the one input that
        // captures the chord — Esc cancels, Enter commits, clicking the
        // scrim dismisses. The keyboard stage keeps lighting the strokes.
        if let Some(capture) = &ui.capture {
            let command_title = ui
                .snapshot
                .rows
                .iter()
                .find(|row| row.descriptor.id == capture.command)
                .map(|row| row.descriptor.title().to_string())
                .unwrap_or_else(|| capture.command.to_string());
            let chord = capture
                .strokes
                .iter()
                .map(|stroke| crate::ui::shortcut::sequence_label(&stroke.join("-")))
                .collect::<Vec<_>>()
                .join("  ");
            let scrim = if theme.is_dark {
                gpui::hsla(0.0, 0.0, 0.0, 0.34)
            } else {
                gpui::hsla(0.0, 0.0, 0.0, 0.16)
            };
            let card = div()
                .id("keybindings-capture-card")
                .track_focus(&ui.capture_focus)
                .w(px(420.0))
                .p(px(16.0))
                .rounded(px(12.0))
                .border(hairline())
                .border_color(theme.border)
                .bg(theme.canvas)
                .shadow_xl()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_size(sp(12.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(command_title),
                )
                .child(
                    div()
                        .h(px(32.0))
                        .px(px(10.0))
                        .rounded(px(6.0))
                        .border(hairline())
                        .border_color(theme.accent)
                        .flex()
                        .items_center()
                        .text_size(sp(13.0))
                        .text_color(if chord.is_empty() {
                            theme.text_tertiary
                        } else {
                            theme.text
                        })
                        .child(if chord.is_empty() {
                            tr!("keybind.capture.waiting")
                        } else {
                            chord
                        }),
                )
                .children(capture.warning.iter().map(|warning| {
                    div()
                        .text_size(sp(11.5))
                        .text_color(theme.warning)
                        .child(warning.clone())
                        .into_any_element()
                }))
                .child(
                    div()
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(tr!("keybind.capture.hint")),
                );
            let layer = div()
                .id("keybindings-capture-layer")
                .absolute()
                .inset_0()
                .occlude()
                .bg(scrim)
                .p(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.keybindings_cancel_capture(window, cx);
                    }),
                )
                .child(motion::modal_enter("keybindings-capture-enter", card));
            page = page.child(
                gpui::deferred(motion::fade_in("keybindings-capture-layer-enter", layer))
                    .into_any_element(),
            );
        }
        page.into_any_element()
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
            .border_b(hairline())
            .border_color(theme.border)
            .child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(summary.unwrap_or_else(|| tr!("keybind.stage.collapsed"))),
            );
    }

    // Caption above the caps names the command whose chord is lit, so the
    // reader doesn't have to glance back at the table. Falls back to a
    // hint while the pointer is off the rows.
    let caption = preview_row.map(|row| row.descriptor.title().to_string());
    let mut stage = div()
        .px(px(20.0))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(KEY_GAP))
        .border_b(hairline())
        .border_color(theme.border)
        .child(
            div()
                .h(px(18.0))
                .flex()
                .items_center()
                .text_size(sp(11.5))
                .text_color(if caption.is_some() {
                    theme.text_secondary
                } else {
                    theme.text_tertiary
                })
                .child(caption.unwrap_or_else(|| tr!("keybind.stage.hover_hint"))),
        );
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
                    .border(hairline())
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
                    // Modifier and named caps wear their platform glyph
                    // (⇧⌃⌥⌘⇪⇥⌫⎋↵) rather than a spelled-out name.
                    .child(
                        crate::ui::shortcut::key_glyph(capdef.unshifted)
                            .unwrap_or(capdef.unshifted)
                            .to_string(),
                    ),
            );
        }
        stage = stage.child(line);
    }
    stage
}

/// The pinned column header. It nests the same outer inset and row padding
/// the rows carry, and reuses their gap and column widths, so each label
/// sits over its column — the table's only alignment contract.
fn render_column_header(
    ui: &KeybindingsUi,
    theme: Theme,
    cx: &mut Context<super::Waku>,
) -> gpui::Div {
    let set_width =
        |this: &mut super::Waku, column: usize, width: f32, cx: &mut Context<super::Waku>| {
            if let Some(ui) = this.keybindings.as_mut()
                && let Some(slot) = ui.col_widths.get_mut(column)
            {
                *slot = width;
                cx.notify();
            }
        };
    let cell = |index: usize, text: String, cx: &mut Context<super::Waku>| {
        div()
            .w(px(ui.col_widths[index]))
            .flex_none()
            .min_w_0()
            .relative()
            .flex()
            .items_center()
            .child(div().min_w_0().truncate().child(text))
            .child(column_resize::column_resize_handle(
                SharedString::from(format!("keybindings-col-{index}")),
                &ui.col_resize,
                index,
                ui.col_widths[index],
                &theme,
                cx,
                set_width,
            ))
    };
    div()
        .relative()
        .px(px(TABLE_INSET))
        .child(
            div()
                .w_full()
                .h(px(26.0))
                .px(px(ROW_PAD))
                .flex()
                .items_center()
                .gap(px(COL_GAP))
                .text_size(sp(11.0))
                .text_color(theme.text_tertiary)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(tr!("keybind.column.command")),
                )
                .child(cell(0, tr!("keybind.column.keybinding"), cx))
                .child(cell(1, tr!("keybind.column.when"), cx))
                .child(cell(2, tr!("keybind.column.category"), cx)),
        )
        .child(column_resize::column_resize_listeners(
            &ui.col_resize,
            cx,
            set_width,
        ))
}

/// One command's keybinding column: a single slot holding the chord, the
/// whole cell being the click target that opens the capture modal. A
/// command with no binding shows an empty slot that assigns one; a
/// non-editable command shows its chord or built-in label, inert.
fn keybinding_cell(
    row: &CommandRow,
    conflict: Option<SharedString>,
    this: gpui::WeakEntity<super::Waku>,
    width: f32,
    theme: Theme,
) -> AnyElement {
    let binding = row.bindings.first();
    let editable = matches!(row.descriptor.editability, Editability::Editable);
    let customized = row
        .bindings
        .iter()
        .any(|binding| binding.source == BindingSource::User);
    let label = binding
        .map(|binding| crate::ui::shortcut::sequence_label(&binding.sequence))
        .or_else(|| row.descriptor.builtin_label.map(str::to_string));
    let cell = div()
        .id(SharedString::from(format!("{}:binding", row.descriptor.id)))
        .w(px(width))
        .h(px(24.0))
        .flex_none()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(6.0))
        .children(label.map(|label| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .px(px(6.0))
                .py(px(2.0))
                .rounded(px(4.0))
                .border(hairline())
                .border_color(theme.border)
                .text_size(sp(11.0))
                .text_color(theme.text)
                .child(label)
        }))
        // A hard conflict is carried by an icon, not by tinting the chord:
        // the chip's border is structure (the click target) and a 1px tint
        // would be a color-only signal. The tooltip names the collision.
        .children(conflict.map(|conflict| {
            div()
                .id(SharedString::from(format!(
                    "{}:conflict",
                    row.descriptor.id
                )))
                .flex_none()
                .flex()
                .items_center()
                .child(icon("icons/alert.svg", 12.0, theme.danger))
                .tooltip(Tooltip::text(conflict))
        }))
        // Customized rows keep a way back to the default chord.
        .children(
            (customized && editable)
                .then(|| {
                    let command = row.descriptor.id;
                    let reset_this = this.clone();
                    div()
                        .id(SharedString::from(format!("{}:reset", row.descriptor.id)))
                        .flex_none()
                        .whitespace_nowrap()
                        .px(px(4.0))
                        .rounded(px(4.0))
                        .text_size(sp(10.0))
                        .text_color(theme.text_tertiary)
                        .cursor_pointer()
                        .hover(|element| element.text_color(theme.text))
                        .child(tr!("keybind.reset"))
                        .on_click(move |_, _window, app| {
                            // Stop the cell's own click from reopening
                            // capture on the freshly restored default.
                            app.stop_propagation();
                            let _ = reset_this.update(app, |this, cx| {
                                this.keybindings_reset(command, cx);
                            });
                        })
                })
                .into_iter(),
        );
    if !editable {
        return cell.into_any_element();
    }
    let command = row.descriptor.id;
    // Replace the existing chord, or record the first one when unbound.
    let slot = row.bindings.first().map(|_| 0);
    cell.cursor_pointer()
        .hover(|element| element.bg(theme.overlay_strong))
        .rounded(px(5.0))
        .on_click(move |_, window, app| {
            let _ = this.update(app, |this, cx| {
                this.keybindings_begin_capture(command, slot, window, cx);
            });
        })
        .into_any_element()
}

fn render_row(
    row: &CommandRow,
    index: usize,
    selected: bool,
    // The page's own hover state, which the pointer sets and the conflicts
    // label sets when it reveals a row. Painted here so a revealed row
    // looks exactly like a hovered one.
    hovered: bool,
    // Tooltip text naming the commands this row's chord collides with,
    // when the collision is a hard one.
    conflict: Option<SharedString>,
    this: gpui::WeakEntity<super::Waku>,
    col_widths: [f32; 3],
    theme: Theme,
) -> AnyElement {
    let category = tr!(row.descriptor.category.title_key());
    let title = row.descriptor.title();
    let hover_this = this.clone();
    div()
        .id(SharedString::from(row.descriptor.id))
        .h(px(ROW_HEIGHT))
        // Without an explicit full width the row sizes to its content, so
        // every row's fixed columns would land at a different x.
        .w_full()
        .flex()
        .items_center()
        .gap(px(COL_GAP))
        .px(px(ROW_PAD))
        .rounded(px(6.0))
        .when(selected, |element| element.bg(theme.sidebar_item_background))
        .when(hovered, |element| element.bg(theme.overlay))
        .hover(|element| element.bg(theme.overlay))
        .on_hover(move |hovered, _window, app| {
            let hovered = *hovered;
            let _ = hover_this.update(app, |this, cx| {
                if let Some(ui) = this.keybindings.as_mut() {
                    // Leaving only clears the pointer when this row is
                    // still the one recorded — a newer row's enter can
                    // arrive before the old row's leave.
                    if hovered {
                        ui.hovered = Some(index);
                    } else if ui.hovered == Some(index) {
                        ui.hovered = None;
                    }
                }
                cx.notify();
            });
        })
        .on_click({
            let this = this.clone();
            move |_, _window, app| {
                let _ = this.update(app, |this, cx| {
                    if let Some(ui) = this.keybindings.as_mut() {
                        ui.selected = Some(index);
                    }
                    cx.notify();
                });
            }
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(sp(12.5))
                .text_color(theme.text)
                .child(title),
        )
        .child(
            // The one keybinding slot: clicking it anywhere opens the
            // capture modal, which replaces the chord in place. Commands
            // with no binding show an empty slot that assigns one.
            keybinding_cell(row, conflict, this.clone(), col_widths[0], theme),
        )
        .child(
            div()
                .w(px(col_widths[1]))
                .flex_none()
                .truncate()
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
                .w(px(col_widths[2]))
                .flex_none()
                .truncate()
                .text_size(sp(11.0))
                .text_color(theme.text_tertiary)
                .child(category),
        )
        .into_any_element()
}
