//! The keyboard-shortcut reference opened from the sidebar's keyboard button.
//! Rows are authored per section so labels and ordering read like a
//! cheatsheet; each row's chord is resolved from the live keymap at open
//! time, so the display cannot drift from the binding it advertises. Chords
//! the keymap cannot see — the terminal's hand-rolled `on_key_down` clear, or
//! hold-modifier gestures — carry authored text instead.

use gpui::{Action, KeyBinding, actions};

use super::archive_dialog::{ConfirmArchiveDialog, DismissArchiveDialog};
use super::command_palette::{
    Confirm, Dismiss, SelectFirst, SelectLast, SelectNext, SelectPageDown, SelectPageUp,
    SelectPrevious,
};
use super::full_access_dialog::{ConfirmFullAccessDialog, DismissFullAccessDialog};
use super::reclaim_dialog::{ConfirmReclaimDialog, DismissReclaimDialog};
use super::*;
use crate::{
    AdjustFontSize, BrowserAddressCancel, BrowserBack, BrowserDevtools, BrowserForward,
    BrowserHardReload, BrowserReload, BrowserStop, FocusBrowserAddress, FontSizeDirection,
    FontSizeTarget, OpenCreatedIssueInGitHub, OpenLocalhostUrl, OpenLocalhostUrlInTab,
    OpenToastSession, Quit, WebviewCopy, WebviewCut, WebviewPaste, WebviewSelectAll,
};
#[cfg(target_os = "macos")]
use crate::{Hide, HideOthers};

actions!(waku_shortcuts_dialog, [DismissShortcutsDialog]);

const DIALOG_CONTEXT: &str = "ShortcutsDialog";
const LIST_MAX_HEIGHT: f32 = 480.0;
const ROW_HEIGHT: f32 = 26.0;
/// Roughly one viewport of the list; page keys step by it.
const PAGE_SCROLL: f32 = LIST_MAX_HEIGHT - ROW_HEIGHT;

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        DismissShortcutsDialog,
        Some(DIALOG_CONTEXT),
    )]);
}

enum ShortcutKeys {
    /// Every binding registered for `action` under exactly this context
    /// predicate — `None` matches context-free bindings — joined into one
    /// label. Resolves empty when the platform never binds the action, and
    /// the row is then omitted.
    Bound {
        action: Box<dyn Action>,
        context: Option<&'static str>,
    },
    /// A chord the keymap never sees, or a gesture a single binding cannot
    /// express (⌘ held through ⌘1–⌘9).
    Text(&'static str),
}

struct ShortcutRow {
    label: String,
    keys: ShortcutKeys,
}

fn bound(
    label: String,
    action: impl Action + 'static,
    context: Option<&'static str>,
) -> ShortcutRow {
    ShortcutRow {
        label,
        keys: ShortcutKeys::Bound {
            action: Box::new(action),
            context,
        },
    }
}

fn text_row(label: String, keys: &'static str) -> ShortcutRow {
    ShortcutRow {
        label,
        keys: ShortcutKeys::Text(keys),
    }
}

/// The ⌘K palette binding is context-free on macOS and `!Terminal` elsewhere.
const PALETTE_CONTEXT: Option<&'static str> = if cfg!(target_os = "macos") {
    None
} else {
    Some("!Terminal")
};

