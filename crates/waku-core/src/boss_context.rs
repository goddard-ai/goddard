//! The boss context router: a two-tier digest of the user's projects, tasks,
//! and automations. Every outbound boss prompt carries the compact header —
//! project names plus live counts — so orientation is never gated. Jev still
//! decides per user prompt whether the full digest rides along; a gate miss
//! only withholds bulk detail, and the boss can pull it on demand through
//! the `context` operation.
//!
//! Jev judges two things on one shared `state`: whether the digest is worth
//! attaching, and which project the user's attention currently centers on —
//! the focus inference, which this module keeps stateful by feeding the
//! previous answer back into the next request's `state`. Everything else is
//! deterministic: thresholds below decide what an answer may apply, and any
//! missing, malformed, or failed evaluation leaves the defaults standing —
//! no digest attachment, no focus change.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use waku_protocol::automations::{AutomationSchedule, AutomationsState};
use waku_protocol::boss::BossState;
use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{AgentSession, Project, SessionStatus, SessionWorkspace, truncate_chars, unix_time};

use crate::persistence::PersistedState;

/// The decision-log feature tag every router call records under.
pub const FEATURE: &str = "boss-context-router";

/// The choice option answering "no project in particular".
const FOCUS_NONE: &str = "none";

/// The attach Noul must reach this probability before the digest steers in.
/// Misses are recoverable — the boss can call the `context` operation
/// itself — so the bar sits at the middle of the scale, not below it.
const ATTACH_THRESHOLD: f64 = 0.5;

/// A focus update needs the suggestion bar: the winning option at least
/// this probable and this far ahead of the runner-up. Below it the previous
/// focus stands — focus is sticky state, not a per-prompt re-roll.
const FOCUS_MIN_PROBABILITY: f64 = 0.5;
const FOCUS_MIN_MARGIN: f64 = 0.15;

/// Session lines a single project section may carry before folding.
const SESSIONS_PER_PROJECT: usize = 10;
/// Total digest budget — large enough to cover a busy project catalog,
/// small enough to attach to a prompt without crowding it.
const DIGEST_CAP: usize = 12_000;
/// Header budget — the always-on block stays near 200 tokens. Over-cap
/// projects fold into a count rather than dropping silently.
const HEADER_CAP: usize = 1_000;
/// Focus choices cap: the option list doubles as the digest's project set,
/// and a Choice distributes probability mass — past this it stops
/// discriminating.
const FOCUS_OPTIONS_CAP: usize = 24;
const TITLE_CAP: usize = 80;
const PATH_CAP: usize = 120;
const EMPLOYEES_LISTED: usize = 8;
const AUTOMATION_PROMPT_CAP: usize = 100;
/// The user prompt's budget inside eval `state`.
const PROMPT_CAP: usize = 4_000;

/// The work snapshot: the always-on header, the rendered digest, and the
/// project names that double as the router's focus options.
pub struct WorkContext {
    /// Compact per-project live counts — attaches to every boss prompt,
    /// never gated.
    pub header: String,
    /// The full digest Jev gates per prompt.
    pub digest: String,
    /// User-project names in digest order — the `focus` Choice's option set.
    pub projects: Vec<String>,
}

