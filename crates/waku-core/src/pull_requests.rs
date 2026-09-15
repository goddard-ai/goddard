//! Pull request reads through the repository host's own CLI.
//!
//! GitHub's `gh` is the only host understood today. A missing or
//! unauthenticated CLI, or a directory the host does not know, reports `None`
//! — callers render that as "unknown", which is a different answer from an
//! empty list ("the host checked and found nothing").

use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use waku_protocol::workspace::{
    PullRequestCheckStatus, PullRequestReviewDecision, PullRequestState, PullRequestSummary,
};

/// Keeps a reused branch's history from paging the whole sidebar scan.
const PULL_REQUEST_LIST_LIMIT: &str = "30";

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

/// `gh` timestamps arrive RFC 3339; the wire and everything reading it speaks
/// unix seconds.
fn gh_time(value: Option<String>) -> Option<u64> {
    value
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(&text).ok())
        .and_then(|time| u64::try_from(time.timestamp()).ok())
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
}

impl GhPullRequest {
    /// `None` for a state `gh` has not documented — dropping one row beats
    /// failing the whole read over a host-side addition.
    fn into_summary(self) -> Option<PullRequestSummary> {
        let state = match self.state.as_str() {
            "OPEN" => PullRequestState::Open,
            "CLOSED" => PullRequestState::Closed,
            "MERGED" => PullRequestState::Merged,
            _ => return None,
        };
        let review_decision = match self.review_decision.as_deref() {
            Some("APPROVED") => Some(PullRequestReviewDecision::Approved),
            Some("CHANGES_REQUESTED") => Some(PullRequestReviewDecision::ChangesRequested),
            Some("REVIEW_REQUIRED") => Some(PullRequestReviewDecision::ReviewRequired),
            _ => None,
        };
        let check_status = gh_check_status(&self.status_check_rollup);
        Some(PullRequestSummary {
            number: self.number,
            title: self.title,
            url: self.url,
            state,
            is_draft: self.is_draft,
            base_branch: self.base_ref_name,
            created_at: gh_time(self.created_at),
            updated_at: gh_time(self.updated_at),
            review_decision,
            check_status,
            additions: self.additions,
            deletions: self.deletions,
        })
    }
}

/// One `statusCheckRollup` entry. `gh` mixes two shapes in the same list:
/// check runs report `status` + `conclusion`, legacy commit statuses report
/// `state`. Both are optional so an unfamiliar entry shape is skipped rather
/// than failing the read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhCheckRollupEntry {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    state: Option<String>,
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
