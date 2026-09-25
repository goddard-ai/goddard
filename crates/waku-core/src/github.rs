//! GitHub repository reads through the user's `gh` CLI.
//!
//! `gh` owns authentication and host selection, so GitHub Enterprise remotes
//! work without extra configuration. Reads follow the `pull_requests`
//! contract: `None` means the host could not be read — `gh` missing,
//! unauthenticated, or the directory not being a repository it knows — which
//! callers render as "unknown", distinct from an empty `Some`.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Output;

use anyhow::Context as _;
use serde::Deserialize;

use waku_protocol::workspace::{
    GitHubAvailability, GitHubRelease, GitHubRepoRef, GitHubWorkflowRun, WorkItemQueryState,
};

/// Run `gh` in `cwd`, returning its output on success. `None` is the shared
/// "host could not answer" signal — spawn failure or non-zero exit, whatever
/// the CLI's own wording for it.
pub(crate) fn gh_output(cwd: &Path, args: &[&OsStr]) -> Option<Output> {
    let output = crate::command_env::search_path_command("gh")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    output.status.success().then_some(output)
}

/// Run `gh` in `cwd` for a write, failing with the CLI's own stderr wording.
/// The reads' `None` "host could not answer" contract does not apply here —
/// a post the user is waiting on must say why it did not land.
pub(crate) fn gh_write(cwd: &Path, args: &[&OsStr]) -> anyhow::Result<()> {
    gh_write_output(cwd, args).map(|_| ())
}

/// `gh_write` for writes that answer on stdout — `gh issue create` prints
/// the new issue's URL.
pub(crate) fn gh_write_output(cwd: &Path, args: &[&OsStr]) -> anyhow::Result<String> {
    let output = crate::command_env::search_path_command("gh")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("could not run `gh`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            anyhow::bail!("the GitHub CLI rejected the request");
        }
        anyhow::bail!("{stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `gh` timestamps arrive RFC 3339; the wire and everything reading it speaks
/// unix seconds.
pub(crate) fn gh_time(value: Option<String>) -> Option<u64> {
    value
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(&text).ok())
        .and_then(|time| u64::try_from(time.timestamp()).ok())
}

/// `gh` exits non-zero for both "not a repo" and "not logged in"; only the
/// stderr wording separates them. Match the auth phrasing so the UI can point
/// at `gh auth login` instead of hiding GitHub support entirely.
fn gh_error_is_auth(stderr: &str) -> bool {
    let lowered = stderr.to_lowercase();
    lowered.contains("gh auth login")
        || lowered.contains("not logged")
        || lowered.contains("requires authentication")
}

fn gh_error_is_no_repo(stderr: &str) -> bool {
    let lowered = stderr.to_lowercase();
    lowered.contains("not a git repository")
        || lowered.contains("no git remotes")
        || lowered.contains("no remotes found")
        || lowered.contains("none of the git remotes configured for this repository")
}

pub fn resolve_repo(cwd: &Path) -> (Option<GitHubRepoRef>, GitHubAvailability) {
    let args = [
        OsStr::new("repo"),
        OsStr::new("view"),
        OsStr::new("--json"),
        OsStr::new("nameWithOwner,url,defaultBranchRef"),
    ];
    let result = crate::command_env::search_path_command("gh")
        .args(args)
        .current_dir(cwd)
        .output();
    let output = match result {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (None, GitHubAvailability::MissingCli);
        }
        // A spawn failure other than a missing executable may be temporary.
        Err(_) => return (None, GitHubAvailability::Unavailable),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Only explicit no-repository/no-remote diagnostics are conclusive;
        // network, API, and unrecognized failures should remain retryable.
        let availability = if gh_error_is_no_repo(&stderr) {
            GitHubAvailability::Ready
        } else if gh_error_is_auth(&stderr) {
            GitHubAvailability::Unauthenticated
        } else {
            GitHubAvailability::Unavailable
        };
        return (None, availability);
    }
    match serde_json::from_slice::<GhRepoView>(&output.stdout) {
        Ok(view) => (Some(view.into_ref()), GitHubAvailability::Ready),
        Err(_) => (None, GitHubAvailability::Unavailable),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhRepoView {
    name_with_owner: String,
    url: String,
    #[serde(default)]
    default_branch_ref: Option<GhRefName>,
}

#[derive(Deserialize)]
struct GhRefName {
    name: String,
}

impl GhRepoView {
    fn into_ref(self) -> GitHubRepoRef {
        let (owner, name) = self
            .name_with_owner
            .split_once('/')
            .map(|(owner, name)| (owner.to_owned(), name.to_owned()))
            .unwrap_or_default();
        // The web URL is https://<host>/<owner>/<repo>; the host between the
        // scheme and the first slash names GHES instances, and `None` says
        // github.com.
        let host = self
            .url
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .filter(|host| !host.eq_ignore_ascii_case("github.com"))
            .map(str::to_owned);
        GitHubRepoRef {
            owner,
            name,
            host,
            web_url: self.url,
            default_branch: self.default_branch_ref.map(|branch| branch.name),
        }
    }
}

pub(crate) fn gh_query_state(state: WorkItemQueryState) -> &'static str {
    match state {
        WorkItemQueryState::Open => "open",
        WorkItemQueryState::Closed => "closed",
        WorkItemQueryState::All => "all",
    }
}

/// `{"login": "…"}` as `gh` reports authors and assignees.
#[derive(Deserialize)]
pub(crate) struct GhUser {
    pub login: String,
}

/// `{"name": "…"}` as `gh` reports labels.
#[derive(Deserialize)]
pub(crate) struct GhLabel {
    pub name: String,
}

/// One issue/PR comment as `gh <item> view --json comments` reports it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GhComment {
    #[serde(default)]
    pub author: Option<GhUser>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

impl GhComment {
    pub(crate) fn into_comment(self) -> waku_protocol::workspace::WorkItemComment {
        waku_protocol::workspace::WorkItemComment {
            author: self.author.map(|author| author.login),
            body: self.body,
            url: self.url,
            created_at: gh_time(self.created_at),
        }
    }
}

/// Parse `gh`'s JSON stdout, which is the only failure mode left once the
/// process itself ran clean — kept as an error rather than `None` so a schema
/// change surfaces as a bug instead of silently empty UI.
pub(crate) fn parse_gh_stdout<T: for<'de> Deserialize<'de>>(
    output: &Output,
    command: &str,
) -> anyhow::Result<T> {
    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("could not parse `{command}` output"))
}

