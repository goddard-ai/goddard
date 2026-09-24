//! The command catalog behind the keybinding manager: stable command
//! identity joined to the bindings that invoke it.
//!
//! `catalog::ENTRIES` mirrors every `KeyBinding` registration in the exact
//! order the `init` functions run it — precedence is positional in GPUI's
//! keymap, so catalog order *is* registration order. `catalog::COMMANDS`
//! carries the per-command metadata (title, category, editability). The
//! parity test in this module proves the two views describe the same keymap
//! before generated registration replaces the hand-written callsites.

use std::collections::HashMap;

use gpui::{App, DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate};

mod catalog;
mod conflict;
pub mod layout;
// Service methods are wired up by the manager UI in later phases.
#[allow(dead_code)]
mod service;

pub use catalog::{COMMANDS, CatalogEntry, CommandDescriptor, ENTRIES, PlatformSet};
pub use conflict::{BindingFact, Conflict, ConflictKind, analyze_conflicts};
// Used by the manager UI/service wiring in later phases.
#[allow(unused_imports)]
pub use layout::{
    KeyboardLayout, LayoutId, LayoutSource, bundled_layouts, detect_layout, layout_by_id,
};

#[allow(unused_imports)]
pub use service::{
    BindingOperation, CommitError, EffectiveBinding, KeybindingEvent, KeybindingService,
    KeybindingsFile, KeymapSnapshot, PersistedLayout, UserOverride, default_path,
};

/// Stable identifier for a command, e.g. `"workspace.focus_terminal"`.
/// Never derived from a Rust type name or a localized string — the value is
/// the contract stored in `keybindings.json`.
pub type CommandId = &'static str;

/// Product grouping shown as the table's Category column. The cheatsheet
/// sections double as the i18n keys.
// Some categories have no commands yet; the full set is part of the spec.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommandCategory {
    Global,
    Workspace,
    TextInput,
    Terminal,
    CommandPalette,
    FileFinder,
    Switchers,
    Find,
    Browser,
    Editor,
    Dialogs,
    BigPicture,
    Settings,
    Projects,
    Git,
    Menus,
    Skills,
    Other,
}

impl CommandCategory {
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Global => "shortcuts.section.global",
            Self::Workspace => "shortcuts.section.workspace",
            Self::TextInput => "shortcuts.section.composer",
            Self::Terminal => "shortcuts.section.terminal",
            Self::CommandPalette => "shortcuts.section.palette",
            Self::FileFinder => "shortcuts.section.finder",
            Self::Switchers => "shortcuts.section.switchers",
            Self::Find => "shortcuts.section.find",
            Self::Browser => "shortcuts.section.browser",
            Self::Editor => "shortcuts.section.editor",
            Self::Dialogs => "shortcuts.section.dialogs",
            Self::BigPicture => "shortcuts.section.bigpicture",
            Self::Settings => "shortcuts.section.settings",
            Self::Projects => "keybind.category.projects",
            Self::Git => "keybind.category.git",
            Self::Menus => "shortcuts.section.menus",
            Self::Skills => "keybind.category.skills",
            Self::Other => "keybind.category.other",
        }
    }

    /// Display order in the manager table — the surfaces a user remaps
    /// most often lead; text-entry internals and miscellany trail.
    fn rank(self) -> u8 {
        match self {
            Self::Global => 0,
            Self::Workspace => 1,
            Self::Switchers => 2,
            Self::Terminal => 3,
            Self::Projects => 4,
            Self::Editor => 5,
            Self::Find => 6,
            Self::FileFinder => 7,
            Self::Browser => 8,
            Self::Git => 9,
            Self::BigPicture => 10,
            Self::CommandPalette => 11,
            Self::Settings => 12,
            Self::Dialogs => 13,
            Self::Menus => 14,
            Self::Skills => 15,
            Self::TextInput => 16,
            Self::Other => 17,
        }
    }
}

