//! Daemon publication uses the same filesystem gate as catalog transactions.
use super::*;
pub(crate) use waku_base::workspace_admission::WorkspaceAdmission;

impl WakuBackend {
    pub(crate) fn admit_workspace_writer(&self, path: &Path) -> anyhow::Result<WorkspaceAdmission> {
        WorkspaceAdmission::writer(self.task_store.path(), path)
    }
}