/// Bounded project activity reads. A failed `gh` invocation keeps the same
/// unknown-vs-empty contract as the issue and PR lists.
pub fn list_releases(cwd: &Path) -> anyhow::Result<Option<Vec<GitHubRelease>>> {
    let Some(output) = gh_output(
        cwd,
        &[
            OsStr::new("release"),
            OsStr::new("list"),
            OsStr::new("--limit"),
            OsStr::new("10"),
            OsStr::new("--json"),
            OsStr::new("tagName,name,isDraft,isPrerelease,publishedAt"),
        ],
    ) else {
        return Ok(None);
    };
    parse_gh_stdout(&output, "gh release list").map(Some)
}

pub fn release(cwd: &Path, tag: &str) -> anyhow::Result<Option<GitHubRelease>> {
    let Some(output) = gh_output(
        cwd,
        &[
            OsStr::new("release"),
            OsStr::new("view"),
            OsStr::new(tag),
            OsStr::new("--json"),
            OsStr::new("tagName,name,isDraft,isPrerelease,publishedAt,body,assets"),
        ],
    ) else {
        return Ok(None);
    };
    parse_gh_stdout(&output, "gh release view").map(Some)
}

pub fn list_workflow_runs(cwd: &Path) -> anyhow::Result<Option<Vec<GitHubWorkflowRun>>> {
    let Some(output) = gh_output(
        cwd,
        &[
            OsStr::new("run"),
            OsStr::new("list"),
            OsStr::new("--limit"),
            OsStr::new("20"),
            OsStr::new("--json"),
            OsStr::new(
                "databaseId,name,displayTitle,status,conclusion,headBranch,createdAt,attempt",
            ),
        ],
    ) else {
        return Ok(None);
    };
    parse_gh_stdout(&output, "gh run list").map(Some)
}

pub fn workflow_run(cwd: &Path, run_id: u64) -> anyhow::Result<Option<GitHubWorkflowRun>> {
    let id = run_id.to_string();
    let Some(output) = gh_output(
        cwd,
        &[
            OsStr::new("run"),
            OsStr::new("view"),
            OsStr::new(&id),
            OsStr::new("--json"),
            OsStr::new(
                "databaseId,name,displayTitle,status,conclusion,headBranch,createdAt,attempt,jobs",
            ),
        ],
    ) else {
        return Ok(None);
    };
    let mut run: GitHubWorkflowRun = parse_gh_stdout(&output, "gh run view")?;
    if run.conclusion.as_deref() == Some("failure") {
        if let Some(log) = gh_output(
            cwd,
            &[
                OsStr::new("run"),
                OsStr::new("view"),
                OsStr::new(&id),
                OsStr::new("--log-failed"),
            ],
        ) {
            const LOG_LIMIT: usize = 256 * 1024;
            let start = log.stdout.len().saturating_sub(LOG_LIMIT);
            let mut text = String::from_utf8_lossy(&log.stdout[start..]).into_owned();
            if log.stdout.len() > LOG_LIMIT {
                text.insert_str(0, "… earlier failed-log output omitted in Goddard\n");
            }
            if !text.trim().is_empty() {
                run.failed_log = Some(text);
            }
        }
    }
    Ok(Some(run))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_view_maps_host_and_default_branch() {
        let json = br#"{
            "nameWithOwner": "acme/widget",
            "url": "https://github.com/acme/widget",
            "defaultBranchRef": {"name": "main"}
        }"#;
        let repo = serde_json::from_slice::<GhRepoView>(json)
            .unwrap()
            .into_ref();
        assert_eq!(repo.owner, "acme");
        assert_eq!(repo.name, "widget");
        assert_eq!(repo.host, None);
        assert_eq!(repo.default_branch.as_deref(), Some("main"));
    }

    #[test]
    fn repo_view_names_enterprise_host() {
        let json = br#"{
            "nameWithOwner": "acme/widget",
            "url": "https://ghe.acme.example/acme/widget",
            "defaultBranchRef": null
        }"#;
        let repo = serde_json::from_slice::<GhRepoView>(json)
            .unwrap()
            .into_ref();
        assert_eq!(repo.host.as_deref(), Some("ghe.acme.example"));
    }

    #[test]
    fn auth_errors_are_recognised() {
        assert!(gh_error_is_auth(
            "To get started with GitHub CLI, please run: gh auth login"
        ));
        assert!(gh_error_is_auth("You are not logged into any GitHub hosts"));
        assert!(!gh_error_is_auth(
            "none of the git remotes correspond to GitHub"
        ));
    }
}