/// Render a cheap employee overview from the persisted boss and task
/// snapshots used by `view()`. This deliberately reads no transcript data.
pub fn employee_roster(
    boss: &BossState,
    sessions: &[AgentSession],
    projects: &[Project],
) -> String {
    let now = unix_time();
    let mut rows = boss.employees.iter().map(|employee| {
        let session = sessions.iter().find(|session| session.id == employee.session_id);
        let status = if employee.expired {
            "expired"
        } else if employee.blocker.is_some() {
            "blocked"
        } else {
            match session.map(|session| session.status) {
                Some(SessionStatus::Connecting | SessionStatus::Working) => "working",
                Some(SessionStatus::Waiting | SessionStatus::Background) => "finishing",
                Some(SessionStatus::Failed) => "blocked",
                _ => "idle",
            }
        };
        let queued = session.is_some_and(|session| !session.queued_messages.is_empty());
        let rank = if status == "working" {
            0
        } else if queued {
            1
        } else if status == "expired" {
            3
        } else {
            2
        };
        let project = session
            .and_then(|session| projects.iter().find(|project| project.id == session.project_id))
            .map(|project| project.name.as_str())
            .unwrap_or("unknown project");
        let branch = session.and_then(|session| match &session.workspace {
            SessionWorkspace::Worktree {
                branch,
                base_branch,
                ..
            } => branch.as_deref().or(base_branch.as_deref()),
            SessionWorkspace::NewWorktree { base_branch } => base_branch.as_deref(),
            SessionWorkspace::Local => None,
        });
        let provider = session
            .map(|session| {
                serde_json::to_value(session.provider)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".into())
            })
            .unwrap_or_else(|| "unknown".into());
        let model = session.and_then(|session| session.model.as_deref()).unwrap_or("auto");
        let state_since = employee
            .expired_at
            .or_else(|| session.map(|session| session.updated_at))
            .unwrap_or(now);
        let elapsed = compact_duration(now.saturating_sub(state_since));
        let location = branch
            .map_or_else(|| project.to_owned(), |branch| format!("{project}/{branch}"));
        let flags = match (employee.blocker.is_some(), queued) {
            (true, true) => " [blocker, queued]",
            (true, false) => " [blocker]",
            (false, true) => " [queued]",
            (false, false) => "",
        };
        (
            rank,
            employee.identity.name.to_lowercase(),
            format!(
                "{} ({}) — {provider}/{model}, {status} {elapsed}, {location}{flags}",
                employee.identity.name, employee.job_title
            ),
            status,
            queued,
            employee.expired,
        )
    }).collect::<Vec<_>>();
    rows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let live = rows.iter().filter(|row| !row.5).count();
    let count = |status: &str| rows.iter().filter(|row| !row.5 && row.3 == status).count();
    let queued = rows.iter().filter(|row| !row.5 && row.4).count();
    let mut digest = format!(
        "{live} live: {} working, {} idle, {} finishing, {} blocked, {queued} queued",
        count("working"), count("idle"), count("finishing"), count("blocked")
    );
    for row in rows {
        digest.push('\n');
        digest.push_str(&row.2);
    }
    for wave in &boss.waves {
        let done = wave
            .members
            .iter()
            .filter(|member| member.outcome.is_some())
            .count();
        let line = if wave.resolved_at.is_some() {
            format!("wave \"{}\" resolved — {done}/{} members done", wave.id, wave.members.len())
        } else {
            format!("wave \"{}\" in flight — {done}/{} members done", wave.id, wave.members.len())
        };
        digest.push('\n');
        digest.push_str(&line);
    }
    digest
}

fn compact_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

