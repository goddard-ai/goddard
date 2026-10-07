//! Persistent Boss state and compartment-aware file access.

use std::collections::VecDeque;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, anyhow, bail};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use uuid::Uuid;
#[cfg(test)]
use waku_protocol::boss::BossPersonaUpsert;
use waku_protocol::boss::{
    AdmissionBlocker, BossDeliverable, BossEmployee, BossFile, BossIdentity, BossOperation,
    BossPersona, BossPlan, BossResourcePolicy, BossResult, BossState, BossWave,
    DispatchNotification, EmployeeExpiry, EmployeeGoal, EmployeeLifecycle, EmployeeSettle,
    ExpiryCause, INTERRUPTION_HISTORY_CAP, InterruptionRecord, MemoryMigrationCandidate,
    MemoryMigrationReport, ModelLimit, PermissionOverrides, PersonaPermissions, PlanActor,
    PlanItem, PlanItemInput, PlanItemState, PlanItemTransition, PlanOutcome, PlanTransition,
    SummonTicket, WaveMember,
    WaveMemberOutcome, WaveNotification,
};
use waku_protocol::model::ProviderKind;

pub fn validate_browse_url(url: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(url).context("invalid browser URL")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        bail!("browser URLs must use http or https");
    }
    Ok(())
}

fn project_bucket_id(path: &Path) -> String {
    // Git's common directory is stable across linked worktrees. Fall back to
    // the canonical project path for non-Git projects.
    let common_dir = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| path.join(value.trim()))
        .and_then(|value| fs::canonicalize(value).ok());
    let stable_path = common_dir
        .or_else(|| fs::canonicalize(path).ok())
        .unwrap_or_else(|| path.to_path_buf());
    let digest = Sha256::digest(stable_path.to_string_lossy().as_bytes());
    let key: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("project-{key}")
}

/// Resolve a `--project` reference — a registered project's name or id, or
/// an absolute project root — to a canonical directory path.
fn resolve_project_reference(
    projects: &[waku_protocol::model::Project],
    reference: &str,
) -> anyhow::Result<PathBuf> {
    if let Some(id) = waku_protocol::persistence::resolve_named_search_project(projects, reference)
        && let Some(project) = projects.iter().find(|project| project.id == id)
    {
        return Ok(project.path.clone());
    }
    let path = PathBuf::from(reference);
    anyhow::ensure!(
        path.is_absolute(),
        "project `{reference}` is unknown to the daemon"
    );
    fs::canonicalize(&path).with_context(|| format!("project `{reference}` does not exist"))
}

/// Pick the concrete bucket a memory operation targets. `project` resolves
/// through the registered-project catalog or an absolute path; an absent
/// pair falls back to the caller's assigned project. The returned path is
/// the project root when the target is project-derived so the caller can
/// lazily materialize the bucket.
fn resolve_memory_bucket(
    bucket: &Option<String>,
    project: &Option<String>,
    own_project: Option<&Path>,
    projects: &[waku_protocol::model::Project],
) -> anyhow::Result<(String, Option<PathBuf>)> {
    match (bucket, project) {
        (Some(_), Some(_)) => bail!("pass a bucket or a project, not both"),
        (_, Some(reference)) => {
            let path = resolve_project_reference(projects, reference)?;
            Ok((project_bucket_id(&path), Some(path)))
        }
        (Some(bucket), None) => Ok((bucket.clone(), None)),
        (None, None) => own_project
            .map(|path| (project_bucket_id(path), Some(path.to_path_buf())))
            .ok_or_else(|| anyhow!("memory operation needs a bucket or a project")),
    }
}

/// The `(bucket, project)` selectors of the bucket-addressed memory ops.
fn memory_bucket_ref(
    operation: &waku_protocol::boss::MemoryOperation,
) -> Option<(&Option<String>, &Option<String>)> {
    use waku_protocol::boss::MemoryOperation;
    match operation {
        MemoryOperation::Overview {
            bucket, project, ..
        }
        | MemoryOperation::Record {
            bucket, project, ..
        }
        | MemoryOperation::SubmitSummary {
            bucket, project, ..
        }
        | MemoryOperation::Scan {
            bucket, project, ..
        }
        | MemoryOperation::ZoomBucket {
            bucket, project, ..
        } => Some((bucket, project)),
        MemoryOperation::ListBuckets
        | MemoryOperation::CreateBucket { .. }
        | MemoryOperation::MigrateLegacy { .. } => None,
    }
}

const LEGACY_MEMORY_FILE_BYTES: u64 = 256 * 1024;
const LEGACY_MEMORY_NOTE_LIMIT: usize = 256;
const LEGACY_MEMORY_TOTAL_BYTES: usize = 2 * 1024 * 1024;
const LEGACY_MEMORY_PART_BYTES: usize = 56 * 1024;

