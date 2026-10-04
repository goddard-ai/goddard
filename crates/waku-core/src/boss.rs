//! Persistent Boss state and compartment-aware file access.

use std::collections::VecDeque;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, anyhow, bail};
use parking_lot::Mutex;
use uuid::Uuid;
use waku_protocol::boss::{
    BossBundle, BossEmployee, BossFile, BossIdentity, BossOperation, BossPersona, BossPlan,
    BossResult, BossState, EmployeeGoal, PermissionOverrides, PersonaPermissions,
};

const MAX_FILE_BYTES: usize = 256 * 1024;
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
/// A finalized planning session stays active this long before the daemon
/// archives it — the grace window in which the boss can still converse in
/// the planning context and summon employees from it. One hour matches the
/// reuse window a finished employee gets, so both managed lifetimes sweep
/// on the same cadence. After the sweep the session is a plain archived
/// managed task; only its frozen plan record marks what it was.
const PLANNING_GRACE_SECONDS: u64 = 60 * 60;
/// A blocker report is one bounded attention item, not an essay — long
/// context belongs in the transcript the finish report indexes.
const MAX_BLOCKER_CHARS: usize = 1_000;

/// A boss session's persisted eval scope — a replaced boss chat starts a
/// fresh scope rather than inheriting its predecessor's variables, the
/// same rule the router slot follows. The mutex also serializes evals so
/// two concurrent calls cannot interleave one scope.
#[derive(Default)]
struct BossEval {
    session: Option<Uuid>,
    scope: rhai::Scope<'static>,
}