fn shortcut_rows() -> Vec<(&'static str, Vec<ShortcutRow>)> {
    use crate::input;
    let mut global = vec![
        bound(tr!("menu.new_task"), NewSession, None),
        bound(tr!("menu.new_task_in"), NewTaskIn, None),
        bound(tr!("menu.new_project"), NewProject, None),
        bound(
            tr!("menu.command_palette"),
            ToggleCommandPalette,
            PALETTE_CONTEXT,
        ),
        bound(tr!("shortcuts.file_finder"), ToggleFileFinder, None),
        bound(tr!("shortcuts.big_picture"), ToggleBigPicture, None),
        text_row(
            tr!("shortcuts.jump_to_task"),
            crate::platform::primary_shortcut("⌘1–⌘9", "Ctrl+1–9"),
        ),
        bound(tr!("menu.toggle_sidebar"), ToggleSidebar, None),
        bound(tr!("menu.toggle_right_panel"), ToggleRightPanel, None),
        bound(tr!("menu.toggle_git_panel"), ToggleGitPanel, None),
        bound(tr!("right_panel.new_terminal"), NewTerminal, None),
        bound(tr!("shortcuts.toggle_terminals"), ToggleTerminals, None),
        bound(tr!("menu.focus_composer"), FocusComposer, None),
        bound(tr!("menu.focus_terminal"), FocusTerminal, None),
        bound(tr!("menu.toggle_model_picker"), ToggleModelPicker, None),
        bound(tr!("menu.toggle_branch_picker"), ToggleBranchPicker, None),
        bound(
            tr!("menu.toggle_runtime_mode_picker"),
            ToggleRuntimeModePicker,
            None,
        ),
        bound(tr!("menu.toggle_environment"), ToggleEnvironment, None),
        bound(tr!("menu.toggle_workspace"), ToggleWorkspace, None),
        bound(tr!("shortcuts.usage_panel"), ToggleUsagePanel, None),
        bound(tr!("menu.run_project_script"), RunProjectScript, None),
        bound(tr!("shortcuts.save_file"), SaveFile, None),
        bound(tr!("shortcuts.open_localhost"), OpenLocalhostUrl, None),
        bound(
            tr!("shortcuts.open_localhost_tab"),
            OpenLocalhostUrlInTab,
            None,
        ),
        bound(
            tr!("shortcuts.open_created_issue"),
            OpenCreatedIssueInGitHub,
            None,
        ),
        bound(
            tr!("shortcuts.view_unarchived_task"),
            OpenToastSession,
            None,
        ),
        bound(tr!("shortcuts.open_settings"), OpenSettings, None),
        bound(
            tr!("shortcuts.font_bigger_ui"),
            AdjustFontSize {
                target: FontSizeTarget::Ui,
                direction: FontSizeDirection::Increase,
            },
            Some("!Browser"),
        ),
        bound(
            tr!("shortcuts.font_smaller_ui"),
            AdjustFontSize {
                target: FontSizeTarget::Ui,
                direction: FontSizeDirection::Decrease,
            },
            Some("!Browser"),
        ),
    ];
    #[cfg(target_os = "macos")]
    global.extend([
        bound(
            tr!("menu.hide", app = crate::identity::APP_NAME),
            Hide,
            None,
        ),
        bound(tr!("menu.hide_others"), HideOthers, None),
    ]);
    global.extend([
        bound(tr!("menu.close_window"), CloseWindow, None),
        bound(
            tr!("menu.quit", app = crate::identity::APP_NAME),
            Quit,
            None,
        ),
        bound(tr!("shortcuts.fps_counter"), ToggleFpsCounter, None),
    ]);
    vec![
        ("shortcuts.section.global", global),
        (
            "shortcuts.section.workspace",
            vec![
                bound(
                    tr!("shortcuts.navigate_back"),
                    NavigateBack,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.navigate_forward"),
                    NavigateForward,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.previous_turn"),
                    GoToPreviousTurn,
                    Some("Workspace && !Terminal"),
                ),
                bound(
                    tr!("shortcuts.next_turn"),
                    GoToNextTurn,
                    Some("Workspace && !Terminal"),
                ),
                bound(
                    tr!("shortcuts.next_unread_completion"),
                    GoToNextUnreadCompletion,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.mark_unread"),
                    MarkSessionUnread,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.mark_unread_next"),
                    MarkUnreadAndGoToNextIdle,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.task_switcher"),
                    SwitchTaskForward,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.task_switcher_back"),
                    SwitchTaskBackward,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.project_switcher"),
                    SwitchProjectForward,
                    None,
                ),
                bound(
                    tr!("shortcuts.project_switcher_back"),
                    SwitchProjectBackward,
                    None,
                ),
                bound(
                    tr!("shortcuts.stop_turn"),
                    CancelTurn { immediate: false },
                    Some("Workspace && !Terminal"),
                ),
                bound(
                    tr!("shortcuts.stop_turn_now"),
                    CancelTurn { immediate: true },
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.add_to_chat"),
                    AddToChat,
                    Some("Transcript || FileEditorPane"),
                ),
                bound(
                    tr!("shortcuts.archive_task"),
                    ArchiveSession,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.pin_task"),
                    ToggleSessionPin,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.push_base"),
                    PushBaseBranch,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.copy_selection"),
                    CopySelection,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.copy_workdir"),
                    CopyWorkingDirectory,
                    Some("Workspace"),
                ),
                bound(tr!("shortcuts.find"), OpenFind, Some("Workspace")),
                bound(
                    tr!("shortcuts.find_replace"),
                    OpenFindReplace,
                    Some("Workspace"),
                ),
                bound(tr!("shortcuts.find_next"), FindNext, Some("Workspace")),
                bound(
                    tr!("shortcuts.find_previous"),
                    FindPrevious,
                    Some("Workspace"),
                ),
                bound(
                    tr!("shortcuts.go_to_line"),
                    OpenGoToLine,
                    Some("Workspace && !Terminal"),
                ),
            ],
        ),
        (
            "shortcuts.section.composer",
            vec![
                bound(tr!("shortcuts.submit"), input::Enter, Some("TextInput")),
                bound(tr!("shortcuts.newline"), input::Newline, Some("TextInput")),
                bound(
                    tr!("shortcuts.steer"),
                    input::SubmitSteer,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.clear_field"),
                    input::Clear,
                    Some("TextInput"),
                ),
                bound(tr!("menu.undo"), input::Undo, Some("TextInput")),
                bound(tr!("menu.redo"), input::Redo, Some("TextInput")),
                bound(tr!("menu.cut"), input::Cut, Some("TextInput")),
                bound(tr!("menu.copy"), input::Copy, Some("TextInput")),
                bound(tr!("menu.paste"), input::Paste, Some("TextInput")),
                bound(tr!("menu.select_all"), input::SelectAll, Some("TextInput")),
            ],
        ),
        (
            "shortcuts.section.editing",
            vec![
                bound(tr!("shortcuts.move_left"), input::Left, Some("TextInput")),
                bound(tr!("shortcuts.move_right"), input::Right, Some("TextInput")),
                bound(tr!("shortcuts.move_up"), input::Up, Some("TextInput")),
                bound(tr!("shortcuts.move_down"), input::Down, Some("TextInput")),
                bound(
                    tr!("shortcuts.word_back"),
                    input::MoveToPreviousWord,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.word_forward"),
                    input::MoveToNextWord,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.line_start"),
                    input::LineStart,
                    Some("TextInput"),
                ),
                bound(tr!("shortcuts.line_end"), input::LineEnd, Some("TextInput")),
                bound(tr!("shortcuts.doc_start"), input::Home, Some("TextInput")),
                bound(tr!("shortcuts.doc_end"), input::End, Some("TextInput")),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.paragraph_back"),
                    input::ParagraphBackward,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.paragraph_forward"),
                    input::ParagraphForward,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.paragraph_start"),
                    input::ParagraphStart,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.paragraph_end"),
                    input::ParagraphEnd,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.delete_back"),
                    input::Backspace,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.delete_forward"),
                    input::Delete,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.delete_word_back"),
                    input::DeleteToPreviousWord,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.delete_word_forward"),
                    input::DeleteToNextWord,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.delete_line_back"),
                    input::DeleteToLineStart,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.delete_line_forward"),
                    input::DeleteToLineEnd,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.kill_line_back"),
                    input::DeleteToParagraphStart,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.kill_line_forward"),
                    input::DeleteToParagraphEnd,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_left"),
                    input::SelectLeft,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_right"),
                    input::SelectRight,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_up"),
                    input::SelectUp,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_down"),
                    input::SelectDown,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_word_back"),
                    input::SelectToPreviousWord,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_word_forward"),
                    input::SelectToNextWord,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_line_start"),
                    input::SelectToLineStart,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_line_end"),
                    input::SelectToLineEnd,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_doc_start"),
                    input::SelectToStart,
                    Some("TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_doc_end"),
                    input::SelectToEnd,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.select_paragraph_back"),
                    input::SelectParagraphBackward,
                    Some("TextInput"),
                ),
                #[cfg(target_os = "macos")]
                bound(
                    tr!("shortcuts.select_paragraph_forward"),
                    input::SelectParagraphForward,
                    Some("TextInput"),
                ),
            ],
        ),
        (
            "shortcuts.section.terminal",
            vec![
                bound(
                    tr!("shortcuts.font_bigger"),
                    AdjustFontSize {
                        target: FontSizeTarget::Terminal,
                        direction: FontSizeDirection::Increase,
                    },
                    Some("Terminal"),
                ),
                bound(
                    tr!("shortcuts.font_smaller"),
                    AdjustFontSize {
                        target: FontSizeTarget::Terminal,
                        direction: FontSizeDirection::Decrease,
                    },
                    Some("Terminal"),
                ),
                text_row(
                    tr!("shortcuts.clear_scrollback"),
                    crate::platform::primary_shortcut("⇧⌘K", "Ctrl+Shift+K"),
                ),
            ],
        ),
        (
            "shortcuts.section.palette",
            vec![
                bound(
                    tr!("shortcuts.select_next"),
                    SelectNext,
                    Some("CommandPalette > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_previous"),
                    SelectPrevious,
                    Some("CommandPalette > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_first"),
                    SelectFirst,
                    Some("CommandPalette > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_last"),
                    SelectLast,
                    Some("CommandPalette > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_page_down"),
                    SelectPageDown,
                    Some("CommandPalette > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_page_up"),
                    SelectPageUp,
                    Some("CommandPalette > TextInput"),
                ),
                bound(
                    tr!("shortcuts.confirm"),
                    Confirm,
                    Some("CommandPalette > TextInput"),
                ),
                bound(tr!("shortcuts.dismiss"), Dismiss, Some("CommandPalette")),
            ],
        ),
        (
            "shortcuts.section.finder",
            vec![
                bound(
                    tr!("shortcuts.select_next"),
                    SelectNext,
                    Some("FileFinder > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_previous"),
                    SelectPrevious,
                    Some("FileFinder > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_first"),
                    SelectFirst,
                    Some("FileFinder > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_last"),
                    SelectLast,
                    Some("FileFinder > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_page_down"),
                    SelectPageDown,
                    Some("FileFinder > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_page_up"),
                    SelectPageUp,
                    Some("FileFinder > TextInput"),
                ),
                bound(
                    tr!("shortcuts.confirm"),
                    Confirm,
                    Some("FileFinder > TextInput"),
                ),
                bound(tr!("shortcuts.dismiss"), Dismiss, Some("FileFinder")),
            ],
        ),
        (
            "shortcuts.section.switchers",
            vec![
                bound(
                    tr!("shortcuts.switcher_next"),
                    SwitchTaskForward,
                    Some("TaskSwitcher"),
                ),
                bound(
                    tr!("shortcuts.switcher_previous"),
                    SwitchTaskBackward,
                    Some("TaskSwitcher"),
                ),
                bound(
                    tr!("shortcuts.switcher_first"),
                    SelectFirstTask,
                    Some("TaskSwitcher"),
                ),
                bound(
                    tr!("shortcuts.switcher_last"),
                    SelectLastTask,
                    Some("TaskSwitcher"),
                ),
                bound(
                    tr!("shortcuts.switcher_confirm"),
                    ConfirmTaskSwitch,
                    Some("TaskSwitcher"),
                ),
                bound(
                    tr!("shortcuts.switcher_cancel"),
                    CancelTaskSwitch,
                    Some("TaskSwitcher"),
                ),
                bound(
                    tr!("shortcuts.project_next"),
                    SwitchProjectForward,
                    Some("ProjectSwitcher"),
                ),
                bound(
                    tr!("shortcuts.project_previous"),
                    SwitchProjectBackward,
                    Some("ProjectSwitcher"),
                ),
                bound(
                    tr!("shortcuts.project_confirm"),
                    ConfirmProjectSwitch,
                    Some("ProjectSwitcher"),
                ),
                bound(
                    tr!("shortcuts.project_cancel"),
                    CancelProjectSwitch,
                    Some("ProjectSwitcher"),
                ),
            ],
        ),
        (
            "shortcuts.section.find",
            vec![
                bound(
                    tr!("shortcuts.find_previous"),
                    FindPrevious,
                    Some("FindBar"),
                ),
                bound(
                    tr!("shortcuts.replace_all"),
                    ReplaceAllMatches,
                    Some("FindBar"),
                ),
                bound(tr!("shortcuts.close_find"), CloseFind, Some("FindBar")),
                bound(
                    tr!("shortcuts.find_case"),
                    ToggleFindCaseSensitive,
                    Some("FileEditorPane"),
                ),
                bound(
                    tr!("shortcuts.find_word"),
                    ToggleFindWholeWord,
                    Some("FileEditorPane"),
                ),
                bound(
                    tr!("shortcuts.find_regex"),
                    ToggleFindRegex,
                    Some("FileEditorPane"),
                ),
            ],
        ),
        (
            "shortcuts.section.browser",
            vec![
                bound(
                    tr!("shortcuts.browser_address"),
                    FocusBrowserAddress,
                    Some("Browser"),
                ),
                bound(
                    tr!("shortcuts.browser_reload"),
                    BrowserReload,
                    Some("Browser"),
                ),
                bound(
                    tr!("shortcuts.browser_hard_reload"),
                    BrowserHardReload,
                    Some("Browser"),
                ),
                bound(tr!("shortcuts.browser_back"), BrowserBack, Some("Browser")),
                bound(
                    tr!("shortcuts.browser_forward"),
                    BrowserForward,
                    Some("Browser"),
                ),
                bound(tr!("shortcuts.browser_stop"), BrowserStop, Some("Browser")),
                bound(
                    tr!("shortcuts.browser_devtools"),
                    BrowserDevtools,
                    Some("Browser"),
                ),
                bound(tr!("menu.copy"), WebviewCopy, Some("Browser")),
                bound(tr!("menu.cut"), WebviewCut, Some("Browser")),
                bound(tr!("menu.paste"), WebviewPaste, Some("Browser")),
                bound(tr!("menu.select_all"), WebviewSelectAll, Some("Browser")),
                bound(
                    tr!("shortcuts.browser_address_cancel"),
                    BrowserAddressCancel,
                    Some("BrowserAddress"),
                ),
            ],
        ),
        (
            "shortcuts.section.editor",
            vec![
                bound(
                    tr!("shortcuts.font_bigger"),
                    AdjustFontSize {
                        target: FontSizeTarget::Code,
                        direction: FontSizeDirection::Increase,
                    },
                    Some("ReviewDiff || FileEditorPane"),
                ),
                bound(
                    tr!("shortcuts.font_smaller"),
                    AdjustFontSize {
                        target: FontSizeTarget::Code,
                        direction: FontSizeDirection::Decrease,
                    },
                    Some("ReviewDiff || FileEditorPane"),
                ),
                bound(
                    tr!("shortcuts.exit_fullscreen_panel"),
                    ExitPanelFullscreen,
                    Some("PanelFullscreen && !Terminal"),
                ),
            ],
        ),
        (
            "shortcuts.section.dialogs",
            vec![
                bound(
                    tr!("shortcuts.git_primary"),
                    git_panel::GitPanelPrimaryAction,
                    Some("GitPanel"),
                ),
                bound(
                    tr!("shortcuts.confirm_dialog"),
                    commit_dialog::ConfirmCommitDialog,
                    Some("CommitDialog"),
                ),
                bound(
                    tr!("shortcuts.generate_message"),
                    commit_dialog::GenerateCommitDialog,
                    Some("CommitDialog"),
                ),
                bound(
                    tr!("shortcuts.confirm_dialog"),
                    goal_dialog::ConfirmGoalDialog,
                    Some("GoalDialog"),
                ),
                bound(
                    tr!("shortcuts.confirm_archive"),
                    ConfirmArchiveDialog,
                    Some("ArchiveDialog"),
                ),
                bound(
                    tr!("shortcuts.confirm_dialog"),
                    ConfirmReclaimDialog,
                    Some("ReclaimDialog"),
                ),
                bound(
                    tr!("shortcuts.confirm_dialog"),
                    ConfirmFullAccessDialog,
                    Some("FullAccessDialog"),
                ),
                bound(
                    tr!("shortcuts.confirm_dialog"),
                    provider_switch_dialog::ConfirmProviderSwitchDialog,
                    Some("ProviderSwitchDialog"),
                ),
                bound(
                    tr!("shortcuts.confirm_dialog"),
                    git_panel::ConfirmGitPanelModal,
                    Some("GitPanelModal"),
                ),
                bound(
                    tr!("shortcuts.dismiss_dialog"),
                    commit_dialog::DismissCommitDialog,
                    Some("CommitDialog"),
                ),
                bound(
                    tr!("shortcuts.dismiss_dialog"),
                    goal_dialog::DismissGoalDialog,
                    Some("GoalDialog"),
                ),
                bound(
                    tr!("shortcuts.dismiss_dialog"),
                    DismissArchiveDialog,
                    Some("ArchiveDialog"),
                ),
                bound(
                    tr!("shortcuts.dismiss_dialog"),
                    DismissReclaimDialog,
                    Some("ReclaimDialog"),
                ),
                bound(
                    tr!("shortcuts.dismiss_dialog"),
                    DismissFullAccessDialog,
                    Some("FullAccessDialog"),
                ),
                bound(
                    tr!("shortcuts.dismiss_dialog"),
                    provider_switch_dialog::DismissProviderSwitchDialog,
                    Some("ProviderSwitchDialog"),
                ),
                bound(
                    tr!("shortcuts.dismiss_dialog"),
                    git_panel::DismissGitPanelModal,
                    Some("GitPanelModal"),
                ),
            ],
        ),
        (
            "shortcuts.section.bigpicture",
            vec![
                bound(
                    tr!("shortcuts.bigpicture_move"),
                    big_picture::BigPictureLeft,
                    Some("BigPicture"),
                ),
                bound(
                    tr!("shortcuts.bigpicture_move_right"),
                    big_picture::BigPictureRight,
                    Some("BigPicture"),
                ),
                bound(
                    tr!("shortcuts.bigpicture_open"),
                    big_picture::BigPictureConfirm,
                    Some("BigPicture"),
                ),
                bound(
                    tr!("shortcuts.dismiss"),
                    big_picture::DismissBigPicture,
                    Some("BigPicture"),
                ),
            ],
        ),
        (
            "shortcuts.section.settings",
            vec![
                bound(
                    tr!("shortcuts.select_next"),
                    SelectNextEntry,
                    Some("SettingsSidebar > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_previous"),
                    SelectPreviousEntry,
                    Some("SettingsSidebar > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_next"),
                    SelectNextEntry,
                    Some("SkillsPane > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_previous"),
                    SelectPreviousEntry,
                    Some("SkillsPane > TextInput"),
                ),
                bound(
                    tr!("shortcuts.next_field"),
                    settings::FocusNext,
                    Some("Settings"),
                ),
                bound(
                    tr!("shortcuts.previous_field"),
                    settings::FocusPrevious,
                    Some("Settings"),
                ),
            ],
        ),
        (
            "shortcuts.section.menus",
            vec![
                bound(
                    tr!("shortcuts.select_next"),
                    SelectNextEntry,
                    Some("Menu > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_previous"),
                    SelectPreviousEntry,
                    Some("Menu > TextInput"),
                ),
                bound(
                    tr!("shortcuts.menu_next_tab"),
                    SelectNextTab,
                    Some("Menu > TextInput"),
                ),
                bound(
                    tr!("shortcuts.menu_previous_tab"),
                    SelectPreviousTab,
                    Some("Menu > TextInput"),
                ),
                bound(
                    tr!("shortcuts.confirm"),
                    ConfirmEntry,
                    Some("Menu > TextInput"),
                ),
                bound(tr!("shortcuts.dismiss"), DismissMenu, Some("Menu")),
                bound(
                    tr!("shortcuts.select_next"),
                    SelectNextEntry,
                    Some("ComposerAutocomplete > TextInput"),
                ),
                bound(
                    tr!("shortcuts.select_previous"),
                    SelectPreviousEntry,
                    Some("ComposerAutocomplete > TextInput"),
                ),
                bound(
                    tr!("shortcuts.confirm"),
                    ConfirmEntry,
                    Some("ComposerAutocomplete > TextInput"),
                ),
                bound(
                    tr!("shortcuts.dismiss"),
                    DismissMenu,
                    Some("ComposerAutocomplete > TextInput"),
                ),
                bound(
                    tr!("shortcuts.cancel_rename"),
                    sidebar::CancelSessionRename,
                    Some("SessionRename > TextInput"),
                ),
                bound(tr!("shortcuts.dismiss"), DismissMenu, Some("Annotation")),
                bound(tr!("shortcuts.dismiss"), DismissMenu, Some("PastedText")),
                bound(
                    tr!("shortcuts.close_preview"),
                    image_preview::DismissImagePreview,
                    Some("ImagePreview"),
                ),
            ],
        ),
    ]
}

