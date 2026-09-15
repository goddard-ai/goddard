//! Pull request reads through the repository host's own CLI.
//!
//! GitHub's `gh` is the only host understood today. A missing or
//! unauthenticated CLI, or a directory the host does not know, reports `None`
//! — callers render that as "unknown", which is a different answer from an
//! empty list ("the host checked and found nothing").

use std::ffi::OsString;
use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use waku_protocol::workspace::{
    PullRequestCheck, PullRequestCheckStatus, PullRequestDetail, PullRequestFile,
    PullRequestReviewDecision, PullRequestState, PullRequestSummary, WorkItemQueryState,
};

use crate::github::{
    GhComment, GhUser, gh_output, gh_query_state, gh_time, parse_gh_stdout,
};

/// Keeps a reused branch's history from paging the whole sidebar scan.
const PULL_REQUEST_LIST_LIMIT: &str = "30";
/// The repo-wide browser pages deeper than the badge scan.
const PULL_REQUEST_REPO_LIST_LIMIT: &str = "100";

const PULL_REQUEST_LIST_FIELDS: &str =
    "number,title,url,state,isDraft,author,headRefName,baseRefName,createdAt,updatedAt,reviewDecision,statusCheckRollup,additions,deletions";

pub fn list(cwd: &Path, head_branch: &str) -> anyhow::Result<Option<Vec<PullRequestSummary>>> {
    let output = crate::command_env::plain_command("gh")
        .args([
            "pr",
            "list",
            "--head",
            head_branch,
            "--state",
            "all",
            "--limit",
            PULL_REQUEST_LIST_LIMIT,
            "--json",
            "number,title,url,state,isDraft,baseRefName,createdAt,updatedAt,reviewDecision,statusCheckRollup,additions,deletions",
        ])
        .current_dir(cwd)
        .output();
    let output = match output {
        Ok(output) if output.status.success() => output,
        _ => return Ok(None),
    };
    let entries: Vec<GhPullRequest> = serde_json::from_slice(&output.stdout)
        .context("could not parse `gh pr list` output")?;
    Ok(Some(
        entries
            .into_iter()
            .filter_map(GhPullRequest::into_summary)
            .collect(),
    ))
}

/// Repo-wide pull-request list for the GitHub browser. `query` forwards to
/// `gh pr list --search`.
pub fn list_for_repo(
    cwd: &Path,
    state: WorkItemQueryState,
    query: Option<&str>,
) -> anyhow::Result<Option<Vec<PullRequestSummary>>> {
    let mut args = vec![
        OsString::from("pr"),
        OsString::from("list"),
        OsString::from("--state"),
        OsString::from(gh_query_state(state)),
        OsString::from("--limit"),
        OsString::from(PULL_REQUEST_REPO_LIST_LIMIT),
        OsString::from("--json"),
        OsString::from(PULL_REQUEST_LIST_FIELDS),
    ];
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        args.push(OsString::from("--search"));
        args.push(OsString::from(query));
    }
    let arg_refs: Vec<_> = args.iter().map(OsString::as_os_str).collect();
    let Some(output) = gh_output(cwd, &arg_refs) else {
        return Ok(None);
    };
    let entries: Vec<GhPullRequest> = parse_gh_stdout(&output, "gh pr list")?;
    Ok(Some(
        entries
            .into_iter()
            .filter_map(GhPullRequest::into_summary)
            .collect(),
    ))
}