pub struct BossService {
    root: PathBuf,
    active: std::sync::atomic::AtomicBool,
    state: Mutex<BossState>,
    notifier: Mutex<Option<crate::share::TaskNotifier>>,
    pub(crate) operation_lock: Mutex<()>,
    backend: Mutex<std::sync::Weak<crate::daemon::WakuBackend>>,
    interrupted: Mutex<Vec<Uuid>>,
    projects: Mutex<std::collections::HashMap<Uuid, PathBuf>>,
    injected: Mutex<std::collections::HashSet<Uuid>>,
    router: Mutex<BossRouter>,
    evals: Mutex<BossEval>,
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
            backend: Mutex::new(std::sync::Weak::new()),
            interrupted: Mutex::new(Vec::new()),
            projects: Mutex::new(std::collections::HashMap::new()),
            injected: Mutex::new(std::collections::HashSet::new()),
            router: Mutex::new(BossRouter::default()),
            evals: Mutex::new(BossEval::default()),
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
        }
        *self.state.lock() = state;
        *self.interrupted.lock() = self
            .state
            .lock()
            .employees
            .iter()
            .filter(|entry| !entry.expired)
            .map(|entry| entry.session_id)
            .collect();
        self.migrate_legacy_pins();
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
                let id = state.employees[index].identity.id;
                let existing_names = state
                    .employees
                    .iter()
                    .enumerate()
                    .filter(|(other_index, _)| *other_index != index)
                    .map(|(_, employee)| employee.identity.name.as_str())
                    .collect::<Vec<_>>();
                state.employees[index].identity.name = employee_human_name(id, existing_names);
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
        }
        let service = Self {
            root,
            active: std::sync::atomic::AtomicBool::new(true),
            interrupted: Mutex::new(
                state
                    .employees
                    .iter()
                    .filter(|entry| !entry.expired)
                    .map(|entry| entry.session_id)
                    .collect(),
            ),
            state: Mutex::new(state),
            notifier: Mutex::new(None),
            operation_lock: Mutex::new(()),
            backend: Mutex::new(std::sync::Weak::new()),
            projects: Mutex::new(std::collections::HashMap::new()),
            injected: Mutex::new(std::collections::HashSet::new()),
            router: Mutex::new(BossRouter::default()),
            evals: Mutex::new(BossEval::default()),
        };
        service.migrate_legacy_pins();
        service.save(&service.state.lock())?;
        Ok(service)
    }

    pub fn document(&self) -> BossState {
        self.state.lock().clone()
    }

    /// Legacy documents pinned knowledge files by their path beneath the
    /// files root; pins are now relative to `memory/` — the one store.
    /// Rewrite `memory/x` as `x`, move a file pinned from elsewhere
    /// beneath `memory/`, and drop pins that cannot resolve: managed
    /// persona documents, missing files, and unsafe paths. Upserts
    /// reject `memory/`-prefixed pins, so a stored prefix can only come
    /// from a legacy document.
    fn migrate_legacy_pins(&self) {
        let mut state = self.state.lock();
        for persona in &mut state.personas {
            self.migrate_pin_list(&mut persona.pinned_files);
        }
        for employee in &mut state.employees {
            self.migrate_pin_list(&mut employee.pinned_files);
        }
    }

    fn migrate_pin_list(&self, pins: &mut Vec<String>) {
        pins.retain_mut(|entry| {
            if let Some(rest) = entry.strip_prefix("memory/") {
                *entry = rest.to_owned();
                return validate_relative(entry, false).is_ok();
            }
            if entry.starts_with("personas/") || validate_relative(entry, false).is_err() {
                return false;
            }
            let (Ok(from), Ok(to)) = (
                self.file_path(entry, false),
                self.file_path(&format!("memory/{entry}"), false),
            ) else {
                return false;
            };
            if fs::symlink_metadata(&from).is_ok_and(|meta| meta.is_file()) && !to.exists() {
                if let Some(parent) = to.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::rename(&from, &to);
            }
            to.is_file()
        });
    }

    pub fn set_task_notifier(&self, notifier: crate::share::TaskNotifier) {
        *self.notifier.lock() = Some(notifier);
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
    /// caller names `memory/`-relative `plans/<name>.md`.
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
    /// and is not already frozen; the stamp is monotonic.
    pub fn finalize_plan(&self, plan_file: &str, now: u64) -> anyhow::Result<BossPlan> {
        let mut finalized = None;
        self.update(|state| {
            let plan = state
                .planning
                .iter_mut()
                .find(|plan| plan.plan_file == plan_file)
                .ok_or_else(|| anyhow!("unknown plan {plan_file}"))?;
            if plan.finalized_at.is_none() {
                plan.finalized_at = Some(now);
            }
            finalized = Some(plan.clone());
            Ok(())
        })?;
        Ok(finalized.unwrap())
    }

    /// Whether `path` — a files-root-relative Boss path — names a finalized
    /// plan document. Frozen plans reject writes; reads stay open.
    fn plan_file_frozen(&self, path: &str) -> bool {
        let normalized = normalize_plan_path(path);
        let Some(rest) = normalized.strip_prefix("memory/") else {
            return false;
        };
        self.state
            .lock()
            .planning
            .iter()
            .any(|plan| plan.plan_file == rest && plan.finalized_at.is_some())
    }

    /// Archive planning sessions whose post-finalization grace period has
    /// elapsed. Runs beside the hourly employee-retirement sweep: the grace
    /// window is the boss's last chance to converse in the planning context
    /// and summon employees from it, then the session becomes a plain
    /// archived managed task.
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
        let Some(backend) = self.backend.lock().upgrade() else {
            return Ok(0);
        };
        backend.archive_sessions(&due)?;
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

    pub fn require_active(&self, session: Uuid) -> anyhow::Result<()> {
        if self.interrupted.lock().contains(&session) {
            bail!("employee was interrupted by daemon restart; summon a new employee");
        }
        if self.employee(session).is_some_and(|entry| entry.expired) {
            bail!("employee has expired; prompt or steer can resume it, or summon a new employee");
        }
        Ok(())
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
        if caller.is_some_and(|caller| !self.is_boss_principal(caller) && employee.supervisor_id != caller) {
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
    ) -> anyhow::Result<BossEmployee> {
        validate_name(&job_title)?;
        let state = self.document();
        let persona = state
            .personas
            .iter()
            .find(|persona| persona.id == persona_id)
            .ok_or_else(|| anyhow!("unknown persona"))?;
        let mut permissions = persona.permissions.clone();
        let mut pinned_files = persona.pinned_files.clone();
        if let Some(overrides) = &overrides {
            self.validate_memory_folders(overrides.memory_folders.as_deref().unwrap_or(&[]))?;
            overrides.apply_to(&mut permissions);
        }
        if self.is_planning(caller)
            && !self
                .backend
                .lock()
                .upgrade()
                .map(|backend| backend.session_active(caller))
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
            pinned_files.retain(|path| {
                self.authorize_file(Some(caller), &format!("memory/{path}"), false)
                    .is_ok()
            });
        }
        let id = Uuid::new_v4();
        Ok(BossEmployee {
            session_id: id,
            supervisor_id: caller,
            identity: BossIdentity {
                id,
                name: employee_human_name(
                    id,
                    state
                        .employees
                        .iter()
                        .map(|employee| employee.identity.name.as_str()),
                ),
                avatar_seed: id.to_string(),
            },
            job_title: job_title.trim().to_owned(),
            persona_id,
            work_goal,
            created_at: Some(waku_protocol::model::unix_time()),
            icon: None,
            permissions,
            pinned_files,
            expired: false,
            expired_at: None,
            blocker: None,
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
        self.validate_memory_folders(overrides.memory_folders.as_deref().unwrap_or(&[]))?;
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

    fn validate_memory_folders(&self, folders: &[String]) -> anyhow::Result<()> {
        for folder in folders {
            validate_relative(folder, false)?;
            self.file_path(&format!("memory/{folder}"), false)?;
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
                EmployeeGoal::Errand => "Finishing delivers your transcript index to your supervisor — keep your final turns self-contained since that is what it reads — and do not expect a reply.",
                EmployeeGoal::Goal => "Finishing is silent — your outcome lands on the human's Goals page without prompting anyone, so do not expect a reply.",
            };
            format!(
                "You are employee {}, whose job title is {}. Your supervisor is task {}. You have no owned memory and must not write memory. Use `goddard-agent boss` to read granted files and retrieve employee transcripts. Native subagents are not Boss employees: delegate only with the Boss summon operation, and only when permitted. Your grants are {}. Pinned memory files: {}. Finish this bounded job, return your results, and expire. {finish} When something genuinely needs your supervisor's attention — you are blocked, a decision is required, or the job failed — flag it with `goddard-agent boss '{{\"type\":\"reportBlocker\",\"message\":\"what needs attention\"}}'`: the report interrupts your supervisor's running work when it can and makes your finish deliver a full report instead of expiring silently. Do not flag routine completions.",
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
                "You are {}, the boss for this daemon. Heavy delegation is your default: promptly assign execution to employees so you stay free for the human. Delegate code changes, research, internet access, builds, code generation, long-running checks and tests, and Git integration (cherry-picks, merges, and conflict resolution). Never run or poll long-running commands yourself; assign them to an employee, including any wait or follow-up check. For builds and code generation, ask employees to use the repository's shared build cache or a dedicated output directory when that avoids contention with the user's tools. You control personas and all employees. Grant each employee only the memory folders required by their role and task. Personal memory is boss-only by default; grant it only when the task genuinely requires personal context. When a persona repeatedly needs shared memory, create a per-role memory folder and grant that folder instead. Persona permissions are the memory grant mechanism for summon; pinnedFiles lists memory files an agent always sees in its context and can read without a folder grant; a per-field `permissions` object on summon — or `setPermissions` via control — tailors one employee's grants without editing the persona. Choose a purpose-specific jobTitle when summoning each employee; Goddard assigns their human name. Summon accepts `workspace: \"worktree\"` and `baseBranch` to run an employee in a daemon-managed Git worktree rather than the primary checkout, and `workGoal` to fix how its finish lands. Never create Git worktrees yourself — summon `workspace`/`baseBranch` covers employee worktree needs, and manual `git worktree` commands are for landing worktrees only when unavoidable. Build a reusable persona library across projects: when work patterns recur, create a named purpose-specific persona such as Researcher, Feature Developer, Bug Investigator, or Verifier, with instructions useful beyond the current project. Before creating one, inspect existing personas and refine a close match rather than making duplicates; update personas as repeated work reveals better responsibilities or boundaries. Use the generic Employee persona only for work that does not fit a reusable role. Keep persona instructions focused on a role's durable methods and limits, not one task's details. Your dedicated tools are `goddard-agent boss` operations: view, summon, control, transcript, context, automation, upsertPersona, listFiles, readFile, writeFile, createFolder, rename, publishBundle, dismissBundle, speak, eval, createPlan, finalizePlan. `automation` lists, creates, updates, deletes, pauses, and resumes user automations; employees cannot use it. `eval` runs a Rhai script inside the daemon with the other operations bound as functions — batch related operations into one call and chain their results; variables persist between evals, and `help()` inside a script lists the bindings. `context` returns a snapshot of the human's projects, tasks, and automations — check it whenever a message concerns their work and no snapshot was already attached. `search` scans every project's task transcripts for you, not just your own project — `project:` narrows to one — and `read` opens any task it surfaces. `speak` voices an utterance through connected clients when their voice feature is on — split it into reusable fragments (proper nouns alone, stock phrases whole) so generated clips are reused and later utterances stay instant. These operations authorize routine delegation without asking the human to approve each employee. Use `goddard-agent schema` for their payloads. Your persona is {}. You can access every memory folder, and memory upkeep is a standing duty rather than a side task: write durable facts, decisions, and outcomes under memory/ as they surface — do not wait for a lull or for the human to ask — keep them in folders per topic or project, and prune or reconcile stale entries instead of accumulating duplicates. Track active work durably: record which employee owns each worktree, what is in flight, and what has landed, then reconcile those notes as work changes. Verify completion from the worktree and its commits before reporting a task done; an employee's summary alone is not proof that work was committed. Queued prompts can be lost when an employee is finishing, so summon a fresh employee for new follow-up work instead of stacking prompts onto someone about to expire. Publish useful employee outputs with bundles so the human can find them later, and use speak when a concise interruption is timely. Respect user-set resource rules, including model routing and employee caps, and record durable constraints in memory so delegation stays within them. Your persistent files root is {}. Broader filesystem editing and internet access are discouraged, not forbidden. Never wait, watch, or poll yourself — no transcript read loops, no sleep-and-recheck cycles, no blocking resource waits: when a job needs a wait, such as watching a task, an employee finishing, or a condition to keep rechecking, summon an employee to do the watching and report, then return to the human. Mark every summon `workGoal`: an `errand` reports its finish to you — choose it when you need the completion to continue the work; a `goal` finishes without you — choose it for fire-and-forget work, which lands on the human's Goals page instead. Goal finishes are silent — no prompt arrives — so read outcomes lazily from `view` or `context`; a finish also reaches you when the employee flagged a blocker through its `reportBlocker` operation or its session failed. A blocker report also interrupts your running turn when it can. There are no managers.",
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
                    format!("\nThis planning session's plan memory/{} is finalized and frozen — the document can no longer be edited.", plan.plan_file)
                } else {
                    format!("\nThis is a planning session for \"{}\". Draft and revise the plan document at memory/{} with `writeFile`; when the plan is ready for the user's approval call `finalizePlan` — the user reviews it before it freezes.", plan.idea, plan.plan_file)
                }
            })
            .unwrap_or_default();
        format!(
            "<boss-persona>\n{}\n\n{}{}\nPinned memory files: {}. Read them selectively through the Boss readFile operation.\n{project_context}\n</boss-persona>\n\n{prompt}",
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

    pub fn bind_backend(&self, backend: &std::sync::Arc<crate::daemon::WakuBackend>) {
        *self.backend.lock() = std::sync::Arc::downgrade(backend);
    }

    pub fn recover_interrupted(&self) {
        let ids = self.interrupted.lock().clone();
        if ids.is_empty() {
            return;
        }
        // An employee the restart cut off mid-job could not flag its own
        // failure; flag it so the finish still reaches the supervisor.
        let _ = self.update(|state| {
            for entry in state
                .employees
                .iter_mut()
                .filter(|entry| ids.contains(&entry.session_id))
            {
                if entry.blocker.is_none() {
                    entry.blocker = Some("interrupted by a daemon restart".into());
                }
            }
            Ok(())
        });
        if let Some(backend) = self.backend.lock().upgrade() {
            let _ = std::thread::Builder::new()
                .name("boss-recover-employees".into())
                .spawn(move || {
                    for id in ids {
                        if let Err(error) = backend.finish_boss_employee(id) {
                            eprintln!("could not recover interrupted employee {id}: {error:#}");
                        }
                    }
                });
        }
    }

    pub fn note_settled(&self, session: Uuid) {
        if !self.employee(session).is_some_and(|entry| !entry.expired) {
            return;
        }
        if let Some(backend) = self.backend.lock().upgrade() {
            // Never join or shut down a driver from its own forwarder.
            let _ = std::thread::Builder::new()
                .name("boss-employee-finish".into())
                .spawn(move || {
                    if let Err(error) = backend.finish_boss_employee(session) {
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
            if !state.employees.iter().any(|entry| entry.session_id == session) {
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
                    entry.expired = false;
                    entry.expired_at = None;
                    entry.blocker = None;
                    state.employees.push(entry);
                    revived = true;
                    return Ok(());
                }
            }
            if let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session && entry.expired)
            {
                entry.expired = false;
                entry.expired_at = None;
                entry.blocker = None;
                revived = true;
            }
            Ok(())
        })?;
        Ok(revived)
    }

    /// Record an employee's attention item. Raising the flag is what makes
    /// the finish report reach the supervisor — the daemon reads `blocker`
    /// back when the employee expires — and it also interrupts a live
    /// supervisor immediately. Only a live employee may flag its own job.
    pub fn report_blocker(
        &self,
        caller: Uuid,
        message: String,
    ) -> anyhow::Result<BossEmployee> {
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
        let supervisor_is_planning = self.is_planning(employee.supervisor_id)
            && self
                .backend
                .lock()
                .upgrade()
                .map(|backend| backend.session_active(employee.supervisor_id))
                .unwrap_or(true);
        if self
            .employee(employee.supervisor_id)
            .is_some_and(|entry| !entry.expired)
            || supervisor_is_planning
        {
            Some(employee.supervisor_id)
        } else {
            self.document().session_id
        }
    }

    pub fn expire(&self, session: Uuid) -> anyhow::Result<Option<BossEmployee>> {
        let mut employee = None;
        self.update(|state| {
            if let Some(entry) = state
                .employees
                .iter_mut()
                .find(|entry| entry.session_id == session && !entry.expired)
            {
                entry.expired = true;
                entry.expired_at = Some(waku_protocol::model::unix_time());
                employee = Some(entry.clone());
            }
            Ok(())
        })?;
        self.reset_context(session);
        self.projects.lock().remove(&session);
        self.interrupted.lock().retain(|id| *id != session);
        Ok(employee)
    }

    /// Remove finished employees once their one-hour reuse window has elapsed.
    /// Their task sessions remain in task storage, including full transcripts.
    /// Goals are exempt: fire-and-forget work keeps its record for the
    /// client's Goals page — the roster's retirement exists to clear errand
    /// noise, and a goal's finish was never noise.
    pub fn retire_expired(&self, now: u64) -> anyhow::Result<Vec<BossEmployee>> {
        let _operation = self.operation_lock.lock();
        let cutoff = now.saturating_sub(EMPLOYEE_RETIREMENT_SECONDS);
        let retires = |employee: &BossEmployee| {
            employee.work_goal == EmployeeGoal::Errand
                && employee.expired
                && employee
                    .expired_at
                    .is_some_and(|expired_at| expired_at <= cutoff)
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

    /// Run a Rhai script through `boss_eval` with `session`'s persisted
    /// scope — a session id the service has not seen starts a fresh scope.
    /// `dispatch` is the daemon's full operation path, so bound functions
    /// keep each operation's own authorization and runtime effects.
    pub fn eval(
        &self,
        session: Uuid,
        script: &str,
        dispatch: &dyn Fn(BossOperation) -> anyhow::Result<BossResult>,
    ) -> anyhow::Result<BossResult> {
        if script.len() > crate::boss_eval::MAX_EVAL_SCRIPT_BYTES {
            bail!(
                "eval script exceeds {} bytes",
                crate::boss_eval::MAX_EVAL_SCRIPT_BYTES
            );
        }
        let mut eval = self.evals.lock();
        if eval.session != Some(session) {
            eval.session = Some(session);
            eval.scope = rhai::Scope::new();
        }
        let scope = std::mem::take(&mut eval.scope);
        let outcome = crate::boss_eval::run(scope, script, dispatch);
        if let Some(scope) = outcome.scope {
            eval.session = Some(session);
            eval.scope = scope;
        }
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
            | BossOperation::CreatePlan { .. }
            | BossOperation::FinalizePlan { .. }
            | BossOperation::Automation { .. }
            | BossOperation::Summon { .. }
            | BossOperation::Control { .. }
            | BossOperation::ReportBlocker { .. }
            | BossOperation::Transcript { .. }
            | BossOperation::Speak { .. }
            | BossOperation::Eval { .. } => {
                bail!("runtime operation requires daemon dispatch")
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
                use crate::memory_engine::{Acl, Grant, Scope, Store};
                use waku_protocol::boss::MemoryOperation;

                let state = self.document();
                let boss_principal = caller.is_none_or(|id| self.is_boss_principal(id));
                let principal = if boss_principal {
                    "boss".to_owned()
                } else {
                    caller.context("missing Boss caller")?.to_string()
                };
                let mut grants = Vec::new();
                if let Some(caller) = caller.filter(|_| !boss_principal) {
                    if let Some(employee) = state.employees.iter().find(|e| e.session_id == caller)
                    {
                        for folder in &employee.permissions.memory_folders {
                            grants.push(Grant {
                                principal_id: principal.clone(),
                                scope_id: "boss".into(),
                                collections: vec![folder.clone()],
                                read: true,
                                write: false,
                                grantor: "boss".into(),
                                revision: state.revision,
                                expires_at: employee.expired.then_some(0),
                            });
                        }
                    }
                }
                let scope = Scope {
                    version: 1,
                    scope_id: "boss".into(),
                    daemon_id: state.identity.id.to_string(),
                    kind: "boss".into(),
                    owner_id: "boss".into(),
                    acl_revision: state.revision,
                };
                let acl = Acl {
                    scopes: vec![scope.clone()],
                    grants,
                    now: waku_protocol::model::unix_time(),
                };
                let store = Store::open(self.root.join("files/memory-engine"))?;
                store.create_scope(&scope)?;
                let mut index = None;
                let mut chunks = Vec::new();
                let mut inserted = None;
                let mut imported = None;
                match operation {
                    MemoryOperation::Insert {
                        collection,
                        title,
                        cue,
                        body,
                        source_id,
                    } => {
                        inserted = Some(store.insert(
                            &acl,
                            &principal,
                            "boss",
                            &collection,
                            &title,
                            &cue,
                            &body,
                            &source_id,
                            "detail",
                        )?);
                    }
                    MemoryOperation::ImportFolder { folder, collection } => {
                        let source = Path::new(&folder);
                        if folder.is_empty()
                            || source
                                .components()
                                .any(|part| !matches!(part, std::path::Component::Normal(_)))
                        {
                            bail!("invalid Boss memory folder");
                        }
                        let source = self.root.join("files/memory").join(source);
                        imported = Some(store.import_folder(
                            &acl,
                            &principal,
                            "boss",
                            source.to_str().context("invalid Boss memory path")?,
                            &collection,
                        )?);
                    }
                    MemoryOperation::Surface { collection, limit } => {
                        chunks =
                            store.surface_fallback(&acl, &principal, "boss", &collection, limit)?;
                    }
                    MemoryOperation::ListIndex => {
                        index = Some(store.list_index(&acl, &principal, "boss")?)
                    }
                    MemoryOperation::Search { collection, query } => {
                        chunks = store.search(&acl, &principal, "boss", &collection, &query)?;
                    }
                    MemoryOperation::ReadChunk {
                        collection,
                        chunk_id,
                    } => {
                        chunks.push(store.read_chunk(
                            &acl,
                            &principal,
                            "boss",
                            &collection,
                            &chunk_id,
                        )?);
                    }
                    MemoryOperation::Zoom { collection, target } => {
                        chunks = store.zoom(&acl, &principal, "boss", &collection, &target)?;
                    }
                }
                Ok(BossResult::Memory {
                    index,
                    chunks,
                    inserted,
                    imported,
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
            BossOperation::UpsertPersona { mut persona } => {
                self.require_owner(caller)?;
                validate_name(&persona.name)?;
                if persona.icon.is_some_and(|icon| !icon.is_employee_icon()) {
                    bail!("persona icon is not in the employee icon set");
                }
                if persona.markdown.len() > MAX_FILE_BYTES {
                    bail!("persona is too large");
                }
                for path in &persona.pinned_files {
                    if path == "memory" || path.starts_with("memory/") {
                        bail!("pinned files are paths beneath memory/");
                    }
                    validate_relative(path, false)?;
                    self.file_path(&format!("memory/{path}"), false)?;
                }
                self.validate_memory_folders(&persona.permissions.memory_folders)?;
                if persona.id.is_nil() {
                    persona.id = Uuid::new_v4();
                }
                self.update(|state| {
                    if let Some(existing) = state
                        .personas
                        .iter_mut()
                        .find(|entry| entry.id == persona.id)
                    {
                        *existing = persona;
                    } else {
                        state.personas.push(persona);
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
                fs::create_dir_all(self.file_path(&path, false)?)?;
                self.update(|_| Ok(()))?;
                Ok(BossResult::Saved)
            }
            BossOperation::PublishBundle { path, name } => {
                self.require_owner(caller)?;
                let target = PathBuf::from(&path);
                if !target.is_absolute() {
                    bail!("bundle paths must be absolute");
                }
                let metadata = fs::metadata(&target).context("bundle path does not exist")?;
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
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    // Re-publishing a path refreshes the bundle in place —
                    // it jumps back into the sidebar's recency window.
                    if let Some(bundle) =
                        state.bundles.iter_mut().find(|bundle| bundle.path == path)
                    {
                        bundle.name = name;
                        bundle.directory = directory;
                        bundle.updated_at = now;
                    } else {
                        state.bundles.push(BossBundle {
                            id: Uuid::new_v4(),
                            name,
                            path,
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
            BossOperation::DismissBundle { id } => {
                self.require_owner(caller)?;
                self.update(|state| {
                    let count = state.bundles.len();
                    state.bundles.retain(|bundle| bundle.id != id);
                    if state.bundles.len() == count {
                        bail!("unknown bundle");
                    }
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::PinBundle { id, pinned } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let bundle = state
                        .bundles
                        .iter_mut()
                        .find(|bundle| bundle.id == id)
                        .ok_or_else(|| anyhow!("unknown bundle"))?;
                    bundle.pinned_at = pinned.then_some(now);
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::SweepBundle { id, dormant } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let bundle = state
                        .bundles
                        .iter_mut()
                        .find(|bundle| bundle.id == id)
                        .ok_or_else(|| anyhow!("unknown bundle"))?;
                    bundle.dormant_at = dormant.then_some(now);
                    if !dormant {
                        // Restoring re-enters the recency window the way a
                        // re-publish does — a dormant bundle can outlive it.
                        bundle.updated_at = now;
                    }
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::ArchiveBundle { id, archived } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let bundle = state
                        .bundles
                        .iter_mut()
                        .find(|bundle| bundle.id == id)
                        .ok_or_else(|| anyhow!("unknown bundle"))?;
                    bundle.archived_at = archived.then_some(now);
                    Ok(())
                })?;
                Ok(BossResult::Saved)
            }
            BossOperation::MarkBundleViewed { id } => {
                self.require_owner(caller)?;
                let now = waku_protocol::model::unix_time();
                self.update(|state| {
                    let bundle = state
                        .bundles
                        .iter_mut()
                        .find(|bundle| bundle.id == id)
                        .ok_or_else(|| anyhow!("unknown bundle"))?;
                    bundle.viewed_at = Some(now);
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

    pub(crate) fn update(
        &self,
        change: impl FnOnce(&mut BossState) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        let mut next = state.clone();
        change(&mut next)?;
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
        for persona in &state.personas {
            let directory = self.file_path(&format!("personas/{}", persona.id), false)?;
            fs::create_dir_all(&directory)?;
            atomic_write(&directory.join("PERSONA.md"), persona.markdown.as_bytes())?;
        }
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
        let permitted = employee
            .permissions
            .memory_folders
            .iter()
            .map(|folder| format!("memory/{folder}"))
            .any(|folder| path == folder || path.starts_with(&format!("{folder}/")))
            || path
                .strip_prefix("memory/")
                .is_some_and(|rest| employee.pinned_files.iter().any(|file| file == rest))
            || path == format!("personas/{}/PERSONA.md", persona.id);
        // Directory discovery reveals only ancestors of a granted file/folder.
        let ancestor = directory
            && (path.is_empty()
                || employee
                    .permissions
                    .memory_folders
                    .iter()
                    .map(|folder| format!("memory/{folder}"))
                    .chain(
                        employee
                            .pinned_files
                            .iter()
                            .map(|file| format!("memory/{file}")),
                    )
                    .any(|file| file.starts_with(&format!("{path}/"))));
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
}

/// Render memory-relative pins as the files-root paths agents pass to
/// `readFile`.
fn pinned_paths(pinned: &[String]) -> impl Iterator<Item = String> + '_ {
    pinned.iter().map(|path| format!("memory/{path}"))
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

/// Collapse separators in a validated Boss path so `"memory//plans/x.md"`
/// and `"memory/plans/x.md"` compare equal — the freeze check cannot be
/// dodged by spelling the same file another way.
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

/// Canonical `plans/<name>.md` beneath the memory root, from whatever the
/// caller sent — `auth.md`, `plans/auth.md`, and `memory/plans/auth.md`
/// all name the same document. Nested subdirectories beneath `plans/`
/// stay nested; traversal, absolute paths, and non-Markdown names fail.
/// Validation runs before normalization so a `..` fails loudly instead of
/// collapsing into an unrelated plan name; the normalized-then-stripped
/// order matches `plan_file_frozen`, so every spelling of one document
/// freezes together.
pub fn normalize_plan_file(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    validate_relative(trimmed, false)?;
    let cleaned = normalize_plan_path(trimmed);
    let rest = cleaned.strip_prefix("memory/").unwrap_or(&cleaned);
    let rest = rest.strip_prefix("plans/").unwrap_or(rest);
    if rest.is_empty() || !rest.ends_with(".md") {
        bail!("plan files are named like `auth.md` under plans/");
    }
    Ok(format!("plans/{rest}"))
}

fn employee_human_name<'a>(id: Uuid, existing_names: impl IntoIterator<Item = &'a str>) -> String {
    const NAMES: &[&str] = &[
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
    ];
    let base = NAMES[(id.as_u128() % NAMES.len() as u128) as usize];
    let existing_names = existing_names
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    if !existing_names.contains(base) {
        return base.into();
    }

    for suffix_len in 1.. {
        let mut suffix = String::with_capacity(suffix_len);
        let mut value = suffix_len;
        while value > 0 {
            value -= 1;
            suffix.insert(0, (b'A' + (value % 26) as u8) as char);
            value /= 26;
        }
        let candidate = format!("{base} {suffix}.");
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
            BossPersona { id: persona_id, name: "Boss".into(), markdown: "You coordinate employees for the human. Heavy delegation is your default: assign code changes, research, internet access, builds, code generation, long-running checks and tests, and Git integration (cherry-picks, merges, conflict resolution) to employees promptly, keeping yourself available for the human. Never run or poll long-running commands yourself. Ask employees to use shared build caches or dedicated output directories when that avoids contention with the user's tools. Never poll, watch, or wait yourself — hand recurring checks and waits to an employee; mark each summon `workGoal` — an `errand` when you need to know when it finishes (its finish reports back to you), a `goal` when it finishes without you (its record lands on the human's Goals page instead) — a finish also reaches you when the employee flagged a blocker or its session failed. Use `roster` for a cheap status check, `view` for employee details, and `context` for the user's work. Verify completion from the worktree and its commits before reporting work done; do not rely on a summary alone.\n\nUse `steer` for mid-flight corrections that change what the employee is writing right now. Use `prompt` for content whose relevance starts after the current step, such as queue additions or follow-ups. Prompt or steer can resume an employee after its idle expiry with the same transcript; summon a fresh employee for a new or distinct job, or when the previous employee is dead or finishing; never stack prompts onto an expiring employee, where queued work may be lost.\n\nTrack employee ownership, worktrees, and landed versus in-flight work in durable memory, and reconcile the notes as work changes. Publish useful employee outputs as bundles. Report outcomes and blockers only; the human does not need narration about expired employees, name releases, expiry timers, summons, integration mechanics, or other internal Boss operations. Speak when work completes, a timely interruption will help the human, the human needs to act, or they ask; stay quiet otherwise. Respect user-set resource constraints, including model routing and employee caps, and preserve them durably in memory. Build and maintain a reusable persona library across projects: notice recurring work patterns, create named purpose-specific roles such as Researcher, Feature Developer, Bug Investigator, or Verifier, and refine existing roles as experience accumulates. Inspect existing personas before adding one; improve a close match instead of creating duplicates. Keep each persona's guidance focused on durable methods and boundaries that transfer across projects. Maintain personas and your own files. Your memory is a standing duty: record durable facts and decisions as they surface, file them under memory/ folders per topic or project, and prune or reconcile stale entries instead of accumulating duplicates. You control all employees and personas. Grant each employee only the memory folders required by their role and task. Personal memory is boss-only by default; grant it only when the task genuinely requires personal context. When a persona repeatedly needs shared memory, create a per-role memory folder and grant that folder instead. Persona permissions are the memory grant mechanism for summon; pinnedFiles lists memory files an agent always sees in its context and can read without a folder grant; a per-field `permissions` object on summon — or `setPermissions` via control — tailors one employee's grants without editing the persona.".into(), pinned_files: Vec::new(), permissions: PersonaPermissions { summon_employees: true, ..Default::default() } , icon: None },
        ],
        employees: Vec::new(),
        retired_employees: Vec::new(),
        bundles: Vec::new(),
        planning: Vec::new(),
        goals_viewed_at: None,
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
        bundles: Vec::new(),
        planning: Vec::new(),
        goals_viewed_at: None,
        revision: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .prepare_employee(boss, state.personas[0].id, "Review".into(), None, EmployeeGoal::Errand)
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
        assert_eq!(
            employee_human_name(
                session_id,
                service
                    .document()
                    .employees
                    .iter()
                    .map(|e| e.identity.name.as_str())
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
        service.update(|state| { state.session_id = Some(boss); Ok(()) }).unwrap();
        let employee = service
            .prepare_employee(
                boss,
                service.document().personas[0].id,
                "Review".into(),
                None,
                EmployeeGoal::Errand,
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
        assert!(error.contains("name has been reused; summon a new employee"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    /// Retirement exists to clear errand noise; a finished goal's record is
    /// what the client's Goals page lists, so it never ages out.
    #[test]
    fn a_finished_goal_stays_on_the_roster_past_retirement() {
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
            .prepare_employee(boss, state.personas[0].id, "Watch".into(), None, EmployeeGoal::Goal)
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

        assert!(service.retire_expired(u64::MAX / 2).unwrap().is_empty());
        assert!(service.is_employee(session_id));
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
        let employee = state
            .personas
            .iter()
            .find(|persona| persona.name == "Employee")
            .unwrap();
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
        assert!(employee.permissions.memory_folders.is_empty());
        assert!(
            boss.markdown
                .contains("Personal memory is boss-only by default")
        );
        assert!(
            boss.markdown
                .contains("only the memory folders required by their role and task")
        );
        assert!(boss.markdown.contains("per-role memory folder"));
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
            .prepare_employee(boss, service.document().personas[0].id, "Review".into(), None, EmployeeGoal::Errand)
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
        // Resurrection hands the employee a fresh job — the old flag goes
        // with the job that raised it.
        service.expire(session_id).unwrap();
        assert!(service.report_blocker(session_id, "too late".into()).is_err());
        assert!(service.resurrect(session_id).unwrap());
        assert_eq!(
            service.employee(session_id).unwrap().blocker,
            None
        );
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
                        persona: persona.clone()
                    },
                )
                .is_err()
        );
        persona.icon = Some(CustomCommandIcon::Search);
        assert!(
            service
                .handle(None, BossOperation::UpsertPersona { persona })
                .is_ok()
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
    fn employees_cannot_read_ungranted_memory_or_mutate_state() {
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
                        memory_folders: vec!["work".into()],
                        ..Default::default()
                    },
                    pinned_files: Vec::new(),
                    expired: false,
                    expired_at: None,
                    blocker: None,
                });
                Ok(())
            })
            .unwrap();
        for (path, content) in [
            ("memory/work/note.md", "public"),
            ("memory/private/note.md", "private"),
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
                        path: "memory/work/note.md".into()
                    }
                )
                .is_ok()
        );
        for path in [
            "memory/private/note.md",
            "memory/work/../private/note.md",
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
                        persona: state.personas[0].clone()
                    }
                )
                .is_err()
        );
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::WriteFile {
                        path: "memory/work/note.md".into(),
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
                persona.permissions.memory_folders = vec!["work".into()];
                persona.permissions.summon_employees = true;
                Ok(())
            })
            .unwrap();
        let parent = service
            .prepare_employee(boss, persona, "Release".into(), None, EmployeeGoal::Errand)
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
                persona.permissions.memory_folders.push("private".into());
                persona.permissions.computer_use = true;
                persona.permissions.integration_ids.push("linear".into());
                Ok(())
            })
            .unwrap();
        let child = service
            .prepare_employee(parent_id, persona, "Child".into(), None, EmployeeGoal::Errand)
            .unwrap();
        assert_eq!(child.permissions.memory_folders, vec!["work"]);
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
                .prepare_employee(parent_id, persona, "Again".into(), None, EmployeeGoal::Errand)
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
                persona.permissions.memory_folders = vec!["shared".into()];
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
                    memory_folders: Some(vec!["work".into()]),
                    computer_use: Some(true),
                    ..Default::default()
                }),
                EmployeeGoal::Errand,
            )
            .unwrap();
        assert_eq!(employee.permissions.memory_folders, vec!["work"]);
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
        // Traversal or absolute memory folders are rejected outright.
        assert!(
            service
                .prepare_employee(
                    boss,
                    persona,
                    "Escape".into(),
                    Some(PermissionOverrides {
                        memory_folders: Some(vec!["../self".into()]),
                        ..Default::default()
                    }),
                    EmployeeGoal::Errand,
                )
                .is_err()
        );
        // An employee summoner's overrides cannot widen past its own grants.
        let child = service
            .prepare_employee(
                parent_id,
                persona,
                "Child".into(),
                Some(PermissionOverrides {
                    memory_folders: Some(vec!["shared".into(), "private".into()]),
                    computer_use: Some(true),
                    summon_employees: Some(false),
                    ..Default::default()
                }),
                EmployeeGoal::Errand,
            )
            .unwrap();
        assert_eq!(child.permissions.memory_folders, Vec::<String>::new());
        assert!(child.permissions.computer_use);
        assert!(!child.permissions.summon_employees);
        // The boss rewrites a live employee's grants field by field.
        service
            .set_employee_permissions(
                Some(boss),
                parent_id,
                PermissionOverrides {
                    memory_folders: Some(vec!["work/release".into()]),
                    computer_use: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        let updated = service.employee(parent_id).unwrap();
        assert_eq!(updated.permissions.memory_folders, vec!["work/release"]);
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
            .prepare_employee(supervisor, persona, "Release engineer".into(), None, EmployeeGoal::Errand)
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
        assert_eq!(migrated.document().employees[0].identity.name, human_name);
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
    fn bundles_publish_dismiss_and_survive_restart() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let output = std::env::temp_dir().join(format!("boss-bundle-{}", Uuid::new_v4()));
        fs::create_dir_all(&output).unwrap();
        let file = output.join("report.md");
        fs::write(&file, "report").unwrap();
        let file_path = file.to_string_lossy().into_owned();
        let dir_path = output.to_string_lossy().into_owned();
        let service = BossService::open(root.clone()).unwrap();

        // Only the boss or a human manages bundles — an employee is refused.
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
                });
                Ok(())
            })
            .unwrap();
        for op in [
            BossOperation::PublishBundle {
                path: file_path.clone(),
                name: None,
            },
            BossOperation::DismissBundle { id: Uuid::nil() },
            BossOperation::PinBundle {
                id: Uuid::nil(),
                pinned: true,
            },
            BossOperation::SweepBundle {
                id: Uuid::nil(),
                dormant: true,
            },
            BossOperation::ArchiveBundle {
                id: Uuid::nil(),
                archived: true,
            },
            BossOperation::MarkBundleViewed { id: Uuid::nil() },
        ] {
            assert!(service.handle(Some(employee_id), op).is_err());
        }
        // Bundles carry absolute paths to real outputs — relative paths and
        // missing files are both refused.
        assert!(
            service
                .handle(
                    None,
                    BossOperation::PublishBundle {
                        path: "outputs/report.md".into(),
                        name: None,
                    },
                )
                .is_err()
        );
        assert!(
            service
                .handle(
                    None,
                    BossOperation::PublishBundle {
                        path: "/definitely/missing".into(),
                        name: None,
                    },
                )
                .is_err()
        );

        service
            .handle(
                None,
                BossOperation::PublishBundle {
                    path: file_path.clone(),
                    name: None,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::PublishBundle {
                    path: dir_path.clone(),
                    name: Some("Deliverables".into()),
                },
            )
            .unwrap();
        let state = service.document();
        assert_eq!(state.bundles.len(), 2);
        let file_bundle = state
            .bundles
            .iter()
            .find(|bundle| bundle.path == file_path)
            .unwrap();
        assert_eq!(file_bundle.name, "report.md");
        assert!(!file_bundle.directory);
        let dir_bundle = state
            .bundles
            .iter()
            .find(|bundle| bundle.path == dir_path)
            .unwrap();
        assert_eq!(dir_bundle.name, "Deliverables");
        assert!(dir_bundle.directory);
        let file_id = file_bundle.id;
        let file_created = file_bundle.created_at;

        // Re-publishing refreshes the same bundle rather than stacking rows.
        service
            .handle(
                None,
                BossOperation::PublishBundle {
                    path: file_path.clone(),
                    name: Some("Report".into()),
                },
            )
            .unwrap();
        let state = service.document();
        assert_eq!(state.bundles.len(), 2);
        let republished = state
            .bundles
            .iter()
            .find(|bundle| bundle.path == file_path)
            .unwrap();
        assert_eq!(republished.id, file_id);
        assert_eq!(republished.name, "Report");
        assert_eq!(republished.created_at, file_created);
        assert!(republished.updated_at >= file_created);

        // Sidebar affordances ride the same owner gate: pin, sweep, and
        // archive mutate the bundle in place and refuse unknown ids.
        for op in [
            BossOperation::PinBundle {
                id: Uuid::new_v4(),
                pinned: true,
            },
            BossOperation::SweepBundle {
                id: Uuid::new_v4(),
                dormant: true,
            },
            BossOperation::ArchiveBundle {
                id: Uuid::new_v4(),
                archived: true,
            },
            BossOperation::MarkBundleViewed { id: Uuid::new_v4() },
        ] {
            assert!(service.handle(None, op).is_err());
        }
        service
            .handle(
                None,
                BossOperation::PinBundle {
                    id: file_id,
                    pinned: true,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::SweepBundle {
                    id: file_id,
                    dormant: true,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::ArchiveBundle {
                    id: file_id,
                    archived: true,
                },
            )
            .unwrap();
        let bundle = service.document().bundles[0].clone();
        assert_eq!(bundle.id, file_id);
        assert!(bundle.pinned_at.is_some());
        assert!(bundle.dormant_at.is_some());
        assert!(bundle.archived_at.is_some());
        // The same operations clear their flags — and unarchiving keeps the
        // row's other state.
        service
            .handle(
                None,
                BossOperation::ArchiveBundle {
                    id: file_id,
                    archived: false,
                },
            )
            .unwrap();
        service
            .handle(
                None,
                BossOperation::SweepBundle {
                    id: file_id,
                    dormant: false,
                },
            )
            .unwrap();
        let bundle = service.document().bundles[0].clone();
        assert_eq!(bundle.archived_at, None);
        assert_eq!(bundle.dormant_at, None);
        assert!(bundle.pinned_at.is_some());

        // A fresh bundle is unread until the owner opens it; re-publishing
        // refreshed content makes it unread again by leaving `viewed_at`
        // behind `updated_at`.
        assert_eq!(bundle.viewed_at, None);
        service
            .handle(None, BossOperation::MarkBundleViewed { id: file_id })
            .unwrap();
        let bundle = service.document().bundles[0].clone();
        let viewed = bundle.viewed_at.expect("opening stamps viewed_at");
        assert!(viewed >= bundle.updated_at);
        service
            .handle(
                None,
                BossOperation::PublishBundle {
                    path: file_path.clone(),
                    name: None,
                },
            )
            .unwrap();
        let bundle = service.document().bundles[0].clone();
        assert_eq!(bundle.viewed_at, Some(viewed));
        assert!(bundle.updated_at >= viewed);

        drop(service);
        let restored = BossService::open(root.clone()).unwrap();
        assert_eq!(restored.document().bundles.len(), 2);
        assert!(
            restored
                .handle(None, BossOperation::DismissBundle { id: Uuid::new_v4() })
                .is_err()
        );
        restored
            .handle(None, BossOperation::DismissBundle { id: file_id })
            .unwrap();
        assert_eq!(restored.document().bundles.len(), 1);
        drop(restored);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(output).unwrap();
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

    #[test]
    fn pinned_memory_files_grant_read_and_survive_legacy_migration() {
        let root = std::env::temp_dir().join(format!("boss-pins-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let session_id = Uuid::new_v4();
        let persona = service.document().personas[1].id;
        service
            .update(|state| {
                state.employees.push(BossEmployee {
                    session_id,
                    supervisor_id: Uuid::new_v4(),
                    identity: BossIdentity {
                        id: session_id,
                        name: "Maren".into(),
                        avatar_seed: session_id.to_string(),
                    },
                    job_title: "Review".into(),
                    persona_id: persona,
                    work_goal: EmployeeGoal::Errand,
                    created_at: None,
                    icon: None,
                    permissions: PersonaPermissions::default(),
                    pinned_files: vec!["work/note.md".into()],
                    expired: false,
                    expired_at: None,
                    blocker: None,
                });
                Ok(())
            })
            .unwrap();
        for (path, content) in [
            ("memory/work/note.md", "pinned"),
            ("memory/work/other.md", "unpinned"),
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
        // A pin grants file-level read without a folder grant; neighbors
        // in the same folder stay closed.
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::ReadFile {
                        path: "memory/work/note.md".into()
                    }
                )
                .is_ok()
        );
        assert!(
            service
                .handle(
                    Some(session_id),
                    BossOperation::ReadFile {
                        path: "memory/work/other.md".into()
                    }
                )
                .is_err()
        );
        // Pins inject the files-root path into the persona context.
        let prompt = service.prompt_with_context(session_id, "job".into());
        assert!(prompt.contains("Pinned memory files: memory/work/note.md"));
        // Pins are memory-relative — writing them with the store prefix is refused.
        let mut persona_doc = service.document().personas[1].clone();
        persona_doc.pinned_files = vec!["memory/work/note.md".into()];
        assert!(
            service
                .handle(
                    None,
                    BossOperation::UpsertPersona {
                        persona: persona_doc
                    },
                )
                .is_err()
        );

        drop(service);
        // Legacy documents pinned knowledge files beneath the files root:
        // `memory/` paths rewrite relative to the store, pins of files
        // elsewhere move the file beneath `memory/`, and unresolvable
        // pins drop.
        let path = root.join("boss.json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::write(root.join("files/stray.md"), "moved").unwrap();
        legacy["personas"][1]
            .as_object_mut()
            .unwrap()
            .remove("pinnedFiles");
        legacy["personas"][1]["knowledgeFiles"] = serde_json::json!([
            "memory/self/core.md",
            "stray.md",
            "gone.md",
            "personas/managed/PERSONA.md"
        ]);
        legacy["employees"][0]
            .as_object_mut()
            .unwrap()
            .remove("pinnedFiles");
        legacy["employees"][0]["knowledgeFiles"] = serde_json::json!(["memory/work/note.md"]);
        fs::create_dir_all(root.join("files/memory/self")).unwrap();
        fs::write(root.join("files/memory/self/core.md"), "core").unwrap();
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let migrated = BossService::open(root.clone()).unwrap();
        let state = migrated.document();
        assert_eq!(
            state.personas[1].pinned_files,
            vec!["self/core.md".to_owned(), "stray.md".to_owned()]
        );
        assert_eq!(
            state.employees[0].pinned_files,
            vec!["work/note.md".to_owned()]
        );
        assert!(!root.join("files/stray.md").exists());
        assert_eq!(
            fs::read_to_string(root.join("files/memory/stray.md")).unwrap(),
            "moved"
        );
        drop(migrated);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod memory_op_tests {
    use super::*;
    use waku_protocol::boss::MemoryOperation;

    #[test]
    fn memory_ops_follow_collection_grants() {
        let root = std::env::temp_dir().join(format!("boss-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service
            .update(|state| {
                state.session_id = Some(boss);
                Ok(())
            })
            .unwrap();
        let boss_memory = |operation| {
            service.handle(Some(boss), BossOperation::Memory { operation })
        };
        let insert = |collection: &str, title: &str, cue: &str, body: &str, source: &str| {
            MemoryOperation::Insert {
                collection: collection.into(),
                title: title.into(),
                cue: cue.into(),
                body: body.into(),
                source_id: source.into(),
            }
        };
        let BossResult::Memory {
            inserted: Some(work),
            ..
        } = boss_memory(insert("work", "Launch owner", "launch owner", "Alec owns the launch.", "t:1"))
            .unwrap()
        else {
            panic!("boss insert returns the chunk");
        };
        boss_memory(insert(
            "private",
            "Private note",
            "boss-only cue",
            "boss-only body",
            "t:2",
        ))
        .unwrap();
        // The same scope/collection/source/body is an idempotent retry.
        let BossResult::Memory {
            inserted: Some(again),
            ..
        } = boss_memory(insert("work", "Launch owner", "launch owner", "Alec owns the launch.", "t:1"))
            .unwrap()
        else {
            panic!("repeat insert returns the existing chunk");
        };
        assert_eq!(again.chunk_id, work.chunk_id);

        let persona = service.document().personas[0].id;
        let employee = service
            .prepare_employee(
                boss,
                persona,
                "Review".into(),
                Some(PermissionOverrides {
                    memory_folders: Some(vec!["work".into()]),
                    ..Default::default()
                }),
                EmployeeGoal::Errand,
            )
            .unwrap();
        let employee_id = employee.session_id;
        service
            .update(|state| {
                state.employees.push(employee);
                Ok(())
            })
            .unwrap();
        let employee_memory = |operation| {
            service.handle(Some(employee_id), BossOperation::Memory { operation })
        };

        let BossResult::Memory {
            index: Some(index),
            ..
        } = employee_memory(MemoryOperation::ListIndex).unwrap()
        else {
            panic!("employee index read succeeds");
        };
        assert!(index.contains("Launch owner"));
        assert!(!index.contains("Private note") && !index.contains("boss-only"));
        let BossResult::Memory { chunks, .. } = employee_memory(MemoryOperation::Search {
            collection: "work".into(),
            query: "launch".into(),
        })
        .unwrap()
        else {
            panic!("employee search inside a granted collection succeeds");
        };
        assert_eq!(chunks.len(), 1);
        assert!(employee_memory(MemoryOperation::ReadChunk {
            collection: "private".into(),
            chunk_id: work.chunk_id.clone(),
        })
        .is_err());
        assert!(employee_memory(MemoryOperation::Search {
            collection: "private".into(),
            query: "boss-only".into(),
        })
        .is_err());
        assert!(employee_memory(MemoryOperation::Surface {
            collection: "private".into(),
            limit: 10,
        })
        .is_err());
        assert!(employee_memory(MemoryOperation::Zoom {
            collection: "private".into(),
            target: "topic:inbox".into(),
        })
        .is_err());
        assert!(employee_memory(insert("work", "sneaky", "sneaky", "sneaky", "t:3")).is_err());

        // Expiry revokes even the granted collection.
        service.expire(employee_id).unwrap();
        assert!(employee_memory(MemoryOperation::Search {
            collection: "work".into(),
            query: "launch".into(),
        })
        .is_err());
        let BossResult::Memory {
            index: Some(index),
            ..
        } = employee_memory(MemoryOperation::ListIndex).unwrap()
        else {
            panic!("an expired employee's index still renders, minus every cue");
        };
        assert!(!index.contains("Launch owner"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn normalize_plan_file_maps_every_spelling_to_the_canonical_document() {
        for spelling in [
            "auth.md",
            "plans/auth.md",
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
                    session_id: planning,
                    plan_file: "plans/auth.md".into(),
                    idea: "Auth".into(),
                    finalized_at: None,
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
                    path: "memory/plans/auth.md".into(),
                    content: "draft".into(),
                },
            )
            .unwrap();
        let stamped = service.finalize_plan("plans/auth.md", 100).unwrap();
        assert_eq!(stamped.finalized_at, Some(100));
        // The stamp is monotonic: re-finalizing cannot re-time it.
        assert_eq!(
            service
                .finalize_plan("plans/auth.md", 200)
                .unwrap()
                .finalized_at,
            Some(100)
        );
        for spelling in ["memory/plans/auth.md", "memory//plans/auth.md"] {
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
                    path: "memory/plans/auth.md".into(),
                },
            )
            .unwrap()
        else {
            panic!("the frozen document still reads")
        };
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
                    session_id: planning,
                    plan_file: "plans/auth.md".into(),
                    idea: "Auth".into(),
                    finalized_at: None,
                });
                Ok(())
            })
            .unwrap();
        let mut employee = service
            .prepare_employee(planning, service.document().personas[0].id, "Job".into(), None, EmployeeGoal::Errand)
            .unwrap();
        employee.supervisor_id = planning;
        assert_eq!(service.report_target(&employee), Some(planning));
        // An employee the planning session did not summon still reports to
        // its own supervisor's chain.
        employee.supervisor_id = Uuid::new_v4();
        assert_eq!(service.report_target(&employee), Some(boss));
        fs::remove_dir_all(root).unwrap();
    }
}
