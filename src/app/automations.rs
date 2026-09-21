//! Automations page — the scheduling surface over the daemon-owned document.
//!
//! The daemon owns definitions, run history, and the tick that fires them;
//! this page is its control plane. Schedules keeps one row per automation
//! with the next/last run at a glance; Runs is the aggregate feed across
//! every connected daemon. A row opens a detail pane with the prompt, the
//! schedule, and run history — each run links to the task it dispatched
//! to, so transcripts come free. Editing happens in a deferred modal like
//! the goal dialog's.

use std::path::PathBuf;

use gpui::{ElementId, KeyBinding, actions};

use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_client::automations::{
    Automation, AutomationInput, AutomationPrecheck, AutomationRun, AutomationRunStatus,
    AutomationSchedule, AutomationTrigger, AutomationWorkspace,
};

/// Key context the page declares; Escape peels it one layer at a time —
/// editor, detail, then the page itself.
const PAGE_CONTEXT: &str = "AutomationsPage";
const EDITOR_CONTEXT: &str = "AutomationEditor";
const EDITOR_INPUT_CONTEXT: &str = "AutomationEditor > TextInput";

actions!(
    waku_automations,
    [
        DismissAutomationsLayer,
        ConfirmAutomationEditor,
        DismissAutomationEditor,
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", DismissAutomationsLayer, Some(PAGE_CONTEXT)),
        KeyBinding::new(
            "secondary-enter",
            ConfirmAutomationEditor,
            Some(EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "secondary-enter",
            ConfirmAutomationEditor,
            Some(EDITOR_INPUT_CONTEXT),
        ),
        KeyBinding::new("escape", DismissAutomationEditor, Some(EDITOR_CONTEXT)),
    ]);
}

/// Which list the page body shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum AutomationsTab {
    #[default]
    Schedules,
    Runs,
}

impl AutomationsTab {
    const ALL: [AutomationsTab; 2] = [AutomationsTab::Schedules, AutomationsTab::Runs];
}

/// The editor's trigger segmented control — one variant per
/// [`AutomationSchedule`] kind, plus Webhook for HTTP-fired automations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SchedulePreset {
    Hourly,
    #[default]
    Daily,
    Weekdays,
    Weekly,
    Cron,
    Webhook,
}

/// A deferred open. The editor's `TextInput`s need a `Window`, which click
/// handlers on rows do not carry, so opening stages a request the next
/// frame materializes — the same shape as the goal dialog.
pub(super) struct AutomationEditorRequest {
    /// `Some` edits that record; `None` creates on the local daemon.
    edit: Option<(DaemonKey, Uuid)>,
}

/// Draft state for the create/edit modal. Choices that map to enums stay
/// typed; freeform fields live in `TextInput`s.
pub(super) struct AutomationEditor {
    daemon: DaemonKey,
    id: Option<Uuid>,
    name: Entity<TextInput>,
    prompt: Entity<TextInput>,
    preset: SchedulePreset,
    /// "HH:MM" for Daily/Weekdays/Weekly, ":MM" for Hourly.
    time: Entity<TextInput>,
    day_of_week: u8,
    cron: Entity<TextInput>,
    timezone: Entity<TextInput>,
    pub(super) provider: ProviderKind,
    /// Model id, or blank for the provider default. The shared model picker
    /// writes this via `sessions::choose_model`; it stays a `TextInput` so
    /// the row can also accept a pasted id the catalog does not know.
    pub(super) model: Entity<TextInput>,
    project_path: Option<PathBuf>,
    workspace: AutomationWorkspace,
    base_branch: Entity<TextInput>,
    session_id: Option<Uuid>,
    enabled: bool,
    advanced_open: bool,
    precheck: Entity<TextInput>,
    precheck_timeout: Entity<TextInput>,
    grace_minutes: Entity<TextInput>,
    reuse_session: bool,
    error: Option<String>,
    save_focus: FocusHandle,
    advanced_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

/// Runs joined with their automation for the aggregate feed.
struct RunRow {
    daemon: DaemonKey,
    automation_name: String,
    automation_id: Uuid,
    run: AutomationRun,
}

impl Waku {
    // ── Page ─────────────────────────────────────────────────────────────

    /// Open the page as a navigation destination; back returns to whatever
    /// the main column showed before.
    pub(super) fn open_automations_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session_navigation.visit(
            self.navigation_location(),
            NavigationLocation::AutomationsPage,
        );
        self.show_automations_page(window, cx);
    }

    /// Put the page on screen. Recording the move is the caller's job, same
    /// split as `show_drafts_page`: opens `visit`, restores `go_back`.
    pub(super) fn show_automations_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = None;
        self.projects_page = None;
        self.drafts_page = false;
        self.notifications.open = false;
        self.selected_terminal = None;
        // An activation still in flight must not hand the area back once
        // its hydration lands — same guard the Projects page takes.
        self.pending_session_activation = None;
        if self
            .sidebar_collapsed_groups
            .insert(SidebarGroup::Terminals)
        {
            self.sidebar_rows_fingerprint.set(None);
        }
        self.automations_page = true;
        // The page owns its own strip — whatever was mounted (a session's,
        // a terminal's) parks until it comes back.
        self.sync_right_panel_owner(cx);
        let focus = self.automations_search.read(cx).focus();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn close_automations_page(&mut self, cx: &mut Context<Self>) {
        if !self.automations_page {
            return;
        }
        self.automations_page = false;
        self.automations_detail = None;
        // Closing is a location change too: the surface underneath comes
        // back, and back returns to the page.
        if let Some(location) = self.navigation_location() {
            self.session_navigation
                .visit(Some(NavigationLocation::AutomationsPage), location);
        }
        self.sync_right_panel_owner(cx);
        cx.notify();
    }

    pub(super) fn toggle_automations_page_action(
        &mut self,
        _: &ToggleAutomationsPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.automations_page {
            self.close_automations_page(cx);
        } else {
            self.open_automations_page(window, cx);
        }
    }