/// Whether the manager may offer capture for this command's bindings.
// `reason_key` is read by the manager UI when it renders editability states.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum Editability {
    /// Standard editable binding.
    Editable,
    /// A gesture the keymap cannot express (hand-rolled `on_key_down`, a
    /// hold-modifier affordance). Visible and searchable, never capturable;
    /// `reason_key` is an i18n key explaining why.
    BuiltIn { reason_key: &'static str },
    /// A real keymap binding whose edit would break text entry or modal
    /// navigation in v1 (bare keys inside `TextInput`).
    Locked { reason_key: &'static str },
}

/// Look up a command descriptor by stable id.
pub fn command(id: &str) -> Option<&'static CommandDescriptor> {
    COMMANDS.iter().find(|command| command.id == id)
}

/// All catalog entries whose platform includes this build.
pub fn current_entries() -> impl Iterator<Item = &'static CatalogEntry> {
    ENTRIES.iter().filter(|entry| entry.platform.is_current())
}

/// Build the GPUI `KeyBinding` list the catalog describes, in catalog order
/// (which is registration order, which is precedence order).
///
/// Uses the same `KeyBinding::load` path `KeyBinding::new` uses — dummy
/// keyboard mapper, no key equivalents — so generated bindings are identical
/// to the hand-written ones they replace.
// Only exercised by the parity test until `bind_catalog_keys` is wired in.
#[allow(dead_code)]
pub fn generate_key_bindings() -> Vec<KeyBinding> {
    current_entries()
        .filter_map(|entry| {
            let command = command(entry.command)?;
            let predicate = entry
                .context
                .and_then(|context| KeyBindingContextPredicate::parse(context).ok())
                .map(std::rc::Rc::from);
            KeyBinding::load(
                entry.sequence,
                (command.action)(),
                predicate,
                false,
                None,
                &DummyKeyboardMapper,
            )
            .ok()
        })
        .collect()
}

/// Rebuild the live keymap from a resolved snapshot — same `KeyBinding::load`
/// path as `generate_key_bindings`, but over the effective (default+override)
/// list. The caller clears the keymap first; ordering is the snapshot's
/// precedence order.
pub fn snapshot_key_bindings(snapshot: &crate::keybindings::KeymapSnapshot) -> Vec<KeyBinding> {
    snapshot
        .bindings
        .iter()
        .filter(|binding| binding.platform.is_current())
        .filter_map(|binding| {
            let command = command(binding.command)?;
            let predicate = binding
                .context
                .as_deref()
                .and_then(|context| KeyBindingContextPredicate::parse(context).ok())
                .map(std::rc::Rc::from);
            KeyBinding::load(
                &binding.sequence,
                (command.action)(),
                predicate,
                false,
                None,
                &DummyKeyboardMapper,
            )
            .ok()
        })
        .collect()
}

/// Effective state of one command for the manager table: the descriptor plus
/// the bindings currently in force for it (defaults this platform, later
/// merged with user overrides by `KeybindingService`).
#[derive(Clone)]
pub struct CommandRow {
    pub descriptor: &'static CommandDescriptor,
    /// Each live binding as `(canonical sequence, context predicate)`.
    pub bindings: Vec<EffectiveBindingRow>,
}