/// Render the daemon's view of the user's work: every registered project
/// with its live tasks and statuses, the boss's employees, and the
/// scheduled automations. Bounded at every level; a daemon with no user
/// projects yields an empty digest, which callers treat as "nothing to
/// route or attach".
pub fn work_context(
    state: &PersistedState,
    boss: &BossState,
    automations: &AutomationsState,
) -> WorkContext {
    let now = unix_time();
    let boss_project = boss.identity.id;
    let boss_session = boss.session_id;
    let employee = |session_id| {
        boss.employees
            .iter()
            .find(|entry| entry.session_id == session_id)
    };
    // Sessions the digest lists: started, unarchived, top-level, and not the
    // boss's own chat — the boss is the reader, not the subject.
    let live = |session: &&AgentSession| {
        Some(session.id) != boss_session
            && session.has_started()
            && session.archived_at.is_none()
            && session.side_chat_of.is_none()
    };
    let mut projects: Vec<&waku_protocol::model::Project> = state
        .projects
        .iter()
        .filter(|project| project.id != boss_project)
        .collect();
    projects.sort_by(|a, b| {
        let activity = |project: &&waku_protocol::model::Project| {
            state
                .sessions
                .iter()
                .filter(|session| session.project_id == project.id)
                .filter(live)
                .map(|session| session.updated_at)
                .max()
                .unwrap_or(project.created_at)
        };
        activity(b).cmp(&activity(a))
    });
    // Live employees attribute to the project their task session lives in,
    // looked up over every stored session — an employee's task may be idle
    // or archived while the employee still counts as live.
    let employee_project = |employee: &waku_protocol::boss::BossEmployee| {
        state
            .sessions
            .iter()
            .find(|session| session.id == employee.session_id)
            .map(|session| session.project_id)
    };
    let live_employees = || boss.employees.iter().filter(|entry| !entry.expired);
    let mut digest = String::new();
    let mut names = Vec::new();
    let mut header_lines = Vec::new();
    let mut mapped_employees = 0usize;
    let mut omitted_projects = 0usize;
    for project in &projects {
        let mut sessions: Vec<&AgentSession> = state
            .sessions
            .iter()
            .filter(|session| session.project_id == project.id)
            .filter(live)
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        // The header lists every user project — only the digest folds.
        let employees_here: Vec<_> = live_employees()
            .filter(|employee| employee_project(employee) == Some(project.id))
            .collect();
        mapped_employees += employees_here.len();
        header_lines.push(header_line(
            project,
            &sessions,
            employees_here.len(),
            employees_here
                .iter()
                .filter(|employee| employee.blocker.is_some())
                .count(),
        ));
        let mut section = String::new();
        let mut header = format!("## {} — {}", project.name, project.path.display());
        if project.starred {
            header.push_str(" (starred)");
        }
        if project.is_friends() {
            header.push_str(" (friend chats)");
        }
        section.push_str(&header);
        section.push('\n');
        let extra = sessions.len().saturating_sub(SESSIONS_PER_PROJECT);
        for session in sessions.iter().take(SESSIONS_PER_PROJECT) {
            section.push_str(&session_line(session, employee(session.id), now));
        }
        if extra > 0 {
            section.push_str(&format!("  - …{extra} more tasks\n"));
        }
        if !digest.is_empty() && digest.len() + section.len() > DIGEST_CAP {
            omitted_projects += 1;
            continue;
        }
        digest.push_str(&section);
        names.push(project.name.clone());
    }
    if omitted_projects > 0 {
        digest.push_str(&format!("## …{omitted_projects} more projects omitted\n"));
    }
    // The always-on header: one counts line per user project, then the
    // fleet tallies no project line can carry. It points at the `context`
    // operation so a gate miss still leaves the boss a path to the detail.
    let unmapped_employees = live_employees().count() - mapped_employees;
    let has_fleet = unmapped_employees > 0 || !automations.automations.is_empty();
    let mut header = String::new();
    if !header_lines.is_empty() || has_fleet {
        header.push_str(
            "Work overview — the `context` operation returns the full digest.\n",
        );
    }
    let mut folded = 0usize;
    for line in header_lines {
        if header.len() + line.len() > HEADER_CAP {
            folded += 1;
            continue;
        }
        header.push_str(&line);
        header.push('\n');
    }
    if folded > 0 {
        header.push_str(&format!("…{folded} more projects\n"));
    }
    if unmapped_employees > 0 {
        header.push_str(&format!("employees: {unmapped_employees} live\n"));
    }
    if !automations.automations.is_empty() {
        header.push_str(&format!(
            "automations: {} scheduled\n",
            automations.automations.len()
        ));
    }
    if !boss.employees.is_empty() {
        digest.push_str("## Employees\n");
        let extra = boss.employees.len().saturating_sub(EMPLOYEES_LISTED);
        for entry in boss.employees.iter().take(EMPLOYEES_LISTED) {
            let status = state
                .sessions
                .iter()
                .find(|session| session.id == entry.session_id)
                .map(|session| status_label(session.status))
                .unwrap_or("idle");
            digest.push_str(&format!(
                "  - {} ({}) — {status}{}{}\n",
                entry.identity.name,
                truncate_chars(&entry.job_title, TITLE_CAP),
                if entry.expired { " — finished" } else { "" },
                if entry.blocker.is_some() {
                    " · blocker flagged"
                } else {
                    ""
                },
            ));
        }
        if extra > 0 {
            digest.push_str(&format!("  - …{extra} more employees\n"));
        }
    }
    if !automations.automations.is_empty() {
        digest.push_str("## Automations\n");
        for automation in &automations.automations {
            let mut line = format!(
                "  - \"{}\" — {} — {}",
                truncate_chars(&automation.name, TITLE_CAP),
                schedule_label(&automation.schedule),
                truncate_chars(&automation.project_path.display().to_string(), PATH_CAP),
            );
            let prompt = automation.prompt.trim();
            if !prompt.is_empty() {
                line.push_str(&format!(
                    " — {}",
                    truncate_chars(&prompt.replace('\n', " "), AUTOMATION_PROMPT_CAP)
                ));
            }
            line.push('\n');
            digest.push_str(&line);
        }
    }
    WorkContext {
        header,
        digest,
        projects: names,
    }
}

