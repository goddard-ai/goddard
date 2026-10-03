//! Read-only GitHub releases and workflow runs for one Projects page.
//! All `gh` work runs through the daemon on a background executor.

use std::rc::Rc;
use std::time::Duration;

use super::*;
use crate::ui::ActivationExt;
use waku_client::{GitHubRelease, GitHubWorkflowRun};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ActivitySelection {
    Release(String),
    Run(u64),
}

#[derive(Clone)]
pub(super) enum ActivityDetail {
    Release(GitHubRelease),
    Run(GitHubWorkflowRun),
}

pub(super) struct ActivityState {
    pub releases: github::GitHubFetch<Rc<Vec<GitHubRelease>>>,
    pub runs: github::GitHubFetch<Rc<Vec<GitHubWorkflowRun>>>,
    pub selected: Option<ActivitySelection>,
    pub detail: github::GitHubFetch<Rc<ActivityDetail>>,
    pub scroll: ScrollHandle,
    generation: u64,
    detail_generation: u64,
}

impl Default for ActivityState {
    fn default() -> Self {
        Self {
            releases: github::GitHubFetch::Loading,
            runs: github::GitHubFetch::Loading,
            selected: None,
            detail: github::GitHubFetch::Loading,
            scroll: ScrollHandle::new(),
            generation: 0,
            detail_generation: 0,
        }
    }
}