/// One pull request with its body, comments, checks, and changed files.
pub fn view(cwd: &Path, number: u64) -> anyhow::Result<Option<PullRequestDetail>> {
    let args = [
        OsString::from("pr"),
        OsString::from("view"),
        OsString::from(number.to_string()),
        OsString::from("--json"),
        OsString::from(format!("{PULL_REQUEST_LIST_FIELDS},body,comments,files")),
    ];
    let arg_refs: Vec<_> = args.iter().map(OsString::as_os_str).collect();
    let Some(output) = gh_output(cwd, &arg_refs) else {
        return Ok(None);
    };
    let entry: GhPullRequest = parse_gh_stdout(&output, "gh pr view")?;
    Ok(Some(entry.into_detail()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
    number: u64,
    title: String,
    url: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    author: Option<GhUser>,
    #[serde(default)]
    head_ref_name: Option<String>,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    status_check_rollup: Vec<GhCheckRollupEntry>,
    #[serde(default)]
    additions: Option<u64>,
    #[serde(default)]
    deletions: Option<u64>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    comments: Vec<GhComment>,
    #[serde(default)]
    files: Vec<GhPullRequestFile>,
}

impl GhPullRequest {
    /// `None` for a state `gh` has not documented.
    fn gh_state(&self) -> Option<PullRequestState> {
        match self.state.as_str() {
            "OPEN" => Some(PullRequestState::Open),
            "CLOSED" => Some(PullRequestState::Closed),
            "MERGED" => Some(PullRequestState::Merged),
            _ => None,
        }
    }

    fn gh_review_decision(&self) -> Option<PullRequestReviewDecision> {
        match self.review_decision.as_deref() {
            Some("APPROVED") => Some(PullRequestReviewDecision::Approved),
            Some("CHANGES_REQUESTED") => Some(PullRequestReviewDecision::ChangesRequested),
            Some("REVIEW_REQUIRED") => Some(PullRequestReviewDecision::ReviewRequired),
            _ => None,
        }
    }

    fn summary(&self, state: PullRequestState) -> PullRequestSummary {
        PullRequestSummary {
            number: self.number,
            title: self.title.clone(),
            url: self.url.clone(),
            state,
            is_draft: self.is_draft,
            base_branch: self.base_ref_name.clone(),
            created_at: gh_time(self.created_at.clone()),
            updated_at: gh_time(self.updated_at.clone()),
            review_decision: self.gh_review_decision(),
            check_status: gh_check_status(&self.status_check_rollup),
            additions: self.additions,
            deletions: self.deletions,
            author: self.author.as_ref().map(|author| author.login.clone()),
            head_branch: self.head_ref_name.clone(),
        }
    }

    /// `None` for a state `gh` has not documented — dropping one row beats
    /// failing the whole read over a host-side addition.
    fn into_summary(self) -> Option<PullRequestSummary> {
        self.gh_state().map(|state| self.summary(state))
    }

    /// A `view` whose state `gh` does not document still returns the detail —
    /// the UI renders unknown state rather than losing the thread.
    fn into_detail(self) -> PullRequestDetail {
        let state = self.gh_state().unwrap_or(PullRequestState::Open);
        PullRequestDetail {
            summary: self.summary(state),
            body: self.body,
            comments: self
                .comments
                .into_iter()
                .map(GhComment::into_comment)
                .collect(),
            checks: self
                .status_check_rollup
                .iter()
                .filter_map(GhCheckRollupEntry::as_check)
                .collect(),
            files: self
                .files
                .into_iter()
                .map(|file| PullRequestFile {
                    path: file.path,
                    additions: file.additions,
                    deletions: file.deletions,
                })
                .collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequestFile {
    path: String,
    #[serde(default)]
    additions: Option<u64>,
    #[serde(default)]
    deletions: Option<u64>,
}

/// One `statusCheckRollup` entry. `gh` mixes two shapes in the same list:
/// check runs report `status` + `conclusion` + `name` + `detailsUrl`, legacy
/// commit statuses report `state` + `context` + `targetUrl`. Every field is
/// optional so an unfamiliar entry shape is skipped rather than failing the
/// read.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhCheckRollupEntry {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    details_url: Option<String>,
    #[serde(default)]
    target_url: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
}

impl GhCheckRollupEntry {
    /// This entry alone, for the detail view's per-run rows. `None` when the
    /// entry carries neither a check-run verdict nor a status state.
    fn as_check(&self) -> Option<PullRequestCheck> {
        let status = match (self.status.as_deref(), self.conclusion.as_deref(), self.state.as_deref()) {
            (Some(status), _, _) if status != "COMPLETED" => PullRequestCheckStatus::Pending,
            (Some(_), Some(conclusion), _) => match conclusion {
                "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "CANCELLED" => {
                    PullRequestCheckStatus::Failing
                }
                _ => PullRequestCheckStatus::Passing,
            },
            (_, _, Some(state)) => match state {
                "FAILURE" | "ERROR" => PullRequestCheckStatus::Failing,
                "PENDING" | "EXPECTED" => PullRequestCheckStatus::Pending,
                "SUCCESS" => PullRequestCheckStatus::Passing,
                _ => return None,
            },
            _ => return None,
        };
        let url = self.details_url.clone().or_else(|| self.target_url.clone());
        Some(PullRequestCheck {
            name: self.name.clone().or_else(|| self.context.clone())?,
            status,
            run_id: url.as_deref().and_then(actions_run_id),
            url,
            duration_seconds: match (gh_time(self.started_at.clone()), gh_time(self.completed_at.clone())) {
                (Some(started), Some(completed)) => completed.checked_sub(started),
                _ => None,
            },
        })
    }
}

/// `…/actions/runs/<id>/…` — the Actions run a details URL points at, so the
/// UI can fetch its log via `gh run view`.
fn actions_run_id(url: &str) -> Option<u64> {
    let marker = "/actions/runs/";
    let start = url.find(marker)? + marker.len();
    let digits: String = url[start..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Rolls the host's check entries into one badge signal: any failure wins,
/// then anything still running. `None` when there is nothing to report.
fn gh_check_status(rollup: &[GhCheckRollupEntry]) -> Option<PullRequestCheckStatus> {
    if rollup.is_empty() {
        return None;
    }
    let mut pending = false;
    for entry in rollup {
        if let Some(status) = entry.status.as_deref() {
            if status != "COMPLETED" {
                pending = true;
            } else if matches!(
                entry.conclusion.as_deref(),
                Some("FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "CANCELLED")
            ) {
                return Some(PullRequestCheckStatus::Failing);
            }
        }
        match entry.state.as_deref() {
            Some("FAILURE" | "ERROR") => return Some(PullRequestCheckStatus::Failing),
            Some("PENDING" | "EXPECTED") => pending = true,
            _ => {}
        }
    }
    Some(if pending {
        PullRequestCheckStatus::Pending
    } else {
        PullRequestCheckStatus::Passing
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_rows_map_to_summaries() {
        let json = br#"[
            {"number": 3, "title": "oldest", "url": "https://github.com/o/r/pull/3",
             "state": "MERGED", "isDraft": false, "baseRefName": "main",
             "createdAt": "2026-09-08T00:00:00Z", "updatedAt": "2026-09-10T00:00:00Z",
             "reviewDecision": "APPROVED", "additions": 5, "deletions": 2},
            {"number": 7, "title": "wip", "url": "https://github.com/o/r/pull/7",
             "state": "OPEN", "isDraft": true, "baseRefName": "main",
             "createdAt": null, "updatedAt": null, "reviewDecision": "",
             "additions": null, "deletions": null},
            {"number": 9, "title": "surprise", "url": "https://github.com/o/r/pull/9",
             "state": "QUARANTINED", "isDraft": false}
        ]"#;
        let rows: Vec<GhPullRequest> = serde_json::from_slice(json).unwrap();
        let summaries: Vec<_> = rows
            .into_iter()
            .filter_map(GhPullRequest::into_summary)
            .collect();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].state, PullRequestState::Merged);
        assert_eq!(
            summaries[0].review_decision,
            Some(PullRequestReviewDecision::Approved)
        );
        assert_eq!(summaries[0].additions, Some(5));
        assert!(summaries[0].created_at < summaries[0].updated_at);
        assert_eq!(summaries[1].state, PullRequestState::Open);
        assert!(summaries[1].is_draft);
        assert_eq!(summaries[1].review_decision, None);
        assert_eq!(summaries[1].created_at, None);
        assert_eq!(summaries[1].updated_at, None);
    }

    #[test]
    fn check_rollup_reports_failure_then_pending_then_passing() {
        fn entry(
            status: Option<&str>,
            conclusion: Option<&str>,
            state: Option<&str>,
        ) -> GhCheckRollupEntry {
            GhCheckRollupEntry {
                status: status.map(str::to_owned),
                conclusion: conclusion.map(str::to_owned),
                state: state.map(str::to_owned),
                ..Default::default()
            }
        }

        assert_eq!(gh_check_status(&[]), None);
        assert_eq!(
            gh_check_status(&[
                entry(Some("COMPLETED"), Some("SUCCESS"), None),
                entry(None, None, Some("SUCCESS")),
            ]),
            Some(PullRequestCheckStatus::Passing)
        );
        // A skipped or neutral conclusion does not count as a failure.
        assert_eq!(
            gh_check_status(&[
                entry(Some("COMPLETED"), Some("SUCCESS"), None),
                entry(Some("COMPLETED"), Some("SKIPPED"), None),
            ]),
            Some(PullRequestCheckStatus::Passing)
        );
        assert_eq!(
            gh_check_status(&[
                entry(Some("COMPLETED"), Some("SUCCESS"), None),
                entry(Some("IN_PROGRESS"), None, None),
            ]),
            Some(PullRequestCheckStatus::Pending)
        );
        assert_eq!(
            gh_check_status(&[entry(None, None, Some("PENDING"))]),
            Some(PullRequestCheckStatus::Pending)
        );
        // Failure beats pending no matter the order.
        assert_eq!(
            gh_check_status(&[
                entry(Some("IN_PROGRESS"), None, None),
                entry(Some("COMPLETED"), Some("FAILURE"), None),
            ]),
            Some(PullRequestCheckStatus::Failing)
        );
        assert_eq!(
            gh_check_status(&[entry(None, None, Some("ERROR"))]),
            Some(PullRequestCheckStatus::Failing)
        );
    }
}