/// One header line — `name: N tasks (M active, …), K employees` — names
/// and live counts only; titles and paths stay in the digest. The state
/// breakdown lists only non-empty buckets, and idle needs no label.
fn header_line(
    project: &waku_protocol::model::Project,
    sessions: &[&AgentSession],
    employees: usize,
    blocked: usize,
) -> String {
    let active = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.status,
                SessionStatus::Connecting | SessionStatus::Working | SessionStatus::Background
            )
        })
        .count();
    let waiting = sessions
        .iter()
        .filter(|session| session.status == SessionStatus::Waiting)
        .count();
    let failed = sessions
        .iter()
        .filter(|session| session.status == SessionStatus::Failed)
        .count();
    let mut line = format!("{}: {}", project.name, plural(sessions.len(), "task"));
    let breakdown = [(active, "active"), (waiting, "waiting"), (failed, "failed")]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect::<Vec<_>>();
    if !breakdown.is_empty() {
        line.push_str(&format!(" ({})", breakdown.join(", ")));
    }
    if employees > 0 {
        line.push_str(&format!(", {}", plural(employees, "employee")));
        if blocked > 0 {
            line.push_str(&format!(" ({blocked} blocked)"));
        }
    }
    if project.starred {
        line.push_str(" · starred");
    }
    if project.is_friends() {
        line.push_str(" · friend chats");
    }
    line
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn session_line(
    session: &AgentSession,
    employee: Option<&waku_protocol::boss::BossEmployee>,
    now: u64,
) -> String {
    let mut line = format!(
        "  - \"{}\" — {} · {}",
        truncate_chars(session.display_title(), TITLE_CAP),
        status_label(session.status),
        session.provider.id(),
    );
    if let Some(employee) = employee {
        line.push_str(&format!(" · employee {}", employee.identity.name));
    }
    if session.landed_at.is_some() {
        line.push_str(" · landed");
    }
    if !session.queued_messages.is_empty() {
        line.push_str(&format!(" · {} queued", session.queued_messages.len()));
    }
    line.push_str(&format!(" · {}\n", age_label(session.updated_at, now)));
    line
}

fn status_label(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Idle => "idle",
        SessionStatus::Connecting => "connecting",
        SessionStatus::Working => "working",
        SessionStatus::Waiting => "waiting for input",
        SessionStatus::Background => "working in background",
        SessionStatus::Failed => "failed",
    }
}

fn schedule_label(schedule: &Option<AutomationSchedule>) -> String {
    match schedule {
        Some(AutomationSchedule::Hourly { minute }) => format!("hourly at :{minute:02}"),
        Some(AutomationSchedule::Daily { hour, minute })
        | Some(AutomationSchedule::Weekdays { hour, minute }) => {
            let days = if matches!(schedule, Some(AutomationSchedule::Weekdays { .. })) {
                " on weekdays"
            } else {
                ""
            };
            format!("at {hour:02}:{minute:02}{days}")
        }
        Some(AutomationSchedule::Weekly {
            day_of_week,
            hour,
            minute,
        }) => format!("weekly day {day_of_week} at {hour:02}:{minute:02}"),
        Some(AutomationSchedule::Cron { expression }) => {
            format!("cron \"{expression}\"")
        }
        None => "manual".to_owned(),
    }
}

