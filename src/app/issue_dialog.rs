//! Modal new-GitHub-issue flow opened from the command palette.
//!
//! The template scan runs in the palette; this dialog takes over once a
//! template (or "blank") is picked. `gh issue create` runs through a
//! workspace operation on the background executor — the card only paints
//! the fields and the reply.

use gpui::{KeyBinding, actions};
use waku_protocol::workspace::GitHubRepoRef;

use super::*;
use crate::OpenCreatedIssueInGitHub;

actions!(
    waku_issue_dialog,
    [ConfirmIssueDialog, DismissIssueDialog]
);

const DIALOG_CONTEXT: &str = "IssueDialog";
const DIALOG_INPUT_CONTEXT: &str = "IssueDialog > TextInput";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(
            "secondary-enter",
            ConfirmIssueDialog,
            Some(DIALOG_INPUT_CONTEXT),
        ),
        KeyBinding::new("secondary-enter", ConfirmIssueDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissIssueDialog, Some(DIALOG_CONTEXT)),
    ]);
}

/// The repository a new issue lands in: `cwd` is where `gh` runs;
/// `project` is the app project the in-app GitHub browser keys on when the
/// checkout maps to one — the toast's "View" needs it to deep-link.
#[derive(Clone, Debug)]
pub(super) struct IssueTarget {
    pub cwd: PathBuf,
    pub project: Option<Uuid>,
}

/// The issue the last `gh issue create` landed — what ⌘⌥I opens.
#[derive(Clone, Debug)]
pub(crate) struct CreatedIssue {
    pub project: Option<Uuid>,
    pub number: Option<u64>,
    pub url: String,
}

pub(super) struct IssueDialogState {
    id: Uuid,
    target: IssueTarget,
    repo_label: String,
    title: Entity<TextInput>,
    body: Entity<TextInput>,
    labels: Entity<TextInput>,
    assignees: Entity<TextInput>,
    milestone: Entity<TextInput>,
    submitting: bool,
    error: Option<String>,
    create_focus: FocusHandle,
}

