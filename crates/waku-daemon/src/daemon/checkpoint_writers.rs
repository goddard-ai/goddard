//! Durable physical capture attempts. Persistence debt and workspace admission
//! remain separate from this bounded inventory of unfinished Git captures.
use super::*;

impl WakuBackend {
    pub(super) fn record_checkpoint_writer_before_capture(
        &self,
        cwd: &Path,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<Uuid> {
        self.retry_checkpoint_writer_confirmations()?;
        let workspace = dunce::canonicalize(cwd)?;
        #[cfg(unix)]
        let identity = Some(workspace_writers::terminal_directory_identity(&workspace)?);
        #[cfg(not(unix))]
        let identity = None;
        let origin_id = Uuid::new_v4();
        let _admission = self.admit_workspace_writer(&workspace)?;
        self.task_store
            .record_checkpoint_writer(&crate::persistence::CheckpointWriterOrigin {
                origin_id,
                session_id,
                turn_count,
                workspace,
                identity,
            })?;
        Ok(origin_id)
    }

    /// Called only after this attempt's capture returned with the existing
    /// physical-quiet evidence. A later successful capture cannot supply it.
    pub(super) fn confirm_checkpoint_writer_after_capture(&self, origin_id: Uuid) {
        self.runtime_lifecycles
            .checkpoint_confirmations
            .lock()
            .insert(origin_id);
        match self.task_store.confirm_checkpoint_writer(origin_id) {
            Ok(()) => {
                self.runtime_lifecycles
                    .checkpoint_confirmations
                    .lock()
                    .remove(&origin_id);
            }
            Err(error) => {
                eprintln!("could not persist checkpoint writer completion {origin_id}: {error}")
            }
        }
    }

    fn retry_checkpoint_writer_confirmations(&self) -> anyhow::Result<()> {
        let pending: Vec<_> = self
            .runtime_lifecycles
            .checkpoint_confirmations
            .lock()
            .iter()
            .take(64)
            .copied()
            .collect();
        let mut failure = None;
        for origin_id in pending {
            match self.task_store.confirm_checkpoint_writer(origin_id) {
                Ok(()) => {
                    self.runtime_lifecycles
                        .checkpoint_confirmations
                        .lock()
                        .remove(&origin_id);
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        if let Some(error) = failure {
            return Err(error.into());
        }
        Ok(())
    }

    pub(super) fn require_no_observed_checkpoint_writers(
        &self,
        target: &Path,
    ) -> anyhow::Result<()> {
        self.retry_checkpoint_writer_confirmations()?;
        let origins = self.task_store.checkpoint_writer_origins()?;
        anyhow::ensure!(
            origins.len() < 65,
            "workspace cleanup deferred: checkpoint writer inventory exceeds the bounded safety scan"
        );
        for origin in origins {
            let canonical = dunce::canonicalize(&origin.workspace)
                .context("workspace cleanup deferred: checkpoint writer origin is unavailable")?;
            #[cfg(unix)]
            let current_identity =
                Some(workspace_writers::terminal_directory_identity(&canonical)?);
            #[cfg(not(unix))]
            let current_identity: Option<String> = None;
            anyhow::ensure!(
                canonical == origin.workspace
                    && origin.identity.is_some()
                    && origin.identity == current_identity,
                "workspace cleanup deferred: checkpoint writer origin moved, was reused or is unqualified"
            );
            anyhow::ensure!(
                !(canonical.starts_with(target) || target.starts_with(&canonical)),
                "workspace cleanup deferred: a durable checkpoint writer has no confirmed completion"
            );
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
#[path = "tests/checkpoint_writers.rs"]
mod tests;