fn age_label(updated_at: u64, now: u64) -> String {
    let seconds = now.saturating_sub(updated_at);
    let (count, unit) = if seconds < 90 {
        return "just now".to_owned();
    } else if seconds < 3_600 {
        (seconds / 60, "m")
    } else if seconds < 86_400 {
        (seconds / 3_600, "h")
    } else {
        (seconds / 86_400, "d")
    };
    format!("{count}{unit} ago")
}

/// The eval `state` one prompt routes through: the message itself, the
/// conversation's recent user prompts, the previously inferred focus, and
/// the work snapshot the prompt could carry.
pub fn router_state(
    prompt: &str,
    previous_focus: Option<&str>,
    recent_prompts: &[String],
    work: &WorkContext,
) -> Value {
    json!({
        "prompt": truncate_chars(prompt, PROMPT_CAP),
        "recentPrompts": recent_prompts,
        "previousFocus": previous_focus,
        "work": work.digest,
    })
}

/// The two questions every routed prompt asks: should the snapshot attach,
/// and where is the user's attention now. Both judge independently against
/// the same `state`; [`apply_verdict`] combines them.
pub fn router_questions(projects: &[String]) -> BTreeMap<String, EvalQuestion> {
    let mut options: BTreeMap<String, Option<String>> = projects
        .iter()
        .take(FOCUS_OPTIONS_CAP)
        .map(|name| (name.clone(), None))
        .collect();
    options.insert(
        FOCUS_NONE.to_owned(),
        Some("the user's attention is on no particular project".to_owned()),
    );
    BTreeMap::from([
        (
            "attach".to_owned(),
            EvalQuestion::Noul {
                instructions: "The state's work field is a snapshot of the user's \
                    projects, tasks, and automations that the daemon can attach to the \
                    boss's context for this message. Should it? Yes when the message \
                    concerns the user's work — a project, task, employee, automation, \
                    status, or progress — or delegates new work, and when the recent \
                    conversation makes shared context about that work likely necessary. \
                    No for greetings, small talk, self-contained questions, and replies \
                    that stay inside what the boss already knows."
                    .to_owned(),
                criteria: None,
            },
        ),
        (
            "focus".to_owned(),
            EvalQuestion::Choice {
                instructions: "Which single project is the user's attention centered \
                    on right now? Judge this message together with recentPrompts and \
                    previousFocus: a message that plainly continues the prior topic \
                    keeps that focus even when it names no project. Choose none only \
                    when the conversation is not about any listed project."
                    .to_owned(),
                criteria: options,
            },
        ),
    ])
}

/// What one evaluation applies: the attachment decision and, separately, a
/// focus transition — `Some` replaces the tracked focus (the inner `None`
/// clears it), while `None` leaves the previous inference standing.
pub struct RouterVerdict {
    pub attach: bool,
    pub focus: Option<Option<String>>,
}