impl Waku {
    pub(super) fn activity_refresh(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| p.path.clone())
        else {
            return;
        };
        let Some(workspace) = self.workspace_client_for_project(project_id) else {
            return;
        };
        let Some(state) = self.projects_page_states.get_mut(&project_id) else {
            return;
        };
        state.activity.generation = state.activity.generation.wrapping_add(1);
        let generation = state.activity.generation;
        if !matches!(
            state.activity.releases,
            github::GitHubFetch::Loaded(Some(_))
        ) {
            state.activity.releases = github::GitHubFetch::Loading;
        }
        if !matches!(state.activity.runs, github::GitHubFetch::Loaded(Some(_))) {
            state.activity.runs = github::GitHubFetch::Loading;
        }
        cx.notify();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    workspace.request(waku_client::WorkspaceOperation::ListGitHubActivity { cwd })
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let Some(state) = waku.projects_page_states.get_mut(&project_id) else {
                    return;
                };
                if state.activity.generation != generation {
                    return;
                }
                let (releases, runs) = match result {
                    Ok(waku_client::WorkspaceResult::GitHubActivity { releases, runs }) => {
                        (releases, runs)
                    }
                    _ => (None, None),
                };
                let has_active_run = runs
                    .as_ref()
                    .is_some_and(|runs| runs.iter().any(|run| run.status != "completed"));
                state.activity.releases = github::GitHubFetch::Loaded(releases.map(Rc::new));
                state.activity.runs = github::GitHubFetch::Loaded(runs.map(Rc::new));
                if has_active_run && waku.projects_page == Some(project_id) {
                    cx.spawn(async move |waku, cx| {
                        cx.background_executor()
                            .timer(Duration::from_secs(20))
                            .await;
                        let _ = waku.update(cx, |waku, cx| {
                            if waku.projects_page != Some(project_id) {
                                return;
                            }
                            let Some(state) = waku.projects_page_states.get(&project_id) else {
                                return;
                            };
                            if state.tab != projects::ProjectsTab::Activity
                                || state.activity.generation != generation
                            {
                                return;
                            }
                            let selected_run = match &state.activity.selected {
                                Some(ActivitySelection::Run(id)) => Some(*id),
                                _ => None,
                            };
                            waku.activity_refresh(project_id, cx);
                            if let Some(id) = selected_run {
                                waku.activity_select(project_id, ActivitySelection::Run(id), cx);
                            }
                        });
                    })
                    .detach();
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn activity_select(
        &mut self,
        project_id: Uuid,
        selection: ActivitySelection,
        cx: &mut Context<Self>,
    ) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| p.path.clone())
        else {
            return;
        };
        let Some(workspace) = self.workspace_client_for_project(project_id) else {
            return;
        };
        let Some(state) = self.projects_page_states.get_mut(&project_id) else {
            return;
        };
        state.activity.selected = Some(selection.clone());
        state.activity.detail = github::GitHubFetch::Loading;
        state.activity.scroll.set_offset(gpui::Point::default());
        state.activity.detail_generation = state.activity.detail_generation.wrapping_add(1);
        let generation = state.activity.detail_generation;
        cx.notify();
        cx.spawn(async move |waku, cx| {
            let requested = selection.clone();
            let detail = cx
                .background_executor()
                .spawn(async move {
                    match selection {
                        ActivitySelection::Release(tag) => workspace
                            .request(waku_client::WorkspaceOperation::GetGitHubRelease { cwd, tag })
                            .ok()
                            .and_then(|result| match result {
                                waku_client::WorkspaceResult::GitHubRelease { detail } => {
                                    detail.map(ActivityDetail::Release)
                                }
                                _ => None,
                            }),
                        ActivitySelection::Run(run_id) => workspace
                            .request(waku_client::WorkspaceOperation::GetGitHubWorkflowRun {
                                cwd,
                                run_id,
                            })
                            .ok()
                            .and_then(|result| match result {
                                waku_client::WorkspaceResult::GitHubWorkflowRun { detail } => {
                                    detail.map(ActivityDetail::Run)
                                }
                                _ => None,
                            }),
                    }
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let Some(state) = waku.projects_page_states.get_mut(&project_id) else {
                    return;
                };
                if state.activity.detail_generation != generation
                    || state.activity.selected.as_ref() != Some(&requested)
                {
                    return;
                }
                state.activity.detail = github::GitHubFetch::Loaded(detail.map(Rc::new));
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_activity_run(
        &mut self,
        project_id: Uuid,
        run_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_projects_tab(project_id, projects::ProjectsTab::Activity, window, cx);
        self.activity_select(project_id, ActivitySelection::Run(run_id), cx);
    }

    pub(super) fn render_activity(
        &mut self,
        project_id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        if let Some((None, availability)) = self
            .github_browsers
            .get(&project_id)
            .and_then(|browser| browser.repo.as_ref())
        {
            let message = match availability {
                waku_client::GitHubAvailability::MissingCli => tr!("github.install_gh"),
                waku_client::GitHubAvailability::Unauthenticated => tr!("github.auth_gh"),
                waku_client::GitHubAvailability::Ready => tr!("github.not_a_repo"),
                waku_client::GitHubAvailability::Unavailable => tr!("github.unavailable"),
            };
            return github::github_centered(
                icon("icons/github.svg", 16.0, theme.text_tertiary).into_any_element(),
                message,
                &theme,
            );
        }
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        let releases = state.activity.releases.clone();
        let runs = state.activity.runs.clone();
        let selected = state.activity.selected.clone();
        let detail = state.activity.detail.clone();
        let scroll = state.activity.scroll.clone();
        let filter = state
            .filter_text(projects::ProjectsTab::Activity, cx)
            .to_lowercase();

        let mut content = div()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .px(px(12.0))
            .py(px(14.0));
        if selected.is_some() {
            let details = match detail {
                github::GitHubFetch::Loading => {
                    activity_note(tr!("activity.loading"), &theme).into_any_element()
                }
                github::GitHubFetch::Loaded(None) => {
                    activity_note(tr!("activity.unavailable"), &theme).into_any_element()
                }
                github::GitHubFetch::Loaded(Some(detail)) => activity_detail(&detail, &theme),
            };
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(activity_heading(tr!("activity.details"), &theme))
                    .child(details),
            );
        }
        let release_section = match releases {
            github::GitHubFetch::Loading => activity_section(
                tr!("activity.releases"),
                None,
                activity_note(tr!("activity.loading"), &theme).into_any_element(),
                &theme,
            ),
            github::GitHubFetch::Loaded(None) => activity_section(
                tr!("activity.releases"),
                None,
                activity_note(tr!("activity.unavailable"), &theme).into_any_element(),
                &theme,
            ),
            github::GitHubFetch::Loaded(Some(entries)) => {
                let visible = entries
                    .iter()
                    .filter(|entry| {
                        filter.is_empty()
                            || entry.tag_name.to_lowercase().contains(&filter)
                            || entry.name.to_lowercase().contains(&filter)
                    })
                    .collect::<Vec<_>>();
                if visible.is_empty() {
                    activity_section(
                        tr!("activity.releases"),
                        Some(0),
                        activity_note(tr!("activity.no_releases"), &theme).into_any_element(),
                        &theme,
                    )
                } else {
                    let count = visible.len();
                    let rows = div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .children(visible.into_iter().map(|entry| {
                            let selection = ActivitySelection::Release(entry.tag_name.clone());
                            let name = entry.name.trim();
                            let same_name =
                                name.is_empty() || name.eq_ignore_ascii_case(&entry.tag_name);
                            let title = if same_name {
                                entry.tag_name.clone()
                            } else {
                                entry.name.clone()
                            };
                            let subtitle = (!same_name).then(|| entry.tag_name.clone());
                            let badge = if entry.is_draft {
                                Some((tr!("activity.draft"), None, theme.text_secondary))
                            } else if entry.is_prerelease {
                                Some((tr!("activity.prerelease"), None, theme.accent))
                            } else {
                                None
                            };
                            let date = entry
                                .published_at
                                .as_deref()
                                .and_then(|date| date.get(..10))
                                .map(str::to_owned);
                            activity_row(
                                project_id,
                                selection.clone(),
                                selected.as_ref() == Some(&selection),
                                "icons/package.svg",
                                theme.text_tertiary,
                                title,
                                subtitle,
                                badge,
                                date,
                                &theme,
                                cx,
                            )
                        }))
                        .into_any_element();
                    activity_section(tr!("activity.releases"), Some(count), rows, &theme)
                }
            }
        };
        content = content.child(release_section);
        let run_section = match runs {
            github::GitHubFetch::Loading => activity_section(
                tr!("activity.runs"),
                None,
                activity_note(tr!("activity.loading"), &theme).into_any_element(),
                &theme,
            ),
            github::GitHubFetch::Loaded(None) => activity_section(
                tr!("activity.runs"),
                None,
                activity_note(tr!("activity.unavailable"), &theme).into_any_element(),
                &theme,
            ),
            github::GitHubFetch::Loaded(Some(entries)) => {
                let visible = entries
                    .iter()
                    .filter(|entry| {
                        filter.is_empty()
                            || entry.name.to_lowercase().contains(&filter)
                            || entry.display_title.to_lowercase().contains(&filter)
                            || entry
                                .head_branch
                                .as_deref()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains(&filter)
                    })
                    .collect::<Vec<_>>();
                if visible.is_empty() {
                    activity_section(
                        tr!("activity.runs"),
                        Some(0),
                        activity_note(tr!("activity.no_runs"), &theme).into_any_element(),
                        &theme,
                    )
                } else {
                    let count = visible.len();
                    let rows = div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .children(visible.into_iter().map(|entry| {
                            let selection = ActivitySelection::Run(entry.database_id);
                            let name = entry.name.trim();
                            let title = if name.is_empty() {
                                entry.display_title.clone()
                            } else {
                                entry.name.clone()
                            };
                            let mut metadata = Vec::with_capacity(2);
                            if !entry.display_title.trim().is_empty()
                                && !entry.display_title.eq_ignore_ascii_case(&title)
                            {
                                metadata.push(entry.display_title.clone());
                            }
                            if let Some(branch) = entry
                                .head_branch
                                .as_deref()
                                .filter(|branch| !branch.trim().is_empty())
                            {
                                metadata.push(branch.to_owned());
                            }
                            let subtitle = (!metadata.is_empty()).then(|| metadata.join(" · "));
                            let raw_status = run_status(entry);
                            let status = raw_status.replace('_', " ");
                            let (status_icon, status_color) =
                                activity_run_status_appearance(raw_status, &theme);
                            let badge = Some((status, Some(status_icon), status_color));
                            let date = entry
                                .created_at
                                .as_deref()
                                .and_then(|date| date.get(..10))
                                .map(str::to_owned);
                            activity_row(
                                project_id,
                                selection.clone(),
                                selected.as_ref() == Some(&selection),
                                "icons/play.svg",
                                theme.text_tertiary,
                                title,
                                subtitle,
                                badge,
                                date,
                                &theme,
                                cx,
                            )
                        }))
                        .into_any_element();
                    activity_section(tr!("activity.runs"), Some(count), rows, &theme)
                }
            }
        };
        content = content.child(run_section);
        div()
            .id("projects-activity-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .child(content)
            .into_any_element()
    }
}

fn activity_section(label: String, count: Option<usize>, body: AnyElement, theme: &Theme) -> Div {
    let mut heading = div()
        .flex()
        .items_center()
        .gap(px(7.0))
        .text_size(sp(13.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(label);
    if let Some(count) = count {
        heading = heading.child(
            div()
                .text_size(sp(11.0))
                .font_weight(FontWeight::NORMAL)
                .text_color(theme.text_tertiary)
                .child(count.to_string()),
        );
    }
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(heading)
        .child(body)
}

fn activity_heading(label: String, theme: &Theme) -> Div {
    div()
        .text_size(sp(14.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(label)
}

fn activity_note(label: String, theme: &Theme) -> Div {
    div()
        .text_size(sp(13.0))
        .text_color(theme.text_secondary)
        .child(label)
}

fn activity_row(
    project_id: Uuid,
    selection: ActivitySelection,
    selected: bool,
    leading_icon: &'static str,
    leading_color: Hsla,
    label: String,
    subtitle: Option<String>,
    badge: Option<(String, Option<&'static str>, Hsla)>,
    date: Option<String>,
    theme: &Theme,
    cx: &mut Context<Waku>,
) -> Stateful<Div> {
    let mut row = div()
        .id(SharedString::from(format!(
            "activity-{project_id}-{selection:?}"
        )))
        .tab_index(0)
        .w_full()
        .h(px(34.0))
        .cursor_pointer()
        .rounded(px(6.0))
        .px(px(8.0))
        .border_b(hairline())
        .border_color(theme.separator)
        .when(selected, |row| row.bg(theme.overlay_strong))
        .hover(|row| row.bg(theme.overlay))
        .focus_visible(|row| row.bg(theme.focus_highlight()))
        .flex()
        .items_center()
        .gap(px(9.0))
        .child(icon(leading_icon, 13.0, leading_color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(sp(13.0))
                .text_color(theme.text)
                .truncate()
                .child(label),
        );
    if let Some(subtitle) = subtitle {
        row = row.child(
            div()
                .flex_none()
                .min_w_0()
                .max_w(px(320.0))
                .text_size(sp(11.5))
                .text_color(theme.text_tertiary)
                .truncate()
                .child(subtitle),
        );
    }
    if let Some((label, icon_path, color)) = badge {
        row = row.child(activity_status_badge(label, icon_path, color, theme));
    }
    row.child(
        div().w(px(82.0)).flex_none().flex().justify_end().child(
            div()
                .text_size(sp(11.0))
                .text_color(theme.text_tertiary)
                .child(date.unwrap_or_default()),
        ),
    )
    .on_activation(cx, move |this, _, cx| {
        this.activity_select(project_id, selection.clone(), cx)
    })
}

fn activity_status_badge(
    label: String,
    icon_path: Option<&'static str>,
    color: Hsla,
    theme: &Theme,
) -> Div {
    let mut badge = div()
        .flex_none()
        .h(px(22.0))
        .px(px(7.0))
        .rounded(px(5.0))
        .bg(theme.inset)
        .flex()
        .items_center()
        .gap(px(4.0));
    if let Some(icon_path) = icon_path {
        badge = badge.child(icon(icon_path, 11.0, color));
    }
    badge.child(div().text_size(sp(11.0)).text_color(color).child(label))
}

fn activity_run_status_appearance(status: &str, theme: &Theme) -> (&'static str, Hsla) {
    match status {
        "success" => ("icons/circle-check.svg", theme.success),
        "failure" | "timed_out" | "action_required" => ("icons/x-bold.svg", theme.danger),
        "in_progress" | "queued" | "requested" | "waiting" => {
            ("icons/loader-circle.svg", theme.accent)
        }
        "cancelled" | "completed" | "skipped" | "neutral" => {
            ("icons/circle-dot.svg", theme.text_tertiary)
        }
        _ => ("icons/circle-dot.svg", theme.text_secondary),
    }
}

fn run_status(run: &GitHubWorkflowRun) -> &str {
    run.conclusion
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&run.status)
}

fn activity_detail(detail: &ActivityDetail, theme: &Theme) -> AnyElement {
    match detail {
        ActivityDetail::Release(release) => {
            let body = release.body.as_deref().unwrap_or("");
            let mut column = div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(activity_heading(release.tag_name.clone(), theme))
                .children(
                    body.lines()
                        .take(200)
                        .map(|line| activity_note(line.to_owned(), theme)),
                );
            if body.lines().nth(200).is_some() {
                column = column.child(activity_note(tr!("activity.truncated"), theme));
            }
            if !release.assets.is_empty() {
                column = column.child(activity_heading(tr!("activity.assets"), theme));
            }
            column
                .children(release.assets.iter().map(|asset| {
                    activity_note(format!("{} · {} bytes", asset.name, asset.size), theme)
                }))
                .into_any_element()
        }
        ActivityDetail::Run(run) => {
            let mut column = div().flex().flex_col().gap(px(7.0)).child(activity_heading(
                format!("{} · {}", run.name, run_status(run)),
                theme,
            ));
            for job in &run.jobs {
                column = column.child(activity_note(
                    format!(
                        "{} · {}",
                        job.name,
                        job.conclusion.as_deref().unwrap_or(&job.status)
                    ),
                    theme,
                ));
                for step in &job.steps {
                    column = column.child(activity_note(
                        format!(
                            "    {} · {}",
                            step.name,
                            step.conclusion.as_deref().unwrap_or(&step.status)
                        ),
                        theme,
                    ));
                }
            }
            if let Some(log) = &run.failed_log {
                column = column.child(activity_heading(tr!("activity.failed_log"), theme));
                let tail = log.lines().rev().take(500).collect::<Vec<_>>();
                if log.lines().nth(500).is_some() {
                    column = column.child(activity_note(tr!("activity.truncated"), theme));
                }
                column = column.children(
                    tail.into_iter()
                        .rev()
                        .map(|line| activity_note(line.to_owned(), theme)),
                );
            } else if run.conclusion.as_deref() == Some("failure") {
                column = column.child(activity_note(tr!("activity.log_unavailable"), theme));
            }
            column.into_any_element()
        }
    }
}