    /// Escape peels the innermost layer first — the editor (handled by its
    /// own context) — then the detail pane, then the page.
    pub(super) fn dismiss_automations_layer_action(
        &mut self,
        _: &DismissAutomationsLayer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.automations_page {
            return;
        }
        if self.automations_detail.take().is_some() {
            let focus = self.automations_search.read(cx).focus();
            window.focus(&focus, cx);
            cx.notify();
            return;
        }
        self.close_automations_page(cx);
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    fn set_automations_tab(&mut self, tab: AutomationsTab, cx: &mut Context<Self>) {
        if self.automations_tab == tab {
            return;
        }
        self.automations_tab = tab;
        cx.notify();
    }

    /// ⌘⌥1–2: switch the open page's tab. The chord only exists inside the
    /// page's key context, so a closed page never sees it.
    pub(super) fn select_automations_tab_action(
        &mut self,
        action: &SelectAutomationsTab,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.automations_page {
            return;
        }
        let Some(tab) = AutomationsTab::ALL.get(action.index).copied() else {
            return;
        };
        self.set_automations_tab(tab, cx);
    }

    /// Toggling the experiment off closes the page and any editor so the
    /// surface cannot linger reachable-but-dead.
    pub(super) fn apply_automations_enabled(&mut self, cx: &mut Context<Self>) {
        if self.state.automations_enabled {
            return;
        }
        self.automations_editor_request = None;
        self.automations_editor = None;
        self.automations_detail = None;
        self.automations_page = false;
        self.sync_right_panel_owner(cx);
        cx.notify();
    }

    // ── Document ─────────────────────────────────────────────────────────

    /// Every automation across connected daemons, name-sorted for the list.
    fn automation_entries(&self) -> Vec<(DaemonKey, &Automation)> {
        let mut entries: Vec<(DaemonKey, &Automation)> = self
            .automations
            .iter()
            .flat_map(|(key, state)| state.automations.iter().map(|a| (*key, a)))
            .collect();
        entries.sort_by(|(a_key, a), (b_key, b)| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a_key.cmp(b_key))
                .then(a.id.cmp(&b.id))
        });
        entries
    }

    fn automation(&self, key: DaemonKey, id: Uuid) -> Option<&Automation> {
        self.automations
            .get(&key)?
            .automations
            .iter()
            .find(|automation| automation.id == id)
    }

    fn automation_runs(&self, key: DaemonKey, id: Uuid) -> Vec<&AutomationRun> {
        self.automations
            .get(&key)
            .map(|state| {
                state
                    .runs
                    .iter()
                    .filter(|run| run.automation_id == id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The aggregate Runs feed: every run across every daemon, newest first.
    fn automation_run_rows(&self, query: &str) -> Vec<RunRow> {
        let needle = query.trim().to_lowercase();
        let mut rows: Vec<RunRow> = self
            .automations
            .iter()
            .flat_map(|(key, state)| state.runs.iter().map(move |run| (*key, run.clone())))
            .filter_map(|(key, run)| {
                let automation = self.automation(key, run.automation_id);
                let name = automation
                    .map(|automation| automation.name.clone())
                    .unwrap_or_else(|| tr!("automations.deleted_name"));
                Some(RunRow {
                    daemon: key,
                    automation_name: name,
                    automation_id: run.automation_id,
                    run,
                })
            })
            .filter(|row| {
                needle.is_empty()
                    || row.automation_name.to_lowercase().contains(&needle)
                    || row
                        .run
                        .error
                        .as_deref()
                        .is_some_and(|error| error.to_lowercase().contains(&needle))
            })
            .collect();
        rows.sort_by(|a, b| b.run.scheduled_for.cmp(&a.run.scheduled_for));
        rows
    }

    /// Whether any row reports a daemon besides the local one — the Host
    /// column only earns its width then.
    fn automations_multi_host(&self) -> bool {
        self.automations
            .iter()
            .any(|(key, state)| key.is_remote() && !state.automations.is_empty())
    }

    fn daemon_label(&self, key: DaemonKey) -> String {
        match key {
            DaemonKey::Local => tr!("automations.host_local"),
            DaemonKey::Remote(host) => self
                .state
                .remote_hosts
                .iter()
                .find(|remote| remote.id == host)
                .map(|remote| remote.name.clone())
                .unwrap_or_else(|| tr!("automations.host_remote")),
        }
    }

    /// The POST URL that fires this automation — plain-HTTP routes share
    /// the daemon's port, so the client's connect address is the base.
    fn automation_webhook_url(&self, key: DaemonKey, automation: &Automation) -> Option<String> {
        let secret = automation.webhook_secret.as_deref()?;
        let client = self.daemons.supervisor(key)?.client();
        Some(format!(
            "{}/automations/{}/trigger?key={secret}",
            webhook_http_base(client.address()),
            automation.id
        ))
    }

    /// A copy-on-click chip showing the webhook URL. The key in the URL is
    /// the credential, so it stays readable for wiring into the caller.
    fn webhook_url_chip(
        &self,
        automation_id: Uuid,
        url: String,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let feedback_id = format!("automation-webhook-copy-{automation_id}");
        let copied = self.control_was_copied(&feedback_id);
        let label = url.clone();
        div()
            .id(ElementId::Name(
                format!("automation-webhook-{automation_id}").into(),
            ))
            .tab_index(0)
            .h(px(24.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .bg(theme.composer)
            .cursor_pointer()
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|element| element.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                this.show_control_copied(feedback_id.clone(), cx);
            })
            .child(
                div()
                    .max_w(px(360.0))
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(label),
            )
            .child(icon(
                if copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                theme.text_tertiary,
            ))
            .into_any_element()
    }

    // ── Commands ─────────────────────────────────────────────────────────

    /// Fire a daemon command on a worker thread. The broadcast that answers
    /// it lands through `apply_automations_document`; only failures surface
    /// here.
    fn automations_command(
        &mut self,
        key: DaemonKey,
        command: waku_client::Command,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            self.show_toast(tr!("automations.daemon_unreachable"));
            cx.notify();
            return;
        };
        cx.spawn(async move |this, cx| {
            let error = cx
                .background_executor()
                .spawn(async move { client.request(Uuid::nil(), Uuid::nil(), command) })
                .await
                .err()
                .map(|error| error.to_string());
            if let Some(error) = error {
                let _ = this.update(cx, |this, _cx| {
                    this.show_toast(tr!("automations.command_failed", error = error));
                });
            }
        })
        .detach();
    }

    fn set_automation_enabled(
        &mut self,
        key: DaemonKey,
        id: Uuid,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(automation) = self.automation(key, id).cloned() else {
            return;
        };
        self.automations_command(
            key,
            waku_client::Command::UpsertAutomation {
                input: automation_input(&automation, enabled),
            },
            cx,
        );
    }

    fn run_automation_now(&mut self, key: DaemonKey, id: Uuid, cx: &mut Context<Self>) {
        self.automations_command(
            key,
            waku_client::Command::RunAutomationNow { automation_id: id },
            cx,
        );
    }

    fn remove_automation(&mut self, key: DaemonKey, id: Uuid, cx: &mut Context<Self>) {
        if self.automations_detail == Some((key, id)) {
            self.automations_detail = None;
        }
        self.automations_command(
            key,
            waku_client::Command::RemoveAutomation { automation_id: id },
            cx,
        );
    }

    // ── Editor ───────────────────────────────────────────────────────────

    /// Stage the editor; the next frame builds it. `None` creates on the
    /// local daemon, `Some((daemon, id))` edits that record.
    fn request_automation_editor(
        &mut self,
        edit: Option<(DaemonKey, Uuid)>,
        cx: &mut Context<Self>,
    ) {
        self.automations_editor_request = Some(AutomationEditorRequest { edit });
        cx.notify();
    }

    fn materialize_automation_editor(
        &mut self,
        request: AutomationEditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = request
            .edit
            .and_then(|(key, id)| self.automation(key, id).cloned());
        let daemon = request.edit.map(|(key, _)| key).unwrap_or(DaemonKey::Local);
        let mut input =
            |build: fn(TextInput) -> TextInput| cx.new(|cx| build(TextInput::new(window, cx)));
        let name = input(|input| {
            input
                .select_all_on_focus_click()
                .accessibility_label(tr!("automations.field_name"))
                .placeholder(tr!("automations.name_placeholder"))
        });
        let prompt = input(|input| {
            input
                .multi_line()
                .auto_height()
                .max_lines(8)
                .accessibility_label(tr!("automations.field_prompt"))
                .placeholder(tr!("automations.prompt_placeholder"))
        });
        let time = input(|input| {
            input
                .accessibility_label(tr!("automations.field_time"))
                .placeholder("09:00")
        });
        let cron = input(|input| {
            input
                .accessibility_label(tr!("automations.field_cron"))
                .placeholder("0 9 * * 1-5")
        });
        let timezone = input(|input| {
            input
                .accessibility_label(tr!("automations.field_timezone"))
                .placeholder(tr!("automations.timezone_placeholder"))
        });
        let model = input(|input| {
            input
                .accessibility_label(tr!("automations.field_model"))
                .placeholder(tr!("automations.model_placeholder"))
        });
        let base_branch = input(|input| {
            input
                .accessibility_label(tr!("automations.field_base_branch"))
                .placeholder("main")
        });
        let precheck = input(|input| {
            input
                .accessibility_label(tr!("automations.field_precheck"))
                .placeholder(tr!("automations.precheck_placeholder"))
        });
        let precheck_timeout = input(|input| {
            input
                .accessibility_label(tr!("automations.field_precheck_timeout"))
                .placeholder("60")
        });
        let grace_minutes = input(|input| {
            input
                .accessibility_label(tr!("automations.field_grace"))
                .placeholder(tr!("automations.grace_placeholder"))
        });

        let mut preset = SchedulePreset::Daily;
        let mut day_of_week = 5u8;
        if let Some(automation) = &existing {
            name.update(cx, |input, cx| {
                input.set_content(automation.name.clone(), cx)
            });
            prompt.update(cx, |input, cx| {
                input.set_content(automation.prompt.clone(), cx)
            });
            timezone.update(cx, |input, cx| {
                input.set_content(automation.timezone.clone().unwrap_or_default(), cx)
            });
            model.update(cx, |input, cx| {
                input.set_content(automation.model.clone().unwrap_or_default(), cx)
            });
            base_branch.update(cx, |input, cx| {
                input.set_content(automation.base_branch.clone().unwrap_or_default(), cx)
            });
            if let Some(check) = &automation.precheck {
                precheck.update(cx, |input, cx| input.set_content(check.command.clone(), cx));
                precheck_timeout.update(cx, |input, cx| {
                    input.set_content(check.timeout_seconds.to_string(), cx)
                });
            }
            grace_minutes.update(cx, |input, cx| {
                input.set_content(
                    automation
                        .missed_run_grace_minutes
                        .map(|minutes| minutes.to_string())
                        .unwrap_or_default(),
                    cx,
                )
            });
            match &automation.schedule {
                Some(AutomationSchedule::Hourly { minute }) => {
                    preset = SchedulePreset::Hourly;
                    time.update(cx, |input, cx| {
                        input.set_content(format!(":{minute:02}"), cx)
                    });
                }
                Some(AutomationSchedule::Daily { hour, minute })
                | Some(AutomationSchedule::Weekdays { hour, minute }) => {
                    preset =
                        if matches!(automation.schedule, Some(AutomationSchedule::Daily { .. })) {
                            SchedulePreset::Daily
                        } else {
                            SchedulePreset::Weekdays
                        };
                    time.update(cx, |input, cx| {
                        input.set_content(format!("{hour:02}:{minute:02}"), cx)
                    });
                }
                Some(AutomationSchedule::Weekly {
                    day_of_week: day,
                    hour,
                    minute,
                }) => {
                    preset = SchedulePreset::Weekly;
                    day_of_week = *day;
                    time.update(cx, |input, cx| {
                        input.set_content(format!("{hour:02}:{minute:02}"), cx)
                    });
                }
                Some(AutomationSchedule::Cron { expression }) => {
                    preset = SchedulePreset::Cron;
                    cron.update(cx, |input, cx| input.set_content(expression.clone(), cx));
                }
                None => {
                    if automation.webhook_secret.is_some() {
                        preset = SchedulePreset::Webhook;
                    }
                }
            }
        }

        let name_focus = name.read(cx).focus();
        self.automations_editor = Some(AutomationEditor {
            daemon,
            id: existing.as_ref().map(|automation| automation.id),
            name,
            prompt,
            preset,
            time,
            day_of_week,
            cron,
            timezone,
            provider: existing
                .as_ref()
                .map(|automation| automation.provider)
                .unwrap_or(self.state.last_provider),
            model,
            project_path: existing
                .as_ref()
                .map(|automation| automation.project_path.clone())
                .or_else(|| {
                    self.state
                        .selected_project
                        .and_then(|project_id| {
                            self.state
                                .projects
                                .iter()
                                .find(|project| project.id == project_id)
                        })
                        .map(|project| project.path.clone())
                }),
            workspace: existing
                .as_ref()
                .map(|automation| automation.workspace)
                .unwrap_or_default(),
            base_branch,
            session_id: existing
                .as_ref()
                .and_then(|automation| automation.session_id),
            enabled: existing
                .as_ref()
                .map(|automation| automation.enabled)
                .unwrap_or(true),
            advanced_open: false,
            precheck,
            precheck_timeout,
            grace_minutes,
            reuse_session: existing
                .as_ref()
                .is_some_and(|automation| automation.reuse_session),
            error: None,
            save_focus: cx.focus_handle(),
            advanced_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
        });
        // Like Goddard's other deferred surfaces, the modal joins the
        // dispatch tree only after it has drawn; focus it two frames later.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&name_focus, cx));
        });
        cx.notify();
    }

    fn close_automation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.automations_editor_request = None;
        if self.automations_editor.take().is_none() {
            return;
        }
        // The picker's target is only meaningful while the editor exists.
        self.model_picker_target = composer::ModelPickerTarget::Composer;
        let focus = self.automations_search.read(cx).focus();
        window.focus(&focus, cx);
        cx.notify();
    }

    fn dismiss_automation_editor_action(
        &mut self,
        _: &DismissAutomationEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_automation_editor(window, cx);
    }

    /// Read the form, build the input, send the upsert. Validation lives
    /// here first so the common typos fail before the daemon sees them.
    fn confirm_automation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.automations_editor.as_mut() else {
            return;
        };
        let name = editor.name.read(cx).content().trim().to_owned();
        let prompt = editor.prompt.read(cx).content().trim().to_owned();
        let fail = |editor: &mut AutomationEditor, message: String| {
            editor.error = Some(message);
        };
        if name.is_empty() {
            fail(editor, tr!("automations.error_name_required"));
            cx.notify();
            return;
        }
        if prompt.is_empty() {
            fail(editor, tr!("automations.error_prompt_required"));
            cx.notify();
            return;
        }
        let schedule = match editor.preset {
            SchedulePreset::Webhook => None,
            _ => {
                let schedule = match editor.preset {
                    SchedulePreset::Hourly => parse_clock(&editor.time.read(cx).content())
                        .map(|(_, minute)| AutomationSchedule::Hourly { minute }),
                    SchedulePreset::Daily => parse_clock(&editor.time.read(cx).content())
                        .map(|(hour, minute)| AutomationSchedule::Daily { hour, minute }),
                    SchedulePreset::Weekdays => parse_clock(&editor.time.read(cx).content())
                        .map(|(hour, minute)| AutomationSchedule::Weekdays { hour, minute }),
                    SchedulePreset::Weekly => {
                        parse_clock(&editor.time.read(cx).content()).map(|(hour, minute)| {
                            AutomationSchedule::Weekly {
                                day_of_week: editor.day_of_week,
                                hour,
                                minute,
                            }
                        })
                    }
                    SchedulePreset::Cron => {
                        let expression = editor.cron.read(cx).content().trim().to_owned();
                        if expression.is_empty() {
                            None
                        } else {
                            Some(AutomationSchedule::Cron { expression })
                        }
                    }
                    SchedulePreset::Webhook => unreachable!(),
                };
                let Some(schedule) = schedule else {
                    fail(editor, tr!("automations.error_schedule_invalid"));
                    cx.notify();
                    return;
                };
                Some(schedule)
            }
        };
        let Some(project_path) = editor.project_path.clone() else {
            fail(editor, tr!("automations.error_project_required"));
            cx.notify();
            return;
        };
        if matches!(editor.workspace, AutomationWorkspace::Existing) && editor.session_id.is_none()
        {
            fail(editor, tr!("automations.error_session_required"));
            cx.notify();
            return;
        }
        let timezone = editor.timezone.read(cx).content().trim().to_owned();
        let model = editor.model.read(cx).content().trim().to_owned();
        let base_branch = editor.base_branch.read(cx).content().trim().to_owned();
        let precheck_command = editor.precheck.read(cx).content().trim().to_owned();
        let grace = editor.grace_minutes.read(cx).content().trim().to_owned();
        let timeout = editor.precheck_timeout.read(cx).content().trim().to_owned();
        let input = AutomationInput {
            id: editor.id,
            name,
            prompt,
            provider: editor.provider,
            model: (!model.is_empty()).then_some(model),
            project_path,
            workspace: editor.workspace,
            base_branch: (!base_branch.is_empty()).then_some(base_branch),
            session_id: editor.session_id,
            schedule,
            webhook: editor.preset == SchedulePreset::Webhook,
            timezone: (!timezone.is_empty()).then_some(timezone),
            enabled: editor.enabled,
            precheck: (!precheck_command.is_empty()).then(|| AutomationPrecheck {
                command: precheck_command,
                timeout_seconds: timeout.parse().unwrap_or(60),
            }),
            missed_run_grace_minutes: grace.parse().ok(),
            reuse_session: editor.reuse_session,
        };
        let daemon = editor.daemon;
        self.automations_command(daemon, waku_client::Command::UpsertAutomation { input }, cx);
        self.close_automation_editor(window, cx);
    }

    // ── Render ───────────────────────────────────────────────────────────

    pub(super) fn render_automations_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let query = self
            .automations_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();

        let body: AnyElement = if let Some((key, id)) = self.automations_detail {
            self.render_automation_detail(key, id, &theme, cx)
        } else {
            match self.automations_tab {
                AutomationsTab::Schedules => self.render_automations_list(&query, &theme, cx),
                AutomationsTab::Runs => self.render_automations_runs(&query, &theme, cx),
            }
        };

        div()
            .key_context(PAGE_CONTEXT)
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::dismiss_automations_layer_action))
            .child(self.render_automations_header(&theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH + 48.0))
                    .mx_auto()
                    .px(px(24.0))
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .into_any_element()
    }

    /// The page's top bar: title (or breadcrumb in detail), the Schedules /
    /// Runs switch, search, and the New button.
    fn render_automations_header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let in_detail = self.automations_detail.is_some();
        let tab = self.automations_tab;

        let tab_chip = |label: String,
                        active: bool,
                        tab: AutomationsTab,
                        id: &'static str,
                        cx: &mut Context<Self>|
         -> Stateful<Div> {
            div()
                .id(id)
                .tab_index(0)
                .h(px(24.0))
                .px(px(10.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .text_size(sp(12.5))
                .text_color(if active {
                    theme.text
                } else {
                    theme.text_tertiary
                })
                .when(active, |element| element.bg(theme.overlay))
                .cursor_pointer()
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|element| element.bg(theme.focus_highlight()))
                .on_activation(cx, move |this, _, cx| {
                    this.automations_detail = None;
                    this.set_automations_tab(tab, cx);
                })
                .child(label)
        };

        let new_button = div()
            .id("automations-new")
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(7.0))
            .flex()
            .items_center()
            .gap(px(5.0))
            .text_size(sp(12.5))
            .text_color(theme.text)
            .bg(theme.raised)
            .cursor_pointer()
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|element| element.bg(theme.focus_highlight()))
            .track_focus(&self.automations_new_focus)
            .tab_index(0)
            .on_click(cx.listener(|this, _, _window, cx| {
                this.request_automation_editor(None, cx);
            }))
            .child(icon("icons/plus.svg", 11.0, theme.text))
            .child(tr!("automations.new"));

        div()
            .flex_none()
            .h(px(52.0))
            .px(px(20.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .id("automations-title")
                            .text_size(sp(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .when(in_detail, |element| {
                                element
                                    .cursor_pointer()
                                    .hover(|element| element.text_color(theme.accent))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.automations_detail = None;
                                        cx.notify();
                                    }))
                            })
                            .child(tr!("automations.title")),
                    )
                    .when(!in_detail, |element| {
                        element.child(
                            div()
                                .h(px(26.0))
                                .px(px(3.0))
                                .rounded(px(7.0))
                                .bg(theme.sidebar_item_background)
                                .flex()
                                .items_center()
                                .gap(px(2.0))
                                .child(tab_chip(
                                    tr!("automations.tab_schedules"),
                                    tab == AutomationsTab::Schedules,
                                    AutomationsTab::Schedules,
                                    "automations-tab-schedules",
                                    cx,
                                ))
                                .child(tab_chip(
                                    tr!("automations.tab_runs"),
                                    tab == AutomationsTab::Runs,
                                    AutomationsTab::Runs,
                                    "automations-tab-runs",
                                    cx,
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(220.0))
                    .flex_none()
                    .child(self.automations_search.clone()),
            )
            .child(new_button)
            .into_any_element()
    }

    /// The Schedules list: one row per automation — enable toggle, name,
    /// schedule, next and last run, agent, and host when it matters.
    fn render_automations_list(
        &mut self,
        query: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let multi_host = self.automations_multi_host();
        let rows: Vec<(DaemonKey, Uuid)> = self
            .automation_entries()
            .into_iter()
            .filter(|(_, automation)| automation_matches(automation, query))
            .map(|(key, automation)| (key, automation.id))
            .collect();
        self.sync_automations_rows(&rows);

        if rows.is_empty() {
            let (title, hint) = if !query.is_empty() {
                (
                    tr!("automations.no_match"),
                    tr!("automations.no_match_hint"),
                )
            } else {
                (
                    tr!("automations.empty_title"),
                    tr!("automations.empty_hint"),
                )
            };
            return automations_status_row(theme, title, hint).into_any_element();
        }

        let entity = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                list(
                    self.automations_list_state.clone(),
                    move |index, _window, cx| {
                        entity
                            .upgrade()
                            .map(|entity| {
                                entity.update(cx, |this, cx| {
                                    this.automation_row(index, multi_host, cx)
                                })
                            })
                            .unwrap_or_else(|| div().into_any_element())
                    },
                )
                .size_full(),
            )
            .child(scrollbar::vertical(
                &self.automations_list_state,
                &self.automations_scrollbar,
            ))
            .into_any_element()
    }

    /// The aggregate run feed across automations and daemons.
    fn render_automations_runs(
        &mut self,
        query: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows: Vec<(DaemonKey, Uuid)> = self
            .automation_run_rows(query)
            .iter()
            .map(|row| (row.daemon, row.run.id))
            .collect();
        self.sync_automations_run_rows(&rows);

        if rows.is_empty() {
            let (title, hint) = if !query.is_empty() {
                (
                    tr!("automations.no_match"),
                    tr!("automations.no_match_hint"),
                )
            } else {
                (
                    tr!("automations.runs_empty_title"),
                    tr!("automations.runs_empty_hint"),
                )
            };
            return automations_status_row(theme, title, hint).into_any_element();
        }
        let entity = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                list(
                    self.automations_runs_list_state.clone(),
                    move |index, _window, cx| {
                        entity
                            .upgrade()
                            .map(|entity| {
                                entity.update(cx, |this, cx| this.automation_run_row(index, cx))
                            })
                            .unwrap_or_else(|| div().into_any_element())
                    },
                )
                .size_full(),
            )
            .child(scrollbar::vertical(
                &self.automations_runs_list_state,
                &self.automations_runs_scrollbar,
            ))
            .into_any_element()
    }

    fn sync_automations_rows(&self, rows: &[(DaemonKey, Uuid)]) {
        let mut cached = self.automations_rows.borrow_mut();
        if *cached != rows {
            *cached = rows.to_vec();
            self.automations_list_state.reset(rows.len());
        }
    }

    fn sync_automations_run_rows(&self, rows: &[(DaemonKey, Uuid)]) {
        let mut cached = self.automations_run_rows.borrow_mut();
        if *cached != rows {
            *cached = rows.to_vec();
            self.automations_runs_list_state.reset(rows.len());
        }
    }

    /// The run behind a cached Runs-tab row: `(daemon, run_id)` plus its
    /// automation's name.
    fn cached_run_row(&self, index: usize) -> Option<RunRow> {
        let (key, run_id) = *self.automations_run_rows.borrow().get(index)?;
        let state = self.automations.get(&key)?;
        let run = state.runs.iter().find(|run| run.id == run_id)?.clone();
        let name = state
            .automations
            .iter()
            .find(|automation| automation.id == run.automation_id)
            .map(|automation| automation.name.clone())
            .unwrap_or_else(|| tr!("automations.deleted_name"));
        Some(RunRow {
            daemon: key,
            automation_name: name,
            automation_id: run.automation_id,
            run,
        })
    }

    fn automation_row(&self, index: usize, multi_host: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some((key, id)) = self.automations_rows.borrow().get(index).copied() else {
            return div().into_any_element();
        };
        let Some(automation) = self.automation(key, id) else {
            return div().into_any_element();
        };
        let now = unix_time();
        let schedule_label = trigger_label(automation);
        let next_label = automation
            .next_run_at
            .filter(|_| automation.enabled)
            .map(|at| relative_time_until(at, now))
            .unwrap_or_else(|| "—".to_owned());
        let (status_icon, status_color, status_label) = automation
            .last_run_status
            .map(|status| run_status_badge(&theme, status))
            .unwrap_or((
                "icons/loader-circle.svg",
                theme.text_tertiary,
                tr!("automations.last_run_never"),
            ));
        let last_label = automation
            .last_run_at
            .map(|at| format!("{status_label} · {}", sidebar::format_time_ago(now - at)))
            .unwrap_or(status_label);
        let host_label = multi_host.then(|| self.daemon_label(key));
        let enabled = automation.enabled;
        let selected = self.automations_detail == Some((key, id));

        // The shared switch, not a bespoke pill — its `on_activation` stops
        // propagation, so toggling does not also open the row's detail.
        let toggle = toggle_switch(
            ElementId::Name(format!("automation-toggle-{id}").into()),
            enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_automation_enabled(key, id, !enabled, cx),
        );

        div()
            .id(ElementId::Name(format!("automation-row-{id}").into()))
            .h(px(44.0))
            .mx(px(-8.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_pointer()
            .when(selected, |element| {
                element.bg(theme.sidebar_item_background)
            })
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|element| element.bg(theme.focus_highlight()))
            .track_focus(&self.automations_row_focus)
            .tab_index(0)
            .on_activation(cx, move |this, _, cx| {
                this.automations_detail = Some((key, id));
                cx.notify();
            })
            .child(toggle)
            .child(
                div()
                    .w(px(200.0))
                    .flex_none()
                    .truncate()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(automation.name.clone()),
            )
            .child(
                div()
                    .w(px(190.0))
                    .flex_none()
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(schedule_label),
            )
            .child(
                div()
                    .w(px(110.0))
                    .flex_none()
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(next_label),
            )
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(icon(status_icon, 11.0, status_color))
                    .child(
                        div()
                            .truncate()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(last_label),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .child(automation.provider.display_name()),
            )
            .when_some(host_label, |element, label| {
                element.child(
                    div()
                        .flex_none()
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded(px(5.0))
                        .bg(theme.overlay)
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(label),
                )
            })
            .into_any_element()
    }

    /// One row of the aggregate Runs feed.
    fn automation_run_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(row) = self.cached_run_row(index) else {
            return div().into_any_element();
        };
        let run = &row.run;
        let (icon_path, color, _) = run_status_badge(&theme, run.status);
        let now = unix_time();
        let when = sidebar::format_time_ago(now - run.scheduled_for);
        let detail = run
            .error
            .clone()
            .or_else(|| run.precheck.as_ref().and_then(|p| p.detail.clone()));
        let session_id = run.session_id;
        let key = row.daemon;
        let automation_id = row.automation_id;
        let refusal = (run.refusal_count > 0)
            .then(|| tr!("automations.refusal_count", count = run.refusal_count + 1));

        let focus = self.transcript_control_focus(format!("automation-run-focus-{}", run.id), cx);
        div()
            .id(ElementId::Name(format!("automation-run-{}", run.id).into()))
            .track_focus(&focus)
            .tab_index(0)
            .min_h(px(40.0))
            .py(px(7.0))
            .mx(px(-8.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_pointer()
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|element| element.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                this.automations_detail = Some((key, automation_id));
                this.automations_tab = AutomationsTab::Schedules;
                cx.notify();
            })
            .child(icon(icon_path, 12.0, color))
            .child(
                div()
                    .w(px(180.0))
                    .flex_none()
                    .truncate()
                    .text_size(sp(12.5))
                    .text_color(theme.text)
                    .child(row.automation_name.clone()),
            )
            .child(
                div()
                    .w(px(90.0))
                    .flex_none()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(when),
            )
            .child(
                div()
                    .w(px(110.0))
                    .flex_none()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(match run.trigger {
                        AutomationTrigger::Scheduled => tr!("automations.trigger_scheduled"),
                        AutomationTrigger::Manual => tr!("automations.trigger_manual"),
                        AutomationTrigger::Webhook => tr!("automations.trigger_webhook"),
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(detail.unwrap_or_else(|| refusal.unwrap_or_else(|| "—".to_owned()))),
            )
            .when_some(session_id, |element, session_id| {
                element.child(
                    div()
                        .id(ElementId::Name(
                            format!("automation-run-task-{}", run.id).into(),
                        ))
                        .tab_index(0)
                        .flex_none()
                        .h(px(22.0))
                        .px(px(7.0))
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .text_size(sp(11.5))
                        .text_color(theme.accent)
                        .bg(theme.overlay)
                        .cursor_pointer()
                        .hover(|element| element.bg(theme.overlay))
                        .focus_visible(|element| element.bg(theme.focus_highlight()))
                        .on_activation(cx, move |this, _, cx| {
                            this.select_session(session_id, cx);
                        })
                        .child(icon("icons/arrow-right.svg", 10.0, theme.accent))
                        .child(tr!("automations.open_task")),
                )
            })
            .into_any_element()
    }

    /// The detail pane: overview, prompt preview, and run history for one
    /// automation.
    fn render_automation_detail(
        &mut self,
        key: DaemonKey,
        id: Uuid,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(automation) = self.automation(key, id).cloned() else {
            self.automations_detail = None;
            return automations_status_row(
                theme,
                tr!("automations.detail_gone"),
                tr!("automations.detail_gone_hint"),
            )
            .into_any_element();
        };
        let now = unix_time();
        let delete_name = automation.name.clone();
        let runs = self.automation_runs(key, id);
        let webhook_url = self.automation_webhook_url(key, &automation);
        let (_, _, status_label) = automation
            .last_run_status
            .map(|status| run_status_badge(&theme, status))
            .unwrap_or((
                "icons/loader-circle.svg",
                theme.text_tertiary,
                tr!("automations.last_run_never"),
            ));

        let field = |label: String, value: String| {
            div()
                .flex()
                .items_baseline()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(96.0))
                        .flex_none()
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(sp(12.5))
                        .text_color(theme.text)
                        .child(value),
                )
        };

        let action_button = |id_name: &'static str,
                             icon_path: &'static str,
                             label: String,
                             color: Hsla,
                             activate: Box<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>,
                             cx: &mut Context<Self>|
         -> AnyElement {
            div()
                .id(id_name)
                .tab_index(0)
                .h(px(26.0))
                .px(px(10.0))
                .rounded(px(7.0))
                .flex()
                .items_center()
                .gap(px(5.0))
                .text_size(sp(12.5))
                .text_color(color)
                .bg(theme.raised)
                .cursor_pointer()
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|element| element.bg(theme.focus_highlight()))
                .on_activation(cx, move |this, window, cx| activate(this, window, cx))
                .child(icon(icon_path, 11.0, color))
                .child(label)
                .into_any_element()
        };

        let run_rows: Vec<AnyElement> = runs
            .iter()
            .map(|run| {
                let (icon_path, color, label) = run_status_badge(&theme, run.status);
                let when = sidebar::format_time_ago(now - run.scheduled_for);
                let detail = run
                    .error
                    .clone()
                    .or_else(|| run.precheck.as_ref().and_then(|p| p.detail.clone()));
                let session_id = run.session_id;
                let duration = run
                    .started_at
                    .zip(run.finished_at)
                    .map(|(start, end)| format!("{}s", end.saturating_sub(start)));
                div()
                    .id(ElementId::Name(format!("detail-run-{}", run.id).into()))
                    .h(px(34.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .border_b_1()
                    .border_color(theme.separator)
                    .child(icon(icon_path, 11.0, color))
                    .child(
                        div()
                            .w(px(130.0))
                            .flex_none()
                            .text_size(sp(12.0))
                            .text_color(theme.text)
                            .child(label),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .flex_none()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(when),
                    )
                    .child(
                        div()
                            .w(px(60.0))
                            .flex_none()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(duration.unwrap_or_else(|| "—".to_owned())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(detail.unwrap_or_else(|| "—".to_owned())),
                    )
                    .when_some(session_id, |element, session_id| {
                        element.child(
                            div()
                                .id(ElementId::Name(
                                    format!("detail-run-task-{}", run.id).into(),
                                ))
                                .tab_index(0)
                                .flex_none()
                                .h(px(20.0))
                                .px(px(6.0))
                                .rounded(px(5.0))
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .text_size(sp(11.0))
                                .text_color(theme.accent)
                                .bg(theme.overlay)
                                .cursor_pointer()
                                .focus_visible(|element| element.bg(theme.focus_highlight()))
                                .on_activation(cx, move |this, _, cx| {
                                    this.select_session(session_id, cx);
                                })
                                .child(icon("icons/arrow-right.svg", 9.0, theme.accent))
                                .child(tr!("automations.open_task")),
                        )
                    })
                    .into_any_element()
            })
            .collect();

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                // Detail header: name, status pill, and the row of actions.
                div()
                    .flex_none()
                    .py(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(automation.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px(px(7.0))
                            .py(px(2.0))
                            .rounded(px(9.0))
                            .bg(theme.overlay)
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(icon(
                                if automation.enabled {
                                    "icons/play.svg"
                                } else {
                                    "icons/stop.svg"
                                },
                                9.0,
                                if automation.enabled {
                                    theme.success
                                } else {
                                    theme.text_tertiary
                                },
                            ))
                            .text_size(sp(11.5))
                            .text_color(if automation.enabled {
                                theme.success
                            } else {
                                theme.text_tertiary
                            })
                            .child(if automation.enabled {
                                tr!("automations.enabled")
                            } else {
                                tr!("automations.paused")
                            }),
                    )
                    .child(action_button(
                        "automation-run-now",
                        "icons/play.svg",
                        tr!("automations.run_now"),
                        theme.text,
                        Box::new(move |this, _window, cx| {
                            this.run_automation_now(key, id, cx);
                        }),
                        cx,
                    ))
                    .child(action_button(
                        "automation-edit",
                        "icons/pencil.svg",
                        tr!("automations.edit"),
                        theme.text,
                        Box::new(move |this, _window, cx| {
                            this.request_automation_editor(Some((key, id)), cx);
                        }),
                        cx,
                    ))
                    .child(action_button(
                        "automation-pause",
                        if automation.enabled {
                            "icons/stop.svg"
                        } else {
                            "icons/play.svg"
                        },
                        if automation.enabled {
                            tr!("automations.pause")
                        } else {
                            tr!("automations.resume")
                        },
                        theme.text,
                        Box::new(move |this, _window, cx| {
                            this.set_automation_enabled(key, id, !automation.enabled, cx);
                        }),
                        cx,
                    ))
                    .child(action_button(
                        "automation-delete",
                        "icons/trash.svg",
                        tr!("automations.delete"),
                        theme.danger,
                        Box::new(move |_this, window, cx| {
                            let answer = window.prompt(
                                gpui::PromptLevel::Warning,
                                &tr!("automations.confirm_delete", name = delete_name.clone()),
                                Some(&tr!("automations.confirm_delete_detail")),
                                &[
                                    gpui::PromptButton::cancel(tr!("common.cancel")),
                                    gpui::PromptButton::ok(tr!("common.delete")),
                                ],
                                cx,
                            );
                            cx.spawn(async move |this, cx| {
                                if answer.await.ok() != Some(1) {
                                    return;
                                }
                                let _ = this.update(cx, |this, cx| {
                                    this.remove_automation(key, id, cx);
                                });
                            })
                            .detach();
                        }),
                        cx,
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .pb(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(field(
                        tr!("automations.field_schedule"),
                        trigger_label(&automation),
                    ))
                    .when_some(webhook_url, |element, url| {
                        element.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .w(px(96.0))
                                        .flex_none()
                                        .text_size(sp(12.0))
                                        .text_color(theme.text_tertiary)
                                        .child(tr!("automations.field_webhook")),
                                )
                                .child(self.webhook_url_chip(id, url, theme, cx)),
                        )
                    })
                    .child(field(
                        tr!("automations.field_next_run"),
                        automation
                            .next_run_at
                            .filter(|_| automation.enabled)
                            .map(|at| relative_time_until(at, now))
                            .unwrap_or_else(|| "—".to_owned()),
                    ))
                    .child(field(
                        tr!("automations.field_last_run"),
                        automation
                            .last_run_at
                            .map(|at| {
                                format!("{status_label} · {}", sidebar::format_time_ago(now - at))
                            })
                            .unwrap_or(status_label),
                    ))
                    .child(field(
                        tr!("automations.field_agent"),
                        format!(
                            "{}{}",
                            automation.provider.display_name(),
                            automation
                                .model
                                .as_deref()
                                .map(|model| format!(" · {model}"))
                                .unwrap_or_default()
                        ),
                    ))
                    .child(field(
                        tr!("automations.field_project"),
                        automation.project_path.display().to_string(),
                    ))
                    .child(field(
                        tr!("automations.field_workspace"),
                        workspace_label(&automation, self),
                    ))
                    .child(field(tr!("automations.field_host"), self.daemon_label(key)))
                    .when_some(automation.precheck.as_ref(), |element, precheck| {
                        element.child(field(
                            tr!("automations.field_precheck"),
                            format!("{} ({}s)", precheck.command, precheck.timeout_seconds),
                        ))
                    })
                    .when_some(automation.missed_run_grace_minutes, |element, grace| {
                        element.child(field(
                            tr!("automations.field_grace"),
                            tr!("automations.grace_value", count = grace),
                        ))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .mb(px(10.0))
                    .p(px(12.0))
                    .rounded(px(8.0))
                    .bg(theme.raised)
                    .text_size(sp(12.5))
                    .line_height(sp(18.0))
                    .text_color(theme.text_secondary)
                    .child(automation.prompt.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .pb(px(6.0))
                    .text_size(sp(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_tertiary)
                    .child(tr!("automations.runs_heading")),
            )
            .child(
                div()
                    .id("automation-detail-runs")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(run_rows),
            )
            .into_any_element()
    }

    /// The deferred editor modal; `None` keeps it out of the dispatch tree.
    pub(super) fn render_automation_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(request) = self.automations_editor_request.take() {
            self.materialize_automation_editor(request, window, cx);
        }
        let editor = self.automations_editor.as_ref()?;
        let theme = Theme::current(cx);
        let editing = editor.id.is_some();

        let field_label = |label: String| {
            div()
                .w(px(88.0))
                .flex_none()
                .pt(px(4.0))
                .text_size(sp(12.0))
                .text_color(theme.text_tertiary)
                .child(label)
        };
        let input_shell = |input: &Entity<TextInput>| {
            div()
                .flex_1()
                .min_w_0()
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(7.0))
                .bg(theme.composer)
                .border_1()
                .border_color(theme.separator)
                .text_size(sp(13.0))
                .text_color(theme.text)
                .child(input.clone())
        };

        let editor_weak = cx.entity().downgrade();
        let preset_chip = |preset: SchedulePreset, label: String, id: &'static str| {
            let weak = editor_weak.clone();
            let active = editor.preset == preset;
            div()
                .id(id)
                .h(px(24.0))
                .px(px(9.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .text_size(sp(12.0))
                .text_color(if active {
                    theme.text
                } else {
                    theme.text_tertiary
                })
                .when(active, |element| element.bg(theme.overlay))
                .cursor_pointer()
                .hover(|element| element.bg(theme.overlay))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if let Some(editor) = this.automations_editor.as_mut() {
                            editor.preset = preset;
                            cx.notify();
                        }
                    });
                })
                .child(label)
        };

        let preset_segment = div()
            .h(px(26.0))
            .px(px(3.0))
            .rounded(px(7.0))
            .bg(theme.sidebar_item_background)
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(preset_chip(
                SchedulePreset::Hourly,
                tr!("automations.preset_hourly"),
                "preset-hourly",
            ))
            .child(preset_chip(
                SchedulePreset::Daily,
                tr!("automations.preset_daily"),
                "preset-daily",
            ))
            .child(preset_chip(
                SchedulePreset::Weekdays,
                tr!("automations.preset_weekdays"),
                "preset-weekdays",
            ))
            .child(preset_chip(
                SchedulePreset::Weekly,
                tr!("automations.preset_weekly"),
                "preset-weekly",
            ))
            .child(preset_chip(
                SchedulePreset::Cron,
                tr!("automations.preset_cron"),
                "preset-cron",
            ))
            .child(preset_chip(
                SchedulePreset::Webhook,
                tr!("automations.preset_webhook"),
                "preset-webhook",
            ));
        let mut schedule_controls = div().flex().items_center().flex_wrap().gap(px(6.0));
        schedule_controls = match editor.preset {
            SchedulePreset::Cron => {
                schedule_controls.child(div().w(px(150.0)).child(input_shell(&editor.cron)))
            }
            SchedulePreset::Webhook => {
                let found = editor.id.and_then(|id| {
                    self.automation(editor.daemon, id).and_then(|automation| {
                        self.automation_webhook_url(editor.daemon, automation)
                            .map(|url| (automation.id, url))
                    })
                });
                match found {
                    Some((id, url)) => {
                        schedule_controls.child(self.webhook_url_chip(id, url, &theme, cx))
                    }
                    None => schedule_controls.child(
                        div()
                            .text_size(sp(12.0))
                            .text_color(theme.text_tertiary)
                            .child(tr!("automations.webhook_hint")),
                    ),
                }
            }
            _ => schedule_controls.child(div().w(px(70.0)).child(input_shell(&editor.time))),
        };
        if editor.preset == SchedulePreset::Weekly {
            let handle = self.menu_handle("automation-editor-weekday", cx);
            let day_of_week = editor.day_of_week;
            let current = weekday_label(day_of_week);
            let weak = cx.entity().downgrade();
            schedule_controls = schedule_controls.child(dropdown_menu(
                MenuChip::new("automation-editor-weekday")
                    .label(current)
                    .outlined()
                    .background(theme.raised)
                    .selected(handle.is_open()),
                "automation-editor-weekday-menu",
                &handle,
                MenuAlign::BelowRight,
                move |_| {
                    (0..7)
                        .map(|day| {
                            let weak = weak.clone();
                            MenuItem::new(weekday_label(day), move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    if let Some(editor) = this.automations_editor.as_mut() {
                                        editor.day_of_week = day;
                                        cx.notify();
                                    }
                                });
                            })
                            .selected(day_of_week == day)
                        })
                        .collect()
                },
            ));
        }
        if editor.preset != SchedulePreset::Webhook {
            schedule_controls =
                schedule_controls.child(div().w(px(130.0)).child(input_shell(&editor.timezone)));
        }
        let schedule_row = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(preset_segment)
            .child(schedule_controls);

        // Provider picker.
        let provider_handle = self.menu_handle("automation-editor-provider", cx);
        let provider = editor.provider;
        let weak = cx.entity().downgrade();
        let provider_menu = dropdown_menu(
            MenuChip::new("automation-editor-provider")
                .label(provider.display_name())
                .outlined()
                .background(theme.raised)
                .selected(provider_handle.is_open()),
            "automation-editor-provider-menu",
            &provider_handle,
            MenuAlign::BelowRight,
            move |_| {
                ProviderKind::ALL
                    .iter()
                    .map(|kind| {
                        let weak = weak.clone();
                        MenuItem::new(kind.display_name(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(editor) = this.automations_editor.as_mut() {
                                    if editor.provider != *kind {
                                        editor.provider = *kind;
                                        // Model ids are provider-scoped — a
                                        // stale pick would fail at dispatch.
                                        editor
                                            .model
                                            .update(cx, |input, cx| input.set_content("", cx));
                                    }
                                    cx.notify();
                                }
                            });
                        })
                        .selected(*kind == provider)
                    })
                    .collect()
            },
        );

        // Project picker: the daemon's projects by display path.
        let project_handle = self.menu_handle("automation-editor-project", cx);
        let project_label = editor
            .project_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| tr!("automations.project_pick"));
        let weak = cx.entity().downgrade();
        let projects: Vec<PathBuf> = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless())
            .filter(|project| self.daemons.project_owner(project.id) == editor.daemon)
            .map(|project| project.path.clone())
            .collect();
        let current_path = editor.project_path.clone();
        let project_menu = dropdown_menu(
            MenuChip::new("automation-editor-project")
                .label(project_label)
                .outlined()
                .background(theme.raised)
                .selected(project_handle.is_open()),
            "automation-editor-project-menu",
            &project_handle,
            MenuAlign::BelowRight,
            move |_| {
                projects
                    .iter()
                    .map(|path| {
                        let weak = weak.clone();
                        let selected = current_path.as_ref() == Some(path);
                        let path = path.clone();
                        MenuItem::new(path.display().to_string(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(editor) = this.automations_editor.as_mut() {
                                    editor.project_path = Some(path.clone());
                                    cx.notify();
                                }
                            });
                        })
                        .selected(selected)
                    })
                    .collect()
            },
        );

        // Workspace mode picker.
        let workspace_handle = self.menu_handle("automation-editor-workspace", cx);
        let workspace = editor.workspace;
        let weak = cx.entity().downgrade();
        let workspace_menu = dropdown_menu(
            MenuChip::new("automation-editor-workspace")
                .label(match workspace {
                    AutomationWorkspace::Local => tr!("automations.workspace_local"),
                    AutomationWorkspace::Worktree => tr!("automations.workspace_worktree"),
                    AutomationWorkspace::Existing => tr!("automations.workspace_existing"),
                })
                .outlined()
                .background(theme.raised)
                .selected(workspace_handle.is_open()),
            "automation-editor-workspace-menu",
            &workspace_handle,
            MenuAlign::BelowRight,
            move |_| {
                [
                    (
                        AutomationWorkspace::Local,
                        tr!("automations.workspace_local"),
                    ),
                    (
                        AutomationWorkspace::Worktree,
                        tr!("automations.workspace_worktree"),
                    ),
                    (
                        AutomationWorkspace::Existing,
                        tr!("automations.workspace_existing"),
                    ),
                ]
                .into_iter()
                .map(|(mode, label)| {
                    let weak = weak.clone();
                    MenuItem::new(label, move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if let Some(editor) = this.automations_editor.as_mut() {
                                editor.workspace = mode;
                                cx.notify();
                            }
                        });
                    })
                    .selected(mode == workspace)
                })
                .collect()
            },
        );

        // Model picker: the composer's own panel, retargeted at the editor's
        // provider/model pair — one row per model, no effort or tier.
        let model_handle = {
            let weak = cx.entity().downgrade();
            let search = self.model_search.clone();
            let search_focus = self.model_search.read(cx).focus_handle(cx);
            let empty_focus = self.model_picker_empty_focus.clone();
            self.menu_handle_with(
                AUTOMATION_MODEL_PICKER_MENU_ID,
                cx,
                move |open, window, cx| {
                    let mut empty = false;
                    let _ = weak.update(cx, |this, cx| {
                        if open {
                            this.model_picker_target =
                                composer::ModelPickerTarget::AutomationEditor;
                            empty = this.model_picker_has_no_providers();
                            for kind in ProviderKind::ALL {
                                if composer::picker_lists_provider(
                                    &this.probes,
                                    &this.state.disabled_providers,
                                    None,
                                    this.daemon.is_remote(),
                                    kind,
                                ) {
                                    this.refresh_provider_model_discovery(kind);
                                }
                            }
                            this.model_picker_highlight = None;
                            search.update(cx, |search, cx| search.clear(cx));
                            this.reveal_selected_picker_model(cx);
                        } else {
                            this.model_picker_target = composer::ModelPickerTarget::Composer;
                            if let Some(editor) = this.automations_editor.as_ref() {
                                let focus = editor.name.read(cx).focus_handle(cx);
                                window.focus(&focus, cx);
                            }
                        }
                        cx.notify();
                    });
                    if open {
                        // Same two-frame wait the composer picker needs: the
                        // deferred panel's input only joins the dispatch tree
                        // after its first draw.
                        let picker_focus = if empty {
                            empty_focus.clone()
                        } else {
                            search_focus.clone()
                        };
                        let reveal_weak = weak.clone();
                        window.on_next_frame(move |window, _| {
                            window.on_next_frame(move |window, cx| {
                                window.focus(&picker_focus, cx);
                                let _ = reveal_weak.update(cx, |this, cx| {
                                    this.reveal_selected_picker_model(cx);
                                });
                            });
                        });
                    }
                },
            )
        };
        let model_text = editor.model.read(cx).content().trim().to_owned();
        let model_label = if model_text.is_empty() {
            tr!("automations.model_default")
        } else {
            self.model_display_name(editor.provider, Some(model_text.as_str()))
        };
        let entity = cx.entity();
        let model_menu = popover(
            MenuChip::new("automation-editor-model")
                .label(model_label)
                .outlined()
                .background(theme.raised)
                .selected(model_handle.is_open()),
            &model_handle,
            MenuAlign::BelowRight,
            move |popover, _window, cx| {
                let weak = entity.downgrade();
                entity.read_with(cx, move |this, cx| {
                    this.render_model_picker_panel(&weak, popover, cx)
                })
            },
        );

        // Target task picker for Existing mode.
        let session_menu = matches!(editor.workspace, AutomationWorkspace::Existing).then(|| {
            let handle = self.menu_handle("automation-editor-session", cx);
            let weak = cx.entity().downgrade();
            let sessions: Vec<(Uuid, String)> = self
                .state
                .sessions
                .iter()
                .filter(|session| {
                    session.has_started() && self.daemons.session_owner(session.id) == editor.daemon
                })
                .map(|session| (session.id, session.display_title().to_owned()))
                .collect();
            let current = editor.session_id;
            let label = current
                .and_then(|id| {
                    sessions
                        .iter()
                        .find(|(session_id, _)| *session_id == id)
                        .map(|(_, title)| title.clone())
                })
                .unwrap_or_else(|| tr!("automations.session_pick"));
            dropdown_menu(
                MenuChip::new("automation-editor-session")
                    .label(label)
                    .outlined()
                    .background(theme.raised)
                    .selected(handle.is_open()),
                "automation-editor-session-menu",
                &handle,
                MenuAlign::BelowRight,
                move |_| {
                    sessions
                        .iter()
                        .map(|(session_id, title)| {
                            let weak = weak.clone();
                            let selected = current == Some(*session_id);
                            let session_id = *session_id;
                            MenuItem::new(title.clone(), move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    if let Some(editor) = this.automations_editor.as_mut() {
                                        editor.session_id = Some(session_id);
                                        cx.notify();
                                    }
                                });
                            })
                            .selected(selected)
                        })
                        .collect()
                },
            )
        });

        let enabled_toggle = editor_toggle_row(
            "automation-editor-enabled",
            tr!("automations.enabled"),
            editor.enabled,
            theme,
            cx,
            |this, _, cx| {
                if let Some(editor) = this.automations_editor.as_mut() {
                    editor.enabled = !editor.enabled;
                    cx.notify();
                }
            },
        );
        let reuse_toggle = editor_toggle_row(
            "automation-editor-reuse",
            tr!("automations.reuse_session"),
            editor.reuse_session,
            theme,
            cx,
            |this, _, cx| {
                if let Some(editor) = this.automations_editor.as_mut() {
                    editor.reuse_session = !editor.reuse_session;
                    cx.notify();
                }
            },
        );

        let row = |label: String, content: AnyElement| {
            div()
                .flex()
                .items_start()
                .gap(px(10.0))
                .py(px(5.0))
                .child(field_label(label))
                .child(div().flex_1().min_w_0().child(content))
        };

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let card = div()
            .id("automation-editor-card")
            .key_context(EDITOR_CONTEXT)
            .on_action(cx.listener(Self::confirm_automation_editor_action))
            .on_action(cx.listener(Self::dismiss_automation_editor_action))
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(520.0))
            .max_h(px(600.0))
            .overflow_hidden()
            .rounded(px(21.0))
            .bg(theme.composer)
            .shadow_xl()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(48.0))
                    .px(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .text_size(sp(14.0))
                    .text_color(theme.text)
                    .child(icon("icons/queue.svg", 15.0, theme.text))
                    .child(div().child(if editing {
                        tr!("automations.edit_title")
                    } else {
                        tr!("automations.new_title")
                    })),
            )
            .child(
                div()
                    .id("automation-editor-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(16.0))
                    .pb(px(8.0))
                    .flex()
                    .flex_col()
                    .child(row(
                        tr!("automations.field_name"),
                        input_shell(&editor.name).into_any_element(),
                    ))
                    .child(row(
                        tr!("automations.field_prompt"),
                        input_shell(&editor.prompt).into_any_element(),
                    ))
                    .child(row(
                        tr!("automations.field_schedule"),
                        schedule_row.into_any_element(),
                    ))
                    .child(row(
                        tr!("automations.field_agent"),
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(provider_menu)
                            .child(model_menu)
                            .into_any_element(),
                    ))
                    .child(row(
                        tr!("automations.field_project"),
                        project_menu.into_any_element(),
                    ))
                    .child(row(
                        tr!("automations.field_workspace"),
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(workspace_menu)
                            .when(
                                matches!(editor.workspace, AutomationWorkspace::Worktree),
                                |element| {
                                    element.child(
                                        div().w(px(120.0)).child(input_shell(&editor.base_branch)),
                                    )
                                },
                            )
                            .when_some(session_menu, |element, menu| element.child(menu))
                            .into_any_element(),
                    ))
                    .child(row(
                        tr!("automations.field_enabled"),
                        enabled_toggle.into_any_element(),
                    ))
                    .child(
                        div()
                            .id("automation-editor-advanced")
                            .py(px(5.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_pointer()
                            .text_size(sp(12.0))
                            .text_color(theme.text_tertiary)
                            .track_focus(&editor.advanced_focus)
                            .tab_index(0)
                            .focus_visible(|element| element.bg(theme.focus_highlight()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(editor) = this.automations_editor.as_mut() {
                                    editor.advanced_open = !editor.advanced_open;
                                    cx.notify();
                                }
                            }))
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    if let Some(editor) = this.automations_editor.as_mut() {
                                        editor.advanced_open = !editor.advanced_open;
                                        cx.notify();
                                    }
                                    cx.stop_propagation();
                                }
                            }))
                            .child(icon(
                                if editor.advanced_open {
                                    "icons/chevron-down.svg"
                                } else {
                                    "icons/chevron-right.svg"
                                },
                                10.0,
                                theme.text_tertiary,
                            ))
                            .child(tr!("automations.advanced")),
                    )
                    .when(editor.advanced_open, |element| {
                        element
                            .child(row(
                                tr!("automations.field_precheck"),
                                input_shell(&editor.precheck).into_any_element(),
                            ))
                            .child(row(
                                tr!("automations.field_precheck_timeout"),
                                div()
                                    .w(px(80.0))
                                    .child(input_shell(&editor.precheck_timeout))
                                    .into_any_element(),
                            ))
                            .child(row(
                                tr!("automations.field_grace"),
                                div()
                                    .w(px(80.0))
                                    .child(input_shell(&editor.grace_minutes))
                                    .into_any_element(),
                            ))
                            .child(row(
                                tr!("automations.field_reuse"),
                                reuse_toggle.into_any_element(),
                            ))
                    })
                    .when_some(editor.error.clone(), |element, error| {
                        element.child(
                            div()
                                .py(px(6.0))
                                .text_size(sp(12.0))
                                .text_color(theme.danger)
                                .child(error),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(16.0))
                    .py(px(10.0))
                    .border_t_1()
                    .border_color(theme.separator)
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        div()
                            .id("automation-editor-cancel")
                            .h(px(26.0))
                            .px(px(10.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .cursor_pointer()
                            .hover(|element| element.bg(theme.overlay))
                            .track_focus(&editor.cancel_focus)
                            .tab_index(0)
                            .focus_visible(|element| element.bg(theme.focus_highlight()))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_automation_editor(window, cx);
                            }))
                            .child(tr!("automations.cancel")),
                    )
                    .child(
                        div()
                            .id("automation-editor-save")
                            .h(px(26.0))
                            .px(px(10.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .text_size(sp(12.5))
                            .text_color(theme.composer)
                            .bg(theme.accent)
                            .cursor_pointer()
                            .hover(|element| element.bg(theme.accent.opacity(0.85)))
                            .track_focus(&editor.save_focus)
                            .tab_index(0)
                            .focus_visible(|element| element.bg(theme.focus_highlight()))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_automation_editor(window, cx);
                            }))
                            .child(icon("icons/check.svg", 11.0, theme.composer))
                            .child(if editing {
                                tr!("automations.save")
                            } else {
                                tr!("automations.create")
                            }),
                    ),
            );

        let layer = div()
            .id("automation-editor-layer")
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
                cx.listener(|this, _, window, cx| this.close_automation_editor(window, cx)),
            )
            .child(motion::modal_enter("automation-editor-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("automation-editor-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }

    fn confirm_automation_editor_action(
        &mut self,
        _: &ConfirmAutomationEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_automation_editor(window, cx);
    }
}