/// Threshold an evaluation into the verdict code may apply. Untrusted
/// input: a missing or low-probability answer changes nothing.
pub fn apply_verdict(evaluation: &Evaluation) -> RouterVerdict {
    let attach = matches!(
        evaluation.answers.get("attach"),
        Some(EvalAnswer::Noul { noul }) if *noul >= ATTACH_THRESHOLD
    );
    let focus = match evaluation.answers.get("focus") {
        Some(EvalAnswer::Choice {
            choice,
            probabilities,
            ..
        }) => {
            let top = probabilities.get(choice).copied().unwrap_or_default();
            let runner_up = probabilities
                .iter()
                .filter(|(option, _)| *option != choice)
                .map(|(_, probability)| *probability)
                .fold(0.0, f64::max);
            if top >= FOCUS_MIN_PROBABILITY && top - runner_up >= FOCUS_MIN_MARGIN {
                Some((choice != FOCUS_NONE).then(|| choice.clone()))
            } else {
                None
            }
        }
        _ => None,
    };
    RouterVerdict { attach, focus }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use waku_protocol::boss::{
        BossEmployee, BossIdentity, BossResourcePolicy, EmployeeGoal, EmployeeLifecycle,
        PersonaPermissions,
    };
    use waku_protocol::model::{AgentSession, Project, ProviderKind};

    fn boss_state() -> BossState {
        BossState {
            identity: BossIdentity {
                id: Uuid::new_v4(),
                name: "Boss".into(),
                avatar_seed: String::new(),
            },
            persona_id: Uuid::new_v4(),
            session_id: Some(Uuid::new_v4()),
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

    fn project(name: &str) -> Project {
        Project {
            id: Uuid::new_v4(),
            name: name.into(),
            path: format!("/work/{name}").into(),
            bookmark: None,
            created_at: unix_time(),
            temporary: false,
            starred: false,
            friend_peer_id: None,
            kind: None,
        }
    }

    fn session(project: &Project, title: &str, status: SessionStatus) -> AgentSession {
        let mut session = AgentSession::new(project.id, ProviderKind::Claude);
        session.set_title(title);
        session.status = status;
        session.updated_at = unix_time();
        // A stored row the daemon never hydrated: started, by definition.
        session.detail_loaded = false;
        session
    }

    fn employee(
        session_id: Uuid,
        name: &str,
        expired: bool,
        supervisor_id: Uuid,
    ) -> BossEmployee {
        BossEmployee {
            session_id,
            supervisor_id,
            identity: BossIdentity { id: Uuid::new_v4(), name: name.into(), avatar_seed: String::new() },
            job_title: "Engineer".into(),
            persona_id: Uuid::new_v4(),
            work_goal: EmployeeGoal::Errand,
            created_at: None,
            icon: None,
            permissions: PersonaPermissions::default(),
            pinned_files: Vec::new(),
            expired,
            expired_at: expired.then(unix_time),
            blocker: None,
            cancelled: false,
            state: EmployeeLifecycle::Working,
            ticket: None,
            queued_at: None,
            request_id: None,
            request_fingerprint: None,
        }
    }

    fn automation(name: &str) -> waku_protocol::automations::Automation {
        use waku_protocol::automations::{Automation, AutomationWorkspace};
        Automation {
            id: Uuid::new_v4(),
            name: name.into(),
            prompt: "do the thing".into(),
            provider: ProviderKind::Claude,
            model: None,
            project_path: "/work/app".into(),
            workspace: AutomationWorkspace::Local,
            base_branch: None,
            session_id: None,
            schedule: Some(AutomationSchedule::Daily { hour: 9, minute: 0 }),
            webhook_secret: None,
            timezone: None,
            enabled: true,
            precheck: None,
            missed_run_grace_minutes: None,
            reuse_session: false,
            last_session_id: None,
            next_run_at: None,
            last_run_at: None,
            last_run_status: None,
            last_refusal_key: None,
            created_at: unix_time(),
            updated_at: unix_time(),
        }
    }

    #[test]
    fn employee_roster_sorts_working_before_expired_and_includes_project_and_model() {
        let mut boss = boss_state();
        let project = project("workspace");
        let active = session(&project, "active", SessionStatus::Working);
        let mut done = session(&project, "done", SessionStatus::Idle);
        done.id = Uuid::new_v4();
        let supervisor_id = boss.session_id.unwrap();
        boss.employees = vec![
            employee(done.id, "Zed", true, supervisor_id),
            employee(active.id, "Ada", false, supervisor_id),
        ];

        let digest = employee_roster(&boss, &[active], &[project]);
        assert!(digest.starts_with("1 live: 1 working, 0 idle"));
        assert!(digest.find("Ada (Engineer)").unwrap() < digest.find("Zed (Engineer)").unwrap());
        assert!(digest.contains("claude/auto, working"));
        assert!(digest.contains("workspace"));
    }

    #[test]
    fn digest_lists_live_tasks_and_skips_the_boss_and_archived() {
        let boss = boss_state();
        let app = project("app");
        let lib = project("lib");
        let mut state = PersistedState::empty();
        state.projects.push(app.clone());
        state.projects.push(lib.clone());
        state
            .sessions
            .push(session(&app, "Fix login", SessionStatus::Working));
        state
            .sessions
            .push(session(&app, "Old task", SessionStatus::Idle));
        state.sessions.last_mut().unwrap().archived_at = Some(unix_time());
        state
            .sessions
            .push(session(&lib, "Ship it", SessionStatus::Waiting));
        // The boss's own chat is the reader, never a listed task.
        let mut own = session(&app, "Boss", SessionStatus::Working);
        own.id = boss.session_id.unwrap();
        state.sessions.push(own);

        let work = work_context(&state, &boss, &AutomationsState::default());
        assert!(work.digest.contains("## app — /work/app"));
        assert!(work.digest.contains("\"Fix login\" — working"));
        assert!(work.digest.contains("\"Ship it\" — waiting for input"));
        assert!(!work.digest.contains("Old task"));
        assert!(!work.digest.contains("\"Boss\""));
        assert_eq!(work.projects, ["app", "lib"]);
    }

    #[test]
    fn empty_catalog_means_no_header_digest_or_focus_options() {
        let boss = boss_state();
        let work = work_context(
            &PersistedState::empty(),
            &boss,
            &AutomationsState::default(),
        );
        assert!(work.header.is_empty());
        assert!(work.digest.is_empty());
        assert!(work.projects.is_empty());
    }

    #[test]
    fn header_counts_live_states_and_keeps_digest_detail_out() {
        let boss = boss_state();
        let app = project("app");
        let lib = project("lib");
        let mut state = PersistedState::empty();
        state.projects.push(app.clone());
        state.projects.push(lib.clone());
        state
            .sessions
            .push(session(&app, "Fix login", SessionStatus::Working));
        state
            .sessions
            .push(session(&app, "Awaiting review", SessionStatus::Waiting));
        state
            .sessions
            .push(session(&app, "Nap", SessionStatus::Idle));
        state
            .sessions
            .push(session(&lib, "Ship it", SessionStatus::Failed));
        // The boss's own chat and archived tasks never count.
        let mut own = session(&app, "Boss", SessionStatus::Working);
        own.id = boss.session_id.unwrap();
        state.sessions.push(own);
        let mut archived = session(&lib, "Old", SessionStatus::Working);
        archived.archived_at = Some(unix_time());
        state.sessions.push(archived);

        let work = work_context(&state, &boss, &AutomationsState::default());
        assert!(work.header.starts_with("Work overview"));
        assert!(work.header.contains("app: 3 tasks (1 active, 1 waiting)"));
        assert!(work.header.contains("lib: 1 task (1 failed)"));
        // Names and counts only — titles and paths stay in the digest.
        assert!(!work.header.contains("Fix login"));
        assert!(!work.header.contains("/work/app"));
    }

    #[test]
    fn header_counts_employees_per_project_and_fleet_automations() {
        let mut boss = boss_state();
        let app = project("app");
        let mut state = PersistedState::empty();
        state.projects.push(app.clone());
        let working = session(&app, "Job", SessionStatus::Working);
        let idle_task = session(&app, "Other job", SessionStatus::Idle);
        state.sessions.push(working.clone());
        state.sessions.push(idle_task.clone());
        let supervisor = boss.session_id.unwrap();
        let mut blocked = employee(idle_task.id, "Bea", false, supervisor);
        blocked.blocker = Some("needs a decision".into());
        boss.employees = vec![
            employee(working.id, "Ada", false, supervisor),
            blocked,
            // Expired employees and their counts drop out of the header.
            employee(Uuid::new_v4(), "Cas", true, supervisor),
            // A live employee whose task session is gone lands in the
            // fleet tally, not on a project line.
            employee(Uuid::new_v4(), "Dee", false, supervisor),
        ];
        let mut automations = AutomationsState::default();
        automations.automations.push(automation("nightly"));
        automations.automations.push(automation("standup"));

        let work = work_context(&state, &boss, &automations);
        assert!(
            work.header
                .contains("app: 2 tasks (1 active), 2 employees (1 blocked)")
        );
        assert!(work.header.contains("employees: 1 live"));
        assert!(work.header.contains("automations: 2 scheduled"));
        assert!(!work.header.contains("Bea"));
    }

    #[test]
    fn header_lists_every_project_when_the_digest_folds() {
        let boss = boss_state();
        let mut state = PersistedState::empty();
        // Sixteen busy projects overflow the digest cap but not the
        // header's — the tail folds only from the digest.
        for index in 0..16 {
            let busy = project(&format!("busy-{index:02}"));
            state.projects.push(busy.clone());
            for _ in 0..SESSIONS_PER_PROJECT {
                state.sessions.push(session(
                    &busy,
                    &"x".repeat(TITLE_CAP),
                    SessionStatus::Working,
                ));
            }
        }

        let work = work_context(&state, &boss, &AutomationsState::default());
        assert!(work.digest.contains("more projects omitted"));
        assert!(!work.digest.contains("## busy-15"));
        assert!(work.header.contains("busy-15"));
        assert!(work.header.len() <= HEADER_CAP + 32);
    }

    #[test]
    fn header_folds_past_its_own_budget() {
        let boss = boss_state();
        let mut state = PersistedState::empty();
        for index in 0..60 {
            state.projects.push(project(&format!("project-{index:02}")));
        }

        let work = work_context(&state, &boss, &AutomationsState::default());
        assert!(work.header.len() <= HEADER_CAP + 32);
        assert!(work.header.contains("project-00"));
        assert!(work.header.contains("more projects"));
        assert!(!work.header.contains("project-59:"));
    }

    #[test]
    fn verdict_attaches_at_threshold_and_updates_focus_on_margin() {
        let evaluation = Evaluation {
            model: "test".into(),
            answers: BTreeMap::from([
                ("attach".into(), EvalAnswer::Noul { noul: 0.7 }),
                (
                    "focus".into(),
                    EvalAnswer::Choice {
                        choice: "app".into(),
                        confidence: Some(0.8),
                        probabilities: BTreeMap::from([
                            ("app".into(), 0.7),
                            ("lib".into(), 0.2),
                            ("none".into(), 0.1),
                        ]),
                    },
                ),
            ]),
            usage: Default::default(),
            latency_ms: 1,
            provider_metadata: None,
        };
        let verdict = apply_verdict(&evaluation);
        assert!(verdict.attach);
        assert_eq!(verdict.focus, Some(Some("app".to_owned())));
    }

    #[test]
    fn verdict_keeps_defaults_on_weak_or_missing_answers() {
        let mut answers = BTreeMap::from([
            ("attach".into(), EvalAnswer::Noul { noul: 0.49 }),
            (
                "focus".into(),
                EvalAnswer::Choice {
                    choice: "app".into(),
                    confidence: Some(0.4),
                    probabilities: BTreeMap::from([("app".into(), 0.45), ("lib".into(), 0.4)]),
                },
            ),
        ]);
        let evaluation = |answers: BTreeMap<String, EvalAnswer>| Evaluation {
            model: "test".into(),
            answers,
            usage: Default::default(),
            latency_ms: 1,
            provider_metadata: None,
        };
        let verdict = apply_verdict(&evaluation(answers.clone()));
        assert!(!verdict.attach);
        assert_eq!(verdict.focus, None);

        // A decisive "none" clears the tracked focus.
        answers.insert(
            "focus".into(),
            EvalAnswer::Choice {
                choice: "none".into(),
                confidence: Some(0.9),
                probabilities: BTreeMap::from([("none".into(), 0.9), ("app".into(), 0.05)]),
            },
        );
        assert_eq!(apply_verdict(&evaluation(answers)).focus, Some(None));
    }
}