/// Every binding registered for `action` under exactly `context`, joined for
/// display. `None` when nothing matches — the row is then dropped, which is
/// also how platform-gated bindings disappear on the other platforms.
fn resolve_keys(keys: &ShortcutKeys, cx: &App) -> Option<String> {
    match keys {
        ShortcutKeys::Text(label) => Some((*label).to_owned()),
        ShortcutKeys::Bound { action, context } => {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let labels = keymap
                .bindings_for_action(action.as_ref())
                .filter(|binding| match (&binding.predicate(), context) {
                    (None, None) => true,
                    (Some(predicate), Some(context)) => predicate.to_string() == *context,
                    _ => false,
                })
                .map(crate::ui::shortcut::binding_label)
                .collect::<Vec<_>>();
            (!labels.is_empty()).then(|| labels.join(", "))
        }
    }
}

struct ShortcutsSection {
    title: String,
    rows: Vec<(String, String)>,
}

fn shortcut_sections(cx: &App) -> Vec<ShortcutsSection> {
    shortcut_rows()
        .into_iter()
        .filter_map(|(title_key, rows)| {
            let rows = rows
                .into_iter()
                .filter_map(|row| Some((row.label, resolve_keys(&row.keys, cx)?)))
                .collect::<Vec<_>>();
            (!rows.is_empty()).then(|| ShortcutsSection {
                title: tr!(title_key),
                rows,
            })
        })
        .collect()
}

