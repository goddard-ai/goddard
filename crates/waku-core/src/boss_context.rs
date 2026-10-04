//! The boss context router: a bounded digest of the user's projects, tasks,
//! and automations the boss can read on demand, plus the Jev evaluation that
//! decides per user prompt whether that digest should ride along.
//!
//! Jev judges two things on one shared `state`: whether the digest is worth
//! attaching, and which project the user's attention currently centers on —
//! the focus inference, which this module keeps stateful by feeding the
//! previous answer back into the next request's `state`. Everything else is
//! deterministic: thresholds below decide what an answer may apply, and any
//! missing, malformed, or failed evaluation leaves the defaults standing —
//! no attachment, no focus change.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use waku_protocol::automations::{AutomationSchedule, AutomationsState};
use waku_protocol::boss::BossState;
use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{AgentSession, SessionStatus, truncate_chars, unix_time};

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

/// The work snapshot: the rendered digest plus the project names that double
/// as the router's focus options.
pub struct WorkContext {
    pub digest: String,
    /// User-project names in digest order — the `focus` Choice's option set.
    pub projects: Vec<String>,
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
    let mut digest = String::new();
    let mut names = Vec::new();
    let mut omitted_projects = 0usize;
    for project in &projects {
        let mut sessions: Vec<&AgentSession> = state
            .sessions
            .iter()
            .filter(|session| session.project_id == project.id)
            .filter(live)
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
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
        digest,
        projects: names,
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
    use waku_protocol::boss::BossIdentity;
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
            bundles: Vec::new(),
            planning: Vec::new(),
            goals_viewed_at: None,
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
    fn empty_catalog_means_no_digest_and_no_focus_options() {
        let boss = boss_state();
        let work = work_context(
            &PersistedState::empty(),
            &boss,
            &AutomationsState::default(),
        );
        assert!(work.digest.is_empty());
        assert!(work.projects.is_empty());
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