/// `bug, help wanted` — the comma-separated field values `gh` takes one
/// flag at a time.
fn split_field_list(content: &str) -> Vec<String> {
    content
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Waku {
    pub(super) fn open_issue_dialog(
        &mut self,
        target: IssueTarget,
        repo: Option<GitHubRepoRef>,
        template: Option<waku_protocol::workspace::IssueTemplate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repo_label = repo
            .map(|repo| format!("{}/{}", repo.owner, repo.name))
            .unwrap_or_else(|| {
                settings::abbreviate_home_path(&target.cwd, self.home_directory.as_deref())
            });
        let field = |placeholder: String,
                     label: String,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                TextInput::new(window, cx)
                    .accessibility_label(label)
                    .placeholder(placeholder)
            })
        };
        let title = field(
            tr!("issue.title_placeholder"),
            tr!("issue.title"),
            window,
            cx,
        );
        let body = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("issue.body"))
                .placeholder(tr!("issue.body_placeholder"))
                .multi_line()
                .auto_height()
                .max_lines(12)
        });
        let labels = field(
            tr!("issue.labels_placeholder"),
            tr!("issue.labels"),
            window,
            cx,
        );
        let assignees = field(
            tr!("issue.assignees_placeholder"),
            tr!("issue.assignees"),
            window,
            cx,
        );
        let milestone = field(
            tr!("issue.milestone_placeholder"),
            tr!("issue.milestone"),
            window,
            cx,
        );
        if let Some(template) = &template {
            if let Some(prefix) = template.title_prefix.clone() {
                // Frontmatter `title` is a prefix like "[BUG] " — land the
                // caret past it, restoring the space YAML parsing trimmed.
                let prefix = if prefix.ends_with(char::is_whitespace) {
                    prefix
                } else {
                    format!("{prefix} ")
                };
                title.update(cx, |input, cx| input.set_content(prefix, cx));
            }
            if !template.body.is_empty() {
                body.update(cx, |input, cx| {
                    input.set_content(template.body.clone(), cx)
                });
            }
            if !template.labels.is_empty() {
                labels.update(cx, |input, cx| {
                    input.set_content(template.labels.join(", "), cx)
                });
            }
            if !template.assignees.is_empty() {
                assignees.update(cx, |input, cx| {
                    input.set_content(template.assignees.join(", "), cx)
                });
            }
        }
        let title_focus = title.read(cx).focus();
        self.issue_dialog = Some(IssueDialogState {
            id: Uuid::new_v4(),
            target,
            repo_label,
            title,
            body,
            labels,
            assignees,
            milestone,
            submitting: false,
            error: None,
            create_focus: cx.focus_handle(),
        });
        // Like the commit dialog, the modal joins the dispatch tree only
        // after it has drawn — focus two frames later.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&title_focus, cx));
        });
        cx.notify();
    }

    fn close_issue_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.issue_dialog.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Read the fields back into one `gh` flag list. Whitespace-only
    /// labels, assignees, and milestone drop out entirely rather than
    /// handing `gh` an empty value.
    fn issue_dialog_input(&self, cx: &App) -> waku_protocol::workspace::CreateIssueInput {
        let Some(dialog) = self.issue_dialog.as_ref() else {
            return waku_protocol::workspace::CreateIssueInput {
                title: String::new(),
                body: String::new(),
                labels: Vec::new(),
                assignees: Vec::new(),
                milestone: None,
            };
        };
        let milestone = dialog.milestone.read(cx).content().trim();
        waku_protocol::workspace::CreateIssueInput {
            title: dialog.title.read(cx).content().trim().to_owned(),
            body: dialog.body.read(cx).content().trim().to_owned(),
            labels: split_field_list(dialog.labels.read(cx).content()),
            assignees: split_field_list(dialog.assignees.read(cx).content()),
            milestone: (!milestone.is_empty()).then(|| milestone.to_owned()),
        }
    }

    fn set_issue_dialog_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        let Some(dialog) = self.issue_dialog.as_ref() else {
            return;
        };
        for input in [
            &dialog.title,
            &dialog.body,
            &dialog.labels,
            &dialog.assignees,
            &dialog.milestone,
        ] {
            input.update(cx, |input, _| input.set_read_only(read_only));
        }
    }

    fn request_issue_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.issue_dialog.as_ref() else {
            return;
        };
        if dialog.submitting {
            return;
        }
        let input = self.issue_dialog_input(cx);
        if input.title.is_empty() {
            if let Some(dialog) = self.issue_dialog.as_mut() {
                dialog.error = Some(tr!("issue.title_required"));
            }
            cx.notify();
            return;
        }
        let id = dialog.id;
        let cwd = dialog.target.cwd.clone();
        let project = dialog.target.project;
        // `gh` must run in the checkout's own daemon — an offline remote
        // owner gets the disconnected wording, never a local fallback that
        // would file the issue against a different filesystem.
        let Some(workspace_client) = self.workspace_client_for_path(&cwd) else {
            if let Some(dialog) = self.issue_dialog.as_mut() {
                dialog.error = Some(tr!("errors.daemon_disconnected"));
            }
            cx.notify();
            return;
        };
        if let Some(dialog) = self.issue_dialog.as_mut() {
            dialog.submitting = true;
            dialog.error = None;
        }
        self.set_issue_dialog_read_only(true, cx);
        cx.notify();

        let window_handle = window.window_handle();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace_client.request(
                        waku_client::WorkspaceOperation::CreateIssue { cwd, input },
                    ) {
                        Ok(waku_client::WorkspaceResult::IssueCreated { number, url }) => {
                            Ok((number, url))
                        }
                        Ok(_) => {
                            Err("the daemon returned an invalid issue response".to_owned())
                        }
                        Err(error) => Err(error.to_string()),
                    }
                })
                .await;
            let focus = waku.update(cx, |waku, cx| {
                match result {
                    Ok((number, url)) => {
                        // The reply stands even when Esc retired the dialog
                        // mid-flight — the issue exists either way.
                        let created = CreatedIssue {
                            project,
                            number,
                            url,
                        };
                        waku.show_issue_created_toast(&created);
                        waku.last_created_issue = Some(created);
                        let dialog_was_open = waku
                            .issue_dialog
                            .as_ref()
                            .is_some_and(|dialog| dialog.id == id);
                        if dialog_was_open {
                            waku.issue_dialog = None;
                        }
                        dialog_was_open.then(|| waku.composer_focus(cx))
                    }
                    Err(error) => {
                        if let Some(dialog) =
                            waku.issue_dialog.as_mut().filter(|dialog| dialog.id == id)
                        {
                            dialog.submitting = false;
                            dialog.error = Some(error);
                            waku.set_issue_dialog_read_only(false, cx);
                        } else {
                            waku.show_toast(error);
                        }
                        None
                    }
                }
            });
            if let Ok(Some(focus)) = focus {
                let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
            }
            let _ = waku.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Deep-link the created issue in the Projects page's GitHub browser —
    /// the toast's "View" and ⌘⌥I land here. Without a project or a
    /// parsed number there is nothing to deep-link; open the URL.
    pub(super) fn open_created_issue(
        &mut self,
        project: Option<Uuid>,
        number: Option<u64>,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_toast();
        if let (Some(project_id), Some(number)) = (project, number)
            && self.open_github_issue(project_id, number, window, cx)
        {
            return;
        }
        cx.open_url(url);
    }

    pub(super) fn open_created_issue_in_github_action(
        &mut self,
        _: &OpenCreatedIssueInGitHub,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(created) = self.last_created_issue.clone() else {
            return;
        };
        self.open_created_issue(
            created.project,
            created.number,
            &created.url,
            window,
            cx,
        );
    }

    pub(super) fn render_issue_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.issue_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let submitting = dialog.submitting;
        let error = dialog.error.clone();
        let weak = cx.entity().downgrade();

        let field_row = |id: &'static str,
                         caption: String,
                         input: &Entity<TextInput>|
         -> Stateful<Div> {
            div()
                .id(id)
                .w_full()
                .px(px(16.0))
                .py(px(7.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_none()
                        .w(px(74.0))
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(caption),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_size(sp(13.5))
                        .text_color(theme.text)
                        .child(input.clone()),
                )
        };

        let create_active = submitting;
        let create = {
            let foreground = if create_active {
                theme.text_secondary
            } else {
                theme.text
            };
            let indicator = if create_active {
                motion::spin(icon("icons/loader-circle.svg", 15.0, theme.text_secondary))
            } else {
                icon("icons/github.svg", 15.0, foreground).into_any_element()
            };
            let click_weak = weak.clone();
            let key_weak = weak;
            div()
                .id("issue-dialog-create")
                .track_focus(&dialog.create_focus)
                .when(!submitting, |row| row.tab_index(0))
                .h(px(38.0))
                .w_full()
                .px(px(10.0))
                .rounded(px(11.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .cursor_default()
                .text_size(sp(14.0))
                .text_color(foreground)
                .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                .when(!submitting, |row| {
                    row.hover(|style| style.bg(theme.overlay_strong))
                })
                .child(indicator)
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .child(if create_active {
                            tr!("issue.creating")
                        } else {
                            tr!("issue.create")
                        }),
                )
                .child(
                    div()
                        .h(px(22.0))
                        .min_w(px(34.0))
                        .px(px(7.0))
                        .rounded(px(13.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.overlay_strong)
                        .text_size(sp(12.5))
                        .text_color(if create_active {
                            theme.text_ghost
                        } else {
                            theme.text_secondary
                        })
                        .child(crate::platform::primary_shortcut("⌘↩", "Ctrl+Enter")),
                )
                .when(!submitting, |row| {
                    row.on_click(move |_, window, cx| {
                        let _ = click_weak.update(cx, |waku, cx| {
                            waku.request_issue_create(window, cx)
                        });
                    })
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            let _ = key_weak.update(cx, |waku, cx| {
                                waku.request_issue_create(window, cx)
                            });
                            cx.stop_propagation();
                        }
                    })
                })
        };

        let card = div()
            .id("issue-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmIssueDialog, window, cx| {
                waku.request_issue_create(window, cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissIssueDialog, window, cx| {
                waku.close_issue_dialog(window, cx)
            }))
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(480.0))
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
                    .child(icon("icons/github.svg", 15.0, theme.text))
                    .child(div().flex_none().child(tr!("issue.new")))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_color(theme.text_tertiary)
                            .child(dialog.repo_label.clone()),
                    ),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.border))
            .child(
                div()
                    .w_full()
                    .px(px(16.0))
                    .py(px(10.0))
                    .text_size(sp(14.0))
                    .text_color(theme.text)
                    .child(dialog.title.clone()),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.border))
            .child(
                div()
                    .w_full()
                    .min_h(px(96.0))
                    .px(px(16.0))
                    .py(px(10.0))
                    .text_size(sp(13.5))
                    .line_height(sp(20.0))
                    .text_color(theme.text)
                    .child(dialog.body.clone()),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.border))
            .child(field_row(
                "issue-dialog-labels",
                tr!("issue.labels"),
                &dialog.labels,
            ))
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.border))
            .child(field_row(
                "issue-dialog-assignees",
                tr!("issue.assignees"),
                &dialog.assignees,
            ))
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.border))
            .child(field_row(
                "issue-dialog-milestone",
                tr!("issue.milestone"),
                &dialog.milestone,
            ))
            .when_some(error, |card, error| {
                card.child(
                    div()
                        .px(px(20.0))
                        .pb(px(10.0))
                        .text_size(sp(12.5))
                        .line_height(sp(16.0))
                        .text_color(theme.danger)
                        .child(error),
                )
            })
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.border))
            .child(div().p(px(8.0)).child(create));

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("issue-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.close_issue_dialog(window, cx)),
            )
            .child(motion::modal_enter("issue-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("issue-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}