pub(super) struct ShortcutsDialogState {
    scroll: ScrollHandle,
    focus: FocusHandle,
    done_focus: FocusHandle,
    sections: Vec<ShortcutsSection>,
}

impl Waku {
    pub(super) fn open_shortcuts_dialog(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        let focus = cx.focus_handle();
        self.shortcuts_dialog = Some(ShortcutsDialogState {
            scroll: ScrollHandle::new(),
            focus: focus.clone(),
            done_focus: cx.focus_handle(),
            sections: shortcut_sections(cx),
        });
        cx.notify();
        focus
    }

    fn close_shortcuts_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcuts_dialog.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn scroll_shortcuts_dialog(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = &self.shortcuts_dialog else {
            return;
        };
        let offset = dialog.scroll.offset();
        let max = dialog.scroll.max_offset();
        let delta = match event.keystroke.key.as_str() {
            "down" => px(ROW_HEIGHT * 3.0),
            "up" => px(-ROW_HEIGHT * 3.0),
            "pagedown" | "space" => px(PAGE_SCROLL),
            "pageup" => px(-PAGE_SCROLL),
            "home" => return dialog.scroll.set_offset(point(px(0.0), px(0.0))),
            "end" => return dialog.scroll.set_offset(point(px(0.0), -max.y)),
            _ => return,
        };
        let y = (offset.y + delta).clamp(-max.y, px(0.0));
        dialog.scroll.set_offset(point(px(0.0), y));
        cx.stop_propagation();
    }