/// Build the upsert payload for a pause/resume toggle — every field
/// round-trips except `enabled`.
fn automation_input(automation: &Automation, enabled: bool) -> AutomationInput {
    AutomationInput {
        id: Some(automation.id),
        name: automation.name.clone(),
        prompt: automation.prompt.clone(),
        provider: automation.provider,
        model: automation.model.clone(),
        project_path: automation.project_path.clone(),
        workspace: automation.workspace,
        base_branch: automation.base_branch.clone(),
        session_id: automation.session_id,
        schedule: automation.schedule.clone(),
        webhook: automation.webhook_secret.is_some(),
        timezone: automation.timezone.clone(),
        enabled,
        precheck: automation.precheck.clone(),
        missed_run_grace_minutes: automation.missed_run_grace_minutes,
        reuse_session: automation.reuse_session,
    }
}

/// "HH:MM" or ":MM" → (hour, minute). `:MM` is the Hourly form where the
/// hour is implicit.
fn parse_clock(text: &str) -> Option<(u8, u8)> {
    let text = text.trim();
    if let Some(minute) = text.strip_prefix(':') {
        let minute: u8 = minute.trim().parse().ok()?;
        return (minute < 60).then_some((0, minute));
    }
    let (hour, minute) = text.split_once(':')?;
    let hour: u8 = hour.trim().parse().ok()?;
    let minute: u8 = minute.trim().parse().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// The Schedule column's text — the cron description for scheduled
/// automations, "Webhook" for HTTP-armed ones, "No schedule" for a bare
/// paused definition.
fn trigger_label(automation: &Automation) -> String {
    automation
        .schedule
        .as_ref()
        .map(|schedule| schedule_label(schedule, &automation.timezone))
        .unwrap_or_else(|| {
            if automation.webhook_secret.is_some() {
                tr!("automations.trigger_webhook")
            } else {
                tr!("automations.schedule_none")
            }
        })
}

/// A daemon client address (`ws://host:port/v1`, `wss://`, or bare
/// `host:port`) as the `http(s)://host:port` base the daemon's plain-HTTP
/// routes share.
fn webhook_http_base(address: &str) -> String {
    let http = if let Some(rest) = address.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = address.strip_prefix("ws://") {
        format!("http://{rest}")
    } else {
        format!("http://{address}")
    };
    let http = http.trim_end_matches('/');
    http.strip_suffix("/v1").unwrap_or(http).to_owned()
}

/// Human schedule text for the list and detail — locale-free, matching
/// Orca's choice to keep schedule description unlocalized in shared code.
fn schedule_label(schedule: &AutomationSchedule, timezone: &Option<String>) -> String {
    let zone = timezone
        .as_deref()
        .filter(|zone| !zone.trim().is_empty())
        .map(|zone| format!(" · {zone}"))
        .unwrap_or_default();
    match schedule {
        AutomationSchedule::Hourly { minute } => {
            format!(
                "{}{zone}",
                tr!("automations.schedule_hourly", minute = *minute)
            )
        }
        AutomationSchedule::Daily { hour, minute } => format!(
            "{}{zone}",
            tr!(
                "automations.schedule_daily",
                time = clock_label(*hour, *minute)
            )
        ),
        AutomationSchedule::Weekdays { hour, minute } => format!(
            "{}{zone}",
            tr!(
                "automations.schedule_weekdays",
                time = clock_label(*hour, *minute)
            )
        ),
        AutomationSchedule::Weekly {
            day_of_week,
            hour,
            minute,
        } => format!(
            "{}{zone}",
            tr!(
                "automations.schedule_weekly",
                day = weekday_label(*day_of_week),
                time = clock_label(*hour, *minute)
            )
        ),
        AutomationSchedule::Cron { expression } => {
            format!(
                "{}{zone}",
                tr!("automations.schedule_cron", expression = expression)
            )
        }
    }
}

fn clock_label(hour: u8, minute: u8) -> String {
    format!("{hour:02}:{minute:02}")
}

fn weekday_label(day: u8) -> String {
    match day {
        0 => tr!("automations.weekday_sun"),
        1 => tr!("automations.weekday_mon"),
        2 => tr!("automations.weekday_tue"),
        3 => tr!("automations.weekday_wed"),
        4 => tr!("automations.weekday_thu"),
        5 => tr!("automations.weekday_fri"),
        _ => tr!("automations.weekday_sat"),
    }
}

/// "in 3h" / "tomorrow" style countdown for next-run cells.
fn relative_time_until(at: u64, now: u64) -> String {
    let ahead = at.saturating_sub(now);
    match ahead {
        0..=59 => tr!("automations.soon"),
        60..=3_599 => tr!("automations.in_minutes", count = ahead / 60),
        3_600..=86_399 => tr!("automations.in_hours", count = ahead / 3_600),
        _ => tr!("automations.in_days", count = ahead / 86_400),
    }
}

/// Icon + color + label for one run status.
fn run_status_badge(theme: &Theme, status: AutomationRunStatus) -> (&'static str, Hsla, String) {
    match status {
        AutomationRunStatus::Pending => (
            "icons/queue.svg",
            theme.text_tertiary,
            tr!("automations.status_pending"),
        ),
        AutomationRunStatus::Running => (
            "icons/arrow-up.svg",
            theme.accent,
            tr!("automations.status_running"),
        ),
        AutomationRunStatus::Completed => (
            "icons/check.svg",
            theme.success,
            tr!("automations.status_completed"),
        ),
        AutomationRunStatus::Failed => (
            "icons/x-bold.svg",
            theme.danger,
            tr!("automations.status_failed"),
        ),
        AutomationRunStatus::SkippedPrecheck => (
            "icons/loader-circle.svg",
            theme.warning,
            tr!("automations.status_precheck"),
        ),
        AutomationRunStatus::SkippedMissed => (
            "icons/queue.svg",
            theme.warning,
            tr!("automations.status_missed"),
        ),
        AutomationRunStatus::SkippedUnavailable => (
            "icons/loader-circle.svg",
            theme.warning,
            tr!("automations.status_unavailable"),
        ),
    }
}

/// Where the automation runs, for the detail grid.
fn workspace_label(automation: &Automation, waku: &Waku) -> String {
    match automation.workspace {
        AutomationWorkspace::Local => tr!("automations.workspace_local"),
        AutomationWorkspace::Worktree => format!(
            "{} · {}",
            tr!("automations.workspace_worktree"),
            automation.base_branch.as_deref().unwrap_or("main")
        ),
        AutomationWorkspace::Existing => {
            let title = automation
                .session_id
                .and_then(|id| waku.state.sessions.iter().find(|session| session.id == id))
                .map(|session| session.display_title().to_owned())
                .unwrap_or_else(|| tr!("automations.session_missing"));
            format!("{} · {title}", tr!("automations.workspace_existing"))
        }
    }
}

/// Search match over a definition's visible text.
fn automation_matches(automation: &Automation, needle: &str) -> bool {
    needle.is_empty()
        || automation.name.to_lowercase().contains(needle)
        || automation.prompt.to_lowercase().contains(needle)
        || automation
            .project_path
            .display()
            .to_string()
            .to_lowercase()
            .contains(needle)
}

/// Empty/loading state — same shape as the Drafts page's status row.
fn automations_status_row(theme: &Theme, title: String, hint: String) -> Div {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .text_size(sp(14.0))
                .text_color(theme.text)
                .child(title),
        )
        .child(
            div()
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary)
                .child(hint),
        )
}

/// The small checkbox-style toggle the editor uses for enabled/reuse. The
/// shared pill switch is the tab stop and carries activation; the label row
/// stays click-to-toggle for pointer users.
fn editor_toggle_row<E>(
    id: &'static str,
    label: String,
    enabled: bool,
    theme: Theme,
    cx: &mut Context<E>,
    activate: impl Fn(&mut E, &mut Window, &mut Context<E>) + 'static + Clone,
) -> Stateful<Div>
where
    E: 'static,
{
    let row_activate = activate.clone();
    div()
        .id(id)
        .h(px(24.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, window, cx| {
            row_activate(this, window, cx);
        }))
        .child(toggle_switch(
            ElementId::Name(format!("{id}-switch").into()),
            enabled,
            false,
            theme,
            cx,
            activate,
        ))
        .child(
            div()
                .text_size(sp(12.5))
                .text_color(theme.text)
                .child(label),
        )
}
