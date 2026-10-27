//! Daemon publication uses the same filesystem gate as catalog transactions.
use super::*;
pub(crate) use waku_base::workspace_admission::WorkspaceAdmission;

impl WakuBackend {
    pub(crate) fn admit_workspace_writer(&self, path: &Path) -> anyhow::Result<WorkspaceAdmission> {
        WorkspaceAdmission::writer(self.task_store.path(), path)
    }

    /// Keep restore/materialization requests excluded from retirement through
    /// their complete filesystem operation, including a not-yet-created target.
    pub(super) fn admit_workspace_restore(
        &self,
        operation: &WorkspaceOperation,
    ) -> anyhow::Result<Option<WorkspaceAdmission>> {
        match operation {
            WorkspaceOperation::RestoreRef { cwd, .. } => {
                WorkspaceAdmission::catalog(self.task_store.path(), std::slice::from_ref(cwd))
            }
            WorkspaceOperation::RestoreProjectlessWorkspace { path } => {
                WorkspaceAdmission::catalog(self.task_store.path(), std::slice::from_ref(path))
            }
            WorkspaceOperation::EnsureWorktree {
                project_path, path, ..
            } => WorkspaceAdmission::catalog(
                self.task_store.path(),
                &[project_path.clone(), path.clone()],
            ),
            _ => Ok(None),
        }
    }
}

#[cfg(all(test, unix))]
#[path = "tests/workspace_restore_admission.rs"]
mod tests;
