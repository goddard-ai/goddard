//! The user's GitHub notification inbox — a user-level surface, not a repo
//! one, so none of it is scoped by `cwd`.
//!
//! `gh` stays the single credential store: the poll exchanges `gh auth
//! token` for a bearer token and reads `/notifications` over plain HTTPS so
//! the conditional-request headers (`If-Modified-Since` in,
//! `Last-Modified`/`X-Poll-Interval` out) are ours to manage — `gh api`
//! does not surface them cleanly, and a free 304 is what makes a 60-second
//! cadence cheap. The rare writes shell back to `gh api`, which owns auth
//! and host resolution on its own.

use std::process::Stdio;

use anyhow::Context as _;
use serde::Deserialize;

use waku_protocol::workspace::{
    GitHubAvailability, NotificationPoll, NotificationReason, NotificationSubjectType,
    NotificationThread,
};

const API_ROOT: &str = "https://api.github.com";
/// First page only — the endpoint serves newest-first, and an inbox deeper
/// than one page is a triage problem the list's filters already solve.
const NOTIFICATIONS_PER_PAGE: usize = 50;

/// The `gh` credential check, mapped onto the same availability model the
/// repo-level surfaces report.
fn auth_token() -> Result<String, GitHubAvailability> {
    let output = crate::command_env::search_path_command("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .output();
    let output = match output {
        // The binary itself could not be spawned.
        Err(_) => return Err(GitHubAvailability::MissingCli),
        Ok(output) => output,
    };
    if !output.status.success() {
        return Err(GitHubAvailability::Unauthenticated);
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if token.is_empty() {
        return Err(GitHubAvailability::Unauthenticated);
    }
    Ok(token)
}

/// One page of the notification inbox. `etag`/`if_modified_since` replay
/// the previous poll's validators — a 304 costs no rate limit. The endpoint
/// answers `ETag` on github.com; `Last-Modified` covers hosts that send one
/// instead. `include_read` maps to `?all=true` — GitHub cannot distinguish
/// read from done, so read threads come back indistinguishable from done
/// ones.
pub fn list(
    etag: Option<&str>,
    if_modified_since: Option<&str>,
    include_read: bool,
) -> anyhow::Result<NotificationPoll> {
    let token = match auth_token() {
        Ok(token) => token,
        Err(availability) => return Ok(NotificationPoll::Unavailable { availability }),
    };
    let mut url = format!("{API_ROOT}/notifications?per_page={NOTIFICATIONS_PER_PAGE}");
    if include_read {
        url.push_str("&all=true");
    }
    let mut headers = vec![
        format!("Authorization: Bearer {token}"),
        "Accept: application/vnd.github+json".to_owned(),
        "X-GitHub-Api-Version: 2022-11-28".to_owned(),
        "User-Agent: waku".to_owned(),
    ];
    if let Some(etag) = etag {
        headers.push(format!("If-None-Match: {etag}"));
    }
    if let Some(since) = if_modified_since {
        headers.push(format!("If-Modified-Since: {since}"));
    }
    let response = crate::usage::http_get_response(&url, &headers)?;
    let poll_interval_seconds = header_value(&response.headers, "x-poll-interval")
        .and_then(|value| value.parse::<u64>().ok());
    match response.status {
        // A 304 costs no rate limit — the client's threads still stand.
        304 => {
            return Ok(NotificationPoll::Unchanged {
                poll_interval_seconds,
            });
        }
        200 => {}
        // The token `gh` handed over no longer works — treat it like the
        // auth check did, so the surface hints at `gh auth login`.
        401 => {
            return Ok(NotificationPoll::Unavailable {
                availability: GitHubAvailability::Unauthenticated,
            });
        }
        status => anyhow::bail!("the notifications endpoint returned HTTP {status}"),
    }
    let raw: Vec<GhNotification> = serde_json::from_str(&response.body)
        .context("could not parse the notifications response")?;
    Ok(NotificationPoll::Changed {
        threads: raw.into_iter().map(GhNotification::into_thread).collect(),
        etag: header_value(&response.headers, "etag"),
        last_modified: header_value(&response.headers, "last-modified"),
        poll_interval_seconds,
    })
}

/// `PATCH /notifications/threads/{id}` — the one-way unread→read step.
pub fn mark_read(thread_id: &str) -> anyhow::Result<()> {
    gh_api(&["-X", "PATCH", &format!("notifications/threads/{thread_id}")])
}

/// `DELETE /notifications/threads/{id}` — removes the thread outright;
/// GitHub gives no undo.
pub fn mark_done(thread_id: &str) -> anyhow::Result<()> {
    gh_api(&[
        "-X",
        "DELETE",
        &format!("notifications/threads/{thread_id}"),
    ])
}

/// `PUT /repos/{owner}/{repo}/notifications` — the bulk mark-read backing
/// the inbox's per-repo-group action.
pub fn mark_repo_read(repo: &str) -> anyhow::Result<()> {
    gh_api(&["-X", "PUT", &format!("repos/{repo}/notifications")])
}

/// `PATCH /notifications` — the header's mark-all-read. May run
/// asynchronously host-side; the next poll reconciles.
pub fn mark_all_read() -> anyhow::Result<()> {
    gh_api(&["-X", "PATCH", "notifications"])
}

/// `gh api` for the writes: they need no response headers, and the CLI
/// already owns auth, host, and error wording.
fn gh_api(args: &[&str]) -> anyhow::Result<()> {
    let output = crate::command_env::search_path_command("gh")
        .arg("api")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("could not run `gh api`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            anyhow::bail!("the GitHub CLI rejected the request");
        }
        anyhow::bail!("{stderr}");
    }
    Ok(())
}

/// A response header value from the `-D -` dump, case-insensitive on the
/// name like HTTP requires.
fn header_value(headers: &str, name: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

#[derive(Deserialize)]
struct GhNotification {
    id: String,
    #[serde(default)]
    unread: bool,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    updated_at: Option<String>,
    subject: GhSubject,
    repository: GhRepository,
}

#[derive(Deserialize)]
struct GhSubject {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    latest_comment_url: Option<String>,
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Deserialize)]
struct GhRepository {
    full_name: String,
    html_url: String,
}

impl GhNotification {
    fn into_thread(self) -> NotificationThread {
        let subject_type = subject_type(&self.subject.kind);
        let number = self
            .subject
            .url
            .as_deref()
            .and_then(|url| subject_number(url, subject_type));
        let url = resolve_web_url(&self);
        NotificationThread {
            id: self.id,
            repo: self.repository.full_name,
            repo_url: self.repository.html_url,
            title: self.subject.title,
            subject_type,
            reason: reason(&self.reason),
            url,
            number,
            unread: self.unread,
            updated_at: crate::github::gh_time(self.updated_at).unwrap_or(0),
        }
    }
}

/// The page a thread opens on: the subject's web URL, upgraded to the latest
/// comment's anchor when GitHub names one. `None` when the subject type
/// carries no URL (check suites, workflow runs, invitations) — the caller's
/// fallback is the repository page.
fn resolve_web_url(notification: &GhNotification) -> Option<String> {
    let subject = notification
        .subject
        .url
        .as_deref()
        .and_then(api_url_to_web)?;
    match notification
        .subject
        .latest_comment_url
        .as_deref()
        .and_then(comment_anchor)
    {
        Some(anchor) => Some(format!("{subject}{anchor}")),
        None => Some(subject),
    }
}

/// `https://api.github.com/repos/o/r/pulls/12` →
/// `https://github.com/o/r/pull/12`. Kinds without a web page map to `None`.
fn api_url_to_web(url: &str) -> Option<String> {
    let path = url.strip_prefix(&format!("{API_ROOT}/repos/"))?;
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    let kind = segments.next()?;
    let rest: Vec<&str> = segments.collect();
    let kind = match kind {
        "pulls" => "pull",
        "issues" => "issues",
        "releases" => "releases",
        "commits" => "commit",
        "discussions" => "discussions",
        _ => return None,
    };
    let mut web = format!("https://github.com/{owner}/{name}/{kind}");
    if !rest.is_empty() {
        web.push('/');
        web.push_str(&rest.join("/"));
    }
    Some(web)
}

/// The fragment a latest-comment URL contributes: `…/issues/comments/456` →
/// `#issuecomment-456`, `…/pulls/comments/456` → `#discussion_r456`,
/// `…/comments/456` (commit) → `#commitcomment-456`,
/// `…/discussions/comments/456` → `#discussioncomment-456`.
fn comment_anchor(url: &str) -> Option<String> {
    let path = url.strip_prefix(&format!("{API_ROOT}/repos/"))?;
    let segments: Vec<&str> = path.split('/').collect();
    match segments.as_slice() {
        // owner / name / kind / comments / id
        [_, _, kind, "comments", id] => {
            let prefix = match *kind {
                "issues" => "issuecomment-",
                "pulls" => "discussion_r",
                "discussions" => "discussioncomment-",
                _ => return None,
            };
            Some(format!("#{prefix}{id}"))
        }
        // owner / name / comments / id — a commit comment.
        [_, _, "comments", id] => Some(format!("#commitcomment-{id}")),
        _ => None,
    }
}

/// The pull request or issue number a subject URL ends in — the deep-link
/// key into a project's GitHub detail views.
fn subject_number(url: &str, kind: NotificationSubjectType) -> Option<u64> {
    if !matches!(
        kind,
        NotificationSubjectType::PullRequest | NotificationSubjectType::Issue
    ) {
        return None;
    }
    url.rsplit('/').next()?.parse().ok()
}

fn reason(raw: &str) -> NotificationReason {
    match raw {
        "assign" => NotificationReason::Assign,
        "author" => NotificationReason::Author,
        "ci_activity" => NotificationReason::CiActivity,
        "comment" => NotificationReason::Comment,
        "invitation" => NotificationReason::Invitation,
        "manual" => NotificationReason::Manual,
        "mention" => NotificationReason::Mention,
        "review_requested" => NotificationReason::ReviewRequested,
        "security_alert" => NotificationReason::SecurityAlert,
        "state_change" => NotificationReason::StateChange,
        "subscribed" => NotificationReason::Subscribed,
        "team_mention" => NotificationReason::TeamMention,
        _ => NotificationReason::Other,
    }
}

fn subject_type(raw: &str) -> NotificationSubjectType {
    match raw {
        "PullRequest" => NotificationSubjectType::PullRequest,
        "Issue" => NotificationSubjectType::Issue,
        "Discussion" => NotificationSubjectType::Discussion,
        "Release" => NotificationSubjectType::Release,
        "Commit" => NotificationSubjectType::Commit,
        "CheckSuite" => NotificationSubjectType::CheckSuite,
        "WorkflowRun" => NotificationSubjectType::WorkflowRun,
        "RepositoryInvitation" => NotificationSubjectType::RepositoryInvitation,
        "RepositoryVulnerabilityAlert" => NotificationSubjectType::RepositoryVulnerabilityAlert,
        _ => NotificationSubjectType::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_urls_resolve_to_web_pages() {
        assert_eq!(
            api_url_to_web("https://api.github.com/repos/o/r/pulls/12"),
            Some("https://github.com/o/r/pull/12".to_owned())
        );
        assert_eq!(
            api_url_to_web("https://api.github.com/repos/o/r/issues/4"),
            Some("https://github.com/o/r/issues/4".to_owned())
        );
        assert_eq!(
            api_url_to_web("https://api.github.com/repos/o/r/commits/abc"),
            Some("https://github.com/o/r/commit/abc".to_owned())
        );
        // Check suites and workflow runs carry no subject URL web page.
        assert_eq!(
            api_url_to_web("https://api.github.com/repos/o/r/check-suites/9"),
            None
        );
    }

    #[test]
    fn latest_comment_urls_become_anchors() {
        assert_eq!(
            comment_anchor("https://api.github.com/repos/o/r/issues/comments/456"),
            Some("#issuecomment-456".to_owned())
        );
        assert_eq!(
            comment_anchor("https://api.github.com/repos/o/r/pulls/comments/456"),
            Some("#discussion_r456".to_owned())
        );
        assert_eq!(
            comment_anchor("https://api.github.com/repos/o/r/comments/456"),
            Some("#commitcomment-456".to_owned())
        );
        assert_eq!(
            comment_anchor("https://api.github.com/repos/o/r/discussions/comments/456"),
            Some("#discussioncomment-456".to_owned())
        );
    }

    #[test]
    fn a_thread_resolves_its_deepest_link() {
        let notification = GhNotification {
            id: "1".to_owned(),
            unread: true,
            reason: "review_requested".to_owned(),
            updated_at: Some("2026-09-16T10:00:00Z".to_owned()),
            subject: GhSubject {
                title: "Add the thing".to_owned(),
                url: Some("https://api.github.com/repos/o/r/pulls/12".to_owned()),
                latest_comment_url: Some(
                    "https://api.github.com/repos/o/r/pulls/comments/456".to_owned(),
                ),
                kind: "PullRequest".to_owned(),
            },
            repository: GhRepository {
                full_name: "o/r".to_owned(),
                html_url: "https://github.com/o/r".to_owned(),
            },
        };
        let thread = notification.into_thread();
        assert_eq!(thread.number, Some(12));
        assert_eq!(thread.reason, NotificationReason::ReviewRequested);
        assert_eq!(
            thread.url.as_deref(),
            Some("https://github.com/o/r/pull/12#discussion_r456")
        );
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let headers =
            "HTTP/2 200\r\nLast-Modified: Wed, 16 Sep 2026 10:00:00 GMT\r\nX-Poll-Interval: 60";
        assert_eq!(
            header_value(headers, "last-modified").as_deref(),
            Some("Wed, 16 Sep 2026 10:00:00 GMT")
        );
        assert_eq!(
            header_value(headers, "x-poll-interval").as_deref(),
            Some("60")
        );
    }
}