fn legacy_memory_candidates(
    boss_root: &Path,
    source: &str,
) -> anyhow::Result<Vec<MemoryMigrationCandidate>> {
    let roots = if source == "boss" {
        vec![
            (boss_root.join("files/memory"), "boss/memory".to_owned()),
            (
                boss_root.join("files/memory-engine"),
                "boss/memory-engine".to_owned(),
            ),
        ]
    } else {
        let project = PathBuf::from(source);
        anyhow::ensure!(
            project.is_absolute(),
            "project memory source must be an absolute project root or 'boss'"
        );
        let project = fs::canonicalize(&project).context("project memory source does not exist")?;
        anyhow::ensure!(
            project.is_dir(),
            "project memory source must be a directory"
        );
        vec![
            (
                project.join(".goddard/memory"),
                format!("project/{}/.goddard/memory", project_bucket_id(&project)),
            ),
            (
                project.join(".goddard/memory-engine"),
                format!(
                    "project/{}/.goddard/memory-engine",
                    project_bucket_id(&project)
                ),
            ),
        ]
    };

    fn visit(
        path: &Path,
        relative: &Path,
        source_prefix: &str,
        out: &mut Vec<MemoryMigrationCandidate>,
    ) -> anyhow::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let mut entries = fs::read_dir(path)
            .with_context(|| format!("could not list {}", path.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let child = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let child_relative = relative.join(name.as_ref());
            if file_type.is_dir() {
                // Plan documents remain documents and are excluded even if
                // an old installation still has the former memory/plans path.
                if child_relative
                    .components()
                    .any(|part| part.as_os_str() == "plans")
                {
                    continue;
                }
                visit(&child, &child_relative, source_prefix, out)?;
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let is_log = child.file_name().is_some_and(|name| name == "LOG.txt");
            let is_markdown = child.extension().is_some_and(|ext| ext == "md");
            if !is_log && !is_markdown {
                continue;
            }
            let metadata = entry.metadata()?;
            anyhow::ensure!(
                metadata.len() <= LEGACY_MEMORY_FILE_BYTES,
                "legacy memory file exceeds 256 KiB: {}",
                child.display()
            );
            let content = fs::read_to_string(&child)
                .with_context(|| format!("legacy memory file is not UTF-8: {}", child.display()))?;
            let source = format!("{source_prefix}/{}", child_relative.to_string_lossy());
            if is_log {
                for (index, line) in content.lines().enumerate() {
                    let text =
                        serde_json::from_str::<String>(line).unwrap_or_else(|_| line.to_owned());
                    if !text.trim().is_empty() {
                        push_legacy_candidate(out, format!("{source}#{}", index + 1), &text)?;
                    }
                }
            } else {
                let text = markdown_body(&content).trim().to_owned();
                if !text.is_empty() {
                    push_legacy_candidate(out, source, &text)?;
                }
            }
        }
        Ok(())
    }

    fn markdown_body(content: &str) -> &str {
        if let Some(rest) = content.strip_prefix("---\n")
            && let Some(end) = rest.find("\n---\n")
        {
            return &rest[end + 5..];
        }
        content
    }

    let mut candidates = Vec::new();
    for (path, prefix) in roots {
        visit(&path, Path::new(""), &prefix, &mut candidates)?;
    }
    Ok(candidates)
}

fn push_legacy_candidate(
    out: &mut Vec<MemoryMigrationCandidate>,
    source: String,
    text: &str,
) -> anyhow::Result<()> {
    let mut start = 0;
    let mut part = 1;
    while start < text.len() {
        let mut end = (start + LEGACY_MEMORY_PART_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let source = if text.len() > LEGACY_MEMORY_PART_BYTES {
            format!("{source}#part-{part}")
        } else {
            source.clone()
        };
        out.push(MemoryMigrationCandidate {
            source,
            text: text[start..end].to_owned(),
        });
        start = end;
        part += 1;
        let total_bytes = out
            .iter()
            .map(|candidate| candidate.text.len())
            .sum::<usize>();
        anyhow::ensure!(
            out.len() <= LEGACY_MEMORY_NOTE_LIMIT,
            "legacy memory source exceeds {LEGACY_MEMORY_NOTE_LIMIT} notes"
        );
        anyhow::ensure!(
            total_bytes <= LEGACY_MEMORY_TOTAL_BYTES,
            "legacy memory source exceeds 2 MiB; import a narrower source"
        );
    }
    Ok(())
}

const MAX_FILE_BYTES: usize = 256 * 1024;
/// A published deliverable is a snapshot the daemon stores on the
/// publisher's behalf — bounded so publishing a build tree or dataset by
/// mistake cannot fill the Boss data directory. Larger or living artifacts
/// publish with `reference`, keeping a live path instead of a copy.
const MAX_DELIVERABLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DELIVERABLE_ENTRIES: usize = 10_000;
/// User prompts the router's focus inference sees at once.
const ROUTER_RECENT_PROMPTS: usize = 6;
/// A stored prompt's budget inside the router's eval state.
const ROUTER_PROMPT_CAP: usize = 300;

/// The context router's memory of a boss conversation — session-scoped, so a
/// reopened or replaced boss chat starts fresh rather than inheriting a
/// predecessor's inferred focus.
#[derive(Default)]
struct BossRouter {
    session: Option<Uuid>,
    /// The project Jev last inferred the user's attention centers on.
    focus: Option<String>,
    /// Recent user prompts, oldest first — the continuity the focus
    /// question judges each new message against.
    recent_prompts: VecDeque<String>,
    /// Jev asked for the work digest on a prompt that could not take it
    /// (no steer support, or the turn already settled); the next outbound
    /// boss prompt prepends it instead.
    pending_context: bool,
}

/// `speak` bounds: an utterance is a handful of short fragments, so a
/// malformed or oversized request fails at the daemon rather than reaching
/// clients as a TTS spend.
const MAX_SPEECH_PARTS: usize = 8;
const MAX_SPEECH_PART_CHARS: usize = 160;
const MAX_SPEECH_TOTAL_CHARS: usize = 480;
const EMPLOYEE_RETIREMENT_SECONDS: u64 = 60 * 60;
const GOAL_RETIREMENT_SECONDS: u64 = 24 * 60 * 60;
/// A finalized planning session stays active this long before the daemon
/// archives it — the grace window in which the boss can still converse in
/// the planning context and ask questions about the design it now
/// implements. One hour matches the reuse window a finished employee gets,
/// so both managed lifetimes sweep on the same cadence. After the sweep
/// the session is a plain archived managed task; only its frozen plan
/// record marks what it was.
const PLANNING_GRACE_SECONDS: u64 = 60 * 60;
/// A blocker report is one bounded attention item, not an essay — long
/// context belongs in the transcript the finish report indexes.
const MAX_BLOCKER_CHARS: usize = 1_000;

/// Serializes scripts so their dispatched operations have a defined order.
/// Each invocation creates a fresh Rhai scope.
#[derive(Default)]
struct BossEval;

pub struct BossService {
    root: PathBuf,
    active: std::sync::atomic::AtomicBool,
    state: Mutex<BossState>,
    notifier: Mutex<Option<crate::TaskNotifier>>,
    operation_lock: Mutex<()>,
    finish_employee: Mutex<Option<crate::FinishEmployee>>,
    recover_employee: Mutex<Option<crate::RecoverEmployee>>,
    session_active: Mutex<Option<crate::SessionActive>>,
    session_busy: Mutex<Option<crate::SessionBusy>>,
    archive_sessions: Mutex<Option<crate::ArchiveSessions>>,
    interrupted: Mutex<Vec<Uuid>>,
    projects: Mutex<std::collections::HashMap<Uuid, PathBuf>>,
    injected: Mutex<std::collections::HashSet<Uuid>>,
    router: Mutex<BossRouter>,
    evals: Mutex<BossEval>,
    project_catalog: Mutex<Option<crate::ProjectCatalog>>,
    /// The PERSONA.md bytes each persona id last had `save` write — most
    /// updates touch employees, not personas, so unchanged files skip
    /// their write+fsync entirely.
    persona_writes: Mutex<std::collections::HashMap<Uuid, String>>,
}

impl BossService {
    /// A process-local empty service used when the Boss experiment is off.
    /// It deliberately does not inspect or create the Boss data directory.
    pub fn disabled(root: PathBuf) -> Self {
        Self {
            root,
            active: std::sync::atomic::AtomicBool::new(false),
            state: Mutex::new(disabled_state()),
            notifier: Mutex::new(None),
            operation_lock: Mutex::new(()),
            finish_employee: Mutex::new(None),
            recover_employee: Mutex::new(None),
            session_active: Mutex::new(None),
            session_busy: Mutex::new(None),
            archive_sessions: Mutex::new(None),
            interrupted: Mutex::new(Vec::new()),
            projects: Mutex::new(std::collections::HashMap::new()),
            injected: Mutex::new(std::collections::HashSet::new()),
            router: Mutex::new(BossRouter::default()),
            evals: Mutex::new(BossEval::default()),
            project_catalog: Mutex::new(None),
            persona_writes: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Load durable Boss data on the first enabled operation. A daemon that
    /// starts with the experiment off never reads or creates the Boss store.
    pub fn activate(&self) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        if self.active.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut state = {
            fs::create_dir_all(self.root.join("files/memory"))?;
            let path = self.root.join("boss.json");
            match fs::read(&path) {
                Ok(bytes) => serde_json::from_slice(&bytes).context("invalid Boss document")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => fresh_state(),
                Err(error) => return Err(error.into()),
            }
        };
        let now = waku_protocol::model::unix_time();
        for employee in &mut state.employees {
            if employee.expired && employee.expired_at.is_none() {
                employee.expired_at = Some(now);
            }
            normalize_lifecycle(employee);
        }
        reconcile_waves(&mut state, now);
        *self.state.lock() = state;
        *self.interrupted.lock() = self
            .state
            .lock()
            .employees
            .iter()
            .filter(|entry| restart_interrupted(entry))
            .map(|entry| entry.session_id)
            .collect();
        self.migrate_plan_documents();
        self.sweep_deliverable_store(&self.state.lock());
        self.save(&self.state.lock())?;
        self.active.store(true, Ordering::Release);
        Ok(())
    }

    pub fn deactivate(&self) {
        use std::sync::atomic::Ordering;
        *self.state.lock() = disabled_state();
        self.interrupted.lock().clear();
        self.projects.lock().clear();
        self.injected.lock().clear();
        *self.router.lock() = BossRouter::default();
        *self.evals.lock() = BossEval::default();
        self.persona_writes.lock().clear();
        self.active.store(false, Ordering::Release);
    }

    pub fn open(root: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(root.join("files/memory"))?;
        let path = root.join("boss.json");
        // Fail closed on corruption: replacing the document would lose identity
        // and grants while leaving its private files behind.
        let mut state: BossState = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("invalid Boss document")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fresh_state(),
            Err(error) => return Err(error.into()),
        };
        for index in 0..state.employees.len() {
            if state.employees[index].job_title.is_empty() {
                state.employees[index].job_title = state.employees[index].identity.name.clone();
                let existing_names = state
                    .employees
                    .iter()
                    .enumerate()
                    .filter(|(other_index, _)| *other_index != index)
                    .map(|(_, employee)| employee.identity.name.as_str())
                    .collect::<Vec<_>>();
                state.employees[index].identity.name =
                    employee_human_name(existing_names, &mut state.name_cursor);
            }
        }
        const OLD_EXPIRY_GUIDANCE: &str = "Summon a fresh employee for a new job or when the previous employee is dead or finishing; never stack prompts onto an expiring employee, where queued work may be lost.";
        for persona in &mut state.personas {
            if persona.name == "Boss" && persona.markdown.contains(OLD_EXPIRY_GUIDANCE) {
                persona.markdown = persona.markdown.replace(
                    OLD_EXPIRY_GUIDANCE,
                    "Prompt or steer can resume an employee after its idle expiry with the same transcript; summon a fresh employee for a distinct job.",
                );
            }
        }
        // Older documents recorded only the expired flag, not when the
        // employee finished. Give those entries a full reuse window after
        // this version first sees them.
        let now = waku_protocol::model::unix_time();
        for employee in &mut state.employees {
            if employee.expired && employee.expired_at.is_none() {
                employee.expired_at = Some(now);
            }
            normalize_lifecycle(employee);
        }
        reconcile_waves(&mut state, now);
        let service = Self {
            root,
            active: std::sync::atomic::AtomicBool::new(true),
            interrupted: Mutex::new(
                state
                    .employees
                    .iter()
                    .filter(|entry| restart_interrupted(entry))
                    .map(|entry| entry.session_id)
                    .collect(),
            ),
            state: Mutex::new(state),
            notifier: Mutex::new(None),
            operation_lock: Mutex::new(()),
            finish_employee: Mutex::new(None),
            recover_employee: Mutex::new(None),
            session_active: Mutex::new(None),
            session_busy: Mutex::new(None),
            archive_sessions: Mutex::new(None),
            projects: Mutex::new(std::collections::HashMap::new()),
            injected: Mutex::new(std::collections::HashSet::new()),
            router: Mutex::new(BossRouter::default()),
            evals: Mutex::new(BossEval::default()),
            project_catalog: Mutex::new(None),
            persona_writes: Mutex::new(std::collections::HashMap::new()),
        };
        service.migrate_plan_documents();
        service.sweep_deliverable_store(&service.state.lock());
        service.save(&service.state.lock())?;
        Ok(service)
    }

    pub fn document(&self) -> BossState {
        self.state.lock().clone()
    }

    /// The pair `BossOperation::Open` needs — identity plus chat session id —
    /// without cloning the whole document.
    pub fn identity_and_session(&self) -> (BossIdentity, Option<Uuid>) {
        let state = self.state.lock();
        (state.identity.clone(), state.session_id)
    }

    pub fn set_session_id(&self, session_id: Uuid) -> anyhow::Result<()> {
        self.update(|state| {
            state.session_id = Some(session_id);
            Ok(())
        })
    }

    /// Publish a staged chat only if the session it replaces is still active.
    pub fn replace_session_id(&self, old: Uuid, next: Uuid) -> anyhow::Result<()> {
        self.update(|state| {
            anyhow::ensure!(
                state.session_id == Some(old),
                "Boss session changed during rotation"
            );
            state.session_id = Some(next);
            Ok(())
        })
    }

    pub fn add_employee(&self, employee: BossEmployee) -> anyhow::Result<()> {
        self.update(|state| {
            state.employees.push(employee);
            Ok(())
        })
    }

    pub fn add_plan(&self, plan: BossPlan) -> anyhow::Result<()> {
        self.update(|state| {
            state.planning.push(plan);
            Ok(())
        })
    }

    pub fn set_employee_expired(&self, session_id: Uuid, expired: bool) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(employee) = state
                .employees
                .iter_mut()
                .find(|employee| employee.session_id == session_id)
            {
                employee.expired = expired;
            }
            Ok(())
        })
    }

    pub fn set_employee_expired_at(&self, session_id: Uuid, at: u64) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(employee) = state
                .employees
                .iter_mut()
                .find(|employee| employee.session_id == session_id)
            {
                employee.expired = true;
                employee.expired_at = Some(at);
            }
            Ok(())
        })
    }

    pub fn set_employee_goal(&self, session_id: Uuid, goal: EmployeeGoal) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(employee) = state
                .employees
                .iter_mut()
                .find(|employee| employee.session_id == session_id)
            {
                employee.work_goal = goal;
            }
            Ok(())
        })
    }

    pub fn set_plan_finalized_at(&self, session_id: Uuid, at: u64) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(plan) = state
                .planning
                .iter_mut()
                .find(|plan| plan.session_id == session_id)
            {
                apply_plan_approval(plan, None, at)?;
            }
            Ok(())
        })
    }

    pub fn clear_employee_prompt_queue(&self, session_id: Uuid) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(ticket) = state
                .employees
                .iter_mut()
                .find(|employee| employee.session_id == session_id)
                .and_then(|employee| employee.ticket.as_mut())
            {
                ticket.prompt.clear();
                ticket.pending_prompts.clear();
            }
            Ok(())
        })
    }

    pub fn set_employee_blocker_if_empty(
        &self,
        session_id: Uuid,
        blocker: String,
    ) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(employee) = state
                .employees
                .iter_mut()
                .find(|employee| employee.session_id == session_id)
                && employee.blocker.is_none()
            {
                employee.blocker = Some(blocker);
            }
            Ok(())
        })
    }

    pub fn set_employee_blocker(&self, session_id: Uuid, blocker: String) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(employee) = state
                .employees
                .iter_mut()
                .find(|employee| employee.session_id == session_id)
            {
                employee.blocker = Some(blocker);
            }
            Ok(())
        })
    }

    /// Drop a flagged blocker once fresh direction reaches the employee —
    /// a steer delivered mid-turn is the supervisor's answer, and the flag
    /// would otherwise linger until expiry. The flag check keeps a routine
    /// steer from paying `update`'s clone-and-diff.
    pub fn clear_employee_blocker(&self, session_id: Uuid) -> anyhow::Result<()> {
        let flagged = self.state.lock().employees.iter().any(|employee| {
            employee.session_id == session_id && employee.blocker.is_some()
        });
        if !flagged {
            return Ok(());
        }
        self.update(|state| {
            if let Some(employee) = state
                .employees
                .iter_mut()
                .find(|employee| employee.session_id == session_id)
            {
                employee.blocker = None;
            }
            Ok(())
        })
    }

    /// Plan documents moved out of `memory/` to `plans/` at the files
    /// root — they are work product, not memory. Move a legacy
    /// `memory/plans/` wholesale when `plans/` is absent, else merge the
    /// entries the new root lacks and leave name conflicts behind.
    fn migrate_plan_documents(&self) {
        let (Ok(legacy), Ok(plans)) = (
            self.file_path("memory/plans", false),
            self.file_path("plans", false),
        ) else {
            return;
        };
        if !legacy.is_dir() {
            return;
        }
        if !plans.exists() {
            let _ = fs::rename(&legacy, &plans);
            return;
        }
        move_missing_entries(&legacy, &plans);
        let _ = fs::remove_dir(&legacy);
    }

    pub fn set_task_notifier(&self, notifier: crate::TaskNotifier) {
        *self.notifier.lock() = Some(notifier);
    }

    /// Install the daemon callback without retaining the daemon itself.
    pub fn set_finish_employee(&self, finish: crate::FinishEmployee) {
        *self.finish_employee.lock() = Some(finish);
    }

    pub fn set_recover_employee(&self, recover: crate::RecoverEmployee) {
        *self.recover_employee.lock() = Some(recover);
    }

    pub fn set_session_active(&self, is_active: crate::SessionActive) {
        *self.session_active.lock() = Some(is_active);
    }

    pub fn set_session_busy(&self, is_busy: crate::SessionBusy) {
        *self.session_busy.lock() = Some(is_busy);
    }

    pub fn set_archive_sessions(&self, archive: crate::ArchiveSessions) {
        *self.archive_sessions.lock() = Some(archive);
    }

    /// Install the daemon's registered-project lookup without retaining the
    /// daemon itself. Used to resolve `--project` references in memory
    /// operations; absent, only an employee's own project resolves.
    pub fn set_project_catalog(&self, catalog: crate::ProjectCatalog) {
        *self.project_catalog.lock() = Some(catalog);
    }

    /// Serialize Boss chat prompts and cold starts with session publication.
    pub fn operation_guard(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.operation_lock.lock()
    }

    /// Serialize a host-side operation with Boss mutations.
    pub fn with_operation_lock<R>(&self, operation: impl FnOnce() -> R) -> R {
        let _guard = self.operation_lock.lock();
        operation()
    }

    pub fn is_boss(&self, session: Uuid) -> bool {
        self.state.lock().session_id == Some(session)
    }

    /// The boss chat itself — a planning session is the same principal
    /// working in a dedicated session, so owner-level operations and the
    /// boss persona apply to both.
    pub fn is_boss_principal(&self, session: Uuid) -> bool {
        self.is_boss(session) || self.is_planning(session)
    }

    pub fn is_employee(&self, session: Uuid) -> bool {
        self.state
            .lock()
            .employees
            .iter()
            .any(|employee| employee.session_id == session)
    }

    /// A planning session the boss opened — registered whether or not its
    /// plan has been finalized.
    pub fn is_planning(&self, session: Uuid) -> bool {
        self.state
            .lock()
            .planning
            .iter()
            .any(|plan| plan.session_id == session)
    }

    pub fn is_managed(&self, session: Uuid) -> bool {
        self.is_boss(session) || self.is_employee(session) || self.is_planning(session)
    }

    /// The planning record for a session, or for a plan file when the
    /// caller names the files-root-relative `plans/<name>.md`.
    pub fn plan(&self, session: Uuid) -> Option<BossPlan> {
        self.state
            .lock()
            .planning
            .iter()
            .find(|plan| plan.session_id == session)
            .cloned()
    }

    pub fn plan_for_file(&self, plan_file: &str) -> Option<BossPlan> {
        self.state
            .lock()
            .planning
            .iter()
            .find(|plan| plan.plan_file == plan_file)
            .cloned()
    }

    /// Freeze the named plan at `now`. Callers validate the record exists
    /// and is not already frozen; the stamp is monotonic. `items` seeds
    /// the work breakdown from the approved document's course of work —
    /// the declared list replaces whatever a session declared early, and
    /// the approval lands on the audit trail as the user's act.
    pub fn finalize_plan(
        &self,
        plan_file: &str,
        items: Option<Vec<String>>,
        now: u64,
    ) -> anyhow::Result<BossPlan> {
        let mut finalized = None;
        self.update(|state| {
            let plan = state
                .planning
                .iter_mut()
                .find(|plan| plan.plan_file == plan_file)
                .ok_or_else(|| anyhow!("unknown plan {plan_file}"))?;
            if plan.finalized_at.is_none() {
                apply_plan_approval(plan, items, now)?;
            }
            finalized = Some(plan.clone());
            Ok(())
        })?;
        Ok(finalized.unwrap())
    }

    /// Resolve a summon or `setPlan` `plan`+`item` tag to the link ids —
    /// or the reason the tag is invalid. Unknown plans and plans with a
    /// closed outcome fail, as do unknown items and items already done
    /// or dropped; a still-open draft plan tags fine.
    pub fn plan_assignment(
        &self,
        plan: &str,
        item: Option<Uuid>,
    ) -> anyhow::Result<(Uuid, Option<Uuid>)> {
        resolve_plan_assignment(&self.state.lock(), plan, item)
    }

    /// Re-tag an employee's plan and item links — the mutable half of the
    /// summon tag, applied through `control`'s `setPlan`. Each field is
    /// tri-state: `None` keeps the current link, `Some(None)` clears it,
    /// `Some(Some(_))` re-tags with the same validation a summon tag
    /// gets. Re-tagging the plan without restating the item drops the
    /// employee to the plan's unallocated list.
    pub fn set_employee_plan(
        &self,
        session_id: Uuid,
        plan: Option<Option<String>>,
        item: Option<Option<Uuid>>,
    ) -> anyhow::Result<()> {
        self.update(|state| {
            let plan_id = match &plan {
                Some(Some(reference)) => {
                    Some(Some(resolve_plan_assignment(state, reference, None)?.0))
                }
                Some(None) => Some(None),
                None => None,
            };
            let current = state
                .employees
                .iter()
                .chain(state.retired_employees.iter())
                .find(|entry| entry.session_id == session_id)
                .cloned()
                .ok_or_else(|| anyhow!("not a Boss employee"))?;
            let effective_plan = plan_id.unwrap_or(current.plan_id);
            let item_id = match item {
                Some(Some(item)) => {
                    let plan = effective_plan.ok_or_else(|| {
                        anyhow!("setPlan needs a plan before it can link an item")
                    })?;
                    resolve_plan_assignment(state, &plan.to_string(), Some(item))?.1
                }
                Some(None) => None,
                // A re-tag without a restated item clears the item link;
                // an untouched plan keeps whatever item it had.
                None if plan_id.is_some() => None,
                None => current.item_id,
            };
            let employee = state
                .employees
                .iter_mut()
                .chain(state.retired_employees.iter_mut())
                .find(|entry| entry.session_id == session_id)
                .expect("employee presence checked above");
            employee.plan_id = effective_plan;
            employee.item_id = item_id;
            Ok(())
        })
    }

    /// `updatePlanItems` — replace a plan's ordered work breakdown. An
    /// entry naming an existing item id renames and repositions it; an
    /// entry without one appends a new `toDo` item; an item the list
    /// omits is marked dropped — struck, audited, reopenable — rather
    /// than deleted, keeping "the plan said X, where did X go" answerable.
    pub fn update_plan_items(
        &self,
        caller: Option<Uuid>,
        reference: &str,
        items: Vec<PlanItemInput>,
    ) -> anyhow::Result<BossPlan> {
        self.require_owner(caller)?;
        let actor = self.plan_actor(caller);
        let now = waku_protocol::model::unix_time();
        let mut updated = None;
        self.update(|state| {
            let plan = plan_mut(state, reference)?;
            if plan.terminal() {
                bail!(
                    "plan {} is {}; reopen it to reshape its work",
                    plan.plan_file,
                    plan.outcome().label()
                );
            }
            let old = std::mem::take(&mut plan.items);
            let mut next = Vec::with_capacity(items.len());
            for input in &items {
                let title = input.title.trim();
                anyhow::ensure!(!title.is_empty(), "work item titles cannot be empty");
                match input.id {
                    Some(id) => {
                        let existing = old
                            .iter()
                            .find(|item| item.id == id)
                            .ok_or_else(|| anyhow!("unknown work item {id}"))?;
                        anyhow::ensure!(
                            !next.iter().any(|item: &PlanItem| item.id == id),
                            "work item {id} is listed twice"
                        );
                        next.push(PlanItem {
                            title: title.to_owned(),
                            ..existing.clone()
                        });
                    }
                    None => next.push(PlanItem {
                        id: Uuid::new_v4(),
                        title: title.to_owned(),
                        state: PlanItemState::ToDo,
                        history: Vec::new(),
                    }),
                }
            }
            let kept: std::collections::HashSet<Uuid> = next.iter().map(|item| item.id).collect();
            for mut item in old {
                if kept.contains(&item.id) {
                    continue;
                }
                if item.state != PlanItemState::Dropped {
                    item.state = PlanItemState::Dropped;
                    item.history.push(PlanItemTransition {
                        state: PlanItemState::Dropped,
                        at: now,
                        actor,
                    });
                }
                next.push(item);
            }
            plan.items = next;
            updated = Some(plan.clone());
            Ok(())
        })?;
        Ok(updated.unwrap())
    }

    /// `setPlanItemState` — check an item off, drop it, or reopen it to
    /// `toDo`. Each change lands on the item's audit trail.
    pub fn set_plan_item_state(
        &self,
        caller: Option<Uuid>,
        reference: &str,
        item: Uuid,
        item_state: PlanItemState,
    ) -> anyhow::Result<BossPlan> {
        self.require_owner(caller)?;
        let actor = self.plan_actor(caller);
        let now = waku_protocol::model::unix_time();
        let mut updated = None;
        self.update(|state| {
            let plan = plan_mut(state, reference)?;
            if plan.terminal() {
                bail!(
                    "plan {} is {}; reopen it to reshape its work",
                    plan.plan_file,
                    plan.outcome().label()
                );
            }
            let entry = plan
                .items
                .iter_mut()
                .find(|known| known.id == item)
                .ok_or_else(|| anyhow!("unknown work item {item}"))?;
            if entry.state != item_state {
                entry.state = item_state;
                entry.history.push(PlanItemTransition {
                    state: item_state,
                    at: now,
                    actor,
                });
            }
            updated = Some(plan.clone());
            Ok(())
        })?;
        Ok(updated.unwrap())
    }

    /// `setPlanOutcome` — the audited outcome lifecycle: `approved`
    /// plans close to `completed`/`abandoned`, and a closed plan reopens
    /// to `approved`. A draft has no outcome to set.
    pub fn set_plan_outcome(
        &self,
        caller: Option<Uuid>,
        reference: &str,
        outcome: PlanOutcome,
    ) -> anyhow::Result<BossPlan> {
        self.require_owner(caller)?;
        let actor = self.plan_actor(caller);
        let now = waku_protocol::model::unix_time();
        let mut updated = None;
        self.update(|state| {
            let plan = plan_mut(state, reference)?;
            if plan.finalized_at.is_none() {
                bail!(
                    "plan {} is still in planning; it has no outcome to set",
                    plan.plan_file
                );
            }
            let current = plan.outcome();
            if current == outcome {
                bail!("plan {} is already {}", plan.plan_file, outcome.label());
            }
            let allowed = matches!(
                (current, outcome),
                (PlanOutcome::Approved, PlanOutcome::Completed | PlanOutcome::Abandoned)
                    | (
                        PlanOutcome::Completed | PlanOutcome::Abandoned,
                        PlanOutcome::Approved,
                    )
            );
            anyhow::ensure!(
                allowed,
                "plan {} is {}; reopen it before setting another outcome",
                plan.plan_file,
                current.label()
            );
            plan.outcome = Some(outcome);
            plan.history.push(PlanTransition {
                outcome,
                at: now,
                actor,
            });
            updated = Some(plan.clone());
            Ok(())
        })?;
        Ok(updated.unwrap())
    }

    /// Whether `path` — a files-root-relative Boss path — names a finalized
    /// plan document. Frozen plans reject writes; reads stay open.
    fn plan_file_frozen(&self, path: &str) -> bool {
        let normalized = normalize_plan_path(path);
        self.state
            .lock()
            .planning
            .iter()
            .any(|plan| plan.plan_file == normalized && plan.finalized_at.is_some())
    }

    /// Archive planning sessions whose post-finalization grace period has
    /// elapsed. Runs beside the hourly employee-retirement sweep: the grace
    /// window is the boss's last chance to converse in the planning context
    /// — the design's author can still answer questions — then the session
    /// becomes a plain archived managed task.
    pub fn archive_graced_plans(&self, now: u64) -> anyhow::Result<usize> {
        let due: Vec<Uuid> = self
            .state
            .lock()
            .planning
            .iter()
            .filter(|plan| {
                plan.finalized_at
                    .is_some_and(|at| at.saturating_add(PLANNING_GRACE_SECONDS) <= now)
            })
            .map(|plan| plan.session_id)
            .collect();
        if due.is_empty() {
            return Ok(0);
        }
        let Some(archive) = self.archive_sessions.lock().clone() else {
            return Ok(0);
        };
        if !archive(&due)? {
            return Ok(0);
        }
        Ok(due.len())
    }

    pub fn employee(&self, session: Uuid) -> Option<BossEmployee> {
        self.state
            .lock()
            .employees
            .iter()
            .find(|entry| entry.session_id == session)
            .cloned()
    }

    /// The record wherever it rests — the live roster or retirement.
    /// Reads that must see finished employees (worktree ownership) use
    /// this; live-only checks stay on `employee`.
    pub fn employee_including_retired(&self, session: Uuid) -> Option<BossEmployee> {
        let state = self.state.lock();
        state
            .employees
            .iter()
            .chain(state.retired_employees.iter())
            .find(|entry| entry.session_id == session)
            .cloned()
    }

    pub fn require_active(&self, session: Uuid) -> anyhow::Result<()> {
        if self.interrupted.lock().contains(&session) {
            bail!("employee was interrupted by daemon restart; summon a new employee");
        }
        match self.employee(session).map(|entry| entry.lifecycle()) {
            Some(EmployeeLifecycle::Finishing) => {
                bail!("employee is finishing; try again or summon a fresh employee")
            }
            Some(EmployeeLifecycle::Expired) => {
                bail!(
                    "employee has expired; prompt or steer can resume it, or summon a new employee"
                );
            }
            _ => Ok(()),
        }
    }

    pub fn require_control(&self, caller: Option<Uuid>, target: Uuid) -> anyhow::Result<()> {
        let employee = self
            .employee(target)
            .or_else(|| {
                self.state
                    .lock()
                    .retired_employees
                    .iter()
                    .find(|entry| entry.session_id == target)
                    .cloned()
            })
            .ok_or_else(|| anyhow!("not a Boss employee"))?;
        if caller.is_some_and(|caller| {
            !self.is_boss_principal(caller) && employee.supervisor_id != caller
        }) {
            bail!("only the boss or this employee's supervisor can control it");
        }
        if let Some(caller) = caller {
            self.require_active(caller)?;
        }
        Ok(())
    }

    pub fn authorize_transcript(&self, caller: Option<Uuid>, target: Uuid) -> anyhow::Result<()> {
        let Some(caller) = caller else {
            return Ok(());
        };
        if caller == target || self.is_boss_principal(caller) {
            return Ok(());
        }
        let state = self.document();
        let mut current = target;
        for _ in 0..state.employees.len() {
            let Some(employee) = state
                .employees
                .iter()
                .find(|entry| entry.session_id == current)
            else {
                break;
            };
            if employee.supervisor_id == caller {
                return Ok(());
            }
            current = employee.supervisor_id;
        }
        bail!("persona does not grant access to this transcript")
    }

    pub fn prepare_employee(
        &self,
        caller: Uuid,
        persona_id: Uuid,
        job_title: String,
        overrides: Option<PermissionOverrides>,
        work_goal: EmployeeGoal,
        icon: Option<waku_protocol::custom_commands::CustomCommandIcon>,
    ) -> anyhow::Result<BossEmployee> {
        validate_name(&job_title)?;
        let state = self.document();
        let persona = state
            .personas
            .iter()
            .find(|persona| persona.id == persona_id)
            .ok_or_else(|| anyhow!("unknown persona"))?;
        self.validate_bucket_ids(&persona.permissions.bucket_ids)?;
        // Summon overrides the persona default; either explicit choice is
        // copied onto the record — a snapshot the client renders directly,
        // untouched by later persona edits. `None` leaves the title
        // heuristic in charge.
        let icon = icon.or(persona.icon);
        if icon.is_some_and(|icon| !icon.is_employee_icon()) {
            bail!("icon is not in the employee icon set");
        }
        let mut permissions = persona.permissions.clone();
        let mut pinned_files = persona.pinned_files.clone();
        if let Some(overrides) = &overrides {
            self.validate_bucket_ids(overrides.bucket_ids.as_deref().unwrap_or(&[]))?;
            overrides.apply_to(&mut permissions);
        }
        if self.is_planning(caller)
            && !self
                .session_active
                .lock()
                .clone()
                .map(|is_active| is_active(caller))
                .unwrap_or(true)
        {
            // Grace is over — an archived planning session is a plain
            // managed task and cannot summon any more.
            bail!("this planning session is archived");
        }
        if !self.is_boss_principal(caller) {
            let parent = state
                .employees
                .iter()
                .find(|entry| entry.session_id == caller)
                .ok_or_else(|| {
                    anyhow!("only the boss or a permitted employee can summon employees")
                })?;
            if parent.expired || !parent.permissions.summon_employees {
                bail!("this persona cannot summon employees");
            }
            if parent.persona_id != persona_id {
                bail!("employees inherit their supervisor's boss-assigned persona");
            }
            permissions.clamp_within(&parent.permissions);
            pinned_files.retain(|path| self.authorize_file(Some(caller), path, false).is_ok());
        }
        let id = Uuid::new_v4();
        let mut name = String::new();
        self.update(|state| {
            name = employee_human_name(
                state
                    .employees
                    .iter()
                    .map(|employee| employee.identity.name.as_str()),
                &mut state.name_cursor,
            );
            Ok(())
        })?;
        Ok(BossEmployee {
            session_id: id,
            supervisor_id: caller,
            identity: BossIdentity {
                id,
                name,
                avatar_seed: id.to_string(),
            },
            job_title: job_title.trim().to_owned(),
            persona_id,
            work_goal,
            created_at: Some(waku_protocol::model::unix_time()),
            icon,
            permissions,
            pinned_files,
            expired: false,
            expired_at: None,
            blocker: None,
            cancelled: false,
            expiry: None,
            state: EmployeeLifecycle::Queued,
            ticket: None,
            queued_at: None,
            request_id: None,
            request_fingerprint: None,
            plan_id: None,
            item_id: None,
        })
    }

    /// Apply per-field grant overrides to an employee's persisted record —
    /// each `Some` replaces that grant. An employee supervisor's edits stay
    /// clamped to its own permissions; the boss and human callers are not.
    /// Memory and delegation grants take effect immediately, while MCP
    /// server and Computer Use changes wait for the employee's next launch.
    pub fn set_employee_permissions(
        &self,
        caller: Option<Uuid>,
        session_id: Uuid,
        overrides: PermissionOverrides,
    ) -> anyhow::Result<()> {
        self.validate_bucket_ids(overrides.bucket_ids.as_deref().unwrap_or(&[]))?;
        let ceiling = match caller {
            Some(caller) if !self.is_boss_principal(caller) => Some(
                self.employee(caller)
                    .ok_or_else(|| anyhow!("this task is not a Boss employee"))?
                    .permissions,
            ),
            _ => None,
        };
        self.update(|state| {
            let employee = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session_id)
                .ok_or_else(|| anyhow!("unknown employee"))?;
            overrides.apply_to(&mut employee.permissions);
            if let Some(ceiling) = &ceiling {
                employee.permissions.clamp_within(ceiling);
            }
            Ok(())
        })
    }

    /// Retitle an employee's persisted record — a `steer` that redirects
    /// the assignment carries the new label. Bookkeeping only: the change
    /// queues no prompt, wakes nothing, and writes no transcript entry.
    /// Retired records update too so a resurrecting steer still relabels
    /// the job it revives.
    pub fn set_employee_job_title(&self, session_id: Uuid, job_title: &str) -> anyhow::Result<()> {
        validate_name(job_title)?;
        self.update(|state| {
            let employee = state
                .employees
                .iter_mut()
                .chain(state.retired_employees.iter_mut())
                .find(|entry| entry.session_id == session_id)
                .ok_or_else(|| anyhow!("not a Boss employee"))?;
            employee.job_title = job_title.trim().to_owned();
            Ok(())
        })
    }

    fn validate_bucket_ids(&self, buckets: &[String]) -> anyhow::Result<()> {
        let known =
            waku_memory_engine::buckets::BucketStore::open(self.root.join("files/memory-engine"))?
                .list_buckets()?
                .into_iter()
                .map(|bucket| bucket.id)
                .collect::<std::collections::HashSet<_>>();
        for bucket in buckets {
            anyhow::ensure!(known.contains(bucket), "unknown memory bucket {bucket}");
        }
        Ok(())
    }

    pub fn owned_workspace(&self) -> anyhow::Result<PathBuf> {
        let path = self.root.join("workspace");
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn workspace(&self, session: Uuid) -> anyhow::Result<PathBuf> {
        let path = if self.is_boss_principal(session) {
            return self.owned_workspace();
        } else {
            self.root.join("workspaces").join(session.to_string())
        };
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn set_project_context(&self, session: Uuid, path: PathBuf) {
        self.projects.lock().insert(session, path);
    }

    pub fn reset_context(&self, session: Uuid) {
        self.injected.lock().remove(&session);
    }

    /// The session-scoped router slot — a different boss session resets it
    /// so a replaced chat never inherits its predecessor's focus.
    fn router_entry(&self, session: Uuid) -> parking_lot::MutexGuard<'_, BossRouter> {
        let mut router = self.router.lock();
        if router.session != Some(session) {
            *router = BossRouter {
                session: Some(session),
                ..Default::default()
            };
        }
        router
    }

    /// Remember a user prompt for the focus question's continuity.
    pub fn router_note_prompt(&self, session: Uuid, prompt: &str) {
        let mut router = self.router_entry(session);
        router
            .recent_prompts
            .push_back(waku_protocol::model::truncate_chars(
                prompt,
                ROUTER_PROMPT_CAP,
            ));
        while router.recent_prompts.len() > ROUTER_RECENT_PROMPTS {
            router.recent_prompts.pop_front();
        }
    }

    /// The focus inference plus the recent prompts the next evaluation
    /// judges against.
    pub fn router_snapshot(&self, session: Uuid) -> (Option<String>, Vec<String>) {
        let router = self.router_entry(session);
        (
            router.focus.clone(),
            router.recent_prompts.iter().cloned().collect(),
        )
    }

    /// Store a focus transition the verdict cleared — `None` means Jev
    /// decided the user's attention sits on no particular project.
    pub fn router_set_focus(&self, session: Uuid, focus: Option<String>) {
        self.router_entry(session).focus = focus;
    }

    /// Defer an attachment the just-prompted turn could not take; the next
    /// outbound boss prompt prepends the digest instead.
    pub fn router_defer_context(&self, session: Uuid) {
        self.router_entry(session).pending_context = true;
    }

    /// Consume the deferred-attachment flag — one prompt carries it, never
    /// more.
    pub fn router_take_pending(&self, session: Uuid) -> bool {
        std::mem::take(&mut self.router_entry(session).pending_context)
    }

    pub fn prompt_with_context(&self, session: Uuid, prompt: String) -> String {
        if !self.is_managed(session) || !self.injected.lock().insert(session) {
            return prompt;
        }
        let state = self.document();
        let employee = state
            .employees
            .iter()
            .find(|entry| entry.session_id == session);
        let persona_id = employee
            .map(|entry| entry.persona_id)
            .unwrap_or(state.persona_id);
        let Some(persona) = state.personas.iter().find(|entry| entry.id == persona_id) else {
            return prompt;
        };
        let role = if let Some(employee) = employee {
            // The kind the summon fixed decides what the finish does —
            // an errand reports to its supervisor while a goal lands on
            // the human's Goals page without prompting anyone.
            let finish = match employee.work_goal {
                EmployeeGoal::Errand => {
                    "Finishing delivers your transcript index to your supervisor — keep your final turns self-contained since that is what it reads — and do not expect a reply."
                }
                EmployeeGoal::Goal => {
                    "Finishing is silent — your outcome lands on the human's Goals page without prompting anyone, so do not expect a reply."
                }
            };
            format!(
                "You are employee {}, whose job title is {}. Your supervisor is task {}. Use named memory buckets explicitly through `goddard-agent boss memory`; omit the bucket to use your assigned project bucket. You can read and record only in buckets granted to you and your assigned project bucket. Bucket access does not preload its contents. Use `goddard-agent boss` to read pinned documents and retrieve employee transcripts. Native subagents are not Boss employees: delegate only with the Boss summon operation, and only when permitted. Your grants are {}. Pinned documents: {}. Finish this bounded job, return your results, and expire. {finish} When something genuinely needs your supervisor's attention — you are blocked, a decision is required, or the job failed — flag it with `goddard-agent boss '{{\"type\":\"reportBlocker\",\"message\":\"what needs attention\"}}'`: the report interrupts your supervisor's running work when it can and makes your finish deliver a full report instead of expiring silently. Do not flag routine completions. Files you produce inside your workspace can be published to the human's sidebar with `goddard-agent boss deliverable publish ABSOLUTE_PATH` — the daemon stores a copy, so the entry survives your workspace's cleanup.",
                employee.identity.name,
                employee.job_title,
                employee.supervisor_id,
                serde_json::to_string(&employee.permissions).unwrap_or_default(),
                pinned_paths(employee.pinned_files.as_slice())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            format!(
                "You are {}, the boss for this daemon. Heavy delegation is your default: promptly assign execution to employees so you stay free for the human. Delegate code changes, research, internet access, builds, code generation, long-running checks and tests, and, for daemon-managed worktrees, require each assigned employee to include 'agent-merge per rules', commit its unit, and run 'goddard-agent merge submit' itself; do not summon a separate Worktree Integrator. The employee reports conflicts or verification failures with 'reportBlocker' and reports the landed SHA on success. Never run or poll long-running commands yourself; assign them to an employee, including any wait or follow-up check. For builds and code generation, ask employees to use the repository's shared build cache or a dedicated output directory when that avoids contention with the user's tools. You control personas and all employees.\n\nDispatch speed: when the human hands you a task, summon promptly — seconds, not minutes. Do not research the codebase before summoning. The only pre-summon research allowed is identifying which project the task belongs to when that is genuinely ambiguous. Write a competent brief and let the employee locate files, verify line numbers, and orient itself — that is what employees are for.\n\nEmployees assigned to a project automatically receive read and insert access to its shared project bucket. Grant additional Boss-created buckets deliberately through persona bucket IDs or per-employee permission overrides. Personal buckets remain Boss-only unless granted. Pinned files are documents only; they do not grant memory access; a per-field `permissions` object on summon — or `setPermissions` via control — tailors one employee's grants without editing the persona. Choose a purpose-specific jobTitle when summoning each employee; Goddard assigns their human name. Choose the employee's `icon` deliberately from the summon field — pick the icon that fits the actual work rather than leaving it to the job-title heuristic, and give jobs accurate titles since titles feed the fallback classifier. Summon accepts `workspace: \"worktree\"` and `baseBranch` to run an employee in a daemon-managed Git worktree rather than the primary checkout, or `workspace: \"adopt\"` with `adoptWorktree` to adopt a finished employee's worktree and continue its work, an optional `reasoningEffort` to pin the employee's effort — the id must be one the resolved model supports or the summon fails — and `workGoal` to fix how its finish lands. `control` with `setWorkspace` moves a live employee between the primary checkout and a fresh worktree as one action — it stops the current turn, rebinds the workspace, and resumes the same transcript, and a failure leaves the employee running in its old workspace. Declare an employee's host-resource needs at summon with `resources` — `{{\"native_builds\": 1}}` for a device build, `exclusive` `ios:<UDID>`/`android:<AVD>` names plus `resident_devices`, `desktop_input` for shared input; the employee's own `resource run` calls borrow subsets of the granted set, a contested set queues the ticket instead of erroring, and the broker never steals devices the user claimed. `control` with `setResources` changes a live employee's set — a queued ticket re-enters admission on it and a running employee swaps once capacity frees without interrupting its turn. Never create Git worktrees yourself — summon `workspace`/`baseBranch` and `setWorkspace` cover employee worktree needs, and manual `git worktree` commands are for landing worktrees only when unavoidable. Build a reusable persona library across projects: when work patterns recur, create a named purpose-specific persona such as Researcher, Feature Developer, Bug Investigator, or Verifier, with instructions useful beyond the current project. Before creating one, inspect existing personas and refine a close match rather than making duplicates; update personas as repeated work reveals better responsibilities or boundaries. Use the generic Employee persona only for work that does not fit a reusable role. Keep persona instructions focused on a role's durable methods and limits, not one task's details. Your dedicated tools are `goddard-agent boss` operations: view, summon, control, transcript, context, automation, upsertPersona, listFiles, readFile, writeFile, createFolder, rename, publishDeliverable, dismissDeliverable, speak, browse, eval, createPlan, finalizePlan, terminal. `terminal(title, cwd[, command])` creates a pinned standalone terminal in the desktop app; choose an existing directory and use it only for the boss or a planning session, never an employee. `automation` lists, creates, updates, deletes, pauses, and resumes user automations; employees cannot use it. `eval` runs a Rhai script inside the daemon with the other operations bound as functions — batch related operations into one call and chain their results; variables persist between evals, and `help()` inside a script lists the bindings. `context` returns a snapshot of the human's projects, tasks, and automations — check it whenever a message concerns their work and no snapshot was already attached. `search` scans every project's task transcripts for you, not just your own project — `project:` narrows to one — and `read` opens any task it surfaces. `browse(url[, title])` opens an http(s) page in the boss chat’s right panel for the user. `speak` voices an utterance through connected clients when their voice feature is on — split it into reusable fragments (proper nouns alone, stock phrases whole) so generated clips are reused and later utterances stay instant. These operations authorize routine delegation without asking the human to approve each employee. Use `goddard-agent schema` for their payloads. Your persona is {}. You can access every named memory bucket, but their contents are never loaded automatically. Choose the relevant bucket — a project's name resolves to its shared bucket — and use its explicit overview, zoom, scan, record, and summary operations. Record concise, useful notes directly in the appropriate bucket; notes survive sessions, employees, and model changes. Project-assigned employees have automatic read and insert access to that project's shared bucket. Personal buckets remain private unless you grant them deliberately. Track active work durably: record which employee owns each worktree, what is in flight, and what has landed, then reconcile those notes as work changes. Verify completion from the worktree and its commits before reporting a task done; an employee's summary alone is not proof that work was committed. Queued prompts can be lost when an employee is finishing, so summon a fresh employee for new follow-up work instead of stacking prompts onto someone about to expire. Publish useful employee outputs with deliverables so the human can find them later, and use speak when a concise interruption is timely. Respect user-set resource rules, including model routing and employee caps, and record durable constraints in memory so delegation stays within them. Your persistent files root is {}. Broader filesystem editing and internet access are discouraged, not forbidden. Never wait, watch, or poll yourself — no transcript read loops, no sleep-and-recheck cycles, no blocking resource waits: when a job needs a wait, such as watching a task, an employee finishing, or a condition to keep rechecking, summon an employee to do the watching and report, then return to the human. Mark every summon `workGoal`: an `errand` reports its finish to you — choose it when you need the completion to continue the work; a `goal` finishes without you — choose it for fire-and-forget work, which lands on the human's Goals page instead. Goal finishes are silent — no prompt arrives — so read outcomes lazily from `view` or `context`; a finish also reaches you when the employee flagged a blocker through its `reportBlocker` operation, its persona grants `alwaysReport`, or its session failed. A blocker report also interrupts your running turn when it can. `createPlan` opens a design session that drafts a product design for the human's approval — it launches on codex/gpt-6.1-sol at medium effort unless you pass provider/model/reasoningEffort overrides — when the plan finalizes, the approved design is reported to your chat and you coordinate its implementation from there; a finalized planning session answers questions about its design but does not implement. There are no managers.",
                state.identity.name,
                state.persona_id,
                self.root.join("files").display()
            )
        };
        let project_context = self.projects.lock().get(&session)
            .map(|path| format!("Project context: {}. For a job in another project, supply its absolute path when summoning. If this path is unavailable inside a sandbox, the guest's current working directory is the assigned project.", path.display()))
            .unwrap_or_default();
        let planning = state
            .planning
            .iter()
            .find(|plan| plan.session_id == session)
            .map(|plan| {
                if plan.finalized_at.is_some() {
                    format!("\nThis planning session's design doc {} is finalized and frozen — the document can no longer be edited. Implementation is the boss's job now: the approval was reported to the boss chat, which coordinates the work from there. Your remaining role is answering questions about the approved design — direct implementation requests back to the boss chat.", plan.plan_file)
                } else {
                    format!("\nThis is a planning session for \"{}\", and your role is product designer. Draft and revise the design doc at {} with `writeFile`: cover the user experience, flows, behaviors, edge cases, tradeoffs, and decisions with their rationale — implementation details like file paths and code structure are out of scope; the employees who implement it decide the technical how. While drafting you may summon employees for design research and audits, but never to build. When the design is ready for the user's approval call `finalizePlan` — the user reviews it before it freezes. Approval ends your design work: the finalized doc is reported to the boss chat and the boss coordinates implementation from there.", plan.idea, plan.plan_file)
                }
            })
            .unwrap_or_default();
        format!(
            "<boss-persona>\n{}\n\n{}{}\nPinned documents: {}. Read them selectively through the Boss readFile operation.\n{project_context}\n</boss-persona>\n\n{prompt}",
            persona.markdown,
            role,
            planning,
            pinned_paths(
                employee
                    .map(|entry| entry.pinned_files.as_slice())
                    .unwrap_or(&persona.pinned_files),
            )
            .collect::<Vec<_>>()
            .join(", ")
        )
    }

    pub fn recover_interrupted(&self) {
        let entries: Vec<BossEmployee> = {
            let ids = self.interrupted.lock().clone();
            let state = self.state.lock();
            state
                .employees
                .iter()
                .filter(|entry| ids.contains(&entry.session_id))
                .cloned()
                .collect()
        };
        if entries.is_empty() {
            return;
        }
        if let Some(recover) = self.recover_employee.lock().clone() {
            let _ = std::thread::Builder::new()
                .name("boss-recover-employees".into())
                .spawn(move || {
                    for entry in entries {
                        if let Err(error) = recover(entry.session_id) {
                            eprintln!(
                                "could not recover interrupted employee {}: {error:#}",
                                entry.session_id
                            );
                        }
                    }
                });
        }
    }

    /// A provider settle ended the employee's turn — the forwarder
    /// reports how it ended so the finish can classify the expiry
    /// legibly (`EmployeeSettle` → `ExpiryCause`).
    pub fn note_settled(&self, session: Uuid, settle: EmployeeSettle) {
        if !self.employee(session).is_some_and(|entry| !entry.expired) {
            return;
        }
        if let Some(finish) = self.finish_employee.lock().clone() {
            // Never join or shut down a driver from its own forwarder.
            let _ = std::thread::Builder::new()
                .name("boss-employee-finish".into())
                .spawn(move || {
                    if let Err(error) = finish(session, settle) {
                        eprintln!("could not settle Boss employee {session}: {error:#}");
                    }
                });
        }
    }

    /// Return an expired employee to duty — the boss may send a finished
    /// employee another prompt rather than summoning a replacement. The
    /// state update notifies clients, so the sidebar's finished list gives
    /// the row back to the working set as soon as a prompt lands.
    /// Non-employees and employees still on the clock return `false`.
    pub fn resurrect(&self, session: Uuid) -> anyhow::Result<bool> {
        let mut revived = false;
        self.update(|state| {
            if !state
                .employees
                .iter()
                .any(|entry| entry.session_id == session)
            {
                if let Some(index) = state
                    .retired_employees
                    .iter()
                    .position(|entry| entry.session_id == session)
                {
                    let mut entry = state.retired_employees[index].clone();
                    if state
                        .employees
                        .iter()
                        .any(|active| active.identity.name == entry.identity.name)
                    {
                        bail!("name has been reused; summon a new employee");
                    }
                    state.retired_employees.remove(index);
                    entry.set_lifecycle(EmployeeLifecycle::Working, 0);
                    entry.expired_at = None;
                    entry.blocker = None;
                    entry.cancelled = false;
                    entry.expiry = None;
                    state.employees.push(entry);
                    wave_member_in_flight(state, session);
                    revived = true;
                    return Ok(());
                }
            }
            if let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session && entry.expired)
            {
                entry.set_lifecycle(EmployeeLifecycle::Working, 0);
                entry.expired_at = None;
                entry.blocker = None;
                entry.cancelled = false;
                entry.expiry = None;
                revived = true;
            }
            if revived {
                wave_member_in_flight(state, session);
            }
            Ok(())
        })?;
        Ok(revived)
    }

    /// Record an employee's attention item. Raising the flag is what makes
    /// the finish report reach the supervisor — the daemon reads `blocker`
    /// back when the employee expires — and it also interrupts a live
    /// supervisor immediately. Only a live employee may flag its own job.
    pub fn report_blocker(&self, caller: Uuid, message: String) -> anyhow::Result<BossEmployee> {
        self.require_active(caller)?;
        let message = message.trim().to_owned();
        if message.is_empty() {
            bail!("a blocker report needs a message");
        }
        if message.chars().count() > MAX_BLOCKER_CHARS {
            bail!("a blocker report exceeds {MAX_BLOCKER_CHARS} characters");
        }
        let mut flagged = None;
        self.update(|state| {
            let employee = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == caller)
                .ok_or_else(|| anyhow!("only a Boss employee can report a blocker"))?;
            employee.blocker = Some(message);
            flagged = Some(employee.clone());
            Ok(())
        })?;
        Ok(flagged.unwrap())
    }

    /// Where an employee's report lands: its live supervisor, escalating to
    /// the boss session when the supervisor cannot take prompts — expired,
    /// retired from the roster, archived after finalization, or never an
    /// employee.
    pub fn report_target(&self, employee: &BossEmployee) -> Option<Uuid> {
        self.report_target_for(employee.supervisor_id)
    }

    /// The supervisor's live session, or the boss session when it cannot
    /// take prompts — the escalation wave-resolution notices share.
    pub fn report_target_for(&self, supervisor: Uuid) -> Option<Uuid> {
        let supervisor_is_planning = self.is_planning(supervisor)
            && self
                .session_active
                .lock()
                .clone()
                .map(|is_active| is_active(supervisor))
                .unwrap_or(true);
        let supervisor_admitted = self.employee(supervisor).is_some_and(|entry| {
            matches!(
                entry.lifecycle(),
                EmployeeLifecycle::Queued
                    | EmployeeLifecycle::Dispatching
                    | EmployeeLifecycle::Working
            )
        });
        if supervisor_admitted || supervisor_is_planning {
            Some(supervisor)
        } else {
            self.document().session_id
        }
    }

    pub fn expire(&self, session: Uuid) -> anyhow::Result<Option<BossEmployee>> {
        let employee =
            self.begin_finishing(session, false, true, ExpiryCause::Finished)?;
        if employee.is_some() {
            self.complete_expiry(session, None)?;
        }
        Ok(employee)
    }

    /// A supervisor stop marked the employee before teardown — the wave
    /// tally counts the member as cancelled rather than finished.
    pub fn mark_cancelled(&self, session: Uuid) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            {
                entry.cancelled = true;
            }
            Ok(())
        })
    }

    /// Start an employee's teardown: persist `finishing` so the model
    /// slot stays held through shutdown and the release can happen
    /// exactly once. Returns the employee only on the winning
    /// transition — a second finish, a queued cancel, or a settled
    /// generation all see `None`. Queued tickets skip `finishing` and
    /// expire directly: they hold no capacity to release.
    ///
    /// `failed` is the session-side terminal verdict the Boss document
    /// cannot see (`SessionStatus::Failed`). The transition is the point
    /// of no return — a `finishing` record always settles into `expired`
    /// through the tail or recovery — so the wave member's outcome lands
    /// in the same durable write, and an all-terminal wave pushes its
    /// completion notice into the outbox here.
    ///
    /// `drain` marks a settle-triggered finish: while the session's
    /// provider turn is still open — a prompt that landed between the
    /// settle event and this pass, or a parked turn waiting on detached
    /// work — the transition yields and that turn's own settle re-drives
    /// the finish, so expiry never cuts an in-flight tool call. Forced
    /// finishes (a supervisor stop, restart recovery, a failed launch)
    /// pass `false` and tear down immediately.
    ///
    /// `cause` is the daemon's settle classification — the record carries
    /// it on `expiry` so an interruption reads legibly, the wave outcome
    /// counts an interrupted settle as failed, and the ticket's
    /// interruption history appends here so a restart recovering a
    /// `finishing` record never double-counts.
    pub fn begin_finishing(
        &self,
        session: Uuid,
        failed: bool,
        drain: bool,
        cause: ExpiryCause,
    ) -> anyhow::Result<Option<BossEmployee>> {
        let now = waku_protocol::model::unix_time();
        let session_busy = self.session_busy.lock().clone();
        let mut employee = None;
        self.update(|state| {
            if let Some(entry) = state.employees.iter_mut().find(|entry| {
                entry.session_id == session
                    && matches!(
                        entry.lifecycle(),
                        EmployeeLifecycle::Queued
                            | EmployeeLifecycle::Dispatching
                            | EmployeeLifecycle::Working
                    )
            }) {
                let lifecycle = entry.lifecycle();
                // A queued ticket never owns an open turn, so the probe is
                // false for it anyway; a ticket-less `queued` record is a
                // plain working employee and drains like one.
                if drain
                    && session_busy
                        .as_ref()
                        .is_some_and(|is_busy| is_busy(session))
                {
                    return Ok(());
                }
                entry.set_lifecycle(
                    if lifecycle == EmployeeLifecycle::Queued {
                        EmployeeLifecycle::Expired
                    } else {
                        EmployeeLifecycle::Finishing
                    },
                    now,
                );
                // A stop the supervisor already marked outranks whatever
                // settle signal arrived — the record stays terminal.
                let cause = if entry.cancelled {
                    ExpiryCause::Stopped
                } else {
                    cause
                };
                entry.expiry = Some(EmployeeExpiry::settle(cause));
                if cause.counts_interruption()
                    && let Some(ticket) = &mut entry.ticket
                {
                    ticket.interruptions.push(InterruptionRecord {
                        cause,
                        at: now,
                    });
                    if ticket.interruptions.len() > INTERRUPTION_HISTORY_CAP {
                        let overflow = ticket.interruptions.len() - INTERRUPTION_HISTORY_CAP;
                        ticket.interruptions.drain(..overflow);
                    }
                }
                let outcome = if entry.cancelled {
                    WaveMemberOutcome::Cancelled
                } else if failed || entry.blocker.is_some() || cause.interrupted() {
                    WaveMemberOutcome::Failed
                } else {
                    WaveMemberOutcome::Finished
                };
                let supervisor_id = entry.supervisor_id;
                let group = entry
                    .ticket
                    .as_ref()
                    .and_then(|ticket| ticket.group_id.clone());
                employee = Some(entry.clone());
                if let Some(group) = group {
                    record_wave_outcome(state, &group, session, supervisor_id, outcome, now);
                }
            }
            Ok(())
        })?;
        if employee.is_some() {
            self.reset_context(session);
            self.projects.lock().remove(&session);
            self.interrupted.lock().retain(|id| *id != session);
        }
        Ok(employee)
    }

    /// Close out a finishing (or cancelled queued) employee: mark the
    /// record expired and hand back the reservation ids still owed a
    /// broker release — the held admission plus a parked update's id —
    /// empty when the slot is already free, so the caller releases
    /// exactly once. `expiry` is the tail's refined settle record —
    /// `None` preserves whatever `begin_finishing` recorded.
    pub fn complete_expiry(
        &self,
        session: Uuid,
        expiry: Option<EmployeeExpiry>,
    ) -> anyhow::Result<Vec<Uuid>> {
        let now = waku_protocol::model::unix_time();
        let mut reservations = Vec::new();
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            entry.set_lifecycle(EmployeeLifecycle::Expired, now);
            if let Some(expiry) = expiry {
                entry.expiry = Some(expiry);
            }
            if let Some(ticket) = &mut entry.ticket {
                reservations.extend(ticket.reservation.take());
                reservations.extend(ticket.pending_reservation.take());
                ticket.pending_resources = None;
            }
            Ok(())
        })?;
        Ok(reservations)
    }

    /// Whether the daemon loaded durable Boss state — a disabled service
    /// runs no scheduler and reconciles nothing.
    pub fn is_active(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Acquire)
    }

    /// The employee's admission lifecycle — `lifecycle()` folds legacy
    /// expired-only records into the projection. A `queued` record that
    /// holds no ticket never entered admission (a pre-queue summon or a
    /// malformed document), so it reports `working` like the plain record
    /// it is.
    pub fn employee_lifecycle(&self, session: Uuid) -> Option<EmployeeLifecycle> {
        self.employee(session).map(|entry| {
            if entry.lifecycle() == EmployeeLifecycle::Queued && entry.ticket.is_none() {
                EmployeeLifecycle::Working
            } else {
                entry.lifecycle()
            }
        })
    }

    /// The queue root an employee's supervisor chain ends at: the boss
    /// session or a planning session. Strict FIFO is per root — only the
    /// group's oldest queued sequence may dispatch.
    fn queue_root(state: &BossState, employee: &BossEmployee) -> Uuid {
        let mut current = employee.supervisor_id;
        for _ in 0..state.employees.len() {
            let Some(parent) = state
                .employees
                .iter()
                .find(|entry| entry.session_id == current)
            else {
                return current;
            };
            current = parent.supervisor_id;
        }
        current
    }

    /// Every queued ticket in sequence order — position and blocked
    /// display read this, the scheduler reads `queued_heads`.
    pub fn queued(&self) -> Vec<BossEmployee> {
        let state = self.state.lock();
        let mut queued: Vec<BossEmployee> = state
            .employees
            .iter()
            .filter(|entry| entry.lifecycle() == EmployeeLifecycle::Queued)
            .cloned()
            .collect();
        queued.sort_by_key(|entry| entry.ticket.as_ref().map(|ticket| ticket.sequence));
        queued
    }

    /// All queued tickets in admission order. Priority chooses which ticket
    /// is attempted first; sequence breaks ties. A blocked ticket does not
    /// prevent later tickets from being considered in the same pass.
    pub fn queued_heads(&self) -> Vec<BossEmployee> {
        let state = self.state.lock();
        let mut queued: Vec<&BossEmployee> = state
            .employees
            .iter()
            .filter(|entry| entry.lifecycle() == EmployeeLifecycle::Queued)
            .collect();
        queued.sort_by(|left, right| {
            let left_ticket = left.ticket.as_ref();
            let right_ticket = right.ticket.as_ref();
            right_ticket
                .and_then(|ticket| ticket.priority)
                .unwrap_or(0)
                .cmp(&left_ticket.and_then(|ticket| ticket.priority).unwrap_or(0))
                .then_with(|| {
                    left_ticket
                        .map(|ticket| ticket.sequence)
                        .unwrap_or(0)
                        .cmp(&right_ticket.map(|ticket| ticket.sequence).unwrap_or(0))
                })
        });
        queued.into_iter().cloned().collect()
    }

    /// The employee's position inside its own queue group, 1-based —
    /// `None` when it is not waiting.
    pub fn queue_position(&self, session: Uuid) -> Option<u64> {
        let state = self.state.lock();
        let employee = state
            .employees
            .iter()
            .find(|entry| entry.session_id == session)?;
        if employee.lifecycle() != EmployeeLifecycle::Queued {
            return None;
        }
        let root = Self::queue_root(&state, employee);
        let sequence = employee.ticket.as_ref()?.sequence;
        Some(
            state
                .employees
                .iter()
                .filter(|entry| {
                    entry.lifecycle() == EmployeeLifecycle::Queued
                        && Self::queue_root(&state, entry) == root
                        && entry
                            .ticket
                            .as_ref()
                            .is_some_and(|ticket| ticket.sequence <= sequence)
                })
                .count() as u64,
        )
    }

    /// Persist an accepted admission: the employee joins the roster as
    /// `queued` with its assigned sequence stamped — one durable ticket
    /// covering the employee, its envelope, and its queue position.
    pub fn enqueue_ticket(
        &self,
        mut employee: BossEmployee,
        mut ticket: SummonTicket,
    ) -> anyhow::Result<BossEmployee> {
        let now = waku_protocol::model::unix_time();
        self.update(|state| {
            state.next_sequence = state.next_sequence.saturating_add(1);
            ticket.sequence = state.next_sequence;
            employee.set_lifecycle(EmployeeLifecycle::Queued, now);
            employee.queued_at = Some(now);
            if let Some(group) = ticket.group_id.clone() {
                join_wave(state, &group, &employee);
            }
            employee.ticket = Some(ticket);
            state.employees.push(employee.clone());
            Ok(())
        })?;
        Ok(employee)
    }

    /// Park a follow-up on a queued ticket — it joins the dispatch
    /// envelope in submission order and survives restart. Working or
    /// finished employees reject with `false`.
    pub fn append_queued_prompt(&self, session: Uuid, prompt: String) -> anyhow::Result<bool> {
        let mut appended = false;
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            if entry.lifecycle() != EmployeeLifecycle::Queued {
                return Ok(());
            }
            if let Some(ticket) = &mut entry.ticket {
                ticket.pending_prompts.push(prompt);
                appended = true;
            }
            Ok(())
        })?;
        Ok(appended)
    }

    /// Rewrite a queued ticket's resolved selection — `setModel` keeps
    /// the ticket's sequence, so a model change never buys queue position.
    /// Working generations reject; the caller re-queues those itself.
    pub fn reticket(
        &self,
        session: Uuid,
        change: impl FnOnce(&mut SummonTicket),
    ) -> anyhow::Result<bool> {
        let mut done = false;
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            if entry.lifecycle() != EmployeeLifecycle::Queued {
                return Ok(());
            }
            if let Some(ticket) = &mut entry.ticket {
                change(ticket);
                done = true;
            }
            Ok(())
        })?;
        Ok(done)
    }

    /// Park a resource-set change on a working employee's ticket: the
    /// scheduler retries admission for `resources` under `reservation`
    /// — a fresh id distinct from the held claim's — and
    /// [`Self::apply_resource_update`] swaps it in once granted. Naming
    /// the currently held set cancels a parked update instead. `base`
    /// supplies a ticket for employees whose record predates them, like
    /// `requeue_employee`. Returns the superseded pending reservation
    /// id, still owed a broker release by the caller.
    pub fn request_resource_update(
        &self,
        session: Uuid,
        base: SummonTicket,
        resources: waku_protocol::resources::ResourceSet,
        reservation: Uuid,
    ) -> anyhow::Result<Option<Uuid>> {
        let mut replaced = None;
        self.update(|state| {
            let entry = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
                .context("employee record is missing")?;
            // A ticketless `queued` record is a pre-ticket employee the
            // lifecycle fold reports as working — its update rides the
            // synthesized ticket `base` supplies.
            let working = entry.lifecycle() == EmployeeLifecycle::Working
                || (entry.lifecycle() == EmployeeLifecycle::Queued && entry.ticket.is_none());
            if !working {
                bail!("employee is not running — its ticket cannot hold a resource update");
            }
            let mut ticket = entry.ticket.take().unwrap_or(base);
            replaced = ticket.pending_reservation.take();
            if ticket.resources == resources {
                // Naming the held set retires a parked update without
                // re-admitting.
                ticket.pending_resources = None;
                ticket.blocked_by.clear();
            } else {
                ticket.pending_resources = Some(resources);
                ticket.pending_reservation = Some(reservation);
            }
            entry.ticket = Some(ticket);
            Ok(())
        })?;
        Ok(replaced)
    }

    /// Employees carrying a parked resource update — the scheduler's
    /// second pass tries their admissions each wake. A parked set only
    /// ever lives on a running employee's ticket: queued tickets edit
    /// `resources` outright and expiry clears the fields, so the parked
    /// set alone is the signal.
    pub fn pending_resource_updates(&self) -> Vec<BossEmployee> {
        self.state
            .lock()
            .employees
            .iter()
            .filter(|entry| {
                entry
                    .ticket
                    .as_ref()
                    .is_some_and(|ticket| ticket.pending_resources.is_some())
            })
            .cloned()
            .collect()
    }

    /// Settle a granted resource update: on a matching parked id the new
    /// set and reservation replace the ticket's — `true` plus the
    /// previous reservation id still owed a broker release by the
    /// caller. Anything else — a superseded update, a requeue that
    /// folded the set into a fresh admission, a record that left
    /// `working` — is stale: `false`, and the caller releases the grant
    /// it just took.
    pub fn apply_resource_update(
        &self,
        session: Uuid,
        reservation: Uuid,
    ) -> anyhow::Result<(bool, Option<Uuid>)> {
        let mut swapped = (false, None);
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            let Some(ticket) = &mut entry.ticket else {
                return Ok(());
            };
            // The parked id match is the state check: a requeue folds
            // the set and takes the id, and expiry takes it too — only
            // the exact update this grant answers may swap in.
            if ticket.pending_reservation != Some(reservation) {
                return Ok(());
            }
            if let Some(resources) = ticket.pending_resources.take() {
                ticket.resources = resources;
            }
            ticket.pending_reservation = None;
            ticket.blocked_by.clear();
            swapped = (true, ticket.reservation.replace(reservation));
            Ok(())
        })?;
        Ok(swapped)
    }

    /// Grant a queued ticket its claims and move it to `dispatching`.
    /// `false` means the generation went stale — the ticket was stopped
    /// or re-admitted while the broker grant was in flight, and the
    /// caller must release what it just took.
    pub fn mark_dispatching(
        &self,
        session: Uuid,
        generation: u64,
        reservation: Option<Uuid>,
    ) -> anyhow::Result<bool> {
        let mut marked = false;
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            let stale = entry.lifecycle() != EmployeeLifecycle::Queued
                || entry
                    .ticket
                    .as_ref()
                    .is_none_or(|ticket| ticket.generation != generation);
            if stale {
                return Ok(());
            }
            entry.set_lifecycle(EmployeeLifecycle::Dispatching, 0);
            entry.queued_at = None;
            if let Some(ticket) = &mut entry.ticket {
                ticket.reservation = reservation.or(ticket.reservation);
                ticket.blocked_by.clear();
            }
            marked = true;
            Ok(())
        })?;
        Ok(marked)
    }

    /// The launch intent landed: the initial prompt reached the provider,
    /// or the re-admitted employee's slot is confirmed held. `false`
    /// settles a stale generation — the launch result is discarded.
    pub fn mark_working(&self, session: Uuid, generation: u64) -> anyhow::Result<bool> {
        let mut marked = false;
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            let stale = entry.lifecycle() != EmployeeLifecycle::Dispatching
                || entry
                    .ticket
                    .as_ref()
                    .is_none_or(|ticket| ticket.generation != generation);
            if stale {
                return Ok(());
            }
            entry.set_lifecycle(EmployeeLifecycle::Working, 0);
            marked = true;
            Ok(())
        })?;
        Ok(marked)
    }

    /// Record why a queued head cannot dispatch — cleared by the next
    /// grant. Skips the write (and the revision bump that would redraw
    /// clients) when the reasons are unchanged.
    pub fn record_blocked(
        &self,
        session: Uuid,
        blockers: Vec<AdmissionBlocker>,
    ) -> anyhow::Result<()> {
        if self
            .employee(session)
            .and_then(|entry| entry.ticket)
            .is_some_and(|ticket| ticket.blocked_by == blockers)
        {
            return Ok(());
        }
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            // A working employee carries a wait reason too when its parked
            // resource update cannot grant.
            let waits = entry.lifecycle() == EmployeeLifecycle::Queued
                || entry
                    .ticket
                    .as_ref()
                    .is_some_and(|ticket| ticket.pending_resources.is_some());
            if waits && let Some(ticket) = &mut entry.ticket {
                ticket.blocked_by = blockers;
            }
            Ok(())
        })
    }

    /// Re-enter an employee into admission: a fresh generation and a new
    /// sequence at the tail — resurrection never inherits queue position.
    /// The employee record revives (from the roster or retirement) as
    /// `queued`. The previous ticket is kept and adjusted; `base` supplies
    /// one for employees summoned before tickets existed. A parked
    /// resource update folds into the fresh admission wholesale rather
    /// than surviving as a second try. Returns the employee plus every
    /// reservation id still owed a broker release by the caller — the
    /// previous generation's claim and a parked update's id.
    pub fn requeue_employee(
        &self,
        session: Uuid,
        base: SummonTicket,
        adjust: impl FnOnce(&mut SummonTicket),
    ) -> anyhow::Result<(BossEmployee, Vec<Uuid>)> {
        let now = waku_protocol::model::unix_time();
        let mut outcome = None;
        self.update(|state| {
            let previous = state
                .employees
                .iter()
                .chain(state.retired_employees.iter())
                .find(|entry| entry.session_id == session)
                .and_then(|entry| entry.ticket.clone());
            let generation = previous.as_ref().map(|t| t.generation).unwrap_or(0) + 1;
            let mut ticket = previous.unwrap_or(base);
            let mut stale_reservations = Vec::new();
            stale_reservations.extend(ticket.reservation.take());
            stale_reservations.extend(ticket.pending_reservation.take());
            if let Some(resources) = ticket.pending_resources.take() {
                ticket.resources = resources;
            }
            ticket.blocked_by.clear();
            ticket.dispatch_event = None;
            adjust(&mut ticket);
            state.next_sequence = state.next_sequence.saturating_add(1);
            ticket.sequence = state.next_sequence;
            ticket.generation = generation;
            let employee = if let Some(index) = state
                .employees
                .iter()
                .position(|entry| entry.session_id == session)
            {
                &mut state.employees[index]
            } else if let Some(index) = state
                .retired_employees
                .iter()
                .position(|entry| entry.session_id == session)
            {
                let entry = state.retired_employees.remove(index);
                if state
                    .employees
                    .iter()
                    .any(|active| active.identity.name == entry.identity.name)
                {
                    bail!("name has been reused; summon a new employee");
                }
                state.employees.push(entry);
                state.employees.last_mut().unwrap()
            } else {
                return Ok(());
            };
            employee.set_lifecycle(EmployeeLifecycle::Queued, now);
            employee.expired_at = None;
            employee.blocker = None;
            employee.cancelled = false;
            employee.expiry = None;
            employee.queued_at = Some(now);
            employee.ticket = Some(ticket);
            outcome = Some((employee.clone(), stale_reservations));
            // A re-admitted member is back in flight — its recorded
            // outcome clears, and a resolved wave reopens for one more
            // resolution.
            wave_member_in_flight(state, session);
            Ok(())
        })?;
        outcome.context("not a Boss employee")
    }

    /// Atomically replace the admission policy. `expected_revision`
    /// guards lost-update races between boss and human callers; the
    /// returned document is what was persisted.
    pub fn set_resource_policy(
        &self,
        caller: Option<Uuid>,
        expected_revision: u64,
        model_limits: Vec<ModelLimit>,
        host: Option<waku_protocol::resources::ResourcePolicy>,
    ) -> anyhow::Result<BossResourcePolicy> {
        if caller.is_some_and(|id| !self.is_boss_principal(id)) {
            bail!("only the boss or a human can set the resource policy");
        }
        for rule in &model_limits {
            if rule.model.trim().is_empty() {
                bail!("model limit rules need a model id");
            }
            if rule.live_limit > rule.hard_cap {
                bail!(
                    "{} / {} liveLimit cannot exceed hardCap",
                    rule.provider.display_name(),
                    rule.model
                );
            }
        }
        let mut applied = None;
        self.update(|state| {
            if state.resource_policy.revision != expected_revision {
                bail!(
                    "resource policy revision mismatch: expected {expected_revision}, have {}",
                    state.resource_policy.revision
                );
            }
            state.resource_policy = BossResourcePolicy {
                revision: expected_revision + 1,
                model_limits,
                host,
            };
            applied = Some(state.resource_policy.clone());
            Ok(())
        })?;
        Ok(applied.unwrap())
    }

    /// The policy rule a resolved provider+model counts against — `None`
    /// imposes no model cap.
    pub fn model_limit(&self, provider: ProviderKind, model: &str) -> Option<ModelLimit> {
        self.state
            .lock()
            .resource_policy
            .model_limits
            .iter()
            .find(|rule| rule.provider == provider && rule.model == model)
            .cloned()
    }

    /// Record a dispatch notification in the durable outbox and return
    /// its event id — delivery dedupes on it, so a restart can re-drive
    /// the same entry instead of guessing whether it went out.
    pub fn outbox_push(
        &self,
        session: Uuid,
        generation: u64,
        provider: ProviderKind,
        model: String,
        goal_id: Option<Uuid>,
    ) -> anyhow::Result<u64> {
        let now = waku_protocol::model::unix_time();
        let mut id = 0;
        self.update(|state| {
            state.next_event_id = state.next_event_id.saturating_add(1);
            id = state.next_event_id;
            state.outbox.push(DispatchNotification {
                id,
                session_id: session,
                generation,
                provider,
                model,
                goal_id,
                created_at: now,
                delivered_at: None,
            });
            Ok(())
        })?;
        Ok(id)
    }

    /// Notifications still owed a supervisor.
    pub fn outbox_pending(&self) -> Vec<DispatchNotification> {
        self.state
            .lock()
            .outbox
            .iter()
            .filter(|entry| entry.delivered_at.is_none())
            .cloned()
            .collect()
    }

    /// A notification reached its supervisor's durable prompt queue —
    /// parked or delivered, it can no longer be lost.
    pub fn outbox_mark_delivered(&self, id: u64) -> anyhow::Result<()> {
        let now = waku_protocol::model::unix_time();
        self.update(|state| {
            if let Some(entry) = state.outbox.iter_mut().find(|entry| entry.id == id) {
                entry.delivered_at = Some(now);
            }
            Ok(())
        })
    }

    /// Wave-resolution notices still owed a supervisor.
    pub fn wave_outbox_pending(&self) -> Vec<WaveNotification> {
        self.state
            .lock()
            .wave_outbox
            .iter()
            .filter(|entry| entry.delivered_at.is_none())
            .cloned()
            .collect()
    }

    /// A wave notice reached its supervisor's durable prompt queue.
    pub fn wave_outbox_mark_delivered(&self, id: u64) -> anyhow::Result<()> {
        let now = waku_protocol::model::unix_time();
        self.update(|state| {
            if let Some(entry) = state.wave_outbox.iter_mut().find(|entry| entry.id == id) {
                entry.delivered_at = Some(now);
            }
            Ok(())
        })
    }

    /// Revert a `dispatching` ticket whose launch never reached the
    /// provider — a crash between grant and prompt leaves it here, and
    /// recovery puts it back in line rather than reporting an
    /// interruption for a job that never started.
    pub fn revert_dispatch(&self, session: Uuid) -> anyhow::Result<()> {
        let now = waku_protocol::model::unix_time();
        self.update(|state| {
            let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session)
            else {
                return Ok(());
            };
            if entry.lifecycle() == EmployeeLifecycle::Dispatching {
                entry.set_lifecycle(EmployeeLifecycle::Queued, now);
                entry.queued_at = Some(now);
                if let Some(ticket) = &mut entry.ticket {
                    ticket.reservation = None;
                }
            }
            Ok(())
        })
    }

    /// Remove expired errands after one hour and goals after 24 hours.
    /// Their task sessions and retired roster records remain available,
    /// including full transcripts and supervisor-driven resurrection.
    pub fn retire_expired(&self, now: u64) -> anyhow::Result<Vec<BossEmployee>> {
        let _operation = self.operation_lock.lock();
        let retires = |employee: &BossEmployee| {
            employee.expired
                && employee
                    .expired_at
                    .and_then(|expired_at| now.checked_sub(expired_at))
                    .is_some_and(|age| match employee.work_goal {
                        EmployeeGoal::Errand => age >= EMPLOYEE_RETIREMENT_SECONDS,
                        EmployeeGoal::Goal => age > GOAL_RETIREMENT_SECONDS,
                    })
        };
        if !self.state.lock().employees.iter().any(&retires) {
            return Ok(Vec::new());
        }
        let mut retired = Vec::new();
        self.update(|state| {
            let mut keep = Vec::with_capacity(state.employees.len());
            for employee in state.employees.drain(..) {
                if retires(&employee) {
                    retired.push(employee.clone());
                    state.retired_employees.push(employee);
                } else {
                    keep.push(employee);
                }
            }
            state.employees = keep;
            Ok(())
        })?;
        Ok(retired)
    }

    fn require_owner(&self, caller: Option<Uuid>) -> anyhow::Result<()> {
        if caller.is_some_and(|id| !self.is_boss_principal(id)) {
            bail!("only the boss or a human can change personas and Boss files");
        }
        Ok(())
    }

    /// The audit-trail actor for an owner-gated caller — the boss and its
    /// planning sessions record as the boss, a client call as the user.
    /// Call after `require_owner`: employees never reach the channel.
    fn plan_actor(&self, caller: Option<Uuid>) -> PlanActor {
        match caller {
            Some(_) => PlanActor::Boss,
            None => PlanActor::User,
        }
    }

    /// A deliverable is the caller's work product, not Boss-owned state, so
    /// the owner gate relaxes for employees: the boss and human publish any
    /// existing path, while an employee publishes only inside its assigned
    /// workspace — the project checkout or its daemon-managed worktree —
    /// recorded as its run directory at launch.
    fn require_deliverable_publisher(
        &self,
        caller: Option<Uuid>,
        target: &Path,
    ) -> anyhow::Result<()> {
        let Some(caller) = caller.filter(|id| !self.is_boss_principal(*id)) else {
            return Ok(());
        };
        let workspace = self.projects.lock().get(&caller).cloned();
        let inside = workspace
            .and_then(|root| {
                fs::canonicalize(root)
                    .ok()
                    .zip(fs::canonicalize(target).ok())
            })
            .is_some_and(|(root, target)| target.starts_with(root));
        if !inside {
            bail!("employees can publish deliverables only from inside their own workspace");
        }
        Ok(())
    }

    /// Authorize and normalize a `speak` request: only the boss session or a
    /// human client may voice an utterance, and the fragments stay small —
    /// speak is for short canned phrases, not narration.
    pub fn speak_parts(
        &self,
        caller: Option<Uuid>,
        parts: Vec<String>,
    ) -> anyhow::Result<Vec<String>> {
        if caller.is_some_and(|caller| !self.is_boss_principal(caller)) {
            bail!("only the boss or a human can speak");
        }
        if parts.is_empty() || parts.len() > MAX_SPEECH_PARTS {
            bail!("speak takes between 1 and {MAX_SPEECH_PARTS} parts");
        }
        let parts: Vec<String> = parts.iter().map(|part| part.trim().to_owned()).collect();
        let mut total = 0;
        for part in &parts {
            let chars = part.chars().count();
            if chars == 0 {
                bail!("speak parts cannot be empty");
            }
            if chars > MAX_SPEECH_PART_CHARS {
                bail!("a speak part exceeds {MAX_SPEECH_PART_CHARS} characters");
            }
            total += chars;
        }
        if total > MAX_SPEECH_TOTAL_CHARS {
            bail!("speak exceeds {MAX_SPEECH_TOTAL_CHARS} characters");
        }
        Ok(parts)
    }

    /// Run one stateless Rhai script. `dispatch` is the daemon's full operation path, so bound functions
    /// keep each operation's own authorization and runtime effects.
    pub fn eval(
        &self,
        _session: Uuid,
        script: &str,
        dispatch: &dyn Fn(BossOperation) -> anyhow::Result<BossResult>,
    ) -> anyhow::Result<BossResult> {
        if script.len() > crate::boss_eval::MAX_EVAL_SCRIPT_BYTES {
            bail!(
                "eval script exceeds {} bytes",
                crate::boss_eval::MAX_EVAL_SCRIPT_BYTES
            );
        }
        let _serial = self.evals.lock();
        let outcome = crate::boss_eval::run(rhai::Scope::new(), script, dispatch);
        let output = outcome.output;
        match outcome.value {
            Ok(value) => Ok(BossResult::Eval { value, output }),
            Err(error) if output.is_empty() => bail!("{error}"),
            Err(error) => bail!("{error}\n\nscript output before the failure:\n{output}"),
        }
    }

    pub fn handle(
        &self,
        caller: Option<Uuid>,
        operation: BossOperation,
    ) -> anyhow::Result<BossResult> {
        match operation {
            BossOperation::Context
            | BossOperation::Roster
            | BossOperation::Open { .. }
            | BossOperation::Browse { .. }
            | BossOperation::CreatePlan { .. }
            | BossOperation::Terminal { .. }
            | BossOperation::FinalizePlan { .. }
            | BossOperation::Automation { .. }
            | BossOperation::Summon { .. }
            | BossOperation::Control { .. }
            | BossOperation::Resume { .. }
            | BossOperation::ReportBlocker { .. }
            | BossOperation::Transcript { .. }
            | BossOperation::Speak { .. }
            | BossOperation::SetResourcePolicy { .. }
            | BossOperation::SetProjectSubmissions { .. }
            | BossOperation::SetProjectQaBranch { .. }
            | BossOperation::Eval { .. } => {
                bail!("runtime operation requires daemon dispatch")
            }
            BossOperation::UpdatePlanItems { plan, items } => {
                self.update_plan_items(caller, &plan, items)?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::SetPlanItemState {
                plan,
                item,
                state,
            } => {
                self.set_plan_item_state(caller, &plan, item, state)?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::SetPlanOutcome { plan, outcome } => {
                self.set_plan_outcome(caller, &plan, outcome)?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::View => {
                let mut state = self.document();
                if let Some(caller) = caller.filter(|id| !self.is_boss_principal(*id)) {
                    let employee = state
                        .employees
                        .iter()
                        .find(|entry| entry.session_id == caller)
                        .ok_or_else(|| anyhow!("this task is not a Boss employee"))?;
                    let persona = employee.persona_id;
                    state.personas.retain(|entry| entry.id == persona);
                    state.employees.retain(|entry| {
                        entry.session_id == caller || entry.supervisor_id == caller
                    });
                    // Waves stay scoped to what the caller can already see:
                    // its own memberships and the groups it summoned.
                    state.waves.retain(|wave| {
                        wave.supervisor_id == caller
                            || wave
                                .members
                                .iter()
                                .any(|member| member.session_id == caller)
                    });
                }
                Ok(BossResult::State { state })
            }
            BossOperation::Rename { name } => {
                self.require_owner(caller)?;
                validate_name(&name)?;
                self.update(|state| {
                    state.identity.name = name.trim().to_owned();
                    Ok(())
                })?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::Memory { operation } => {
                use waku_protocol::boss::MemoryOperation;

                let state = self.document();
                let boss_principal = caller.is_none_or(|id| self.is_boss_principal(id));
                let principal = if boss_principal {
                    "boss".to_owned()
                } else {
                    caller.context("missing Boss caller")?.to_string()
                };
                let buckets = waku_memory_engine::buckets::BucketStore::open(
                    self.root.join("files/memory-engine"),
                )?;
                let mut known_buckets = buckets.list_buckets()?;
                let active_employee = caller.filter(|caller| {
                    state
                        .employees
                        .iter()
                        .any(|employee| employee.session_id == *caller && !employee.expired)
                });
                let own_project_path =
                    active_employee.and_then(|session| self.projects.lock().get(&session).cloned());
                let own_bucket_id = own_project_path.as_deref().map(project_bucket_id);
                // `project` references resolve through the registered-project
                // catalog; an absent target defaults to the caller's project.
                let (resolved_bucket, resolved_project) = match memory_bucket_ref(&operation) {
                    Some((bucket, project)) => {
                        let catalog = self
                            .project_catalog
                            .lock()
                            .as_ref()
                            .map(|get| get())
                            .unwrap_or_default();
                        let (id, path) = resolve_memory_bucket(
                            bucket,
                            project,
                            own_project_path.as_deref(),
                            &catalog,
                        )?;
                        (Some(id), path)
                    }
                    None => (None, None),
                };
                // Project buckets materialize on first touch: the caller's own
                // always, an explicitly named one only for the boss — an
                // employee naming a foreign project must not mint it.
                for path in [
                    own_project_path,
                    resolved_project.filter(|_| boss_principal),
                ]
                .into_iter()
                .flatten()
                {
                    let bucket_id = project_bucket_id(&path);
                    if known_buckets.iter().any(|bucket| bucket.id == bucket_id) {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .and_then(|part| part.to_str())
                        .unwrap_or("Project")
                        .to_owned();
                    let bucket = waku_memory_engine::buckets::Bucket {
                        id: bucket_id.clone(),
                        name,
                        purpose: "Shared project memory".into(),
                        project_id: Some(bucket_id),
                    };
                    buckets.create_bucket(&bucket)?;
                    known_buckets.push(bucket);
                }
                let granted_bucket_ids = if boss_principal {
                    Vec::new()
                } else {
                    state
                        .employees
                        .iter()
                        .find(|employee| Some(employee.session_id) == caller && !employee.expired)
                        .map(|employee| employee.permissions.bucket_ids.clone())
                        .unwrap_or_default()
                };
                let visible_buckets = known_buckets
                    .iter()
                    .filter(|bucket| {
                        boss_principal
                            || Some(bucket.id.as_str()) == own_bucket_id.as_deref()
                            || granted_bucket_ids.contains(&bucket.id)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let bucket_access = waku_memory_engine::buckets::BucketAccess {
                    buckets: known_buckets.clone(),
                    grants: visible_buckets
                        .iter()
                        .map(|bucket| waku_memory_engine::buckets::BucketGrant {
                            bucket_id: bucket.id.clone(),
                            principal_id: principal.clone(),
                            read: true,
                            insert: true,
                        })
                        .collect(),
                    boss: boss_principal,
                };
                let mut bucket_list = Vec::new();
                let mut overview = None;
                let mut notes = Vec::new();
                let mut compression = None;
                let mut bucket = None;
                let mut recorded = None;
                let mut migration = None;
                match operation {
                    MemoryOperation::ListBuckets => {
                        bucket_list = visible_buckets
                            .iter()
                            .map(serde_json::to_value)
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                    }
                    MemoryOperation::CreateBucket { name, purpose } => {
                        anyhow::ensure!(boss_principal, "only the Boss can create memory buckets");
                        let name = name.trim();
                        anyhow::ensure!(!name.is_empty(), "bucket name cannot be empty");
                        let slug = name
                            .to_ascii_lowercase()
                            .chars()
                            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                            .collect::<String>();
                        let id = format!("{}-{}", slug.trim_matches('-'), Uuid::new_v4());
                        let created = waku_memory_engine::buckets::Bucket {
                            id: id.clone(),
                            name: name.to_owned(),
                            purpose,
                            project_id: None,
                        };
                        buckets.create_bucket(&created)?;
                        bucket_list.push(serde_json::to_value(created)?);
                    }
                    MemoryOperation::Overview { .. } => {
                        let bucket_id = resolved_bucket
                            .clone()
                            .expect("bucket-addressed ops resolve a target");
                        bucket = Some(bucket_id.clone());
                        let result = buckets.overview(&bucket_access, &principal, &bucket_id)?;
                        compression = result
                            .compression
                            .as_ref()
                            .map(serde_json::to_value)
                            .transpose()?;
                        overview = Some(serde_json::to_value(result)?);
                    }
                    MemoryOperation::Record {
                        kind,
                        text,
                        retry_key,
                        ..
                    } => {
                        let bucket_id = resolved_bucket
                            .clone()
                            .expect("bucket-addressed ops resolve a target");
                        bucket = Some(bucket_id.clone());
                        let kind = match kind {
                            waku_protocol::boss::MemoryNoteKind::Fact => {
                                waku_memory_engine::buckets::NoteKind::Fact
                            }
                            waku_protocol::boss::MemoryNoteKind::Observation => {
                                waku_memory_engine::buckets::NoteKind::Observation
                            }
                            waku_protocol::boss::MemoryNoteKind::Question => {
                                waku_memory_engine::buckets::NoteKind::Question
                            }
                        };
                        let result = buckets.insert(
                            &bucket_access,
                            &principal,
                            &bucket_id,
                            kind,
                            &text,
                            &retry_key,
                        )?;
                        recorded = Some(serde_json::to_value(result.note)?);
                        compression = result.compression.map(serde_json::to_value).transpose()?;
                    }
                    MemoryOperation::SubmitSummary {
                        start, end, text, ..
                    } => {
                        let bucket_id = resolved_bucket
                            .clone()
                            .expect("bucket-addressed ops resolve a target");
                        bucket = Some(bucket_id.clone());
                        compression = buckets
                            .submit_summary(
                                &bucket_access,
                                &principal,
                                &bucket_id,
                                start,
                                end,
                                &text,
                            )?
                            .map(serde_json::to_value)
                            .transpose()?;
                    }
                    MemoryOperation::Scan { query, .. } => {
                        let bucket_id = resolved_bucket
                            .clone()
                            .expect("bucket-addressed ops resolve a target");
                        bucket = Some(bucket_id.clone());
                        notes = buckets
                            .search(&bucket_access, &principal, &bucket_id, &query)?
                            .into_iter()
                            .map(serde_json::to_value)
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                    }
                    MemoryOperation::ZoomBucket { start, end, .. } => {
                        let bucket_id = resolved_bucket
                            .clone()
                            .expect("bucket-addressed ops resolve a target");
                        bucket = Some(bucket_id.clone());
                        notes = buckets
                            .zoom(&bucket_access, &principal, &bucket_id, start, end)?
                            .into_iter()
                            .map(serde_json::to_value)
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                    }
                    MemoryOperation::MigrateLegacy {
                        bucket: bucket_id,
                        source,
                        dry_run,
                    } => {
                        anyhow::ensure!(boss_principal, "legacy memory migration is Boss-only");
                        anyhow::ensure!(
                            known_buckets.iter().any(|known| known.id == bucket_id),
                            "unknown memory bucket {bucket_id}"
                        );
                        let candidates = legacy_memory_candidates(&self.root, &source)?;
                        if !dry_run {
                            for candidate in &candidates {
                                let text = format!(
                                    "Imported from {}\n\n{}",
                                    candidate.source, candidate.text
                                );
                                let retry_key = format!(
                                    "legacy-{}",
                                    Sha256::digest(
                                        format!("{}\0{}", candidate.source, candidate.text)
                                            .as_bytes()
                                    )
                                    .iter()
                                    .map(|byte| format!("{byte:02x}"))
                                    .collect::<String>()
                                );
                                let result = buckets.insert(
                                    &bucket_access,
                                    &principal,
                                    &bucket_id,
                                    waku_memory_engine::buckets::NoteKind::Observation,
                                    &text,
                                    &retry_key,
                                )?;
                                compression =
                                    result.compression.map(serde_json::to_value).transpose()?;
                            }
                        }
                        bucket = Some(bucket_id.clone());
                        migration = Some(MemoryMigrationReport {
                            bucket: bucket_id,
                            source,
                            dry_run,
                            imported: if dry_run { 0 } else { candidates.len() },
                            candidates,
                        });
                    }
                }
                Ok(BossResult::Memory {
                    buckets: bucket_list,
                    overview,
                    notes,
                    compression,
                    recorded,
                    bucket,
                    migration,
                })
            }
            BossOperation::RenameEmployee { session_id, name } => {
                self.require_owner(caller)?;
                validate_name(&name)?;
                self.update(|state| {
                    let employee = state
                        .employees
                        .iter_mut()
                        .find(|entry| entry.session_id == session_id)
                        .ok_or_else(|| anyhow!("not a Boss employee"))?;
                    employee.identity.name = name.trim().to_owned();
                    Ok(())
                })?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::RegenerateAvatar { session_id } => {
                self.require_owner(caller)?;
                let seed = Uuid::new_v4().to_string();
                self.update(|state| {
                    if session_id.is_none() || session_id == state.session_id {
                        state.identity.avatar_seed = seed;
                    } else {
                        let employee = state
                            .employees
                            .iter_mut()
                            .find(|entry| Some(entry.session_id) == session_id)
                            .ok_or_else(|| anyhow!("not a Boss employee"))?;
                        employee.identity.avatar_seed = seed;
                    }
                    Ok(())
                })?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::UpsertPersona { persona } => {
                self.require_owner(caller)?;
                validate_name(&persona.name)?;
                if persona
                    .icon
                    .flatten()
                    .is_some_and(|icon| !icon.is_employee_icon())
                {
                    bail!("persona icon is not in the employee icon set");
                }
                if persona.markdown.len() > MAX_FILE_BYTES {
                    bail!("persona is too large");
                }
                for path in &persona.pinned_files {
                    if path == "memory" || path.starts_with("memory/") {
                        bail!("pinned files cannot point into legacy memory storage");
                    }
                    validate_relative(path, false)?;
                    self.file_path(path, false)?;
                }
                self.validate_bucket_ids(&persona.permissions.bucket_ids)?;
                self.update(|state| {
                    let id = if persona.id.is_nil() {
                        Uuid::new_v4()
                    } else {
                        persona.id
                    };
                    // `icon` is tri-state on the wire — an absent field
                    // preserves the stored default, null clears it, an
                    // identifier replaces it.
                    let stored_icon = state
                        .personas
                        .iter()
                        .find(|entry| entry.id == id)
                        .and_then(|entry| entry.icon);
                    let record = BossPersona {
                        id,
                        name: persona.name,
                        markdown: persona.markdown,
                        pinned_files: persona.pinned_files,
                        permissions: persona.permissions,
                        icon: persona.icon.unwrap_or(stored_icon),
                    };
                    if let Some(existing) = state
                        .personas
                        .iter_mut()
                        .find(|entry| entry.id == record.id)
                    {
                        *existing = record;
                    } else {
                        state.personas.push(record);
                    }
                    Ok(())
                })?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::SetEmployeeIcon { session_id, icon } => {
                self.require_owner(caller)?;
                if icon.is_some_and(|icon| !icon.is_employee_icon()) {
                    bail!("icon is not in the employee icon set");
                }
                self.update(|state| {
                    let employee = state
                        .employees
                        .iter_mut()
                        .find(|entry| entry.session_id == session_id)
                        .ok_or_else(|| anyhow!("unknown employee"))?;
                    employee.icon = icon;
                    Ok(())
                })?;
                Ok(BossResult::State {
                    state: self.document(),
                })
            }
            BossOperation::ListFiles { path } => {
                validate_relative(&path, true)?;
                let path = remap_plan_path(&path);
                self.authorize_file(caller, &path, true)?;
                let directory = self.file_path(&path, true)?;
                let mut files = Vec::new();
                for entry in fs::read_dir(directory)? {
                    let entry = entry?;
                    let kind = entry.file_type()?;
                    if kind.is_symlink() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let child = if path.is_empty() {
                        name
                    } else {
                        format!("{path}/{name}")
                    };
                    if self.authorize_file(caller, &child, kind.is_dir()).is_ok() {
                        files.push(BossFile {
                            path: child,
                            directory: kind.is_dir(),
                        });
                    }
                }
                files.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.path.cmp(&b.path)));
                Ok(BossResult::Files { files })
            }
            BossOperation::ReadFile { path } => {
                validate_relative(&path, false)?;
                let path = remap_plan_path(&path);
                self.authorize_file(caller, &path, false)?;
                let file = self.file_path(&path, false)?;
                if fs::metadata(&file)?.len() > MAX_FILE_BYTES as u64 {
                    bail!("Boss file is too large");
                }
                Ok(BossResult::File {
                    path,
                    content: fs::read_to_string(file)?,
                })
            }
            BossOperation::WriteFile { path, content } => {
                self.require_owner(caller)?;
                validate_relative(&path, false)?;
                let path = remap_plan_path(&path);
                if content.len() > MAX_FILE_BYTES {
                    bail!("Boss file is too large");
                }
                if path.starts_with("personas/") && path.ends_with("/PERSONA.md") {
                    bail!("edit persona Markdown using upsertPersona");
                }
                if self.plan_file_frozen(&path) {
                    bail!("this plan is finalized — its document is frozen");
                }
                let file = self.file_path(&path, false)?;
                if let Some(parent) = file.parent() {
                    fs::create_dir_all(parent)?;
                }
                atomic_write(&file, content.as_bytes())?;
                self.update(|_| Ok(()))?;
                Ok(BossResult::Saved)
            }
            BossOperation::CreateFolder { path } => {
                self.require_owner(caller)?;
                validate_relative(&path, false)?;
                let path = remap_plan_path(&path);
                fs::create_dir_all(self.file_path(&path, false)?)?;
                self.update(|_| Ok(()))?;
                Ok(BossResult::Saved)
            }
            BossOperation::PublishDeliverable {
                path,
                name,
                reference,
            } => {
                let target = PathBuf::from(&path);
                if !target.is_absolute() {
                    bail!("deliverable paths must be absolute");
                }
                let metadata = fs::metadata(&target).context("deliverable path does not exist")?;
                self.require_deliverable_publisher(caller, &target)?;
                let directory = metadata.is_dir();
                let name = match name {
                    Some(name) => {
                        let name = name.trim().to_owned();
                        validate_name(&name)?;
                        name
                    }
                    None => target
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&path)
                        .to_owned(),
                };
                // Re-publishing a source refreshes the deliverable in place —
                // it jumps back into the sidebar's recency window under the
                // same row. The publish key is the source path; for a copied
                // deliverable `path` has already moved into the store.
                let id = self
                    .state
                    .lock()
                    .deliverables
                    .iter()
                    .find(|deliverable| {
                        deliverable
                            .source_path
                            .as_deref()
                            .unwrap_or(&deliverable.path)
                            == path
                    })
                    .map(|deliverable| deliverable.id)
                    .unwrap_or_else(Uuid::new_v4);
                // A copied deliverable's bytes live under the Boss data
                // directory, so the sidebar entry survives the workspace it
                // was published from — an employee's worktree is cleaned up
                // with its chat. A `reference` publish keeps the live path,
                // and switching modes drops the snapshot a copy left behind.
                let stored = if reference {
                    let _ = fs::remove_dir_all(self.deliverable_dir(id));
                    None
                } else {
                    Some(self.store_deliverable(id, &target)?)
                };
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    if let Some(deliverable) = state
                        .deliverables
                        .iter_mut()
                        .find(|deliverable| deliverable.id == id)
                    {
                        deliverable.name = name;
                        deliverable.directory = directory;
                        deliverable.updated_at = now;
                        deliverable.path = stored.clone().unwrap_or_else(|| path.clone());
                        deliverable.source_path = (!reference).then(|| path.clone());
                    } else {
                        state.deliverables.push(BossDeliverable {
                            id,
                            name,
                            path: stored.clone().unwrap_or_else(|| path.clone()),
                            source_path: (!reference).then(|| path.clone()),
                            directory,
                            created_at: now,
                            updated_at: now,
                            pinned_at: None,
                            dormant_at: None,
                            archived_at: None,
                            viewed_at: None,
                        });
                    }
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::DismissDeliverable { id } => {
                self.require_owner(caller)?;
                self.update(|state| {
                    let count = state.deliverables.len();
                    state
                        .deliverables
                        .retain(|deliverable| deliverable.id != id);
                    if state.deliverables.len() == count {
                        bail!("unknown deliverable");
                    }
                    Ok(())
                })?;
                // A dismissed deliverable's snapshot leaves with its record —
                // best-effort, since a leftover store dir is only disk.
                let _ = fs::remove_dir_all(self.deliverable_dir(id));
                Ok(BossResult::Saved)
            }
            BossOperation::PinDeliverable { id, pinned } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let deliverable = state
                        .deliverables
                        .iter_mut()
                        .find(|deliverable| deliverable.id == id)
                        .ok_or_else(|| anyhow!("unknown deliverable"))?;
                    deliverable.pinned_at = pinned.then_some(now);
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::SweepDeliverable { id, dormant } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let deliverable = state
                        .deliverables
                        .iter_mut()
                        .find(|deliverable| deliverable.id == id)
                        .ok_or_else(|| anyhow!("unknown deliverable"))?;
                    deliverable.dormant_at = dormant.then_some(now);
                    if !dormant {
                        // Restoring re-enters the recency window the way a
                        // re-publish does — a dormant deliverable can outlive it.
                        deliverable.updated_at = now;
                    }
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::ArchiveDeliverable { id, archived } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let deliverable = state
                        .deliverables
                        .iter_mut()
                        .find(|deliverable| deliverable.id == id)
                        .ok_or_else(|| anyhow!("unknown deliverable"))?;
                    deliverable.archived_at = archived.then_some(now);
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::MarkDeliverableViewed { id } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let deliverable = state
                        .deliverables
                        .iter_mut()
                        .find(|deliverable| deliverable.id == id)
                        .ok_or_else(|| anyhow!("unknown deliverable"))?;
                    deliverable.viewed_at = Some(now);
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::MarkGoalsViewed => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    state.goals_viewed_at = Some(now);
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
        }
    }

    fn update(
        &self,
        change: impl FnOnce(&mut BossState) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        let mut next = state.clone();
        change(&mut next)?;
        // A change that lands nothing — e.g. a scheduler pass over an
        // already-settled ticket — pays no revision bump and none of the
        // write set `save` performs under this lock.
        if serde_json::to_vec(&next)? == serde_json::to_vec(&*state)? {
            return Ok(());
        }
        next.revision = next.revision.saturating_add(1);
        self.save(&next)?;
        *state = next;
        drop(state);
        if let Some(notifier) = self.notifier.lock().clone() {
            notifier();
        }
        Ok(())
    }

    fn save(&self, state: &BossState) -> anyhow::Result<()> {
        let mut writes = self.persona_writes.lock();
        writes.retain(|id, _| state.personas.iter().any(|persona| persona.id == *id));
        for persona in &state.personas {
            let directory = self.file_path(&format!("personas/{}", persona.id), false)?;
            let file = directory.join("PERSONA.md");
            if writes.get(&persona.id) == Some(&persona.markdown) && file.exists() {
                continue;
            }
            fs::create_dir_all(&directory)?;
            atomic_write(&file, persona.markdown.as_bytes())?;
            writes.insert(persona.id, persona.markdown.clone());
        }
        drop(writes);
        atomic_write(
            &self.root.join("boss.json"),
            &serde_json::to_vec_pretty(state)?,
        )
    }

    fn authorize_file(
        &self,
        caller: Option<Uuid>,
        path: &str,
        directory: bool,
    ) -> anyhow::Result<()> {
        validate_relative(path, directory)?;
        if caller.is_none() || caller.is_some_and(|id| self.is_boss_principal(id)) {
            return Ok(());
        }
        let state = self.state.lock();
        let employee = state
            .employees
            .iter()
            .find(|entry| Some(entry.session_id) == caller)
            .ok_or_else(|| anyhow!("this task is not a Boss employee"))?;
        if employee.expired {
            bail!("this employee has expired");
        }
        let persona = state
            .personas
            .iter()
            .find(|entry| entry.id == employee.persona_id)
            .ok_or_else(|| anyhow!("employee persona is unavailable"))?;
        let permitted = !path.starts_with("memory/")
            && employee
            .pinned_files
            .iter()
            .any(|file| path == file || path.starts_with(&format!("{file}/")))
            || path == format!("personas/{}/PERSONA.md", persona.id);
        // Directory discovery reveals only ancestors of a granted file/folder.
        let ancestor = directory
            && (path.is_empty()
                || employee
                            .pinned_files
                            .iter()
                    .cloned()
                    .any(|file| {
                        !file.starts_with("memory/") && file.starts_with(&format!("{path}/"))
                    }));
        if !permitted && !ancestor {
            bail!("persona does not grant access to this Boss file");
        }
        Ok(())
    }

    fn file_path(&self, path: &str, allow_empty: bool) -> anyhow::Result<PathBuf> {
        validate_relative(path, allow_empty)?;
        let mut result = self.root.join("files");
        for component in Path::new(path).components() {
            result.push(component);
            match fs::symlink_metadata(&result) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("symlinks are not allowed in Boss files")
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(result)
    }

    /// The daemon's deliverable store: `deliverables/<id>/<name>` beside the
    /// files root. Copies live there rather than under `files/` so Boss file
    /// operations cannot rename or overwrite a snapshot behind its record.
    fn deliverables_root(&self) -> PathBuf {
        self.root.join("deliverables")
    }

    fn deliverable_dir(&self, id: Uuid) -> PathBuf {
        self.deliverables_root().join(id.to_string())
    }

    /// Snapshot `source` under `deliverables/<id>/`, returning the stored
    /// absolute path. The copy lands by renaming a staging sibling so a
    /// reader never sees a half-written tree; a failed copy removes the
    /// staging directory and leaves any previous snapshot in place.
    fn store_deliverable(&self, id: Uuid, source: &Path) -> anyhow::Result<String> {
        let base = source
            .file_name()
            .ok_or_else(|| anyhow!("deliverable path must name a file or folder"))?;
        let stage = self.deliverables_root().join(format!(".stage-{id}"));
        let target = self.deliverable_dir(id);
        let _ = fs::remove_dir_all(&stage);
        fs::create_dir_all(&stage)?;
        let mut budget = DeliverableBudget::default();
        if let Err(error) = copy_deliverable(source, &stage.join(base), &mut budget) {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
        let _ = fs::remove_dir_all(&target);
        fs::rename(&stage, &target)?;
        Ok(target.join(base).to_string_lossy().into_owned())
    }

    /// Drop copied snapshots no live record points at — a crash between the
    /// copy landing and the state save leaves the store dir behind.
    fn sweep_deliverable_store(&self, state: &BossState) {
        let live: std::collections::HashSet<String> = state
            .deliverables
            .iter()
            .filter(|deliverable| deliverable.source_path.is_some())
            .map(|deliverable| deliverable.id.to_string())
            .collect();
        let Ok(entries) = fs::read_dir(self.deliverables_root()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_str().is_some_and(|name| live.contains(name)) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// Copy `source` — file or directory — to `dest`, tracking the running
/// total in `budget`. Symlinks are refused rather than followed so a copied
/// tree cannot pull bytes from outside the publisher's workspace.
fn copy_deliverable(
    source: &Path,
    dest: &Path,
    budget: &mut DeliverableBudget,
) -> anyhow::Result<()> {
    let mut pending = vec![(source.to_path_buf(), dest.to_path_buf())];
    while let Some((source, dest)) = pending.pop() {
        let metadata = fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() {
            bail!("deliverables cannot contain symlinks: {}", source.display());
        }
        budget.entries += 1;
        if budget.entries > MAX_DELIVERABLE_ENTRIES {
            bail!("deliverable exceeds {MAX_DELIVERABLE_ENTRIES} entries");
        }
        if metadata.is_dir() {
            fs::create_dir_all(&dest)?;
            for entry in fs::read_dir(&source)? {
                let entry = entry?;
                pending.push((entry.path(), dest.join(entry.file_name())));
            }
        } else {
            budget.bytes += metadata.len();
            if budget.bytes > MAX_DELIVERABLE_BYTES {
                bail!(
                    "deliverable exceeds {} MiB",
                    MAX_DELIVERABLE_BYTES / (1024 * 1024)
                );
            }
            fs::copy(&source, &dest)?;
        }
    }
    Ok(())
}

/// Running totals a deliverable copy is allowed to spend — see
/// `MAX_DELIVERABLE_ENTRIES`/`MAX_DELIVERABLE_BYTES`.
#[derive(Default)]
struct DeliverableBudget {
    entries: usize,
    bytes: u64,
}

/// Return pinned document paths for the files-root `readFile` operation.
fn pinned_paths(pinned: &[String]) -> impl Iterator<Item = String> + '_ {
    pinned.iter().cloned()
}

fn validate_relative(path: &str, allow_empty: bool) -> anyhow::Result<()> {
    if (!allow_empty && path.is_empty())
        || path.contains('\\')
        || path.contains(':')
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("Boss paths must be relative and cannot contain traversal");
    }
    Ok(())
}

/// Move every entry `to` lacks out of `from` — the files merge for the
/// plan-document migration, recursing into directories that exist in both
/// and leaving name conflicts behind in `from`.
fn move_missing_entries(from: &Path, to: &Path) {
    let Ok(entries) = fs::read_dir(from) else {
        return;
    };
    for entry in entries.flatten() {
        let target = to.join(entry.file_name());
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && target.is_dir() {
            move_missing_entries(&entry.path(), &target);
            let _ = fs::remove_dir(entry.path());
        } else if !target.exists() {
            let _ = fs::rename(entry.path(), &target);
        }
    }
}

/// A legacy `memory/plans/` spelling resolves to the document's `plans/`
/// home — plan files moved out of memory wholesale, and a caller still
/// spelling the old path lands on the document rather than forking it
/// back into memory.
fn remap_plan_path(path: &str) -> String {
    let normalized = normalize_plan_path(path);
    if normalized == "memory/plans" {
        return "plans".to_owned();
    }
    normalized
        .strip_prefix("memory/plans/")
        .map(|rest| format!("plans/{rest}"))
        .unwrap_or(normalized)
}

/// Collapse separators in a validated Boss path so `"plans//x.md"` and
/// `"plans/x.md"` compare equal — the freeze check cannot be dodged by
/// spelling the same file another way.
fn normalize_plan_path(path: &str) -> String {
    Path::new(path)
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Resolve a plan reference — a `BossPlan::id` or planning-session id in
/// UUID form, or any accepted `plans/<file>.md` spelling — to the record.
fn find_plan<'a>(planning: &'a [BossPlan], reference: &str) -> Option<&'a BossPlan> {
    let reference = reference.trim();
    if let Ok(id) = Uuid::parse_str(reference) {
        return planning
            .iter()
            .find(|plan| plan.id == id || plan.session_id == id);
    }
    let plan_file = normalize_plan_file(reference).ok()?;
    planning.iter().find(|plan| plan.plan_file == plan_file)
}

/// The mutable half of [`find_plan`] for bookkeeping operations.
fn plan_mut<'a>(state: &'a mut BossState, reference: &str) -> anyhow::Result<&'a mut BossPlan> {
    let index = state
        .planning
        .iter()
        .position(|plan| find_plan(std::slice::from_ref(plan), reference).is_some())
        .ok_or_else(|| anyhow!("unknown plan {reference}"))?;
    Ok(&mut state.planning[index])
}

/// Resolve a `plan`+`item` tag against the registry — the stored
/// `planId`/`itemId` pair, or the reason the tag is invalid.
fn resolve_plan_assignment(
    state: &BossState,
    reference: &str,
    item: Option<Uuid>,
) -> anyhow::Result<(Uuid, Option<Uuid>)> {
    let plan =
        find_plan(&state.planning, reference).ok_or_else(|| anyhow!("unknown plan {reference}"))?;
    if plan.terminal() {
        bail!(
            "plan {} is {}; reopen it to tag work",
            plan.plan_file,
            plan.outcome().label()
        );
    }
    let item = match item {
        Some(item) => {
            let entry = plan
                .items
                .iter()
                .find(|known| known.id == item)
                .ok_or_else(|| anyhow!("unknown work item {item} in plan {}", plan.plan_file))?;
            match entry.state {
                PlanItemState::ToDo => Some(item),
                state => bail!(
                    "work item \"{}\" in plan {} is {}; reopen it to tag work",
                    entry.title,
                    plan.plan_file,
                    state.label()
                ),
            }
        }
        None => None,
    };
    Ok((plan.id, item))
}

/// Stamp an approved plan: freeze time, outcome, and the approval audit
/// entry — always the user's act, since every finalization passes a
/// user-facing approval — and, when the caller declared one, the work
/// breakdown seeded from the document's course of work.
fn apply_plan_approval(
    plan: &mut BossPlan,
    items: Option<Vec<String>>,
    now: u64,
) -> anyhow::Result<()> {
    plan.finalized_at = Some(now);
    plan.outcome = Some(PlanOutcome::Approved);
    plan.history.push(PlanTransition {
        outcome: PlanOutcome::Approved,
        at: now,
        actor: PlanActor::User,
    });
    if let Some(items) = items {
        for title in &items {
            anyhow::ensure!(!title.trim().is_empty(), "work item titles cannot be empty");
        }
        // The doc is the source at approval — earlier drafts of the
        // breakdown drop to the struck tail rather than linger as
        // declared work the user never signed off on.
        let mut next: Vec<PlanItem> = items
            .into_iter()
            .map(|title| PlanItem {
                id: Uuid::new_v4(),
                title: title.trim().to_owned(),
                state: PlanItemState::ToDo,
                history: Vec::new(),
            })
            .collect();
        for mut item in std::mem::take(&mut plan.items) {
            if item.state != PlanItemState::Dropped {
                item.state = PlanItemState::Dropped;
                item.history.push(PlanItemTransition {
                    state: PlanItemState::Dropped,
                    at: now,
                    actor: PlanActor::User,
                });
            }
            next.push(item);
        }
        plan.items = next;
    }
    Ok(())
}

/// Canonical `plans/<name>.md` beneath the files root, from whatever the
/// caller sent — `auth.md`, `plans/auth.md`, `workspace/plans/auth.md`,
/// and the legacy `memory/plans/auth.md` all name the same document.
/// Nested subdirectories beneath `plans/` stay nested; traversal, absolute
/// paths, and non-Markdown names fail. Validation runs before
/// normalization so a `..` fails loudly instead of collapsing into an
/// unrelated plan name; the normalized-then-stripped order matches
/// `plan_file_frozen`, so every spelling of one document freezes together.
pub fn normalize_plan_file(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    validate_relative(trimmed, false)?;
    let cleaned = normalize_plan_path(trimmed);
    let rest = cleaned
        .strip_prefix("memory/")
        .or_else(|| cleaned.strip_prefix("workspace/"))
        .unwrap_or(&cleaned);
    let rest = rest.strip_prefix("plans/").unwrap_or(rest);
    if rest.is_empty() || !rest.ends_with(".md") {
        bail!("plan files are named like `auth.md` under plans/");
    }
    Ok(format!("plans/{rest}"))
}

/// Curated first names employees draw randomly from, excluding held names.
const EMPLOYEE_NAMES: &[&str] = &[
    "Alden",
    "Ansel",
    "Blythe",
    "Celia",
    "Dorian",
    "Edith",
    "Elin",
    "Emery",
    "Estelle",
    "Flora",
    "Galen",
    "Hugo",
    "Ida",
    "Inez",
    "Ivo",
    "Leander",
    "Lenora",
    "Linus",
    "Lucian",
    "Maren",
    "Mavis",
    "Milo",
    "Nell",
    "Orson",
    "Otis",
    "Petra",
    "Rhea",
    "Rosalind",
    "Rufus",
    "Selma",
    "Soren",
    "Sylvie",
    "Thalia",
    "Thea",
    "Tobin",
    "Vera",
    "Willa",
    "Cleo",
    "Ada",
    "Ambrose",
    "Abigail",
    "Adelaide",
    "Agnes",
    "Alma",
    "Amara",
    "Amos",
    "Arthur",
    "Astrid",
    "Beatrice",
    "Beckett",
    "Benedict",
    "Bernadette",
    "Calder",
    "Calliope",
    "Cassian",
    "Cecily",
    "Clementine",
    "Conrad",
    "Cordelia",
    "Cosima",
    "Desmond",
    "Dorothea",
    "Eleanor",
    "Elias",
    "Eliza",
    "Emmeline",
    "Ephraim",
    "Etta",
    "Evelyn",
    "Felix",
    "Fern",
    "Finch",
    "Florence",
    "Frances",
    "Frederick",
    "Genevieve",
    "Georgia",
    "Greta",
    "Gwendolyn",
    "Harriet",
    "Hazel",
    "Heath",
    "Henrietta",
    "Isadora",
    "Isidore",
    "Jasper",
    "Josephine",
    "Julian",
    "Juniper",
    "Lavinia",
    "Lazarus",
    "Lillian",
    "Lottie",
    "Louisa",
    "Magnus",
    "Matilda",
    "Maude",
    "Maxwell",
    "Mirabel",
    "Nico",
    "Nina",
    "Noel",
    "Octavia",
    "Opal",
    "Oscar",
    "Penelope",
    "Percival",
    "Phoebe",
    "Quentin",
    "Quincy",
    "Ramona",
    "Reuben",
    "Rowan",
    "Sabine",
    "Silas",
    "Simone",
    "Sterling",
    "Tamsin",
    "Theodore",
    "Ulysses",
    "Valentina",
    "Victor",
    "Viola",
    "Vivian",
    "Wallace",
    "Wilfred",
    "Winifred",
    "Xanthe",
    "Yvette",
    "Zelda",
    "Zinnia",
    "Aurelia",
    "Basil",
    "Cyrus",
    "Delphine",
    "Evander",
    "Felicity",
    "Gideon",
    "Hollis",
    "Imogen",
    "Juno",
    "Kit",
    "Lydia",
    "Marcel",
    "Nadia",
    "Odette",
    "Peregrine",
    "Romy",
    "Sasha",
    "Tilda",
    "Una",
    "Violet",
    "Wesley",
    "Yara",
    "Zachary",
    "Alistair",
    "Briony",
    "Cora",
    "Daphne",
    "Edmund",
    "Freya",
    "Graham",
    "Iris",
    "Jonah",
    "Kieran",
    "Margot",
    "Nora",
    "Rafael",
    "Stella",
    "Thomas",
    "Wren",
    "Ari",
    "Bram",
    "Caspian",
    "Della",
    "Esme",
    "Faye",
    "Harlan",
    "Ivolette",
    "Lyle",
    "Mina",
    "Niles",
    "Rory",
    "Agatha",
    "Albert",
    "Alfred",
    "Althea",
    "Amalia",
    "Anton",
    "Antonia",
    "Arlo",
    "Aubrey",
    "Augustine",
    "Barnaby",
    "Bernice",
    "Bertram",
    "Blanche",
    "Bramwell",
    "Bridget",
    "Bruno",
    "Caleb",
    "Camille",
    "Carina",
    "Cedric",
    "Celeste",
    "Chester",
    "Clara",
    "Clarence",
    "Claude",
    "Colette",
    "Cornelius",
    "Cressida",
    "Dahlia",
    "Damian",
    "Dashiell",
    "Deborah",
    "Declan",
    "Delia",
    "Dinah",
    "Dominic",
    "Doris",
    "Duncan",
    "Edgar",
    "Edwina",
    "Eileen",
    "Eliot",
    "Eloise",
    "Elsa",
    "Elsie",
    "Emilia",
    "Emmett",
    "Enid",
    "Enzo",
    "Ernest",
    "Esther",
    "Ethel",
    "Eudora",
    "Eunice",
    "Ezra",
    "Ferdinand",
    "Fletcher",
    "Florian",
    "Francesca",
    "Franklin",
    "Frida",
    "Gabriel",
    "Gemma",
    "Geoffrey",
    "Gerald",
    "Gilbert",
    "Gloria",
    "Gordon",
    "Gregory",
    "Guinevere",
    "Gus",
    "Hannah",
    "Harold",
    "Harvey",
    "Hector",
    "Helena",
    "Herbert",
    "Herman",
    "Hester",
    "Horace",
    "Howard",
    "Ignatius",
    "Ilse",
    "Ingrid",
    "Ira",
    "Irene",
    "Irving",
    "Isaac",
    "Ivan",
    "Jacques",
    "Jerome",
    "Joan",
    "Jocelyn",
    "Judith",
    "Kathleen",
    "Kenneth",
    "Klaus",
    "Lambert",
    "Laurence",
    "Leah",
    "Leon",
    "Leonard",
    "Leopold",
    "Lester",
    "Lionel",
    "Lorena",
    "Lucille",
    "Luther",
    "Maeve",
    "Malcolm",
    "Margaret",
    "Marian",
    "Marina",
    "Marjorie",
    "Marlene",
    "Martin",
    "Matthias",
    "Mercy",
    "Millicent",
    "Minerva",
    "Miriam",
    "Morris",
    "Muriel",
    "Myrtle",
    "Nadine",
    "Nathaniel",
    "Nelson",
    "Nicola",
    "Nikolai",
    "Norma",
    "Obadiah",
    "Odessa",
    "Olive",
    "Oliver",
    "Olympia",
    "Omar",
    "Orville",
    "Oswald",
    "Otto",
    "Owen",
    "Pablo",
    "Palmer",
    "Patience",
    "Patrick",
    "Paul",
    "Pearl",
    "Philippa",
    "Phineas",
    "Phyllis",
    "Porter",
    "Prudence",
    "Randall",
    "Raphael",
    "Raymond",
    "Rebecca",
    "Regina",
    "Rex",
    "Rhett",
    "Rita",
    "Roland",
    "Rosamund",
    "Rose",
    "Rosemary",
    "Ross",
    "Roxana",
    "Rudolph",
    "Russell",
    "Ruth",
    "Sadie",
    "Salvador",
    "Samson",
    "Samuel",
    "Sebastian",
    "Seraphina",
    "Seymour",
    "Shirley",
    "Sibyl",
    "Sidney",
    "Solomon",
    "Sonia",
    "Stanley",
    "Susannah",
    "Sven",
    "Tabitha",
    "Tamara",
    "Tatiana",
    "Tessa",
    "Thaddeus",
    "Theodora",
    "Theresa",
    "Trudy",
    "Ursula",
    "Upton",
    "Valentine",
    "Vanessa",
    "Vernon",
    "Veronica",
    "Vincent",
    "Virgil",
    "Virginia",
    "Walter",
    "Wendell",
    "Wilbur",
    "Wilhelmina",
    "Winslow",
    "Wolfgang",
    "Woodrow",
    "Xavier",
    "Yolanda",
    "Yusuf",
    "Yvonne",
    "Zadie",
    "Zane",
    "Zora",
];

/// Bijective base-26 suffix: 1 → "A", 26 → "Z", 27 → "AA", and so on.
fn surname_suffix(mut value: usize) -> String {
    let mut suffix = String::new();
    while value > 0 {
        value -= 1;
        suffix.insert(0, (b'A' + (value % 26) as u8) as char);
        value /= 26;
    }
    suffix
}

/// Draw from a shuffled pool, preferring initials not held by the roster.
/// Retired names become available again; only a fully held pool requires a
/// suffix. The durable cursor also separates draws prepared before insertion.
fn employee_human_name<'a>(
    existing_names: impl IntoIterator<Item = &'a str>,
    cursor: &mut u64,
) -> String {
    static SHUFFLED_NAMES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    let names = SHUFFLED_NAMES.get_or_init(|| {
        let mut names = EMPLOYEE_NAMES.to_vec();
        // UUID v4 supplies random sort keys without another RNG dependency.
        names.sort_by_cached_key(|_| Uuid::new_v4());
        names
    });
    let existing_names = existing_names
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let existing_initials = existing_names
        .iter()
        .filter_map(|name| name.as_bytes().first().map(u8::to_ascii_uppercase))
        .collect::<std::collections::HashSet<_>>();
    let len = names.len() as u64;
    for prefer_unused_initial in [true, false] {
        for step in 0..len {
            let index = ((*cursor % len) + step) % len;
            let candidate = names[index as usize];
            let unused_initial = candidate
                .as_bytes()
                .first()
                .is_some_and(|initial| !existing_initials.contains(&initial.to_ascii_uppercase()));
            if !existing_names.contains(candidate) && (!prefer_unused_initial || unused_initial) {
                *cursor = (*cursor).saturating_add(step + 1);
                return candidate.into();
            }
        }
    }

    let base = names[(*cursor % len) as usize];
    *cursor = (*cursor).saturating_add(1);
    let start = base
        .bytes()
        .fold(0u8, |acc, byte| acc.wrapping_mul(31).wrapping_add(byte))
        % 26;
    for step in 0..26u8 {
        let initial = (b'A' + (start + step) % 26) as char;
        let candidate = format!("{base} {initial}.");
        if !existing_names.contains(candidate.as_str()) {
            return candidate;
        }
    }
    for value in 27usize.. {
        let candidate = format!("{base} {}.", surname_suffix(value));
        if !existing_names.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("there is always an unused employee name suffix")
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.trim().is_empty() || name.chars().count() > 100 {
        bail!("name must contain 1–100 characters");
    }
    Ok(())
}

/// Keep the lifecycle field and the `expired` projection consistent for
/// documents written before `state` existed: the flag was the only
/// record, so it wins wherever they disagree.
fn normalize_lifecycle(employee: &mut BossEmployee) {
    if employee.expired {
        employee.state = EmployeeLifecycle::Expired;
    } else if employee.state == EmployeeLifecycle::Expired {
        employee.expired = true;
        if employee.expired_at.is_none() {
            employee.expired_at = Some(waku_protocol::model::unix_time());
        }
    }
}

/// Which employees a restart cut off mid-flight. Queued tickets hold no
/// runtime or claims, so they re-dispatch instead of finishing as
/// interrupted; `dispatching`/`finishing` records reconcile through
/// `recover_interrupted` like working ones. A `queued` record without a
/// ticket is malformed or predates admission — it interrupts like a
/// working record.
fn restart_interrupted(employee: &BossEmployee) -> bool {
    if employee.expired {
        return false;
    }
    employee.lifecycle() != EmployeeLifecycle::Queued || employee.ticket.is_none()
}

/// Admit the employee under its ticket's wave id — first admission
/// creates the record, later ones append. A member joining a resolved
/// wave reopens it: the enlarged membership resolves once more.
fn join_wave(state: &mut BossState, group: &str, employee: &BossEmployee) {
    let member = WaveMember {
        session_id: employee.session_id,
        outcome: None,
    };
    match state.waves.iter_mut().find(|wave| wave.id == group) {
        Some(wave) => {
            wave.resolved_at = None;
            if !wave
                .members
                .iter()
                .any(|entry| entry.session_id == employee.session_id)
            {
                wave.members.push(member);
            }
        }
        None => state.waves.push(BossWave {
            id: group.to_owned(),
            supervisor_id: employee.supervisor_id,
            members: vec![member],
            resolved_at: None,
        }),
    }
}

/// A member re-entered admission or revived — it is in flight again, so
/// its recorded outcome clears and a resolved wave reopens.
fn wave_member_in_flight(state: &mut BossState, session: Uuid) {
    for wave in &mut state.waves {
        if let Some(member) = wave
            .members
            .iter_mut()
            .find(|entry| entry.session_id == session)
        {
            member.outcome = None;
            wave.resolved_at = None;
        }
    }
}

/// Record a member's terminal outcome inside the state update that
/// commits it, then resolve the wave when every member has one: tally
/// the membership, stamp `resolved_at`, and park the completion notice
/// in `wave_outbox` — one durable write, so a restart can neither lose
/// the resolution nor fire it twice.
fn record_wave_outcome(
    state: &mut BossState,
    group: &str,
    session: Uuid,
    supervisor_id: Uuid,
    outcome: WaveMemberOutcome,
    now: u64,
) {
    let wave = match state.waves.iter_mut().find(|wave| wave.id == group) {
        Some(wave) => wave,
        None => {
            // A summon written before wave records existed — build the
            // record late so the group still resolves.
            state.waves.push(BossWave {
                id: group.to_owned(),
                supervisor_id,
                members: Vec::new(),
                resolved_at: None,
            });
            state.waves.last_mut().unwrap()
        }
    };
    match wave
        .members
        .iter_mut()
        .find(|entry| entry.session_id == session)
    {
        Some(member) => member.outcome = Some(outcome),
        None => wave.members.push(WaveMember {
            session_id: session,
            outcome: Some(outcome),
        }),
    }
    if wave.resolved_at.is_none()
        && !wave.members.is_empty()
        && wave.members.iter().all(|entry| entry.outcome.is_some())
    {
        wave.resolved_at = Some(now);
        let tally = |outcome| {
            wave.members
                .iter()
                .filter(|entry| entry.outcome == Some(outcome))
                .count() as u32
        };
        state.next_event_id = state.next_event_id.saturating_add(1);
        state.wave_outbox.push(WaveNotification {
            id: state.next_event_id,
            wave_id: group.to_owned(),
            supervisor_id: wave.supervisor_id,
            finished: tally(WaveMemberOutcome::Finished),
            failed: tally(WaveMemberOutcome::Failed),
            cancelled: tally(WaveMemberOutcome::Cancelled),
            created_at: now,
            delivered_at: None,
        });
    }
}

/// Startup repair: a member whose outcome never landed — an expiry path
/// that predates waves, or a crash between writes — would stall its wave
/// forever. Fold every expired member's record back into the tally, then
/// resolve whatever went all-terminal; the pushed notice drains through
/// the normal outbox delivery.
fn reconcile_waves(state: &mut BossState, now: u64) {
    let expired: Vec<(Uuid, WaveMemberOutcome, Uuid)> = state
        .employees
        .iter()
        .chain(state.retired_employees.iter())
        .filter(|entry| entry.lifecycle() == EmployeeLifecycle::Expired)
        .map(|entry| {
            let outcome = if entry.cancelled {
                WaveMemberOutcome::Cancelled
            } else if entry.blocker.is_some() {
                WaveMemberOutcome::Failed
            } else {
                WaveMemberOutcome::Finished
            };
            (entry.session_id, outcome, entry.supervisor_id)
        })
        .collect();
    let groups: Vec<String> = state
        .waves
        .iter()
        .filter(|wave| wave.resolved_at.is_none())
        .flat_map(|wave| {
            wave.members
                .iter()
                .filter(|member| member.outcome.is_none())
                .map(|_| wave.id.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    for group in groups {
        let members: Vec<Uuid> = state
            .waves
            .iter()
            .find(|wave| wave.id == group)
            .map(|wave| {
                wave.members
                    .iter()
                    .filter(|member| member.outcome.is_none())
                    .map(|member| member.session_id)
                    .collect()
            })
            .unwrap_or_default();
        for session in members {
            if let Some((_, outcome, supervisor)) = expired.iter().find(|(id, _, _)| *id == session)
            {
                record_wave_outcome(state, &group, session, *supervisor, *outcome, now);
            }
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(temporary);
        return Err(error.into());
    }
    Ok(())
}

fn fresh_state() -> BossState {
    let id = Uuid::new_v4();
    let persona_id = Uuid::new_v4();
    let employee_id = Uuid::new_v4();
    let names = ["Atlas", "Nova", "Sage", "Orion", "Clover", "Quinn"];
    BossState {
        identity: BossIdentity { id, name: names[id.as_bytes()[0] as usize % names.len()].into(), avatar_seed: id.to_string() },
        persona_id,
        session_id: None,
        personas: vec![
            BossPersona { id: employee_id, name: "Employee".into(), markdown: "Complete the bounded job assigned by your supervisor. Report useful results concisely. You have no memory of your own and must not write memory. Read only the memory granted to or pinned by your persona.".into(), pinned_files: Vec::new(), permissions: PersonaPermissions::default() , icon: None },
            BossPersona { id: persona_id, name: "Boss".into(), markdown: "You coordinate employees for the human. Heavy delegation is your default: assign code changes, research, internet access, builds, code generation, long-running checks and tests, for daemon-managed worktrees, require each assigned employee to include 'agent-merge per rules', commit its unit, and run 'goddard-agent merge submit' itself; do not summon a separate Worktree Integrator. Employees report conflicts or verification failures with 'reportBlocker' and report the landed SHA on success. Never run or poll long-running commands yourself. Ask employees to use shared build caches or dedicated output directories when that avoids contention with the user's tools. Never poll, watch, or wait yourself — hand recurring checks and waits to an employee; mark each summon `workGoal` — an `errand` when you need to know when it finishes (its finish reports back to you), a `goal` when it finishes without you (its record lands on the human's Goals page instead) — a finish also reaches you when the employee flagged a blocker or its session failed. Use `roster` for a cheap status check, `view` for employee details, and `context` for the user's work. Verify completion from the worktree and its commits before reporting work done; do not rely on a summary alone.\n\nUse `steer` for mid-flight corrections that change what the employee is writing right now — a steer that redirects the assignment itself may carry `jobTitle` to relabel the job. Use `prompt` for content whose relevance starts after the current step, such as queue additions or follow-ups. Prompt or steer can resume an employee after its idle expiry with the same transcript; summon a fresh employee for a new or distinct job, or when the previous employee is dead or finishing; never stack prompts onto an expiring employee, where queued work may be lost.\n\nTrack employee ownership, worktrees, and landed versus in-flight work in the appropriate memory bucket, and reconcile the notes as work changes. Publish useful employee outputs as deliverables. Report outcomes and blockers only; the human does not need narration about expired employees, name releases, expiry timers, summons, integration mechanics, or other internal Boss operations. Speak when work completes, a timely interruption will help the human, the human needs to act, or they ask; stay quiet otherwise. Respect user-set resource constraints, including model routing and employee caps, and preserve them durably in memory. Build and maintain a reusable persona library across projects: notice recurring work patterns, create named purpose-specific roles such as Researcher, Feature Developer, Bug Investigator, or Verifier, and refine existing roles as experience accumulates. Inspect existing personas before adding one; improve a close match instead of creating duplicates. Keep each persona's guidance focused on durable methods and boundaries that transfer across projects. Maintain personas and your own files. Your memory is a standing duty: record durable facts and decisions in explicit named buckets, keep notes concise and scoped, and append corrections rather than rewriting history. You control all employees and personas.\n\nDispatch speed: when the human hands you a task, summon promptly — seconds, not minutes. Do not research the codebase before summoning. The only pre-summon research allowed is identifying which project the task belongs to when that is genuinely ambiguous. Write a competent brief and let the employee locate files, verify line numbers, and orient itself — that is what employees are for.\n\nEmployees assigned to a project automatically receive read and insert access to its shared project bucket. Grant additional Boss-created buckets deliberately through persona bucket IDs or per-employee permission overrides. Personal buckets remain Boss-only unless granted. Pinned files are documents only; they do not grant memory access; a per-field `permissions` object on summon — or `setPermissions` via control — tailors one employee's grants without editing the persona. Choose a purpose-specific jobTitle when summoning each employee; Goddard assigns their human name. Choose the employee's `icon` deliberately from the summon field — pick the icon that fits the actual work rather than leaving it to the job-title heuristic, and give jobs accurate titles since titles feed the fallback classifier.".into(), pinned_files: Vec::new(), permissions: PersonaPermissions { summon_employees: true, ..Default::default() } , icon: None },
        ],
        employees: Vec::new(),
        retired_employees: Vec::new(),
        deliverables: Vec::new(),
        planning: Vec::new(),
        goals_viewed_at: None,
        resource_policy: BossResourcePolicy::default(),
        next_sequence: 0,
        next_event_id: 0,
        name_cursor: 0,
        outbox: Vec::new(),
        waves: Vec::new(),
        wave_outbox: Vec::new(),
        revision: 0,
    }
}

fn disabled_state() -> BossState {
    BossState {
        identity: BossIdentity {
            id: Uuid::nil(),
            name: String::new(),
            avatar_seed: String::new(),
        },
        persona_id: Uuid::nil(),
        session_id: None,
        personas: Vec::new(),
        employees: Vec::new(),
        retired_employees: Vec::new(),
        deliverables: Vec::new(),
        planning: Vec::new(),
        goals_viewed_at: None,
        resource_policy: BossResourcePolicy::default(),
        next_sequence: 0,
        next_event_id: 0,
        name_cursor: 0,
        outbox: Vec::new(),
        waves: Vec::new(),
        wave_outbox: Vec::new(),
        revision: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_urls_allow_http_and_https_only() {
        assert!(validate_browse_url("http://example.com").is_ok());
        assert!(validate_browse_url("https://example.com/path?q=1").is_ok());
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.com",
            "https://",
        ] {
            assert!(validate_browse_url(url).is_err(), "accepted {url}");
        }
    }
    use waku_protocol::boss::BossEmployee;
    use waku_protocol::custom_commands::CustomCommandIcon;

    #[test]
    fn disabled_service_does_not_read_or_create_boss_storage() {
        let root = std::env::temp_dir().join(format!("boss-disabled-{}", Uuid::new_v4()));
        let service = BossService::disabled(root.clone());
        assert!(service.document().session_id.is_none());
        assert!(service.document().identity.id.is_nil());
        assert!(!root.exists());
    }

    #[test]
    fn employee_names_are_distinct_until_the_pool_is_held() {
        let mut cursor = 0;
        let mut held = Vec::new();
        for _ in EMPLOYEE_NAMES {
            let name = employee_human_name(held.iter().map(String::as_str), &mut cursor);
            assert!(EMPLOYEE_NAMES.contains(&name.as_str()));
            assert!(!held.contains(&name));
            held.push(name);
        }
        let suffixed = employee_human_name(held.iter().map(String::as_str), &mut cursor);
        assert!(!held.contains(&suffixed));
        assert!(suffixed.ends_with('.'));
    }

    #[test]
    fn employee_names_prefer_unused_initials_until_the_alphabet_is_held() {
        let mut cursor = 0;
        let mut held = Vec::new();
        let mut initials = std::collections::HashSet::new();
        let initial_capacity = EMPLOYEE_NAMES
            .iter()
            .filter_map(|name| name.as_bytes().first().copied())
            .collect::<std::collections::HashSet<_>>()
            .len()
            .min(26);

        for _ in 0..initial_capacity {
            let name = employee_human_name(held.iter().map(String::as_str), &mut cursor);
            let initial = name.as_bytes()[0].to_ascii_uppercase();
            assert!(
                initials.insert(initial),
                "duplicate initial in {held:?} + {name}"
            );
            held.push(name);
        }

        let repeated = employee_human_name(held.iter().map(String::as_str), &mut cursor);
        assert!(initials.contains(&repeated.as_bytes()[0].to_ascii_uppercase()));
    }

    #[test]
    fn employee_names_reuse_the_only_free_name_and_skip_held_suffixes() {
        let mut cursor = 0;
        let free = EMPLOYEE_NAMES[0];
        assert_eq!(
            employee_human_name(EMPLOYEE_NAMES.iter().copied().skip(1), &mut cursor),
            free
        );
        let mut held: Vec<String> = EMPLOYEE_NAMES.iter().map(|name| name.to_string()).collect();
        for base in EMPLOYEE_NAMES {
            for initial in 'A'..='Z' {
                held.push(format!("{base} {initial}."));
            }
        }
        let name = employee_human_name(held.iter().map(String::as_str), &mut cursor);
        assert!(!held.contains(&name));
        assert!(name.ends_with(" AA."));
    }

    #[test]
    fn employee_summon_batch_names_are_distinct_across_a_restart() {
        let root = std::env::temp_dir().join(format!("boss-name-rotation-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[0].id;
        let first = service
            .prepare_employee(
                boss,
                persona,
                "One".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        service.add_employee(first.clone()).unwrap();
        drop(service);

        let reopened = BossService::open(root.clone()).unwrap();
        let second = reopened
            .prepare_employee(
                boss,
                persona,
                "Two".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        assert_ne!(first.identity.name, second.identity.name);
        reopened.add_employee(second).unwrap();
        let mut batch = Vec::new();
        for index in 0..32 {
            let employee = reopened
                .prepare_employee(
                    boss,
                    persona,
                    format!("Job {index}"),
                    None,
                    EmployeeGoal::Errand,
                    None,
                )
                .unwrap();
            batch.push(employee);
        }
        for employee in batch {
            reopened.add_employee(employee).unwrap();
        }
        let state = reopened.document();
        let names: std::collections::HashSet<_> = state
            .employees
            .iter()
            .map(|employee| employee.identity.name.as_str())
            .collect();
        assert_eq!(names.len(), state.employees.len());
        assert!(names.iter().all(|name| EMPLOYEE_NAMES.contains(name)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retirement_releases_employee_name_and_keeps_roster_persisted() {
        let root = std::env::temp_dir().join(format!("boss-retire-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let state = service.document();
        let employee = service
            .prepare_employee(
                boss,
                state.personas[0].id,
                "Review".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let session_id = employee.session_id;
        let name = employee.identity.name.clone();
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        service.expire(session_id).unwrap();
        service
            .update(|state| {
                state.employees[0].expired_at = Some(100);
                Ok(())
            })
            .unwrap();

        assert!(service.retire_expired(3_699).unwrap().is_empty());
        assert!(service.is_employee(session_id));
        let retired = service.retire_expired(3_700).unwrap();
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].identity.name, name);
        assert!(!service.is_employee(session_id));
        let mut cursor = service.document().name_cursor;
        assert_eq!(
            employee_human_name(
                EMPLOYEE_NAMES
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate != name),
                &mut cursor,
            ),
            name,
            "the released name can be assigned again"
        );

        let restored = BossService::open(root.clone()).unwrap();
        assert!(!restored.is_employee(session_id));
        assert!(restored.resurrect(session_id).unwrap());
        assert!(restored.is_employee(session_id));
        assert_eq!(restored.employee(session_id).unwrap().identity.name, name);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retired_employee_cannot_revive_after_its_name_is_reused() {
        let root = std::env::temp_dir().join(format!("boss-revive-reused-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let employee = service
            .prepare_employee(
                boss,
                service.document().personas[0].id,
                "Review".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let old_id = employee.session_id;
        let old_name = employee.identity.name.clone();
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        service.expire(old_id).unwrap();
        service
            .update(|state| {
                state.employees[0].expired_at = Some(1);
                Ok(())
            })
            .unwrap();
        assert_eq!(service.retire_expired(3_601).unwrap().len(), 1);
        let mut replacement = service
            .prepare_employee(
                boss,
                service.document().personas[0].id,
                "New job".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        replacement.identity.name = old_name;
        service
            .update(|state| {
                state.employees.push(replacement);
                Ok(())
            })
            .unwrap();
        let error = service.resurrect(old_id).unwrap_err().to_string();
        assert!(
            error.contains("name has been reused; summon a new employee"),
            "{error}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// Goals stay visible for a day, then retire durably without losing
    /// the supervisor's ability to resume their task.
    #[test]
    fn finished_goals_retire_only_after_24_hours() {
        let root = std::env::temp_dir().join(format!("boss-goal-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let state = service.document();
        let employee = service
            .prepare_employee(
                boss,
                state.personas[0].id,
                "Watch".into(),
                None,
                EmployeeGoal::Goal,
                None,
            )
            .unwrap();
        let session_id = employee.session_id;
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        service.expire(session_id).unwrap();
        service
            .update(|state| {
                state.employees[0].expired_at = Some(100);
                Ok(())
            })
            .unwrap();

        assert!(service.retire_expired(100 + 23 * 60 * 60).unwrap().is_empty());
        assert!(service.is_employee(session_id));
        assert!(service.retire_expired(100 + 24 * 60 * 60).unwrap().is_empty());
        let retired = service.retire_expired(101 + 24 * 60 * 60).unwrap();
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].session_id, session_id);
        assert!(!service.is_employee(session_id));
        let restored = BossService::open(root.clone()).unwrap();
        assert!(!restored.is_employee(session_id));
        assert!(restored.resurrect(session_id).unwrap());
        assert!(restored.is_employee(session_id));
        fs::remove_dir_all(root).unwrap();
    }

    /// The Goals page stamps `goals_viewed_at` when it opens — the sidebar
    /// row's unread dot compares goal finishes against it, so only an owner
    /// may stamp.
    #[test]
    fn mark_goals_viewed_stamps_the_clock_and_is_owner_only() {
        let root = std::env::temp_dir().join(format!("boss-viewed-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        assert_eq!(service.document().goals_viewed_at, None);
        assert!(
            service
                .handle(Some(Uuid::new_v4()), BossOperation::MarkGoalsViewed)
                .is_err()
        );
        service
            .handle(None, BossOperation::MarkGoalsViewed)
            .unwrap();
        let stamped = service.document().goals_viewed_at;
        assert!(stamped.is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_boss_persona_prioritizes_delegation_and_commit_verification() {
        let state = fresh_state();
        let boss = state
            .personas
            .iter()
            .find(|persona| persona.id == state.persona_id)
            .unwrap();

        assert!(boss.markdown.contains("Heavy delegation is your default"));
        assert!(
            boss.markdown
                .contains("Never run or poll long-running commands yourself")
        );
        assert!(
            boss.markdown
                .contains("Verify completion from the worktree")
        );
        assert!(boss.markdown.contains("employee caps"));
        assert!(boss.markdown.contains("Personal buckets remain Boss-only"));
        assert!(boss.markdown.contains("automatically receive read and insert access"));
    }

    #[test]
    fn report_blocker_flags_the_employee_and_clears_on_resurrect() {
        let root = std::env::temp_dir().join(format!("boss-blocker-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let employee = service
            .prepare_employee(
                boss,
                service.document().personas[0].id,
                "Review".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let session_id = employee.session_id;
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        // Only a live employee flags its own job, with a bounded message.
        assert!(service.report_blocker(boss, "self".into()).is_err());
        assert!(service.report_blocker(session_id, "   ".into()).is_err());
        assert!(
            service
                .report_blocker(session_id, "x".repeat(MAX_BLOCKER_CHARS + 1))
                .is_err()
        );
        let flagged = service
            .report_blocker(session_id, " needs an approval ".into())
            .unwrap();
        assert_eq!(flagged.blocker.as_deref(), Some("needs an approval"));
        // Reports land with a live supervisor employee and escalate to the
        // boss session once that supervisor expires.
        let supervisor_id = Uuid::new_v4();
        service
            .update(|state| {
                state.employees.push(BossEmployee {
                    session_id: supervisor_id,
                    supervisor_id: boss,
                    identity: BossIdentity {
                        id: supervisor_id,
                        name: "Super".into(),
                        avatar_seed: supervisor_id.to_string(),
                    },
                    job_title: "Super".into(),
                    persona_id: state.personas[1].id,
                    work_goal: EmployeeGoal::Errand,
                    created_at: None,
                    icon: None,
                    permissions: PersonaPermissions::default(),
                    pinned_files: Vec::new(),
                    expired: false,
                    expired_at: None,
                    blocker: None,
                    cancelled: false,
                    expiry: None,
                    state: EmployeeLifecycle::Working,
                    ticket: None,
                    queued_at: None,
                    request_id: None,
                    request_fingerprint: None,
                    plan_id: None,
                    item_id: None,
                });
                state
                    .employees
                    .iter_mut()
                    .find(|entry| entry.session_id == session_id)
                    .unwrap()
                    .supervisor_id = supervisor_id;
                Ok(())
            })
            .unwrap();
        let flagged = service.employee(session_id).unwrap();
        assert_eq!(service.report_target(&flagged), Some(supervisor_id));
        service.expire(supervisor_id).unwrap();
        assert_eq!(service.report_target(&flagged), Some(boss));
        // A steer delivered mid-turn answers the flag in place — the record
        // clears without a resurrection, and clearing an unflagged or
        // unmanaged session is a no-op.
        service.clear_employee_blocker(session_id).unwrap();
        assert_eq!(service.employee(session_id).unwrap().blocker, None);
        service.clear_employee_blocker(Uuid::new_v4()).unwrap();
        service
            .report_blocker(session_id, "still waiting".into())
            .unwrap();
        // Resurrection hands the employee a fresh job — the old flag goes
        // with the job that raised it.
        service.expire(session_id).unwrap();
        assert!(
            service
                .report_blocker(session_id, "too late".into())
                .is_err()
        );
        assert!(service.resurrect(session_id).unwrap());
        assert_eq!(service.employee(session_id).unwrap().blocker, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn boss_can_set_and_clear_employee_icon_override() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let session_id = Uuid::new_v4();
        service
            .update(|state| {
                state.employees.push(BossEmployee {
                    session_id,
                    supervisor_id: state.identity.id,
                    identity: BossIdentity {
                        id: session_id,
                        name: "Nova".into(),
                        avatar_seed: session_id.to_string(),
                    },
                    job_title: "Research".into(),
                    persona_id: state.personas[1].id,
                    work_goal: EmployeeGoal::Errand,
                    created_at: None,
                    icon: None,
                    permissions: PersonaPermissions::default(),
                    pinned_files: Vec::new(),
                    expired: false,
                    expired_at: None,
                    blocker: None,
                    cancelled: false,
                    expiry: None,
                    state: EmployeeLifecycle::Working,
                    ticket: None,
                    queued_at: None,
                    request_id: None,
                    request_fingerprint: None,
                    plan_id: None,
                    item_id: None,
                });
                Ok(())
            })
            .unwrap();
        service
            .handle(
                None,
                BossOperation::SetEmployeeIcon {
                    session_id,
                    icon: Some(CustomCommandIcon::Star),
                },
            )
            .unwrap();
        assert_eq!(
            service.document().employees[0].icon,
            Some(CustomCommandIcon::Star)
        );
        service
            .handle(
                None,
                BossOperation::SetEmployeeIcon {
                    session_id,
                    icon: None,
                },
            )
            .unwrap();
        assert_eq!(service.document().employees[0].icon, None);
        assert!(
            service
                .handle(
                    None,
                    BossOperation::SetEmployeeIcon {
                        session_id,
                        icon: Some(CustomCommandIcon::Bot),
                    },
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn personas_reject_icons_outside_the_employee_set() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let mut persona = service.document().personas[0].clone();
        persona.icon = Some(CustomCommandIcon::Bot);
        assert!(
            service
                .handle(
                    None,
                    BossOperation::UpsertPersona {
                        persona: persona.clone().into()
                    },
                )
                .is_err()
        );
        persona.icon = Some(CustomCommandIcon::Search);
        assert!(
            service
                .handle(
                    None,
                    BossOperation::UpsertPersona {
                        persona: persona.into()
                    }
                )
                .is_ok()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn updates_that_change_nothing_skip_the_persona_write_set() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let revision = service.document().revision;
        // A change that lands nothing bumps no revision and writes nothing.
        service.update(|_| Ok(())).unwrap();
        assert_eq!(service.document().revision, revision);
        // A real update rewrites boss.json but leaves a persona file whose
        // content already matches the state untouched.
        let state = service.document();
        let persona_file = root.join(format!(
            "files/personas/{}/PERSONA.md",
            state.personas[0].id
        ));
        fs::write(&persona_file, b"locally edited").unwrap();
        service.set_session_id(Uuid::new_v4()).unwrap();
        assert_eq!(fs::read_to_string(&persona_file).unwrap(), "locally edited");
        // A persona change still reaches disk.
        let mut upsert = BossPersonaUpsert::from(service.document().personas[0].clone());
        upsert.markdown = "revised persona".into();
        service
            .handle(None, BossOperation::UpsertPersona { persona: upsert })
            .unwrap();
        assert_eq!(
            fs::read_to_string(&persona_file).unwrap(),
            "revised persona"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persona_icon_updates_distinguish_omission_from_null() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let mut upsert = BossPersonaUpsert::from(service.document().personas[0].clone());
        upsert.icon = Some(Some(CustomCommandIcon::Eye));
        service
            .handle(None, BossOperation::UpsertPersona { persona: upsert })
            .unwrap();
        assert_eq!(
            service.document().personas[0].icon,
            Some(CustomCommandIcon::Eye)
        );
        // An omitted field preserves the stored default.
        let mut upsert = BossPersonaUpsert::from(service.document().personas[0].clone());
        upsert.markdown = "revised".into();
        upsert.icon = None;
        service
            .handle(None, BossOperation::UpsertPersona { persona: upsert })
            .unwrap();
        assert_eq!(
            service.document().personas[0].icon,
            Some(CustomCommandIcon::Eye)
        );
        // Null clears it.
        let mut upsert = BossPersonaUpsert::from(service.document().personas[0].clone());
        upsert.icon = Some(None);
        service
            .handle(None, BossOperation::UpsertPersona { persona: upsert })
            .unwrap();
        assert_eq!(service.document().personas[0].icon, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn summon_icon_snapshots_the_resolved_explicit_choice() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[0].id;
        // Neither a summon icon nor a persona default — the explicit field
        // stays unset so the title heuristic owns the render.
        let plain = service
            .prepare_employee(
                boss,
                persona,
                "Research".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        assert_eq!(plain.icon, None);
        // A summon override wins over the persona default and lands as a
        // snapshot on the employee record.
        let overridden = service
            .prepare_employee(
                boss,
                persona,
                "Research".into(),
                None,
                EmployeeGoal::Errand,
                Some(CustomCommandIcon::FolderSearch),
            )
            .unwrap();
        assert_eq!(overridden.icon, Some(CustomCommandIcon::FolderSearch));
        // A configured persona default snapshots onto new summons.
        let mut upsert = BossPersonaUpsert::from(service.document().personas[0].clone());
        upsert.icon = Some(Some(CustomCommandIcon::Eye));
        service
            .handle(None, BossOperation::UpsertPersona { persona: upsert })
            .unwrap();
        let inherited = service
            .prepare_employee(
                boss,
                persona,
                "Research".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        assert_eq!(inherited.icon, Some(CustomCommandIcon::Eye));
        // Later persona edits do not reach the earlier snapshot.
        let mut upsert = BossPersonaUpsert::from(service.document().personas[0].clone());
        upsert.icon = Some(Some(CustomCommandIcon::Lock));
        service
            .handle(None, BossOperation::UpsertPersona { persona: upsert })
            .unwrap();
        assert_eq!(inherited.icon, Some(CustomCommandIcon::Eye));
        // An identifier outside the employee set fails before any record.
        assert!(
            service
                .prepare_employee(
                    boss,
                    persona,
                    "Research".into(),
                    None,
                    EmployeeGoal::Errand,
                    Some(CustomCommandIcon::Bot),
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn identity_personas_and_files_survive_restart() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let state = service.document();
        service
            .handle(
                None,
                BossOperation::WriteFile {
                    path: "memory/work/notes.md".into(),
                    content: "Remember the release deadline".into(),
                },
            )
            .unwrap();
        drop(service);
        let restored = BossService::open(root.clone()).unwrap();
        assert_eq!(restored.document().identity.id, state.identity.id);
        assert_eq!(
            restored.document().identity.avatar_seed,
            state.identity.avatar_seed
        );
        assert!(
            root.join(format!("files/personas/{}/PERSONA.md", state.persona_id))
                .is_file()
        );
        assert!(
            matches!(restored.handle(None, BossOperation::ReadFile { path: "memory/work/notes.md".into() }).unwrap(), BossResult::File { content, .. } if content == "Remember the release deadline")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn employees_can_read_only_pinned_documents_and_cannot_mutate_personas() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let state = service.document();
        let session_id = Uuid::new_v4();
        service
            .update(|state| {
                state.employees.push(BossEmployee {
                    session_id,
                    supervisor_id: Uuid::new_v4(),
                    identity: BossIdentity {
                        id: session_id,
                        name: "Release".into(),
                        avatar_seed: session_id.to_string(),
                    },
                    job_title: "Release".into(),
                    persona_id: state.personas[1].id,
                    work_goal: EmployeeGoal::Errand,
                    created_at: None,
                    icon: None,
                    permissions: PersonaPermissions {
                        ..Default::default()
                    },
                    pinned_files: vec![
                        "docs/release.md".into(),
                        "memory/work/old.md".into(),
                    ],
                    expired: false,
                    expired_at: None,
                    blocker: None,
                    cancelled: false,
                    expiry: None,
                    state: EmployeeLifecycle::Working,
                    ticket: None,
                    queued_at: None,
                    request_id: None,
                    request_fingerprint: None,
                    plan_id: None,
                    item_id: None,
                });
                Ok(())
            })
            .unwrap();
        for (path, content) in [
            ("docs/release.md", "public"),
            ("docs/private.md", "private"),
            ("memory/work/old.md", "legacy memory"),
        ] {
            service
                .handle(
                    None,
                    BossOperation::WriteFile {
                        path: path.into(),
                        content: content.into(),
                    },
                )
                .unwrap();
        }
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::ReadFile {
                        path: "docs/release.md".into()
                    }
                )
                .is_ok()
        );
        for path in [
            "docs/private.md",
            "memory/work/old.md",
            "docs/../private.md",
            "../boss.json",
            "/etc/passwd",
        ] {
            assert!(
                service
                    .handle(
                        Some(session_id),
                        BossOperation::ReadFile { path: path.into() }
                    )
                    .is_err()
            );
        }
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::UpsertPersona {
                        persona: state.personas[0].clone().into()
                    }
                )
                .is_err()
        );
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::WriteFile {
                        path: "docs/release.md".into(),
                        content: "changed".into()
                    }
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_the_files_root() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        std::os::unix::fs::symlink(std::env::temp_dir(), root.join("files/outside")).unwrap();
        assert!(
            service
                .handle(
                    None,
                    BossOperation::WriteFile {
                        path: "outside/escape.md".into(),
                        content: "no".into()
                    }
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn delegation_cannot_expand_grants_and_expired_employees_cannot_delegate() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        let persona = service.document().personas[1].id;
        service
            .update(|state| {
                state.session_id = Some(boss);
                let persona = state
                    .personas
                    .iter_mut()
                    .find(|entry| entry.id == persona)
                    .unwrap();
                persona.permissions.summon_employees = true;
                Ok(())
            })
            .unwrap();
        let parent = service
            .prepare_employee(
                boss,
                persona,
                "Release".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let parent_id = parent.session_id;
        service
            .update(|state| {
                state.employees.push(parent);
                Ok(())
            })
            .unwrap();
        service
            .update(|state| {
                let persona = state
                    .personas
                    .iter_mut()
                    .find(|entry| entry.id == persona)
                    .unwrap();
                persona.permissions.computer_use = true;
                persona.permissions.integration_ids.push("linear".into());
                Ok(())
            })
            .unwrap();
        let child = service
            .prepare_employee(
                parent_id,
                persona,
                "Child".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        assert!(!child.permissions.computer_use);
        assert!(child.permissions.integration_ids.is_empty());
        let child_id = child.session_id;
        service
            .update(|state| {
                state.employees.push(child);
                Ok(())
            })
            .unwrap();
        assert!(
            service
                .authorize_transcript(Some(parent_id), child_id)
                .is_ok()
        );
        assert!(service.authorize_transcript(Some(child_id), boss).is_err());
        assert!(
            service
                .authorize_transcript(Some(Uuid::new_v4()), child_id)
                .is_err()
        );
        let restarted = BossService::open(root.clone()).unwrap();
        assert!(
            restarted.require_active(child_id).is_err(),
            "interrupted employees cannot resume before cleanup"
        );
        drop(restarted);
        assert!(service.expire(parent_id).unwrap().is_some());
        assert!(service.expire(parent_id).unwrap().is_none());
        assert!(
            service
                .prepare_employee(
                    parent_id,
                    persona,
                    "Again".into(),
                    None,
                    EmployeeGoal::Errand,
                    None
                )
                .is_err()
        );
        assert!(service.require_active(parent_id).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn summon_overrides_replace_persona_grants_and_stay_clamped_for_employees() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        let persona = service.document().personas[1].id;
        service
            .update(|state| {
                state.session_id = Some(boss);
                let persona = state
                    .personas
                    .iter_mut()
                    .find(|entry| entry.id == persona)
                    .unwrap();
                persona.permissions.summon_employees = true;
                persona.permissions.integration_ids = vec!["linear".into()];
                Ok(())
            })
            .unwrap();
        // Each `Some` replaces the persona grant; omitted fields inherit.
        let employee = service
            .prepare_employee(
                boss,
                persona,
                "Release".into(),
                Some(PermissionOverrides {
                    computer_use: Some(true),
                    ..Default::default()
                }),
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        assert!(employee.permissions.computer_use);
        assert!(employee.permissions.summon_employees);
        assert_eq!(employee.permissions.integration_ids, vec!["linear"]);
        let parent_id = employee.session_id;
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        // An employee summoner's overrides cannot widen past its own grants.
        let child = service
            .prepare_employee(
                parent_id,
                persona,
                "Child".into(),
                Some(PermissionOverrides {
                    computer_use: Some(true),
                    summon_employees: Some(false),
                    ..Default::default()
                }),
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        assert!(child.permissions.computer_use);
        assert!(!child.permissions.summon_employees);
        // The boss rewrites a live employee's grants field by field.
        service
            .set_employee_permissions(
                Some(boss),
                parent_id,
                PermissionOverrides {
                    computer_use: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        let updated = service.employee(parent_id).unwrap();
        assert!(!updated.permissions.computer_use);
        assert!(updated.permissions.summon_employees);
        // The same edit from an employee supervisor clamps to its grants.
        let child_id = child.session_id;
        service
            .update(|state| {
                state.employees.push(child);
                Ok(())
            })
            .unwrap();
        service
            .set_employee_permissions(
                Some(parent_id),
                child_id,
                PermissionOverrides {
                    integration_ids: Some(vec!["github".into()]),
                    computer_use: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        let child = service.employee(child_id).unwrap();
        assert!(child.permissions.integration_ids.is_empty());
        assert!(!child.permissions.computer_use);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn employee_job_titles_and_human_names_survive_legacy_migration() {
        let root = std::env::temp_dir().join(format!("boss-names-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let supervisor = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(supervisor);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[1].id;
        let employee = service
            .prepare_employee(
                supervisor,
                persona,
                "Release engineer".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        assert_eq!(employee.job_title, "Release engineer");
        assert_ne!(employee.identity.name, employee.job_title);
        let human_name = employee.identity.name.clone();
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        drop(service);
        let restored = BossService::open(root.clone()).unwrap();
        assert_eq!(restored.document().employees[0].identity.name, human_name);
        assert_eq!(
            restored.document().employees[0].job_title,
            "Release engineer"
        );
        drop(restored);
        let path = root.join("boss.json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        legacy["employees"][0]
            .as_object_mut()
            .unwrap()
            .remove("jobTitle");
        legacy["employees"][0]["identity"]["name"] = "Release engineer".into();
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let migrated = BossService::open(root.clone()).unwrap();
        assert_eq!(
            migrated.document().employees[0].job_title,
            "Release engineer"
        );
        let migrated_name = migrated.document().employees[0].identity.name.clone();
        assert!(
            EMPLOYEE_NAMES.contains(&migrated_name.as_str()),
            "migration draws a fresh name from the rotation: {migrated_name}"
        );
        let operation: BossOperation = serde_json::from_value(serde_json::json!({
            "type": "summon", "personaId": persona, "name": "Release engineer",
            "prompt": "Check release", "project": "/project"
        }))
        .unwrap();
        assert!(
            matches!(operation, BossOperation::Summon { job_title, .. } if job_title == "Release engineer")
        );
        drop(migrated);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn speak_parts_authorizes_and_bounds_fragments() {
        let root = std::env::temp_dir().join(format!("boss-speak-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        // Humans and the boss session may speak; other callers may not.
        assert!(service.speak_parts(None, vec!["Heads up".into()]).is_ok());
        assert!(
            service
                .speak_parts(Some(boss), vec!["Heads up".into()])
                .is_ok()
        );
        assert!(
            service
                .speak_parts(Some(Uuid::new_v4()), vec!["Heads up".into()])
                .is_err()
        );
        // Trimming normalizes; empties, oversize parts, and totals are refused.
        assert_eq!(
            service
                .speak_parts(None, vec![" a ".into(), "b".into()])
                .unwrap(),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert!(service.speak_parts(None, vec![]).is_err());
        assert!(service.speak_parts(None, vec![" ".into()]).is_err());
        assert!(
            service
                .speak_parts(None, vec!["x".repeat(MAX_SPEECH_PART_CHARS + 1)])
                .is_err()
        );
        assert!(
            service
                .speak_parts(None, vec!["ok".into(); MAX_SPEECH_PARTS + 1])
                .is_err()
        );
        assert!(
            service
                .speak_parts(
                    None,
                    vec!["x".repeat(100); (MAX_SPEECH_TOTAL_CHARS / 100) + 1],
                )
                .is_err()
        );
        // `speak` is a runtime operation — the plain handler refuses it.
        assert!(
            service
                .handle(
                    None,
                    BossOperation::Speak {
                        parts: vec!["hi".into()],
                    },
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deliverables_publish_dismiss_and_survive_restart() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let output = std::env::temp_dir().join(format!("boss-deliverable-{}", Uuid::new_v4()));
        fs::create_dir_all(&output).unwrap();
        let file = output.join("report.md");
        fs::write(&file, "report").unwrap();
        let file_path = file.to_string_lossy().into_owned();
        let dir_path = output.to_string_lossy().into_owned();
        let service = BossService::open(root.clone()).unwrap();

        // Only the boss or a human manages deliverables — an employee is refused.
        let employee_id = Uuid::new_v4();
        service
            .update(|state| {
                state.employees.push(BossEmployee {
                    session_id: employee_id,
                    supervisor_id: Uuid::new_v4(),
                    identity: BossIdentity {
                        id: employee_id,
                        name: "Release".into(),
                        avatar_seed: employee_id.to_string(),
                    },
                    job_title: "Release".into(),
                    persona_id: state.personas[1].id,
                    work_goal: EmployeeGoal::Errand,
                    created_at: None,
                    icon: None,
                    permissions: PersonaPermissions::default(),
                    pinned_files: Vec::new(),
                    expired: false,
                    expired_at: None,
                    blocker: None,
                    cancelled: false,
                    expiry: None,
                    state: EmployeeLifecycle::Working,
                    ticket: None,
                    queued_at: None,
                    request_id: None,
                    request_fingerprint: None,
                    plan_id: None,
                    item_id: None,
                });
                Ok(())
            })
            .unwrap();
        // Sidebar affordances stay owner-gated for employees; publish is
        // refused too until the employee has an assigned workspace.
        for op in [
            BossOperation::PublishDeliverable {
                path: file_path.clone(),
                name: None,
                reference: false,
            },
            BossOperation::DismissDeliverable { id: Uuid::nil() },
            BossOperation::PinDeliverable {
                id: Uuid::nil(),
                pinned: true,
            },
            BossOperation::SweepDeliverable {
                id: Uuid::nil(),
                dormant: true,
            },
            BossOperation::ArchiveDeliverable {
                id: Uuid::nil(),
                archived: true,
            },
            BossOperation::MarkDeliverableViewed { id: Uuid::nil() },
        ] {
            assert!(service.handle(Some(employee_id), op).is_err());
        }
        // With an assigned workspace the employee publishes its own outputs —
        // paths outside it are still refused.
        service.set_project_context(employee_id, output.clone());
        let outside = std::env::temp_dir().join(format!("boss-foreign-{}", Uuid::new_v4()));
        fs::create_dir_all(&outside).unwrap();
        let foreign = outside.join("foreign.md");
        fs::write(&foreign, "not mine").unwrap();
        assert!(
            service
                .handle(
                    Some(employee_id),
                    BossOperation::PublishDeliverable {
                        path: foreign.to_string_lossy().into_owned(),
                        name: None,
                        reference: false,
                    },
                )
                .is_err()
        );
        service
            .handle(
                Some(employee_id),
                BossOperation::PublishDeliverable {
                    path: file_path.clone(),
                    name: Some("Employee Report".into()),
                    reference: false,
                },
            )
            .unwrap();
        // Publishing is the only deliverable door that opens — the sidebar
        // affordances still refuse the employee.
        assert!(
            service
                .handle(
                    Some(employee_id),
                    BossOperation::PinDeliverable {
                        id: Uuid::nil(),
                        pinned: true,
                    },
                )
                .is_err()
        );
        // Deliverables carry absolute paths to real outputs — relative paths and
        // missing files are both refused.
        assert!(
            service
                .handle(
                    None,
                    BossOperation::PublishDeliverable {
                        path: "outputs/report.md".into(),
                        name: None,
                        reference: false,
                    },
                )
                .is_err()
        );
        assert!(
            service
                .handle(
                    None,
                    BossOperation::PublishDeliverable {
                        path: "/definitely/missing".into(),
                        name: None,
                        reference: false,
                    },
                )
                .is_err()
        );

        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: file_path.clone(),
                    name: None,
                    reference: false,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: dir_path.clone(),
                    name: Some("Deliverables".into()),
                    reference: false,
                },
            )
            .unwrap();
        let state = service.document();
        assert_eq!(state.deliverables.len(), 2);
        let file_deliverable = state
            .deliverables
            .iter()
            .find(|deliverable| deliverable.source_path.as_deref() == Some(file_path.as_str()))
            .unwrap();
        assert_eq!(file_deliverable.name, "report.md");
        assert!(!file_deliverable.directory);
        let dir_deliverable = state
            .deliverables
            .iter()
            .find(|deliverable| deliverable.source_path.as_deref() == Some(dir_path.as_str()))
            .unwrap();
        assert_eq!(dir_deliverable.name, "Deliverables");
        assert!(dir_deliverable.directory);
        let file_id = file_deliverable.id;
        let file_created = file_deliverable.created_at;

        // Re-publishing refreshes the same deliverable rather than stacking rows.
        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: file_path.clone(),
                    name: Some("Report".into()),
                    reference: false,
                },
            )
            .unwrap();
        let state = service.document();
        assert_eq!(state.deliverables.len(), 2);
        let republished = state
            .deliverables
            .iter()
            .find(|deliverable| deliverable.source_path.as_deref() == Some(file_path.as_str()))
            .unwrap();
        assert_eq!(republished.id, file_id);
        assert_eq!(republished.name, "Report");
        assert_eq!(republished.created_at, file_created);
        assert!(republished.updated_at >= file_created);

        // Sidebar affordances ride the same owner gate: pin, sweep, and
        // archive mutate the deliverable in place and refuse unknown ids.
        for op in [
            BossOperation::PinDeliverable {
                id: Uuid::new_v4(),
                pinned: true,
            },
            BossOperation::SweepDeliverable {
                id: Uuid::new_v4(),
                dormant: true,
            },
            BossOperation::ArchiveDeliverable {
                id: Uuid::new_v4(),
                archived: true,
            },
            BossOperation::MarkDeliverableViewed { id: Uuid::new_v4() },
        ] {
            assert!(service.handle(None, op).is_err());
        }
        service
            .handle(
                None,
                BossOperation::PinDeliverable {
                    id: file_id,
                    pinned: true,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::SweepDeliverable {
                    id: file_id,
                    dormant: true,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::ArchiveDeliverable {
                    id: file_id,
                    archived: true,
                },
            )
            .unwrap();
        let deliverable = service.document().deliverables[0].clone();
        assert_eq!(deliverable.id, file_id);
        assert!(deliverable.pinned_at.is_some());
        assert!(deliverable.dormant_at.is_some());
        assert!(deliverable.archived_at.is_some());
        // The same operations clear their flags — and unarchiving keeps the
        // row's other state.
        service
            .handle(
                None,
                BossOperation::ArchiveDeliverable {
                    id: file_id,
                    archived: false,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::SweepDeliverable {
                    id: file_id,
                    dormant: false,
                },
            )
            .unwrap();
        let deliverable = service.document().deliverables[0].clone();
        assert_eq!(deliverable.archived_at, None);
        assert_eq!(deliverable.dormant_at, None);
        assert!(deliverable.pinned_at.is_some());

        // A fresh deliverable is unread until the owner opens it; re-publishing
        // refreshed content makes it unread again by leaving `viewed_at`
        // behind `updated_at`.
        assert_eq!(deliverable.viewed_at, None);
        service
            .handle(None, BossOperation::MarkDeliverableViewed { id: file_id })
            .unwrap();
        let deliverable = service.document().deliverables[0].clone();
        let viewed = deliverable.viewed_at.expect("opening stamps viewed_at");
        assert!(viewed >= deliverable.updated_at);
        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: file_path.clone(),
                    name: None,
                    reference: false,
                },
            )
            .unwrap();
        let deliverable = service.document().deliverables[0].clone();
        assert_eq!(deliverable.viewed_at, Some(viewed));
        assert!(deliverable.updated_at >= viewed);

        drop(service);
        let restored = BossService::open(root.clone()).unwrap();
        assert_eq!(restored.document().deliverables.len(), 2);
        assert!(
            restored
                .handle(
                    None,
                    BossOperation::DismissDeliverable { id: Uuid::new_v4() }
                )
                .is_err()
        );
        restored
            .handle(None, BossOperation::DismissDeliverable { id: file_id })
            .unwrap();
        assert_eq!(restored.document().deliverables.len(), 1);
        drop(restored);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(output).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn published_deliverables_survive_their_source_workspace() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        // A worktree-like source: a directory employee cleanup deletes with
        // the chat that produced it.
        let worktree = std::env::temp_dir().join(format!("boss-worktree-{}", Uuid::new_v4()));
        fs::create_dir_all(&worktree).unwrap();
        let report_path = worktree.join("report.md");
        fs::write(&report_path, "first draft").unwrap();
        let report = report_path.to_string_lossy().into_owned();
        let service = BossService::open(root.clone()).unwrap();

        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: report.clone(),
                    name: None,
                    reference: false,
                },
            )
            .unwrap();
        let state = service.document();
        let deliverable = state
            .deliverables
            .iter()
            .find(|deliverable| deliverable.source_path.as_deref() == Some(report.as_str()))
            .unwrap();
        // The record points at the daemon's copy, not the workspace file —
        // and remembers the source for provenance and re-publish.
        let stored_path = PathBuf::from(&deliverable.path);
        assert!(stored_path.starts_with(root.join("deliverables")));
        assert_eq!(deliverable.source_path.as_deref(), Some(report.as_str()));
        assert_eq!(fs::read_to_string(&stored_path).unwrap(), "first draft");

        // Re-publishing the same source refreshes the snapshot in place.
        fs::write(&report_path, "final draft").unwrap();
        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: report.clone(),
                    name: None,
                    reference: false,
                },
            )
            .unwrap();
        let state = service.document();
        assert_eq!(state.deliverables.len(), 1);
        assert_eq!(state.deliverables[0].id, deliverable.id);
        assert_eq!(fs::read_to_string(&stored_path).unwrap(), "final draft");

        // Archiving the employee's chat removes its worktree — the
        // deliverable still reads, across a daemon restart too.
        fs::remove_dir_all(&worktree).unwrap();
        assert_eq!(fs::read_to_string(&stored_path).unwrap(), "final draft");
        drop(service);
        let restored = BossService::open(root.clone()).unwrap();
        assert_eq!(fs::read_to_string(&stored_path).unwrap(), "final draft");
        assert_eq!(
            restored.document().deliverables[0].path,
            stored_path.to_string_lossy()
        );

        // Dismissing the record removes its snapshot with it.
        restored
            .handle(
                None,
                BossOperation::DismissDeliverable { id: deliverable.id },
            )
            .unwrap();
        assert!(!stored_path.exists());
        drop(restored);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deliverable_copies_directories_while_reference_keeps_a_live_path() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let source = std::env::temp_dir().join(format!("boss-tree-{}", Uuid::new_v4()));
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("nested/out.txt"), "nested output").unwrap();
        let dir = source.to_string_lossy().into_owned();
        let service = BossService::open(root.clone()).unwrap();

        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: dir.clone(),
                    name: None,
                    reference: false,
                },
            )
            .unwrap();
        let deliverable = service.document().deliverables[0].clone();
        assert!(deliverable.directory);
        let stored_dir = PathBuf::from(&deliverable.path);
        assert!(stored_dir.starts_with(root.join("deliverables")));
        fs::remove_dir_all(&source).unwrap();
        assert_eq!(
            fs::read_to_string(stored_dir.join("nested/out.txt")).unwrap(),
            "nested output"
        );

        // A symlinked tree is refused rather than followed out of the
        // publisher's workspace.
        #[cfg(unix)]
        {
            let linked = std::env::temp_dir().join(format!("boss-linked-{}", Uuid::new_v4()));
            fs::create_dir_all(&linked).unwrap();
            std::os::unix::fs::symlink(&stored_dir, linked.join("link")).unwrap();
            assert!(
                service
                    .handle(
                        None,
                        BossOperation::PublishDeliverable {
                            path: linked.to_string_lossy().into_owned(),
                            name: None,
                            reference: false,
                        },
                    )
                    .is_err()
            );
            fs::remove_dir_all(&linked).unwrap();
        }

        // `reference` is the explicit opt-out: the record keeps the live
        // path and the daemon stores nothing.
        let live_dir = std::env::temp_dir().join(format!("boss-live-{}", Uuid::new_v4()));
        fs::create_dir_all(&live_dir).unwrap();
        let live_file = live_dir.join("live.md");
        fs::write(&live_file, "live").unwrap();
        let live = live_file.to_string_lossy().into_owned();
        service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: live.clone(),
                    name: None,
                    reference: true,
                },
            )
            .unwrap();
        let referenced = service
            .document()
            .deliverables
            .iter()
            .find(|deliverable| deliverable.path == live)
            .unwrap()
            .clone();
        assert_eq!(referenced.source_path, None);
        assert!(!service.deliverable_dir(referenced.id).exists());

        drop(service);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(live_dir).unwrap();
    }

    #[test]
    fn summon_accepts_an_optional_worktree_workspace() {
        let operation: BossOperation = serde_json::from_value(serde_json::json!({
            "type": "summon", "personaId": Uuid::nil(), "jobTitle": "Review",
            "prompt": "Check the diff", "project": "/project",
            "workspace": "worktree", "baseBranch": "main"
        }))
        .unwrap();
        assert!(
            matches!(operation, BossOperation::Summon { workspace: Some(waku_protocol::AgentWorkspace::Worktree), base_branch: Some(branch), .. } if branch == "main")
        );
        let operation: BossOperation = serde_json::from_value(serde_json::json!({
            "type": "summon", "personaId": Uuid::nil(), "jobTitle": "Review",
            "prompt": "Check the diff", "project": "/project"
        }))
        .unwrap();
        assert!(matches!(
            operation,
            BossOperation::Summon {
                workspace: None,
                base_branch: None,
                ..
            }
        ));
    }

    #[test]
    fn summon_icon_field_parses_and_rejects_unknown_identifiers() {
        // Omitted and null both mean "no override" on the wire.
        for extra in [serde_json::json!({}), serde_json::json!({ "icon": null })] {
            let mut payload = serde_json::json!({
                "type": "summon", "personaId": Uuid::nil(), "jobTitle": "Review",
                "prompt": "Check the diff", "project": "/project"
            });
            payload
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let operation: BossOperation = serde_json::from_value(payload).unwrap();
            assert!(matches!(
                operation,
                BossOperation::Summon { icon: None, .. }
            ));
        }
        let operation: BossOperation = serde_json::from_value(serde_json::json!({
            "type": "summon", "personaId": Uuid::nil(), "jobTitle": "Review",
            "prompt": "Check the diff", "project": "/project", "icon": "eye"
        }))
        .unwrap();
        assert!(matches!(
            operation,
            BossOperation::Summon {
                icon: Some(CustomCommandIcon::Eye),
                ..
            }
        ));
        // An unknown identifier fails deserialization, so no summon runs.
        assert!(
            serde_json::from_value::<BossOperation>(serde_json::json!({
                "type": "summon", "personaId": Uuid::nil(), "jobTitle": "Review",
                "prompt": "Check the diff", "project": "/project", "icon": "banana"
            }))
            .is_err()
        );
    }

    #[test]
    fn persona_icon_wire_contract() {
        // Records written before the field existed still load.
        let record: BossPersona = serde_json::from_value(serde_json::json!({
            "id": Uuid::nil(), "name": "Review", "markdown": "m", "permissions": {}
        }))
        .unwrap();
        assert_eq!(record.icon, None);
        // The upsert input is tri-state: absent preserves, null clears, an
        // identifier replaces.
        let upsert = |extra: serde_json::Value| {
            let mut persona = serde_json::json!({
                "id": Uuid::nil(), "name": "Review", "markdown": "m", "permissions": {}
            });
            persona
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let BossOperation::UpsertPersona { persona } = serde_json::from_value::<BossOperation>(
                    serde_json::json!({ "type": "upsertPersona", "persona": persona }),
                )
            .unwrap() else {
                panic!("expected upsertPersona");
            };
            persona.icon
        };
        assert_eq!(upsert(serde_json::json!({})), None);
        assert_eq!(upsert(serde_json::json!({ "icon": null })), Some(None));
        assert_eq!(
            upsert(serde_json::json!({ "icon": "eye" })),
            Some(Some(CustomCommandIcon::Eye))
        );
        // New identifiers round-trip snake_case.
        for (icon, wire) in [
            (CustomCommandIcon::Bug, "bug"),
            (CustomCommandIcon::Languages, "languages"),
            (CustomCommandIcon::Database, "database"),
            (CustomCommandIcon::CircleCheck, "circle_check"),
            (CustomCommandIcon::Beaker, "beaker"),
            (CustomCommandIcon::GitMerge, "git_merge"),
            (CustomCommandIcon::Eye, "eye"),
            (CustomCommandIcon::Pencil, "pencil"),
            (CustomCommandIcon::FolderSearch, "folder_search"),
            (CustomCommandIcon::FileText, "file_text"),
            (CustomCommandIcon::Lock, "lock"),
            (CustomCommandIcon::Fork, "fork"),
            (CustomCommandIcon::Brain, "brain"),
        ] {
            assert_eq!(serde_json::to_value(icon).unwrap(), serde_json::json!(wire));
            assert_eq!(
                serde_json::from_value::<CustomCommandIcon>(serde_json::json!(wire)).unwrap(),
                icon
            );
            assert!(icon.is_employee_icon());
        }
    }

    #[test]
    fn employee_rename_and_avatar_regeneration_are_owner_only() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let session_id = Uuid::new_v4();
        service
            .update(|state| {
                state.employees.push(BossEmployee {
                    session_id,
                    supervisor_id: Uuid::new_v4(),
                    identity: BossIdentity {
                        id: session_id,
                        name: "Quinn".into(),
                        avatar_seed: session_id.to_string(),
                    },
                    job_title: "Release engineer".into(),
                    persona_id: state.personas[1].id,
                    work_goal: EmployeeGoal::Errand,
                    created_at: None,
                    icon: None,
                    permissions: PersonaPermissions::default(),
                    pinned_files: Vec::new(),
                    expired: false,
                    expired_at: None,
                    blocker: None,
                    cancelled: false,
                    expiry: None,
                    state: EmployeeLifecycle::Working,
                    ticket: None,
                    queued_at: None,
                    request_id: None,
                    request_fingerprint: None,
                    plan_id: None,
                    item_id: None,
                });
                Ok(())
            })
            .unwrap();
        // Employees cannot rename themselves or re-roll their own face.
        for operation in [
            BossOperation::RenameEmployee {
                session_id,
                name: "Rogue".into(),
            },
            BossOperation::RegenerateAvatar {
                session_id: Some(session_id),
            },
        ] {
            assert!(service.handle(Some(session_id), operation).is_err());
        }
        let BossResult::State { state } = service
            .handle(
                None,
                BossOperation::RenameEmployee {
                    session_id,
                    name: "Scout".into(),
                },
            )
            .unwrap()
        else {
            panic!("employee rename returns the updated state");
        };
        assert_eq!(state.employees[0].identity.name, "Scout");
        let seed = state.employees[0].identity.avatar_seed.clone();
        let BossResult::State { state } = service
            .handle(
                None,
                BossOperation::RegenerateAvatar {
                    session_id: Some(session_id),
                },
            )
            .unwrap()
        else {
            panic!("avatar regeneration returns the updated state");
        };
        assert_ne!(state.employees[0].identity.avatar_seed, seed);
        // A missing target — or the boss's own session — re-rolls the
        // boss's face instead.
        let boss_seed = state.identity.avatar_seed;
        let BossResult::State { state } = service
            .handle(None, BossOperation::RegenerateAvatar { session_id: None })
            .unwrap()
        else {
            panic!("avatar regeneration returns the updated state");
        };
        assert_ne!(state.identity.avatar_seed, boss_seed);
        fs::remove_dir_all(root).unwrap();
    }


}

#[cfg(test)]
mod memory_op_tests {
    use super::*;
    use waku_protocol::boss::MemoryOperation;

    #[test]
    fn project_bucket_is_automatic_and_other_buckets_require_an_explicit_grant() {
        let root = std::env::temp_dir().join(format!("boss-buckets-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let create = |name: &str| {
            let BossResult::Memory { buckets, .. } = service
                .handle(
                    Some(boss),
                    BossOperation::Memory {
                        operation: MemoryOperation::CreateBucket {
                            name: name.into(),
                            purpose: String::new(),
                        },
                    },
                )
        .unwrap()
        else {
                panic!("create bucket returns metadata")
        };
            buckets.into_iter().next().unwrap()["id"]
                .as_str()
        .unwrap()
                .to_owned()
        };
        let shared = create("Shared");
        let private = create("Private");
        let persona = service.document().personas[1].id;
        let mut employee = service
            .prepare_employee(
                boss,
                persona,
                "Review".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        employee.permissions.bucket_ids.push(shared.clone());
        let employee_id = employee.session_id;
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        let project = root.join("repo");
        fs::create_dir_all(&project).unwrap();
        service.set_project_context(employee_id, project);

        let BossResult::Memory { buckets, .. } = service
            .handle(
                Some(employee_id),
                BossOperation::Memory {
                    operation: MemoryOperation::ListBuckets,
                },
            )
            .unwrap()
        else {
            panic!("list buckets returns visible metadata")
        };
        assert!(buckets.iter().any(|bucket| bucket["id"] == shared));
        assert!(!buckets.iter().any(|bucket| bucket["id"] == private));
        let project_bucket = buckets
            .iter()
            .find(|bucket| bucket["projectId"].as_str().is_some())
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let BossResult::Memory {
            recorded: Some(note),
            ..
        } = service
            .handle(
                Some(employee_id),
                BossOperation::Memory {
                    operation: MemoryOperation::Record {
                        bucket: Some(project_bucket),
                        project: None,
                        kind: waku_protocol::boss::MemoryNoteKind::Fact,
                        text: "Project access needs no special grant".into(),
                        retry_key: "project-note".into(),
                    },
                },
            )
            .unwrap()
        else {
            panic!("employee can write to its project bucket")
        };
        assert_eq!(note["text"], "Project access needs no special grant");
        assert!(
            service
                .handle(
                    Some(employee_id),
                    BossOperation::Memory {
                        operation: MemoryOperation::Overview {
                            bucket: Some(private),
                            project: None,
                        }
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("access denied")
        );
        assert!(
            service
                .handle(
                    Some(employee_id),
                    BossOperation::Memory {
                        operation: MemoryOperation::CreateBucket {
                            name: "Forbidden".into(),
                            purpose: String::new()
                        }
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("only the Boss")
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// `project` resolves registered names and absolute roots to the
    /// project bucket; an absent selector lands on the caller's project.
    #[test]
    fn project_memory_resolves_names_paths_and_the_callers_own_project() {
        let root = std::env::temp_dir().join(format!("boss-project-memory-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let own = root.join("own-repo");
        let foreign = root.join("foreign-repo");
        fs::create_dir_all(&own).unwrap();
        fs::create_dir_all(&foreign).unwrap();
        let registered = |name: &str, path: &Path| waku_protocol::model::Project {
            id: Uuid::new_v4(),
            name: name.into(),
            path: path.to_path_buf(),
            bookmark: None,
            created_at: 0,
            temporary: false,
            starred: false,
            submissions_enabled: false,
            qa_branch: None,
            friend_peer_id: None,
            kind: None,
            resolved_name: None,
        };
        let catalog = vec![
            registered("own-repo", &own),
            registered("foreign-repo", &foreign),
        ];
        service.set_project_catalog(std::sync::Arc::new(move || catalog.clone()));
        let persona = service.document().personas[1].id;
        let employee = service
            .prepare_employee(
                boss,
                persona,
                "Review".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let employee_id = employee.session_id;
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        service.set_project_context(employee_id, own.clone());

        // No selector writes to the employee's assigned project bucket.
        let BossResult::Memory {
            bucket: Some(echoed),
            ..
        } = service
            .handle(
                Some(employee_id),
                BossOperation::Memory {
                    operation: MemoryOperation::Record {
                        bucket: None,
                        project: None,
                        kind: waku_protocol::boss::MemoryNoteKind::Fact,
                        text: "default bucket".into(),
                        retry_key: "default-1".into(),
                    },
                },
            )
            .unwrap()
        else {
            panic!("an unqualified record lands in the employee's project bucket")
        };
        assert_eq!(echoed, project_bucket_id(&own));

        // A registered project name resolves case-insensitively to the same
        // bucket — here the employee's own.
        let BossResult::Memory {
            bucket: Some(echoed),
            ..
        } = service
            .handle(
                Some(employee_id),
                BossOperation::Memory {
                    operation: MemoryOperation::Overview {
                        bucket: None,
                        project: Some("Own-Repo".into()),
                    },
                },
            )
            .unwrap()
        else {
            panic!("a project name resolves to its shared bucket")
        };
        assert_eq!(echoed, project_bucket_id(&own));

        // A foreign project stays out of the employee's reach — resolving it
        // does not mint or grant the bucket.
        assert!(
            service
                .handle(
                    Some(employee_id),
                    BossOperation::Memory {
                        operation: MemoryOperation::Overview {
                            bucket: None,
                            project: Some("foreign-repo".into()),
                        },
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("unknown memory bucket")
        );

        // The boss reaches any registered project by name or by absolute
        // path; the first touch materializes the bucket for both spellings.
        for reference in [
            "foreign-repo".to_owned(),
            foreign.to_string_lossy().into_owned(),
        ] {
            let BossResult::Memory {
                bucket: Some(echoed),
                ..
            } = service
                .handle(
                    Some(boss),
                    BossOperation::Memory {
                        operation: MemoryOperation::Overview {
                            bucket: None,
                            project: Some(reference),
                        },
                    },
                )
                .unwrap()
            else {
                panic!("the boss resolves a project reference to its bucket")
            };
            assert_eq!(echoed, project_bucket_id(&foreign));
        }

        // Misspellings and ambiguous selectors fail loudly.
        assert!(
            service
                .handle(
                    Some(boss),
                    BossOperation::Memory {
                        operation: MemoryOperation::Overview {
                            bucket: None,
                            project: Some("nope".into()),
                        },
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("unknown to the daemon")
        );
        assert!(
            service
                .handle(
                    Some(boss),
                    BossOperation::Memory {
                        operation: MemoryOperation::Overview {
                            bucket: Some("project-x".into()),
                            project: Some("own-repo".into()),
                        },
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("not both")
        );
        assert!(
            service
                .handle(
                    Some(boss),
                    BossOperation::Memory {
                        operation: MemoryOperation::Overview {
                            bucket: None,
                            project: None,
                        },
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("needs a bucket or a project")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_memory_migration_is_inspectable_additive_and_idempotent() {
        let root = std::env::temp_dir().join(format!("boss-memory-import-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let BossResult::Memory { buckets, .. } = service
            .handle(
                Some(boss),
                BossOperation::Memory {
                    operation: MemoryOperation::CreateBucket {
                        name: "Imported notes".into(),
                        purpose: String::new(),
                    },
                },
            )
            .unwrap()
        else {
            panic!("bucket creation returns metadata")
        };
        let bucket = buckets[0]["id"].as_str().unwrap().to_owned();
        let memory_dir = root.join("files/memory/work");
        let engine_dir = root.join("files/memory-engine/scopes/old/collections/work/topics/topic");
        fs::create_dir_all(&memory_dir).unwrap();
        fs::create_dir_all(&engine_dir).unwrap();
        fs::write(
            memory_dir.join("MEMORY.md"),
            "# Old memory\n\nRelease branch is dev.",
        )
        .unwrap();
        fs::write(
            engine_dir.join("note.md"),
            "---\ntitle: Branch\n---\nThe default QA branch is dev.",
        )
        .unwrap();

        let migrate = |dry_run| {
            service.handle(
                Some(boss),
                BossOperation::Memory {
                    operation: MemoryOperation::MigrateLegacy {
                        bucket: bucket.clone(),
                        source: "boss".into(),
                        dry_run,
                    },
                },
            )
        };
        let BossResult::Memory {
            migration: Some(preview),
            ..
        } = migrate(true).unwrap()
        else {
            panic!("dry run returns an inspectable migration report")
        };
        assert!(preview.dry_run);
        assert_eq!(preview.imported, 0);
        assert_eq!(preview.candidates.len(), 2);
        assert!(
            preview
                .candidates
                .iter()
                .any(|candidate| candidate.text.contains("Release branch is dev"))
        );
        assert!(
            fs::read_to_string(memory_dir.join("MEMORY.md"))
                .unwrap()
                .contains("Release branch is dev")
        );

        let BossResult::Memory {
            migration: Some(imported),
            ..
        } = migrate(false).unwrap()
        else {
            panic!("import returns an inspectable report")
        };
        assert_eq!(imported.imported, 2);
        assert!(engine_dir.join("note.md").is_file());
        let BossResult::Memory { notes, .. } = service
            .handle(
                Some(boss),
                BossOperation::Memory {
                    operation: MemoryOperation::Scan {
                        bucket: Some(bucket.clone()),
                        project: None,
                        query: "dev".into(),
                    },
                },
            )
            .unwrap()
        else {
            panic!("scan returns original bucket notes")
        };
        assert_eq!(notes.len(), 2);
        migrate(false).unwrap();
        let BossResult::Memory { notes, .. } = service
            .handle(
                Some(boss),
                BossOperation::Memory {
                    operation: MemoryOperation::Scan {
                        bucket: Some(bucket.clone()),
                        project: None,
                        query: "dev".into(),
                    },
                },
            )
            .unwrap()
        else {
            panic!("scan returns original bucket notes")
        };
        assert_eq!(
            notes.len(),
            2,
            "retrying migration does not duplicate records"
        );
        let project = root.join("repo");
        let project_memory = project.join(".goddard/memory");
        fs::create_dir_all(&project_memory).unwrap();
        fs::write(
            project_memory.join("MEMORY.md"),
            "Project memory survives worktree cleanup.",
        )
        .unwrap();
        let BossResult::Memory {
            migration: Some(project_preview),
            ..
        } = service
            .handle(
                Some(boss),
                BossOperation::Memory {
                    operation: MemoryOperation::MigrateLegacy {
                        bucket: bucket.clone(),
                        source: project.to_string_lossy().into_owned(),
                        dry_run: true,
                    },
                },
            )
            .unwrap()
        else {
            panic!("project dry run returns an inspectable report")
        };
        assert_eq!(project_preview.candidates.len(), 1);
        assert!(project_memory.join("MEMORY.md").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn normalize_plan_file_maps_every_spelling_to_the_canonical_document() {
        for spelling in [
            "auth.md",
            "plans/auth.md",
            "workspace/plans/auth.md",
            "memory/plans/auth.md",
            "memory//plans/auth.md",
            "  plans/auth.md  ",
        ] {
            assert_eq!(
                normalize_plan_file(spelling).unwrap(),
                "plans/auth.md",
                "{spelling}"
            );
        }
        assert_eq!(
            normalize_plan_file("plans/deep/auth.md").unwrap(),
            "plans/deep/auth.md"
        );
    }

    #[test]
    fn normalize_plan_file_rejects_traversal_absolute_and_non_markdown() {
        for spelling in [
            "../auth.md",
            "plans/../auth.md",
            "memory/../../etc/x.md",
            "/etc/x.md",
            "C:/x.md",
            "auth.txt",
            "plans/",
            "",
        ] {
            assert!(normalize_plan_file(spelling).is_err(), "{spelling}");
        }
    }

    /// A planning session is the same principal as the boss chat for
    /// file-owner authority; finalization stamps once, then every spelling
    /// of the document refuses writes while reads stay open.
    #[test]
    fn a_planning_principal_writes_files_and_its_finalized_document_freezes() {
        let root = std::env::temp_dir().join(format!("boss-plan-freeze-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        let planning = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                state.planning.push(BossPlan {
                    id: Uuid::new_v4(),
                    session_id: planning,
                    plan_file: "plans/auth.md".into(),
                    idea: "Auth".into(),
                    finalized_at: None,
                    items: Vec::new(),
                    outcome: None,
                    history: Vec::new(),
                });
                Ok(())
            })
            .unwrap();
        assert!(!service.is_boss(planning));
        assert!(service.is_boss_principal(planning));
        assert!(service.is_managed(planning));
        assert_eq!(service.plan(planning).unwrap().plan_file, "plans/auth.md");
        assert_eq!(
            service.plan_for_file("plans/auth.md").unwrap().session_id,
            planning
        );
        service
            .handle(
                Some(planning),
                BossOperation::WriteFile {
                    path: "memory/notes.md".into(),
                    content: "boss-owned".into(),
                },
            )
            .unwrap();
        assert!(
            service
                .handle(
                    Some(Uuid::new_v4()),
                    BossOperation::WriteFile {
                        path: "memory/notes.md".into(),
                        content: "nope".into(),
                    },
                )
                .is_err()
        );
        service
            .handle(
                None,
                BossOperation::WriteFile {
                    path: "plans/auth.md".into(),
                    content: "draft".into(),
                },
            )
            .unwrap();
        let stamped = service.finalize_plan("plans/auth.md", None, 100).unwrap();
        assert_eq!(stamped.finalized_at, Some(100));
        // The stamp is monotonic: re-finalizing cannot re-time it.
        assert_eq!(
            service
                .finalize_plan("plans/auth.md", None, 200)
                .unwrap()
                .finalized_at,
            Some(100)
        );
        for spelling in ["plans/auth.md", "plans//auth.md", "memory/plans/auth.md"] {
            assert!(
                service
                    .handle(
                        None,
                        BossOperation::WriteFile {
                            path: spelling.into(),
                            content: "edit".into(),
                        },
                    )
                    .is_err(),
                "{spelling}"
            );
        }
        let BossResult::File { content, .. } = service
            .handle(
                None,
                BossOperation::ReadFile {
                    path: "plans/auth.md".into(),
                },
            )
            .unwrap()
        else {
            panic!("the frozen document still reads")
        };
        // The legacy `memory/` spelling reads the same document.
        let BossResult::File {
            content: legacy, ..
        } = service
            .handle(
                None,
                BossOperation::ReadFile {
                    path: "memory/plans/auth.md".into(),
                },
            )
            .unwrap()
        else {
            panic!("the legacy spelling still resolves")
        };
        assert_eq!(legacy, "draft");
        assert_eq!(content, "draft");
        service
            .handle(
                None,
                BossOperation::WriteFile {
                    path: "memory/work/notes.md".into(),
                    content: "unrelated".into(),
                },
            )
            .unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    /// A `memory/plans/` written before plans moved to the files root
    /// lands at `plans/` on open; a `plans/` that already exists keeps its
    /// entries while the legacy tree's missing names move over.
    #[test]
    fn legacy_memory_plans_migrate_to_the_files_root() {
        let root = std::env::temp_dir().join(format!("boss-plan-migrate-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("files/memory/plans/deep")).unwrap();
        fs::write(root.join("files/memory/plans/auth.md"), "auth").unwrap();
        fs::write(root.join("files/memory/plans/deep/notes.md"), "deep").unwrap();
        let service = BossService::open(root.clone()).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("files/plans/auth.md")).unwrap(),
            "auth"
        );
        assert_eq!(
            fs::read_to_string(root.join("files/plans/deep/notes.md")).unwrap(),
            "deep"
        );
        assert!(!root.join("files/memory/plans").exists());
        drop(service);
        fs::remove_dir_all(root).unwrap();

        // Both trees present: the existing `plans/` wins conflicts while
        // the legacy tree's missing names still move.
        let root = std::env::temp_dir().join(format!("boss-plan-merge-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("files/memory/plans")).unwrap();
        fs::create_dir_all(root.join("files/plans")).unwrap();
        fs::write(root.join("files/memory/plans/auth.md"), "old auth").unwrap();
        fs::write(root.join("files/memory/plans/billing.md"), "billing").unwrap();
        fs::write(root.join("files/plans/auth.md"), "new auth").unwrap();
        let service = BossService::open(root.clone()).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("files/plans/auth.md")).unwrap(),
            "new auth"
        );
        assert_eq!(
            fs::read_to_string(root.join("files/plans/billing.md")).unwrap(),
            "billing"
        );
        drop(service);
        fs::remove_dir_all(root).unwrap();
    }

    /// Reports for employees a planning session summoned land back on it
    /// while it is active; an archived supervisor escalates to the boss
    /// session. `session_active` comes from the bound backend — unbound,
    /// a service still answers for live planning sessions.
    #[test]
    fn a_planning_supervisor_takes_reports_while_active() {
        let root = std::env::temp_dir().join(format!("boss-plan-report-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        let planning = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                state.planning.push(BossPlan {
                    id: Uuid::new_v4(),
                    session_id: planning,
                    plan_file: "plans/auth.md".into(),
                    idea: "Auth".into(),
                    finalized_at: None,
                    items: Vec::new(),
                    outcome: None,
                    history: Vec::new(),
                });
                Ok(())
            })
            .unwrap();
        let mut employee = service
            .prepare_employee(
                planning,
                service.document().personas[0].id,
                "Job".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        employee.supervisor_id = planning;
        assert_eq!(service.report_target(&employee), Some(planning));
        // An employee the planning session did not summon still reports to
        // its own supervisor's chain.
        employee.supervisor_id = Uuid::new_v4();
        assert_eq!(service.report_target(&employee), Some(boss));
        fs::remove_dir_all(root).unwrap();
    }

    /// A member admitted under a `groupId` joins the wave in the same
    /// durable write; when the last member commits to terminal the wave
    /// resolves into exactly one outbox notice — and re-admission reopens
    /// it for one more.
    #[test]
    fn a_wave_resolves_once_and_reopens_on_readmission() {
        let root = std::env::temp_dir().join(format!("boss-wave-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[0].id;
        let ticket = || SummonTicket {
            sequence: 0,
            generation: 1,
            provider: ProviderKind::Codex,
            model: "gpt-5.5".into(),
            reasoning_effort: None,
            prompt: "do it".into(),
            project: "/tmp".into(),
            workspace: None,
            base_branch: None,
            adopt_worktree: None,
            resources: waku_protocol::resources::ResourceSet::default(),
            allow_burst: false,
            pending_prompts: Vec::new(),
            group_id: Some("wave".into()),
            priority: None,
            goal_id: None,
            reservation: None,
            pending_resources: None,
            pending_reservation: None,
            blocked_by: Vec::new(),
            dispatch_event: None,
            interruptions: Vec::new(),
            resume_count: 0,
            last_resumed_cause: None,
        };
        let member = |title: &str| {
            service
                .prepare_employee(
                    boss,
                    persona,
                    title.into(),
                    None,
                    EmployeeGoal::Errand,
                    None,
                )
                .unwrap()
        };
        let first = member("A");
        let second = member("B");
        service.enqueue_ticket(first.clone(), ticket()).unwrap();
        service.enqueue_ticket(second.clone(), ticket()).unwrap();
        let wave = &service.document().waves[0];
        assert_eq!(wave.supervisor_id, boss);
        assert_eq!(wave.members.len(), 2);
        assert!(wave.resolved_at.is_none());

        service
            .begin_finishing(first.session_id, false, false, ExpiryCause::Finished)
            .unwrap();
        service.complete_expiry(first.session_id, None).unwrap();
        assert!(
            service.document().wave_outbox.is_empty(),
            "one member still in flight — no notice yet"
        );
        service
            .begin_finishing(second.session_id, false, false, ExpiryCause::Finished)
            .unwrap();
        service.complete_expiry(second.session_id, None).unwrap();
        let document = service.document();
        let wave = &document.waves[0];
        assert!(wave.resolved_at.is_some());
        assert_eq!(document.wave_outbox.len(), 1);
        assert_eq!(document.wave_outbox[0].wave_id, "wave");
        assert_eq!(document.wave_outbox[0].finished, 2);
        assert_eq!(document.wave_outbox[0].supervisor_id, boss);

        // Re-admission puts the member back in flight: the wave reopens,
        // then resolves once more when it settles.
        service
            .requeue_employee(first.session_id, ticket(), |_| {})
            .unwrap();
        let wave = &service.document().waves[0];
        assert!(wave.resolved_at.is_none());
        assert!(
            wave.members
                .iter()
                .find(|member| member.session_id == first.session_id)
                .unwrap()
                .outcome
                .is_none()
        );
        service
            .begin_finishing(first.session_id, false, false, ExpiryCause::Finished)
            .unwrap();
        let document = service.document();
        assert!(document.waves[0].resolved_at.is_some());
        assert_eq!(
            document.wave_outbox.len(),
            2,
            "a distinct resolution, not a repeat"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// A parked resource update rides the ticket until its swap applies:
    /// `request_resource_update` only lands on `working`, the apply CAS
    /// installs the new set and hands the old claim back, a requeue
    /// folds the parked set into the fresh admission, and expiry returns
    /// every live reservation id for release.
    #[test]
    fn resource_updates_park_swap_and_fold_into_a_requeue() {
        let root = std::env::temp_dir().join(format!("boss-resupd-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[0].id;
        let ticket = || SummonTicket {
            sequence: 0,
            generation: 1,
            provider: ProviderKind::Codex,
            model: "gpt-5.5".into(),
            reasoning_effort: None,
            prompt: "do it".into(),
            project: "/tmp".into(),
            workspace: None,
            base_branch: None,
            adopt_worktree: None,
            resources: waku_protocol::resources::ResourceSet::default(),
            allow_burst: false,
            pending_prompts: Vec::new(),
            group_id: None,
            priority: None,
            goal_id: None,
            reservation: None,
            pending_resources: None,
            pending_reservation: None,
            blocked_by: Vec::new(),
            dispatch_event: None,
            interruptions: Vec::new(),
            resume_count: 0,
            last_resumed_cause: None,
        };
        let builds = |count: u32| waku_protocol::resources::ResourceSet {
            native_builds: count,
            ..Default::default()
        };
        let employee = service
            .prepare_employee(
                boss,
                persona,
                "Job".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let session = employee.session_id;
        service.enqueue_ticket(employee, ticket()).unwrap();

        // Queued tickets cannot park an update — they edit `resources`.
        assert!(
            service
                .request_resource_update(session, ticket(), builds(1), Uuid::new_v4())
                .is_err()
        );

        let held = Uuid::new_v4();
        service.mark_dispatching(session, 1, Some(held)).unwrap();
        service.mark_working(session, 1).unwrap();

        let update = Uuid::new_v4();
        assert_eq!(
            service
                .request_resource_update(session, ticket(), builds(1), update)
                .unwrap(),
            None
        );
        assert_eq!(service.pending_resource_updates().len(), 1);
        // A stale apply settles nothing and hands nothing back.
        assert_eq!(
            service
                .apply_resource_update(session, Uuid::new_v4())
                .unwrap(),
            (false, None)
        );
        // The matching apply swaps: new set, new reservation, old claim
        // back for the caller's release.
        assert_eq!(
            service.apply_resource_update(session, update).unwrap(),
            (true, Some(held))
        );
        let settled = service.employee(session).unwrap().ticket.unwrap();
        assert_eq!(settled.resources.native_builds, 1);
        assert_eq!(settled.reservation, Some(update));
        assert!(settled.pending_reservation.is_none());

        // A requeue folds a parked update into the fresh admission and
        // returns every live claim — the held swap and the parked id —
        // for the caller's release.
        let next = Uuid::new_v4();
        service
            .request_resource_update(session, ticket(), builds(2), next)
            .unwrap();
        let (_employee, stale) = service.requeue_employee(session, ticket(), |_| {}).unwrap();
        let requeued = service.employee(session).unwrap().ticket.unwrap();
        assert_eq!(requeued.resources.native_builds, 2);
        assert!(requeued.pending_reservation.is_none());
        assert!(requeued.pending_resources.is_none());
        assert!(stale.contains(&update));
        assert!(stale.contains(&next));

        // Expiry hands back both a held reservation and a parked update
        // id so nothing leaks mid-flight.
        let held_two = Uuid::new_v4();
        service
            .mark_dispatching(session, 2, Some(held_two))
            .unwrap();
        service.mark_working(session, 2).unwrap();
        let parked = Uuid::new_v4();
        service
            .request_resource_update(session, ticket(), builds(1), parked)
            .unwrap();
        service.begin_finishing(session, false, false, ExpiryCause::Finished).unwrap();
        let released = service.complete_expiry(session, None).unwrap();
        assert!(released.contains(&held_two));
        assert!(released.contains(&parked));
        fs::remove_dir_all(root).unwrap();
    }

    /// A member expired before its outcome could land — an old document,
    /// or a crash between writes — still resolves its wave: opening the
    /// service folds expired member records into the tally.
    #[test]
    fn a_wave_reconciles_an_expired_member_missing_its_outcome() {
        let root = std::env::temp_dir().join(format!("boss-wave-heal-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[0].id;
        let employee = service
            .prepare_employee(
                boss,
                persona,
                "Job".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let session_id = employee.session_id;
        // The wave record with the member still in flight, and an expired
        // employee that never reported an outcome — the state a write
        // interrupted mid-flight leaves behind.
        service
            .update(|state| {
                state.waves.push(BossWave {
                    id: "wave".into(),
                    supervisor_id: boss,
                    members: vec![WaveMember {
                        session_id,
                        outcome: None,
                    }],
                    resolved_at: None,
                });
                let mut employee = employee;
                employee.expired = true;
                employee.expired_at = Some(1);
                employee.set_lifecycle(EmployeeLifecycle::Expired, 1);
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        let reopened = BossService::open(root.clone()).unwrap();
        let document = reopened.document();
        assert!(document.waves[0].resolved_at.is_some());
        assert_eq!(document.wave_outbox.len(), 1);
        assert_eq!(document.wave_outbox[0].finished, 1);
        fs::remove_dir_all(root).unwrap();
    }

    /// The settle path drains rather than cutting a live turn: while the
    /// session's provider turn is still open — a prompt that landed
    /// between the settle event and the finish pass — `expire` leaves the
    /// record working, and the open turn's own settle re-drives the
    /// finish. A forced finish (a supervisor stop) still cuts through.
    #[test]
    fn a_draining_expiry_defers_to_an_open_turn() {
        let root = std::env::temp_dir().join(format!("boss-drain-expiry-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[0].id;
        let hire = |title: &str| {
            let mut employee = service
                .prepare_employee(
                    boss,
                    persona,
                    title.into(),
                    None,
                    EmployeeGoal::Errand,
                    None,
                )
                .unwrap();
            employee.set_lifecycle(EmployeeLifecycle::Working, 1);
            let session_id = employee.session_id;
            service
                .update(|state| {
                    state.employees.push(employee);
                    Ok(())
                })
                .unwrap();
            session_id
        };
        let busy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let probe = busy.clone();
        service.set_session_busy(std::sync::Arc::new(move |_| {
            probe.load(std::sync::atomic::Ordering::SeqCst)
        }));

        let draining = hire("Drain");
        assert!(service.expire(draining).unwrap().is_none());
        assert_eq!(
            service.employee_lifecycle(draining),
            Some(EmployeeLifecycle::Working),
            "an in-flight turn holds the ticket at working"
        );

        let forced = hire("Force");
        assert!(
            service
                .begin_finishing(forced, false, false, ExpiryCause::Finished)
                .unwrap()
                .is_some(),
            "a forced finish ignores the open turn"
        );
        assert_eq!(
            service.employee_lifecycle(forced),
            Some(EmployeeLifecycle::Finishing)
        );

        // The deferred employee's turn settles: the next finish pass
        // completes the expiry.
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(service.expire(draining).unwrap().is_some());
        assert!(service.employee(draining).unwrap().expired);
        fs::remove_dir_all(root).unwrap();
    }

    /// The settle classification rides the record and the ticket: an
    /// interruption appends to the ticket's history, a clean finish
    /// records `finished` without one, a marked stop stays `stopped`
    /// whatever the caller reported, and re-admission clears the record
    /// with the rest of the old admission's flags.
    #[test]
    fn an_expiry_records_its_cause_on_the_ticket() {
        let root = std::env::temp_dir().join(format!("boss-expiry-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let persona = service.document().personas[0].id;
        let ticket = || SummonTicket {
            sequence: 0,
            generation: 1,
            provider: ProviderKind::Codex,
            model: "gpt-5.5".into(),
            reasoning_effort: None,
            prompt: "do it".into(),
            project: "/tmp".into(),
            workspace: None,
            base_branch: None,
            adopt_worktree: None,
            resources: waku_protocol::resources::ResourceSet::default(),
            allow_burst: false,
            pending_prompts: Vec::new(),
            group_id: None,
            priority: None,
            goal_id: None,
            reservation: None,
            pending_resources: None,
            pending_reservation: None,
            blocked_by: Vec::new(),
            dispatch_event: None,
            interruptions: Vec::new(),
            resume_count: 0,
            last_resumed_cause: None,
        };
        let member = |title: &str| {
            service
                .prepare_employee(boss, persona, title.into(), None, EmployeeGoal::Errand, None)
                .unwrap()
        };

        let interrupted = member("Crash");
        service
            .enqueue_ticket(interrupted.clone(), ticket())
            .unwrap();
        service
            .begin_finishing(interrupted.session_id, false, false, ExpiryCause::Restarted)
            .unwrap();
        service
            .complete_expiry(interrupted.session_id, None)
            .unwrap();
        let record = service.employee(interrupted.session_id).unwrap();
        let expiry = record.expiry.as_ref().unwrap();
        assert_eq!(expiry.cause, ExpiryCause::Restarted);
        assert!(expiry.resumable);
        let interruptions = &record.ticket.as_ref().unwrap().interruptions;
        assert_eq!(interruptions.len(), 1);
        assert_eq!(interruptions[0].cause, ExpiryCause::Restarted);

        let clean = member("Done");
        service.enqueue_ticket(clean.clone(), ticket()).unwrap();
        service
            .begin_finishing(clean.session_id, false, false, ExpiryCause::Finished)
            .unwrap();
        service.complete_expiry(clean.session_id, None).unwrap();
        let record = service.employee(clean.session_id).unwrap();
        assert_eq!(record.expiry.as_ref().unwrap().cause, ExpiryCause::Finished);
        assert!(record.ticket.as_ref().unwrap().interruptions.is_empty());

        // A marked stop classifies `stopped` even when the caller
        // reported a clean settle — the flag is the durable intent.
        let stopped = member("Halt");
        service.enqueue_ticket(stopped.clone(), ticket()).unwrap();
        service.mark_cancelled(stopped.session_id).unwrap();
        service
            .begin_finishing(stopped.session_id, false, false, ExpiryCause::Finished)
            .unwrap();
        service
            .complete_expiry(stopped.session_id, None)
            .unwrap();
        let record = service.employee(stopped.session_id).unwrap();
        let expiry = record.expiry.as_ref().unwrap();
        assert_eq!(expiry.cause, ExpiryCause::Stopped);
        assert!(!expiry.resumable);
        assert!(record.ticket.as_ref().unwrap().interruptions.is_empty());

        // Re-admission clears the settle record with blocker/cancelled;
        // the ticket's interruption history survives the requeue.
        service
            .requeue_employee(interrupted.session_id, ticket(), |_| {})
            .unwrap();
        let record = service.employee(interrupted.session_id).unwrap();
        assert!(record.expiry.is_none());
        assert_eq!(record.ticket.as_ref().unwrap().interruptions.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    /// A draft plan record for the tests below — `session_id` doubles as
    /// the planning session's identity.
    fn test_plan(plan_file: &str) -> BossPlan {
        BossPlan {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            plan_file: plan_file.into(),
            idea: "Plan".into(),
            finalized_at: None,
            items: Vec::new(),
            outcome: None,
            history: Vec::new(),
        }
    }

    fn add_plan(service: &BossService, plan: BossPlan) -> Uuid {
        let id = plan.id;
        service
            .update(|state| {
                state.planning.push(plan);
                Ok(())
            })
            .unwrap();
        id
    }

    #[test]
    fn plan_items_declare_reshape_and_drop_by_omission() {
        let root = std::env::temp_dir().join(format!("boss-plan-items-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        add_plan(&service, test_plan("plans/auth.md"));
        // Employees never reach the bookkeeping channel.
        let employee = Uuid::new_v4();
        assert!(
            service
                .update_plan_items(Some(employee), "plans/auth.md", Vec::new())
                .is_err()
        );
        // A draft plan declares its course before approval.
        let plan = service
            .update_plan_items(
                Some(boss),
                "auth.md",
                vec![
                    PlanItemInput {
                        id: None,
                        title: "Probe".into(),
                    },
                    PlanItemInput {
                        id: None,
                        title: "Build".into(),
                    },
                    PlanItemInput {
                        id: None,
                        title: "Verify".into(),
                    },
                ],
            )
            .unwrap();
        let titles: Vec<&str> = plan.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Probe", "Build", "Verify"]);
        assert!(
            plan.items
                .iter()
                .all(|item| item.state == PlanItemState::ToDo && item.history.is_empty())
        );
        let probe = plan.items[0].id;
        let verify = plan.items[2].id;
        // Reorder, rename, add, and omit — the omission drops, struck but
        // kept, with the boss recorded on its audit entry.
        let plan = service
            .update_plan_items(
                Some(boss),
                &plan.id.to_string(),
                vec![
                    PlanItemInput {
                        id: Some(verify),
                        title: "Verify all".into(),
                    },
                    PlanItemInput {
                        id: None,
                        title: "Polish".into(),
                    },
                    PlanItemInput {
                        id: Some(probe),
                        title: "Probe".into(),
                    },
                ],
            )
            .unwrap();
        let titles: Vec<(&str, PlanItemState)> = plan
            .items
            .iter()
            .map(|item| (item.title.as_str(), item.state))
            .collect();
        assert_eq!(
            titles,
            [
                ("Verify all", PlanItemState::ToDo),
                ("Polish", PlanItemState::ToDo),
                ("Probe", PlanItemState::ToDo),
                ("Build", PlanItemState::Dropped),
            ]
        );
        let build = &plan.items[3];
        assert_eq!(
            build.history.as_slice(),
            [PlanItemTransition {
                state: PlanItemState::Dropped,
                at: build.history[0].at,
                actor: PlanActor::Boss,
            }]
        );
        // The breakdown survives a daemon restart.
        drop(service);
        let reopened = BossService::open(root.clone()).unwrap();
        let plan = reopened.plan_for_file("plans/auth.md").unwrap();
        assert_eq!(plan.items.len(), 4);
        assert_eq!(plan.items[3].state, PlanItemState::Dropped);
        fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn plan_outcome_lifecycle_is_audited_and_reopens() {
        let root = std::env::temp_dir().join(format!("boss-plan-outcome-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        add_plan(&service, test_plan("plans/auth.md"));
        // A draft has no outcome to set.
        for outcome in [
            PlanOutcome::Completed,
            PlanOutcome::Abandoned,
            PlanOutcome::Approved,
        ] {
            assert!(
                service
                    .set_plan_outcome(None, "plans/auth.md", outcome)
                    .is_err(),
                "{outcome:?} on a draft"
            );
        }
        // Finalization seeds the breakdown and audits the approval.
        let plan = service
            .finalize_plan(
                "plans/auth.md",
                Some(vec!["Probe".into(), "Verify".into()]),
                100,
            )
            .unwrap();
        assert_eq!(plan.outcome(), PlanOutcome::Approved);
        assert_eq!(
            plan.items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["Probe", "Verify"]
        );
        assert_eq!(
            plan.history.as_slice(),
            [PlanTransition {
                outcome: PlanOutcome::Approved,
                at: 100,
                actor: PlanActor::User,
            }]
        );
        // The boss closes the plan; its actor lands on the trail.
        let plan = service
            .set_plan_outcome(Some(boss), "plans/auth.md", PlanOutcome::Completed)
            .unwrap();
        assert_eq!(plan.outcome(), PlanOutcome::Completed);
        assert_eq!(plan.history.len(), 2);
        assert_eq!(plan.history[1].outcome, PlanOutcome::Completed);
        assert_eq!(plan.history[1].actor, PlanActor::Boss);
        // A closed plan refuses re-close and reshaping until reopened.
        assert!(
            service
                .set_plan_outcome(None, "plans/auth.md", PlanOutcome::Abandoned)
                .is_err()
        );
        assert!(
            service
                .update_plan_items(None, "plans/auth.md", Vec::new())
                .is_err()
        );
        // Reopen returns to approved with a third audited transition.
        let plan = service
            .set_plan_outcome(None, "plans/auth.md", PlanOutcome::Approved)
            .unwrap();
        assert_eq!(plan.outcome(), PlanOutcome::Approved);
        assert_eq!(plan.history.len(), 3);
        assert_eq!(plan.history[2].outcome, PlanOutcome::Approved);
        assert_eq!(plan.history[2].actor, PlanActor::User);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_item_states_check_drop_and_reopen() {
        let root = std::env::temp_dir().join(format!("boss-plan-item-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        add_plan(&service, test_plan("plans/auth.md"));
        let plan = service
            .update_plan_items(
                None,
                "plans/auth.md",
                vec![PlanItemInput {
                    id: None,
                    title: "Probe".into(),
                }],
            )
            .unwrap();
        let item = plan.items[0].id;
        // A user check-off lands on the item's audit trail.
        let plan = service
            .set_plan_item_state(None, "plans/auth.md", item, PlanItemState::Done)
            .unwrap();
        assert_eq!(plan.items[0].state, PlanItemState::Done);
        assert_eq!(plan.items[0].history.len(), 1);
        assert_eq!(plan.items[0].history[0].actor, PlanActor::User);
        service
            .set_plan_item_state(None, "plans/auth.md", item, PlanItemState::Dropped)
            .unwrap();
        // Reopen returns the item to to-do.
        let plan = service
            .set_plan_item_state(None, "plans/auth.md", item, PlanItemState::ToDo)
            .unwrap();
        assert_eq!(plan.items[0].state, PlanItemState::ToDo);
        assert_eq!(plan.items[0].history.len(), 3);
        assert_eq!(plan.items[0].history[2].state, PlanItemState::ToDo);
        // A closed plan refuses item bookkeeping until reopened.
        service.finalize_plan("plans/auth.md", None, 100).unwrap();
        service
            .set_plan_outcome(None, "plans/auth.md", PlanOutcome::Abandoned)
            .unwrap();
        assert!(
            service
                .set_plan_item_state(None, "plans/auth.md", item, PlanItemState::Done)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_tag_resolution_accepts_ids_and_file_spellings() {
        let root = std::env::temp_dir().join(format!("boss-plan-tag-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let plan = test_plan("plans/auth.md");
        let id = plan.id;
        let session = plan.session_id;
        add_plan(&service, plan);
        // Drafts tag — a planning session's research employees are plan work.
        for reference in [
            "plans/auth.md",
            "auth.md",
            "memory/plans/auth.md",
            &id.to_string(),
            &session.to_string(),
        ] {
            assert_eq!(
                service.plan_assignment(reference, None).unwrap(),
                (id, None),
                "{reference}"
            );
        }
        assert!(service.plan_assignment("plans/missing.md", None).is_err());
        assert!(service.plan_assignment(&Uuid::new_v4().to_string(), None).is_err());
        // Done or dropped items and closed plans refuse new tags.
        let plan = service
            .update_plan_items(
                None,
                "plans/auth.md",
                vec![PlanItemInput {
                    id: None,
                    title: "Probe".into(),
                }],
            )
            .unwrap();
        let item = plan.items[0].id;
        service
            .set_plan_item_state(None, "plans/auth.md", item, PlanItemState::Done)
            .unwrap();
        assert!(
            service
                .plan_assignment("plans/auth.md", Some(item))
                .is_err()
        );
        assert!(
            service
                .plan_assignment("plans/auth.md", Some(Uuid::new_v4()))
                .is_err()
        );
        service.finalize_plan("plans/auth.md", None, 10).unwrap();
        service
            .set_plan_outcome(None, "plans/auth.md", PlanOutcome::Abandoned)
            .unwrap();
        assert!(service.plan_assignment("plans/auth.md", None).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn set_employee_plan_retags_relinks_and_clears() {
        let root = std::env::temp_dir().join(format!("boss-employee-plan-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        add_plan(&service, test_plan("plans/auth.md"));
        let other = add_plan(&service, test_plan("plans/billing.md"));
        let plan = service
            .update_plan_items(
                Some(boss),
                "plans/auth.md",
                vec![PlanItemInput {
                    id: None,
                    title: "Probe".into(),
                }],
            )
            .unwrap();
        let item = plan.items[0].id;
        let employee = service
            .prepare_employee(
                boss,
                service.document().personas[0].id,
                "Job".into(),
                None,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        let session = employee.session_id;
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        // An item link needs a plan — the employee has none yet.
        assert!(
            service
                .set_employee_plan(session, None, Some(Some(item)))
                .is_err()
        );
        // Tag plan + item, then re-link the item only.
        service
            .set_employee_plan(session, Some(Some("auth.md".into())), Some(Some(item)))
            .unwrap();
        let employee = service.employee(session).unwrap();
        assert_eq!(employee.plan_id, Some(plan.id));
        assert_eq!(employee.item_id, Some(item));
        // Clearing just the item keeps the plan — the employee reads as
        // unallocated plan work.
        service
            .set_employee_plan(session, None, Some(None))
            .unwrap();
        let employee = service.employee(session).unwrap();
        assert_eq!(employee.plan_id, Some(plan.id));
        assert_eq!(employee.item_id, None);
        // Re-tagging the plan without restating the item drops to
        // unallocated.
        service
            .set_employee_plan(session, Some(Some("auth.md".into())), Some(Some(item)))
            .unwrap();
        service
            .set_employee_plan(session, Some(Some(other.to_string())), None)
            .unwrap();
        let employee = service.employee(session).unwrap();
        assert_eq!(employee.plan_id, Some(other));
        assert_eq!(employee.item_id, None);
        // An explicit null clears the tag outright.
        service
            .set_employee_plan(session, Some(None), None)
            .unwrap();
        let employee = service.employee(session).unwrap();
        assert_eq!(employee.plan_id, None);
        assert_eq!(employee.item_id, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_groups_list_open_plans_by_child_activity() {
        let root = std::env::temp_dir().join(format!("boss-plan-groups-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let draft = add_plan(&service, test_plan("plans/draft.md"));
        let approved = test_plan("plans/approved.md");
        let approved_id = add_plan(&service, approved);
        service.finalize_plan("plans/approved.md", None, 10).unwrap();
        let abandoned = test_plan("plans/dead.md");
        let abandoned_id = add_plan(&service, abandoned);
        service
            .finalize_plan("plans/dead.md", None, 5)
            .unwrap();
        service
            .set_plan_outcome(Some(boss), "plans/dead.md", PlanOutcome::Abandoned)
            .unwrap();
        // No employees yet: the approved plan earns a row, the untagged
        // draft does not, and the abandoned plan has left for Finished.
        let groups = service.document();
        let groups = groups.plan_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].plan.id, approved_id);
        // One employee tagged to the draft — work earns it a row.
        let employee = {
            let mut employee = service
                .prepare_employee(
                    boss,
                    service.document().personas[0].id,
                    "Job".into(),
                    None,
                    EmployeeGoal::Errand,
                    None,
                )
                .unwrap();
            employee.plan_id = Some(draft);
            employee.created_at = Some(50);
            employee
        };
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        let groups = service.document();
        let groups = groups.plan_groups();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].plan.id, draft, "fresher child activity leads");
        assert_eq!(groups[0].unallocated.len(), 1);
        assert_eq!(groups[1].plan.id, approved_id);
        // A second employee on the older plan out-ranks it once active.
        let employee = {
            let mut employee = service
                .prepare_employee(
                    boss,
                    service.document().personas[0].id,
                    "Other".into(),
                    None,
                    EmployeeGoal::Errand,
                    None,
                )
                .unwrap();
            employee.plan_id = Some(approved_id);
            employee.created_at = Some(60);
            employee
        };
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        let groups = service.document();
        let groups = groups.plan_groups();
        assert_eq!(groups[0].plan.id, approved_id);
        let _ = abandoned_id;
        fs::remove_dir_all(root).unwrap();
    }
}
