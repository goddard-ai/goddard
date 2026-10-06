//! Workspace filesystem and Git operations, composer completion, and skill management.
//! `waku-core` re-exports these modules to preserve existing paths.

pub(crate) use waku_base::{frontmatter, i18n, issue_templates, projectless};
pub(crate) use waku_drivers::slash_command_catalog;
pub(crate) use waku_exec::command_env;
pub(crate) use waku_git::{checkpoint, git_branch, git_commit, git_panel, worktree};
pub(crate) use waku_vcs::{
    github, issues, notifications, pull_requests, repo, review, shell_command,
};

pub mod composer_complete;
pub mod skills;
pub mod workspace;
