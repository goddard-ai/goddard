//! Bounded, generation-fenced consumption of durable cleanup metadata.
//! Preservation and retirement need their own executor; this stage can finish
//! non-owning descriptors and retain everything uncertain for a later unit.
use super::*;

const PAGE_SIZE: usize = 128;
const MAX_DESCRIPTOR_BYTES: i64 = 65_536;

const READY: &str = "SELECT intent.session_id, intent.generation, intent.phase,
       intent.failures,
       CASE WHEN length(CAST(intent.project_path AS BLOB)) <= ?2
            THEN intent.project_path END,
       CASE WHEN length(CAST(intent.workspace AS BLOB)) <= ?2
            THEN intent.workspace END,
       coalesce(length(CAST(intent.project_path AS BLOB)), 0) > ?2
         OR coalesce(length(CAST(intent.workspace AS BLOB)), 0) > ?2
 FROM workspace_cleanup_intents AS intent INDEXED BY workspace_cleanup_dispatch
 WHERE intent.phase NOT IN ('complete', 'ignored') AND intent.retry_at <= ?1
 ORDER BY intent.retry_at, intent.session_id LIMIT 128";

const CURRENT_CATALOG: &str = "SELECT 1 FROM sessions AS session
 JOIN projects AS project ON project.id = session.project_id
 WHERE session.id = intent.session_id
   AND session.project_id IS intent.project_id
   AND project.path IS intent.project_path
   AND session.workspace IS intent.workspace
   AND session.archived_at IS intent.archived_at
   AND session.archived_at IS NOT NULL";

struct QueuedIntent {
    session_id: String,
    generation: String,
    phase: String,
    failures: i64,
    project_path: Option<String>,
    workspace: Option<String>,
    oversized: bool,
}

enum Disposition {
    Ignore,
    AwaitExecutor,
    Invalid(&'static str),
}

impl QueuedIntent {
    fn disposition(&self) -> Disposition {
        if Uuid::parse_str(&self.session_id).is_err()
            || Uuid::parse_str(&self.generation).is_err()
            || self.failures < 0
        {
            return Disposition::Invalid("invalid-cleanup-identity");
        }
        if self.oversized {
            return Disposition::Invalid("oversized-cleanup-descriptor");
        }
        if self.phase != "pending" {
            return if matches!(self.phase.as_str(), "captured" | "verified" | "retired") {
                Disposition::AwaitExecutor
            } else {
                Disposition::Invalid("invalid-cleanup-phase")
            };
        }
        let workspace: crate::model::SessionWorkspace = match self.workspace.as_deref() {
            Some(value) => match serde_json::from_str(value) {
                Ok(workspace) => workspace,
                Err(_) => return Disposition::Invalid("invalid-workspace-descriptor"),
            },
            None => Default::default(),
        };
        match workspace {
            crate::model::SessionWorkspace::Worktree { borrowed: true, .. }
            | crate::model::SessionWorkspace::Worktree {
                adopted_by: Some(_),
                ..
            } => Disposition::Ignore,
            crate::model::SessionWorkspace::Worktree { path, .. } => {
                if path.is_absolute() {
                    Disposition::AwaitExecutor
                } else {
                    Disposition::Invalid("invalid-workspace-path")
                }
            }
            crate::model::SessionWorkspace::Local
            | crate::model::SessionWorkspace::NewWorktree { .. } => {
                let Some(path) = self
                    .project_path
                    .as_deref()
                    .map(Path::new)
                    .filter(|path| path.is_absolute())
                else {
                    return Disposition::Invalid("invalid-project-path");
                };
                if waku_protocol::projectless::workspace_root().is_none() {
                    return Disposition::Invalid("workspace-root-unavailable");
                }
                if waku_protocol::projectless::is_projectless_path(path)
                    || waku_protocol::projectless::is_legacy_root_path(path)
                {
                    Disposition::AwaitExecutor
                } else {
                    // Ordinary project checkouts are user-owned. A draft that
                    // never materialized a worktree owns no isolated directory.
                    Disposition::Ignore
                }
            }
        }
    }
}

impl StateStore {
    /// Consume at most 128 narrow due descriptors in one idle-admitted unit.
    /// No transcript or filesystem content is loaded. Each update compares
    /// the observed generation and phase again; ignored completion also checks
    /// the actual promoted catalog at the statement's commit boundary.
    pub fn advance_workspace_cleanup_queue(
        &self,
        now: u64,
        should_continue: impl Fn() -> bool,
    ) -> io::Result<()> {
        if !should_continue() {
            return Ok(());
        }
        // Startup already initialized this database. Do not run migrations,
        // change journal mode, create directories, or wait behind a mandatory
        // writer from an optional bounded maintenance unit.
        let connection =
            Connection::open_with_flags(self.path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(to_io_error)?;
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(to_io_error)?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(to_io_error)?;
        let now = i64::try_from(now).unwrap_or(i64::MAX);
        let mut statement = connection.prepare(READY).map_err(to_io_error)?;
        let rows = statement
            .query_map(params![now, MAX_DESCRIPTOR_BYTES], |row| {
                Ok(QueuedIntent {
                    session_id: row.get(0)?,
                    generation: row.get(1)?,
                    phase: row.get(2)?,
                    failures: row.get(3)?,
                    project_path: row.get(4)?,
                    workspace: row.get(5)?,
                    oversized: row.get(6)?,
                })
            })
            .map_err(to_io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_io_error)?;
        drop(statement);
        debug_assert!(rows.len() <= PAGE_SIZE);
        let ignore = format!(
            "UPDATE workspace_cleanup_intents AS intent
             SET phase = 'ignored', retry_at = 0, failures = 0, last_error = NULL
             WHERE session_id = ?1 AND generation = ?2 AND phase = ?3
               AND EXISTS ({CURRENT_CATALOG})"
        );
        for row in rows {
            if !should_continue() {
                break;
            }
            let (reason, failure) = match row.disposition() {
                Disposition::Ignore => {
                    let changed = connection
                        .execute(&ignore, params![row.session_id, row.generation, row.phase])
                        .map_err(to_io_error)?;
                    if changed != 0 {
                        continue;
                    }
                    ("catalog-state-mismatch", true)
                }
                Disposition::AwaitExecutor => ("awaiting-preservation-executor", false),
                Disposition::Invalid(reason) => (reason, true),
            };
            let failures = if failure {
                row.failures.max(0).saturating_add(1)
            } else {
                row.failures
            };
            let delay = if failure {
                (2i64.saturating_pow(failures.min(8) as u32)).min(300)
            } else {
                30
            };
            connection
                .execute(
                    "UPDATE workspace_cleanup_intents
                SET retry_at = ?4, failures = ?5, last_error = ?6
                WHERE session_id = ?1 AND generation = ?2 AND phase = ?3",
                    params![
                        row.session_id,
                        row.generation,
                        row.phase,
                        now.saturating_add(delay),
                        failures,
                        reason
                    ],
                )
                .map_err(to_io_error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
