//! Archive-triggered stops retain their committed archive and physical owner.
use super::*;
use crate::driver::WriterCompletion;

pub(super) struct ArchivedRuntimeRetirement {
    session_id: Uuid,
    archive_generation: Uuid,
    runtime: Option<(Uuid, WriterCompletion)>,
    clear_state: bool,
}

impl WakuBackend {
    /// Called with the catalog locked immediately after its archive commit.
    /// Witnesses do not retain a driver or turn an absent runtime into proof
    /// of quiescence. In-flight starts still require workspace admission.
    pub(super) fn archived_runtime_retirements(
        &self,
        ids: &[Uuid],
        clear_state: bool,
    ) -> Vec<ArchivedRuntimeRetirement> {
        let mut retirements = Vec::new();
        for id in ids {
            let runtime = self
                .sessions
                .lock()
                .get(id)
                .map(|entry| (entry.runtime_id, entry.driver.writer_completion()));
            match self.task_store.workspace_cleanup_generation(*id) {
                Ok(Some(archive_generation)) => retirements.push(ArchivedRuntimeRetirement {
                    session_id: *id,
                    archive_generation,
                    runtime,
                    clear_state,
                }),
                Ok(None) => {}
                Err(error) => eprintln!("could not qualify archived runtime stop: {error}"),
            }
        }
        retirements
    }

    /// A delayed archive may stop only the owner captured at its commit. The
    /// lifecycle gate excludes a successor between validation and removal.
    /// Stop authority is claimed under the catalog lock; no driver operation
    /// runs while holding that global lock.
    pub(super) fn retire_archived_runtimes(&self, retirements: &[ArchivedRuntimeRetirement]) {
        for retirement in retirements {
            let lifecycle = self.runtime_lifecycles.session(retirement.session_id);
            let _owner = lifecycle.lock();
            let state = self.task_state.lock();
            if !state
                .sessions
                .iter()
                .find(|row| row.id == retirement.session_id)
                .is_some_and(|row| row.archived_at.is_some())
            {
                continue;
            }
            match self
                .task_store
                .workspace_cleanup_generation(retirement.session_id)
            {
                Ok(Some(generation)) if generation == retirement.archive_generation => {}
                Ok(_) => continue,
                Err(error) => {
                    eprintln!("could not validate archived runtime stop: {error}");
                    continue;
                }
            }
            let matches = match (
                &retirement.runtime,
                self.sessions.lock().get(&retirement.session_id),
            ) {
                (Some((runtime_id, writer)), Some(entry)) => {
                    entry.runtime_id == *runtime_id
                        && entry.driver.writer_completion().same_writer(writer)
                }
                // A task without a provider still needs its parked credentials
                // revoked. An old absence must never match a new live owner.
                (None, None) => true,
                _ => false,
            };
            drop(state);
            if matches {
                drop_detached(self.remove_runtime(
                    retirement.session_id,
                    retirement.runtime.as_ref().map(|(id, _)| *id),
                    retirement.clear_state,
                ));
            }
        }
    }
}