#[derive(Clone, Debug)]
pub struct EffectiveBindingRow {
    pub sequence: String,
    pub context: Option<String>,
    pub source: BindingSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingSource {
    Default,
    User,
}

/// Snapshot of the catalog for the manager: commands in authored order with
/// their active bindings attached.
pub fn command_rows() -> Vec<CommandRow> {
    let mut rows: Vec<CommandRow> = COMMANDS
        .iter()
        .map(|descriptor| CommandRow {
            descriptor,
            bindings: Vec::new(),
        })
        .collect();
    let index: HashMap<CommandId, usize> = COMMANDS
        .iter()
        .enumerate()
        .map(|(index, command)| (command.id, index))
        .collect();
    for entry in current_entries() {
        if let Some(&index) = index.get(entry.command) {
            rows[index].bindings.push(EffectiveBindingRow {
                sequence: entry.sequence.to_string(),
                context: entry.context.map(str::to_string),
                source: BindingSource::Default,
            });
        }
    }
    // Table order is by product surface, not registration order — the
    // stable sort keeps each category's authored sequence.
    rows.sort_by_key(|row| row.descriptor.category.rank());
    rows
}

/// Apply `keybindings.json` over the freshly registered catalog map at
/// startup. No file (or no overrides) leaves the defaults untouched; a
/// corrupt file is quarantined by `KeybindingService::load`, which still
/// resolves to defaults.
pub fn apply_saved_overrides(cx: &mut App) {
    let Some(path) = default_path() else {
        return;
    };
    let service = KeybindingService::load(path);
    if service.overrides().is_empty() {
        return;
    }
    let snapshot = service.snapshot();
    cx.clear_key_bindings();
    cx.bind_keys(snapshot_key_bindings(&snapshot));
}

/// Debug-build assertion the spec asks for: duplicate command ids are a
/// catalog authoring bug, not a runtime condition.
#[allow(dead_code)]
pub fn validate_catalog() -> Result<(), Vec<CommandId>> {
    let mut seen = HashMap::new();
    let mut duplicates = Vec::new();
    for command in COMMANDS {
        if seen.insert(command.id, ()).is_some() {
            duplicates.push(command.id);
        }
    }
    for entry in ENTRIES {
        if command(entry.command).is_none() {
            duplicates.push(entry.command);
        }
    }
    if duplicates.is_empty() {
        Ok(())
    } else {
        duplicates.sort_unstable();
        duplicates.dedup();
        Err(duplicates)
    }
}

/// Register generated bindings — the flag-gated replacement for the
/// hand-written init lists once parity is proven.
#[allow(dead_code)]
pub fn bind_catalog_keys(cx: &mut App) {
    cx.bind_keys(generate_key_bindings());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_no_duplicate_or_dangling_ids() {
        assert_eq!(validate_catalog(), Ok(()));
    }

    /// Every catalog entry parses as a GPUI binding for the platform it
    /// targets, so generated registration can never panic.
    #[test]
    fn every_entry_parses() {
        for entry in ENTRIES {
            if !entry.platform.is_current() {
                continue;
            }
            assert!(
                gpui::Keystroke::parse(
                    entry
                        .sequence
                        .split_whitespace()
                        .next()
                        .unwrap_or(entry.sequence)
                )
                .is_ok(),
                "unparseable sequence {:?} for {}",
                entry.sequence,
                entry.command
            );
            if let Some(context) = entry.context {
                assert!(
                    KeyBindingContextPredicate::parse(context).is_ok(),
                    "unparseable context {context:?} for {}",
                    entry.command
                );
            }
        }
    }

    fn serialize(bindings: &[KeyBinding]) -> Vec<String> {
        bindings
            .iter()
            .map(|binding| {
                format!(
                    "{} :: {} :: {}",
                    binding
                        .keystrokes()
                        .iter()
                        .map(|keystroke| keystroke.unparse())
                        .collect::<Vec<_>>()
                        .join(" "),
                    binding.action().name(),
                    binding
                        .predicate()
                        .map(|predicate| predicate.to_string())
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    /// The option modal's Escape is a real binding, not an `on_key_down`
    /// fallback: under the picker's stack it must resolve behind the
    /// field's own `Clear` (so a filled filter clears first) and ahead of
    /// the root `CancelTurn` (so a propagated `Clear` dismisses the modal
    /// instead of stopping the turn underneath it).
    #[gpui::test]
    fn option_modal_escape_resolves_to_dismiss(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::input::init(cx);
            crate::ui::menu::init(cx);
            crate::bind_keys(cx);
        });

        let stack = [
            gpui::KeyContext::parse("Workspace").unwrap(),
            gpui::KeyContext::parse("KeyboardOptions").unwrap(),
            gpui::KeyContext::parse("TextInput").unwrap(),
        ];
        let (bindings, _) = cx.update(|cx| {
            cx.key_bindings().borrow().bindings_for_input(
                &[gpui::Keystroke::parse("escape").unwrap()],
                &stack,
            )
        });
        let position = |action: &dyn gpui::Action| {
            bindings
                .iter()
                .position(|binding| binding.action().partial_eq(action))
        };
        let clear = position(&crate::input::Clear).expect("field Clear must match escape");
        let dismiss =
            position(&crate::ui::menu::DismissMenu).expect("modal DismissMenu must match escape");
        let cancel =
            position(&crate::CancelTurn { immediate: false }).expect("root CancelTurn still matches");
        assert!(
            clear < dismiss && dismiss < cancel,
            "escape must resolve Clear → DismissMenu → CancelTurn"
        );

        // ⌘-held Escape during the picker's hold gesture must reach the
        // modal's dismiss — otherwise Workspace's `CancelProjectSwitch`
        // eats the keystroke as a no-op and the hold's Escape is dead.
        let (bindings, _) = cx.update(|cx| {
            cx.key_bindings().borrow().bindings_for_input(
                &[gpui::Keystroke::parse("secondary-escape").unwrap()],
                &stack,
            )
        });
        assert!(
            bindings.first().is_some_and(|binding| {
                binding.action().partial_eq(&crate::ui::menu::DismissMenu)
            }),
            "secondary-escape under KeyboardOptions must resolve to DismissMenu"
        );
    }

    /// Parity gate: the generated keymap must be identical — action,
    /// sequence, platform, predicate, and precedence order — to the
    /// hand-written registrations it replaces. This must pass on every
    /// platform before `bind_catalog_keys` replaces the init lists.
    #[gpui::test]
    fn generated_matches_live_keymap(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::input::init(cx);
            crate::ui::menu::init(cx);
            crate::app::init_composer_autocomplete(cx);
            crate::app::init_settings_keys(cx);
            crate::app::init_command_palette(cx);
            crate::app::init_file_finder(cx);
            crate::app::init_sync_branch(cx);
            crate::app::init_commit_dialog_keys(cx);
            crate::app::init_git_panel_keys(cx);
            crate::app::init_archive_dialog_keys(cx);
            crate::app::init_reclaim_dialog_keys(cx);
            crate::app::init_terminal_close_dialog_keys(cx);
            crate::app::init_close_dialog_keys(cx);
            crate::app::init_provider_switch_dialog_keys(cx);
            crate::app::init_push_base_dialog_keys(cx);
            crate::app::init_reset_credit_dialog_keys(cx);
            crate::app::init_big_picture_keys(cx);
            crate::app::init_goal_dialog_keys(cx);
            crate::app::init_send_file_dialog_keys(cx);
            crate::app::init_annotation_keys(cx);
            crate::app::init_composer_keys(cx);
            crate::app::init_image_preview_keys(cx);
            crate::app::init_sidebar_keys(cx);
            crate::app::init_skills_keys(cx);
            crate::app::init_drafts_keys(cx);
            crate::app::init_shortcuts_dialog_keys(cx);
            crate::terminal::init_command_bar_keys(cx);
            crate::bind_keys(cx);
        });

        let live = cx.update(|cx| {
            serialize(
                &cx.key_bindings()
                    .borrow()
                    .bindings()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        });
        // Action payloads (sidebar index, font direction) share `name()`, so
        // compare actions pairwise with partial_eq, then the serialized rest.
        let generated = generate_key_bindings();
        let generated_serialized = serialize(&generated);
        let live_actions: Vec<Box<dyn gpui::Action>> = cx.update(|cx| {
            cx.key_bindings()
                .borrow()
                .bindings()
                .map(|binding| binding.action().boxed_clone())
                .collect()
        });

        {
            let gen_set: std::collections::HashSet<&String> = generated_serialized.iter().collect();
            let live_set: std::collections::HashSet<&String> = live.iter().collect();
            for row in live.iter().filter(|r| !gen_set.contains(*r)) {
                eprintln!("LIVE-ONLY:  {row}");
            }
            for row in generated_serialized
                .iter()
                .filter(|r| !live_set.contains(*r))
            {
                eprintln!("GEN-ONLY:   {row}");
            }
        }
        assert_eq!(
            live.len(),
            generated.len(),
            "binding count differs (live {} vs generated {})",
            live.len(),
            generated.len()
        );
        for (index, (live_row, generated_row)) in
            live.iter().zip(generated_serialized.iter()).enumerate()
        {
            assert_eq!(
                live_row, generated_row,
                "binding {index} differs:\nlive:      {live_row}\ngenerated: {generated_row}"
            );
            assert!(
                live_actions[index].partial_eq(generated[index].action()),
                "binding {index} action payload differs"
            );
        }
    }
}
