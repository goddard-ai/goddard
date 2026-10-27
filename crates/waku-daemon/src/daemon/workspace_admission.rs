//! Daemon publication uses the same filesystem gate as catalog transactions.
use super::*;
pub(crate) use waku_base::workspace_admission::WorkspaceAdmission;

impl WakuBackend {
    pub(crate) fn admit_workspace_writer(&self, path: &Path) -> anyhow::Result<WorkspaceAdmission> {
        WorkspaceAdmission::writer(self.task_store.path(), path)
    }

    fn admit_workspace_creation_path(
        &self,
        scope: crate::worktree::CreationScope<'_>,
    ) -> anyhow::Result<WorkspaceAdmission> {
        match scope {
            crate::worktree::CreationScope::Source(path) => self.admit_workspace_writer(path),
            crate::worktree::CreationScope::Destination(path) => {
                WorkspaceAdmission::materialize(self.task_store.path(), path)
            }
        }
    }

    pub(super) fn create_managed_worktree(
        &self,
        project_path: &Path,
        name: Option<&str>,
        base_ref: Option<&str>,
        sync_default_branch: bool,
        sync_branches: &[String],
    ) -> anyhow::Result<crate::worktree::CreatedWorktree> {
        crate::worktree::create_guarded(
            project_path,
            name,
            base_ref,
            sync_default_branch,
            sync_branches,
            |scope| self.admit_workspace_creation_path(scope),
        )
    }

    fn admit_projectless_creation(
        &self,
        scope: crate::projectless::CreationScope<'_>,
    ) -> std::io::Result<WorkspaceAdmission> {
        let result = match scope {
            crate::projectless::CreationScope::Namespace(path) => {
                WorkspaceAdmission::namespace(self.task_store.path(), path)
            }
            crate::projectless::CreationScope::Destination(path) => {
                WorkspaceAdmission::materialize(self.task_store.path(), path)
            }
        };
        result.map_err(|error| match error.downcast::<std::io::Error>() {
            Ok(error) => error,
            Err(error) => std::io::Error::other(error.to_string()),
        })
    }

    pub(super) fn execute_workspace_creation(
        &self,
        operation: &WorkspaceOperation,
    ) -> anyhow::Result<Option<WorkspaceResult>> {
        Ok(Some(match operation {
            WorkspaceOperation::CreateProjectlessWorkspace { prompt } => {
                WorkspaceResult::ProjectlessWorkspace {
                    cwd: crate::projectless::create_workspace_guarded(
                        prompt.as_deref(),
                        |scope| self.admit_projectless_creation(scope),
                    )?
                    .cwd,
                }
            }
            WorkspaceOperation::CreateWorktree {
                project_path,
                name,
                base_ref,
                sync_default_branch,
                sync_branches,
            } => WorkspaceResult::WorktreeCreated {
                worktree: self.create_managed_worktree(
                    project_path,
                    name.as_deref(),
                    base_ref.as_deref(),
                    *sync_default_branch,
                    sync_branches,
                )?,
            },
            WorkspaceOperation::CreateWorktreeFromCheckout { project_path, name } => {
                WorkspaceResult::WorktreeCreated {
                    worktree: crate::worktree::create_from_checkout_guarded(
                        project_path,
                        name.as_deref(),
                        |scope| self.admit_workspace_creation_path(scope),
                    )?,
                }
            }
            WorkspaceOperation::EnsureWorktree {
                project_path,
                path,
                branch,
                base_ref,
            } => {
                match crate::worktree::ensure_guarded(
                    project_path,
                    path,
                    branch.as_deref(),
                    base_ref.as_deref(),
                    |scope| self.admit_workspace_creation_path(scope),
                )? {
                    Some(branch) => WorkspaceResult::WorktreeEnsured {
                        created: true,
                        branch,
                    },
                    None => WorkspaceResult::WorktreeEnsured {
                        created: false,
                        branch: None,
                    },
                }
            }
            WorkspaceOperation::QaStart { cwd, branch } => WorkspaceResult::QaStarted {
                checkout: waku_vcs::qa::start_guarded(cwd, branch, |scope| {
                    self.admit_workspace_creation_path(scope)
                })?,
            },
            _ => return Ok(None),
        }))
    }

    /// Keep restore/materialization requests excluded from retirement through
    /// their complete filesystem operation, including a not-yet-created target.
    pub(super) fn admit_workspace_restore(
        &self,
        operation: &WorkspaceOperation,
    ) -> anyhow::Result<Option<WorkspaceAdmission>> {
        match operation {
            WorkspaceOperation::RestoreRef { cwd, .. } => {
                WorkspaceAdmission::materialize(self.task_store.path(), cwd).map(Some)
            }
            WorkspaceOperation::RestoreProjectlessWorkspace { path } => {
                WorkspaceAdmission::materialize(self.task_store.path(), path).map(Some)
            }
            _ => Ok(None),
        }
    }
}

#[cfg(all(test, unix))]
#[path = "tests/workspace_restore_admission.rs"]
mod tests;