    pub(super) fn render_shortcuts_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.shortcuts_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();

        let mut body = div()
            .id("shortcuts-dialog-list")
            .px(px(12.0))
            .pb(px(8.0))
            .max_h(px(LIST_MAX_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&dialog.scroll)
            .flex()
            .flex_col()
            .gap(px(14.0));

        for section in &dialog.sections {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .px(px(4.0))
                            .pb(px(4.0))
                            .text_size(sp(11.5))
                            .text_color(theme.text_secondary)
                            .child(section.title.clone()),
                    )
                    .children(section.rows.iter().map(|(label, keys)| {
                        div()
                            .h(px(ROW_HEIGHT))
                            .px(px(4.0))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text)
                                    .child(label.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(keys.clone()),
                            )
                            .into_any_element()
                    })),
            );
        }

        let done_weak = weak.clone();
        let done_row = div()
            .id("shortcuts-dialog-done")
            .track_focus(&dialog.done_focus)
            .tab_index(0)
            .h(px(38.0))
            .w_full()
            .px(px(10.0))
            .rounded(px(11.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_default()
            .text_size(sp(14.0))
            .text_color(theme.text)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay_strong))
            .child(icon("icons/keyboard.svg", 15.0, theme.text))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(tr!("shortcuts.done")),
            )
            .on_click(move |_, window, cx| {
                let _ = done_weak.update(cx, |waku, cx| waku.close_shortcuts_dialog(window, cx));
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    let _ = weak.update(cx, |waku, cx| waku.close_shortcuts_dialog(window, cx));
                    cx.stop_propagation();
                }
            });

        let card = div()
            .id("shortcuts-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .track_focus(&dialog.focus)
            .on_action(cx.listener(|waku, _: &DismissShortcutsDialog, window, cx| {
                waku.close_shortcuts_dialog(window, cx)
            }))
            .on_key_down(cx.listener(Self::scroll_shortcuts_dialog))
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(520.0))
            .overflow_hidden()
            .rounded(px(21.0))
            .bg(theme.composer)
            .shadow_xl()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(10.0))
                    .flex_none()
                    .text_size(sp(14.0))
                    .text_color(theme.text)
                    .child(tr!("shortcuts.title")),
            )
            .child(body)
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(div().p(px(8.0)).child(done_row));

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("shortcuts-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.close_shortcuts_dialog(window, cx)),
            )
            .child(card);
        Some(gpui::deferred(layer).with_priority(4).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every authored row must resolve to a live binding (or carry text):
    /// the cheatsheet cannot list a chord the keymap does not hold.
    #[gpui::test]
    fn all_bound_rows_resolve(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::bind_keys(cx);
            crate::input::init(cx);
            crate::ui::menu::init(cx);
            crate::app::init_settings_keys(cx);
            crate::app::init_command_palette(cx);
            crate::app::init_file_finder(cx);
            crate::app::init_sync_branch(cx);
            crate::app::init_commit_dialog_keys(cx);
            crate::app::init_git_panel_keys(cx);
            crate::app::init_archive_dialog_keys(cx);
            crate::app::init_reclaim_dialog_keys(cx);
            crate::app::init_full_access_dialog_keys(cx);
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
            crate::app::init_composer_autocomplete(cx);
        });

        let mut unresolved = Vec::new();
        for (section, rows) in shortcut_rows() {
            for row in rows {
                cx.update(|cx| {
                    if resolve_keys(&row.keys, cx).is_none() {
                        unresolved.push(format!("{section}: {}", row.label));
                    }
                });
            }
        }
        assert!(unresolved.is_empty(), "unresolved rows: {unresolved:?}");
    }
}
