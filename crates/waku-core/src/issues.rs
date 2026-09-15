//! Issue reads through the repository host's CLI — `gh` only today.
//!
//! Same contract as `pull_requests`: `None` is "the host could not answer",
//! an empty `Some` is "the host checked and found nothing".

use std::ffi::OsString;
use std::path::Path;

use serde::Deserialize;

use waku_protocol::workspace::{
    IssueDetail, IssueState, IssueSummary, WorkItemQueryState,
};

use crate::github::{
    GhComment, GhLabel, GhUser, gh_output, gh_query_state, gh_time, parse_gh_stdout,
};

const ISSUE_LIST_LIMIT: &str = "100";
const ISSUE_FIELDS: &str =
    "number,title,url,state,author,labels,assignees,createdAt,updatedAt";

/// Repo-wide issue list for the GitHub browser. `query` forwards to
/// `gh issue list --search`.
pub fn list(
    cwd: &Path,
    state: WorkItemQueryState,
    query: Option<&str>,
) -> anyhow::Result<Option<Vec<IssueSummary>>> {
    let mut args = vec![
        OsString::from("issue"),
        OsString::from("list"),
        OsString::from("--state"),
        OsString::from(gh_query_state(state)),
        OsString::from("--limit"),
        OsString::from(ISSUE_LIST_LIMIT),
        OsString::from("--json"),
        OsString::from(ISSUE_FIELDS),
    ];
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        args.push(OsString::from("--search"));
        args.push(OsString::from(query));
    }
    let Some(output) = gh_output(cwd, &args.iter().map(OsString::as_os_str).collect::<Vec<_>>())
    else {
        return Ok(None);
    };
    let entries: Vec<GhIssue> = parse_gh_stdout(&output, "gh issue list")?;
    Ok(Some(
        entries.into_iter().filter_map(GhIssue::into_summary).collect(),
    ))
}

/// One issue with its body and comment thread.
pub fn view(cwd: &Path, number: u64) -> anyhow::Result<Option<IssueDetail>> {
    let args = [
        OsString::from("issue"),
        OsString::from("view"),
        OsString::from(number.to_string()),
        OsString::from("--json"),
        OsString::from(format!("{ISSUE_FIELDS},body,comments")),
    ];
    let Some(output) = gh_output(cwd, &args.iter().map(OsString::as_os_str).collect::<Vec<_>>())
    else {
        return Ok(None);
    };
    let entry: GhIssue = parse_gh_stdout(&output, "gh issue view")?;
    Ok(entry.into_detail())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhIssue {
    number: u64,
    title: String,
    url: String,
    state: String,
    #[serde(default)]
    author: Option<GhUser>,
    #[serde(default)]
    labels: Vec<GhLabel>,
    #[serde(default)]
    assignees: Vec<GhUser>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    comments: Vec<GhComment>,
}

impl GhIssue {
    /// `None` for a state `gh` has not documented — dropping one row beats
    /// failing the whole read over a host-side addition.
    fn into_state(&self) -> Option<IssueState> {
        match self.state.as_str() {
            "OPEN" => Some(IssueState::Open),
            "CLOSED" => Some(IssueState::Closed),
            _ => None,
        }
    }

    fn into_summary(self) -> Option<IssueSummary> {
        let state = self.into_state()?;
        Some(IssueSummary {
            number: self.number,
            title: self.title,
            url: self.url,
            state,
            author: self.author.map(|author| author.login),
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            assignees: self
                .assignees
                .into_iter()
                .map(|assignee| assignee.login)
                .collect(),
            created_at: gh_time(self.created_at),
            updated_at: gh_time(self.updated_at),
        })
    }

    /// A `view` whose state `gh` does not document still returns the detail —
    /// the UI can render state as unknown rather than losing the thread.
    fn into_detail(self) -> Option<IssueDetail> {
        let state = self.into_state().unwrap_or(IssueState::Open);
        Some(IssueDetail {
            summary: IssueSummary {
                number: self.number,
                title: self.title,
                url: self.url,
                state,
                author: self.author.map(|author| author.login),
                labels: self.labels.into_iter().map(|label| label.name).collect(),
                assignees: self
                    .assignees
                    .into_iter()
                    .map(|assignee| assignee.login)
                    .collect(),
                created_at: gh_time(self.created_at),
                updated_at: gh_time(self.updated_at),
            },
            body: self.body,
            comments: self
                .comments
                .into_iter()
                .map(GhComment::into_comment)
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_issue_rows_map_to_summaries() {
        let json = br#"[
            {"number": 12, "title": "flake", "url": "https://github.com/o/r/issues/12",
             "state": "OPEN", "author": {"login": "sam"},
             "labels": [{"name": "bug"}], "assignees": [{"login": "sam"}],
             "createdAt": "2026-09-08T00:00:00Z", "updatedAt": "2026-09-10T00:00:00Z"},
            {"number": 13, "title": "unknown state", "url": "https://github.com/o/r/issues/13",
             "state": "TRIAGED"}
        ]"#;
        let rows: Vec<GhIssue> = serde_json::from_slice(json).unwrap();
        let summaries: Vec<_> = rows.into_iter().filter_map(GhIssue::into_summary).collect();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].state, IssueState::Open);
        assert_eq!(summaries[0].author.as_deref(), Some("sam"));
        assert_eq!(summaries[0].labels, ["bug"]);
        assert_eq!(summaries[0].assignees, ["sam"]);
    }

    #[test]
    fn gh_issue_view_maps_body_and_comments() {
        let json = br#"{
            "number": 12, "title": "flake", "url": "https://github.com/o/r/issues/12",
            "state": "CLOSED", "author": {"login": "sam"}, "body": "it flakes",
            "comments": [{"author": {"login": "jo"}, "body": "retry?",
                          "url": "https://github.com/o/r/issues/12#issuecomment-1",
                          "createdAt": "2026-09-09T00:00:00Z"}]
        }"#;
        let detail = serde_json::from_slice::<GhIssue>(json)
            .unwrap()
            .into_detail()
            .unwrap();
        assert_eq!(detail.summary.state, IssueState::Closed);
        assert_eq!(detail.body.as_deref(), Some("it flakes"));
        assert_eq!(detail.comments.len(), 1);
        assert_eq!(detail.comments[0].author.as_deref(), Some("jo"));
        assert!(detail.comments[0].created_at.is_some());
    }
}
