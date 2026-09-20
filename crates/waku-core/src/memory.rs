//! Project memory: a per-project store the daemon maintains so new sessions
//! start with relevant prior context instead of zero.
//!
//! The store is two files under `<project>/.goddard/memory/` — `MEMORY.md`
//! (the curated, injectable summary) and `LOG.txt` (append-only raw notes).
//! The daemon is the only writer: a serialized background job distills
//! finished turns into new notes through a headless provider driver, with
//! eval triage bounding what the LLM has to read. Agents never write memory;
//! they read `LOG.txt` with the file tools they already have.
//!
//! Everything here runs on dedicated threads — eval calls and provider
//! drivers block on subprocesses and the network, so none of this may reach
//! a request or render path.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use waku_protocol::eval::EvalQuestion;
use waku_protocol::model::{DriverEvent, MessageRole, ProviderKind};

use crate::driver::{self, DriverStartOptions, event_channel};
use crate::eval::EvalDecisionRecord;
use crate::persistence::{PersistedState, StateStore};
use crate::settings::DaemonSettingsStore;

/// The store's home inside a project: `<project>/.goddard/memory/`.
const STORE_DIR: &str = ".goddard/memory";
const MEMORY_FILE: &str = "MEMORY.md";
const LOG_FILE: &str = "LOG.txt";
const STATE_FILE: &str = "state.json";

/// `MEMORY.md` is the injectable summary — bounded so injection stays cheap.
const MAX_MEMORY_LINES: usize = 60;
/// One log line, one durable fact — matching the discipline a one-line note
/// can carry.
const MAX_NOTE_CHARS: usize = 280;
/// How many log lines relevance ranking considers before scoring; beyond
/// that the oldest are never candidates.
const RANK_CANDIDATES: usize = 60;
/// How many ranked notes the injected block may carry.
const MAX_INJECTED_NOTES: usize = 15;
/// The recency fallback when eval is unavailable or finds nothing relevant.
const RECENT_NOTES_FALLBACK: usize = 20;
/// The minimum new transcript content that justifies a provider call.
const MIN_NEW_MESSAGES: usize = 4;
/// One segment is one rendered message, capped so a paste or a huge tool
/// dump cannot crowd out the rest of the batch.
const MAX_SEGMENT_CHARS: usize = 4_000;
/// Eval calls share a ~32k-token budget between state and questions; keep
/// batches well under it (≈4 chars/token plus headroom for instructions).
const EVAL_BATCH_CHARS: usize = 90_000;
/// Fallback distillation input when eval is not configured: the tail of the
/// new transcript, same order of magnitude as one eval batch.
const FALLBACK_TRANSCRIPT_CHARS: usize = 60_000;
/// The headless provider run gets a hard ceiling; a stuck distill must never
/// pin the per-project worker.
const DISTILL_TIMEOUT: Duration = Duration::from_secs(300);

/// What the distiller sees per session: messages already considered stay
/// below `position`; only the tail is new.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct MemoryState {
    sessions: HashMap<Uuid, SessionWatermark>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct SessionWatermark {
    position: usize,
    /// The store was injected into this session's first prompt; later
    /// prompts pass through untouched.
    injected: bool,
}

/// Owns scheduling and execution of project-memory work. Cloned cheaply into
/// the runtime event forwarder; the real state lives behind `Arc`.
pub struct MemoryService {
    settings: Arc<DaemonSettingsStore>,
    task_state: Arc<Mutex<PersistedState>>,
    task_store: Arc<StateStore>,
    /// Projects with a worker running — one distillation at a time per
    /// project, since two writers on `LOG.txt` would interleave.
    in_flight: Mutex<HashSet<Uuid>>,
    /// Projects that accrued a finished turn while their worker ran; the
    /// worker loops until a pass observes no pending flag.
    pending: Mutex<HashSet<Uuid>>,
}

/// Everything a worker needs that is not behind the service's own handles.
struct SessionSlice {
    provider: ProviderKind,
    model: Option<String>,
    /// New rendered transcript segments for this session.
    segments: Vec<String>,
    /// Message count the watermark should advance to after a successful pass.
    new_position: usize,
}

impl MemoryService {
    pub fn new(
        settings: Arc<DaemonSettingsStore>,
        task_state: Arc<Mutex<PersistedState>>,
        task_store: Arc<StateStore>,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            task_state,
            task_store,
            in_flight: Mutex::new(HashSet::new()),
            pending: Mutex::new(HashSet::new()),
        })
    }

    /// A turn finished (or the runtime exited) inside `session_id`. Marks the
    /// owning project for distillation; the actual run is serialized per
    /// project on a worker thread.
    pub fn note_session_activity(self: &Arc<Self>, session_id: Uuid) {
        if !self.settings.get().memory_experiment_enabled {
            return;
        }
        let Some(project_id) = self.session_project(session_id) else {
            return;
        };
        {
            let mut in_flight = self.in_flight.lock();
            if !in_flight.insert(project_id) {
                self.pending.lock().insert(project_id);
                return;
            }
        }
        let service = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name(format!("goddard-memory-{project_id}"))
            .spawn(move || service.run_project(project_id));
        if spawned.is_err() {
            self.in_flight.lock().remove(&project_id);
        }
    }

    /// Prepend the project's memory block to a session's first prompt.
    /// Returns the prompt untouched when the experiment is off, the project
    /// has no store yet, or this session was already injected. Reads are
    /// small file loads plus at most one eval call — callers run on daemon
    /// threads, never the UI thread.
    pub fn prompt_with_memory(&self, session_id: Uuid, prompt: &str) -> String {
        let Some(block) = self.memory_block(session_id, prompt) else {
            return prompt.to_owned();
        };
        format!("{block}\n\n{prompt}")
    }

    /// Compose the injection block, or `None` when nothing should ship.
    /// A successful composition marks the session injected so the block
    /// lands exactly once.
    fn memory_block(&self, session_id: Uuid, task: &str) -> Option<String> {
        let settings = self.settings.get();
        if !settings.memory_experiment_enabled {
            return None;
        }
        let project_path = self.project_path(self.session_project(session_id)?)?;
        let store = memory_dir(&project_path);
        if !store.join(MEMORY_FILE).exists() && !store.join(LOG_FILE).exists() {
            return None;
        }
        let mut memory_state = load_state(&store);
        if memory_state
            .sessions
            .get(&session_id)
            .is_some_and(|entry| entry.injected)
        {
            return None;
        }

        let memory_md = read_memory(&store);
        let notes = rank_notes(
            settings.eval.as_ref(),
            task,
            &read_log_lines(&store),
        );
        let block = compose_block(&memory_md, &notes, &store.join(LOG_FILE))?;
        memory_state
            .sessions
            .entry(session_id)
            .or_default()
            .injected = true;
        let _ = save_state(&store, &memory_state);
        Some(block)
    }

    fn session_project(&self, session_id: Uuid) -> Option<Uuid> {
        let state = self.task_state.lock();
        state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.project_id)
    }

    fn project_path(&self, project_id: Uuid) -> Option<PathBuf> {
        let state = self.task_state.lock();
        state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
    }

    /// Worker loop: distill once, then re-run while turns landed during the
    /// pass. A failed pass leaves watermarks alone so the next loop retries
    /// the same transcript.
    fn run_project(self: &Arc<Self>, project_id: Uuid) {
        loop {
            if let Err(error) = self.distill_project(project_id) {
                eprintln!("goddard memory distillation failed for project {project_id}: {error:#}");
            }
            let rerun = {
                let mut pending = self.pending.lock();
                pending.remove(&project_id)
            };
            if !rerun {
                break;
            }
        }
        self.in_flight.lock().remove(&project_id);
    }

    /// One distillation pass over every session the project owns.
    fn distill_project(&self, project_id: Uuid) -> anyhow::Result<()> {
        let settings = self.settings.get();
        let project_path = {
            let state = self.task_state.lock();
            state
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        }
        .context("project is unknown to the daemon")?;
        let store = memory_dir(&project_path);
        let mut memory_state = load_state(&store);

        // Collect each session's un-distilled transcript tail. Hydrating here
        // is deliberate: an idle session's messages are released from memory
        // after the recency window, and the distiller needs the real rows.
        let mut slices: Vec<(Uuid, SessionSlice)> = Vec::new();
        {
            let mut state = self.task_state.lock();
            for index in 0..state.sessions.len() {
                if state.sessions[index].project_id != project_id {
                    continue;
                }
                self.task_store
                    .hydrate(&mut state.sessions[index])
                    .context("could not load a task's stored messages")?;
                let session = &state.sessions[index];
                let watermark = memory_state
                    .sessions
                    .get(&session.id)
                    .map(|entry| entry.position)
                    .unwrap_or(0)
                    .min(session.messages.len());
                let segments = render_segments(&session.messages[watermark..]);
                slices.push((
                    session.id,
                    SessionSlice {
                        provider: session.provider,
                        model: session.model.clone(),
                        segments,
                        new_position: session.messages.len(),
                    },
                ));
            }
        }

        let new_message_count: usize = slices.iter().map(|(_, slice)| slice.segments.len()).sum();
        if new_message_count < MIN_NEW_MESSAGES {
            return Ok(());
        }
        ensure_store(&project_path)?;

        // The write driver's provider and model come from whichever session
        // contributed the most new transcript — the user's own choice, so
        // distillation inherits whatever they already pay for.
        let (_, source) = slices
            .iter()
            .max_by_key(|(_, slice)| slice.segments.len())
            .expect("new_message_count is nonzero");
        let segments: Vec<String> = slices
            .iter()
            .flat_map(|(_, slice)| slice.segments.iter().cloned())
            .collect();
        let kept = triage_segments(settings.eval.as_ref(), &segments);
        let excerpts = kept
            .iter()
            .map(|index| segments[*index].as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let prompt = distill_prompt(
            &read_memory(&store),
            &log_tail(&store, 30),
            &excerpts,
        );

        let binary = provider_binary(&settings, source.provider)?;
        let output = headless_prompt(
            source.provider,
            binary,
            project_path.clone(),
            source.model.clone(),
            prompt,
        )?;
        let (notes, memory_md) = parse_distill_output(&output)?;

        append_notes(&store, &notes)?;
        write_memory(&store, &memory_md)?;

        for (session_id, slice) in &slices {
            memory_state
                .sessions
                .entry(*session_id)
                .or_default()
                .position = slice.new_position;
        }
        save_state(&store, &memory_state)?;
        Ok(())
    }
}

/// `<project>/.goddard/memory/`.
pub(crate) fn memory_dir(project_path: &Path) -> PathBuf {
    project_path.join(STORE_DIR)
}

/// One rendered transcript message: a role tag plus content, truncated.
fn render_segments(messages: &[waku_protocol::model::Message]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| !message.hidden && !message.content.trim().is_empty())
        .map(|message| {
            let role = match message.role {
                MessageRole::User => "USER",
                MessageRole::Assistant => "ASSISTANT",
                MessageRole::System => "SYSTEM",
            };
            let content: String = message.content.chars().take(MAX_SEGMENT_CHARS).collect();
            format!("{role}: {content}")
        })
        .collect()
}

/// Score each segment for memory-worthiness through the eval backend and
/// return the indices worth showing the LLM. Unconfigured or failed eval
/// falls back to the transcript tail — the distiller still runs, just on
/// less-focused input.
fn triage_segments(
    eval: Option<&waku_protocol::eval::EvalSettings>,
    segments: &[String],
) -> Vec<usize> {
    let Some(settings) = eval else {
        return tail_fallback(segments);
    };
    let log_path = crate::eval::default_log_path();
    let mut kept = Vec::new();
    let mut batch_start = 0;
    while batch_start < segments.len() {
        // Pack segments into one call until the state budget is nearly full.
        let mut batch_end = batch_start;
        let mut batch_chars = 0usize;
        while batch_end < segments.len() {
            let next = segments[batch_end].len();
            if batch_chars + next > EVAL_BATCH_CHARS && batch_end > batch_start {
                break;
            }
            batch_chars += next;
            batch_end += 1;
        }
        let batch: Vec<String> = segments[batch_start..batch_end].to_vec();
        let state = json!({ "segments": batch });
        let questions: BTreeMap<String, EvalQuestion> = (batch_start..batch_end)
            .map(|index| {
                (
                    format!("seg_{index}"),
                    EvalQuestion::Noul {
                        instructions: format!(
                            "Does `segments[{}]` contain a durable fact, decision, user \
                             preference, failed approach, or environment detail worth \
                             remembering across later coding sessions? Answer yes only \
                             for content that would still matter next session — not \
                             status updates or narration.",
                            index - batch_start
                        ),
                        criteria: None,
                    },
                )
            })
            .collect();
        let mut record = EvalDecisionRecord::empty("memory-triage");
        match crate::eval::evaluate(settings, &state, &questions) {
            Ok(evaluation) => {
                record.model = Some(evaluation.model.clone());
                record.latency_ms = Some(evaluation.latency_ms);
                record.answers = Some(evaluation.answers.clone());
                for index in batch_start..batch_end {
                    let keep = evaluation
                        .answers
                        .get(&format!("seg_{index}"))
                        .is_some_and(|answer| match answer {
                            waku_protocol::eval::EvalAnswer::Noul { noul } => *noul >= 0.5,
                            _ => false,
                        });
                    if keep {
                        kept.push(index);
                    }
                }
            }
            Err(error) => {
                record.error = Some(format!("{error:#}"));
                crate::eval::append_decision_log(&log_path, &record);
                return tail_fallback(segments);
            }
        }
        crate::eval::append_decision_log(&log_path, &record);
        batch_start = batch_end;
    }
    if kept.is_empty() {
        tail_fallback(segments)
    } else {
        kept
    }
}

/// No eval backend (or a failed call): the tail of the new transcript, which
/// is where the durable content usually lands anyway.
fn tail_fallback(segments: &[String]) -> Vec<usize> {
    let mut kept = Vec::new();
    let mut chars = 0usize;
    for (index, segment) in segments.iter().enumerate().rev() {
        if chars + segment.len() > FALLBACK_TRANSCRIPT_CHARS && !kept.is_empty() {
            break;
        }
        chars += segment.len();
        kept.push(index);
    }
    kept.sort_unstable();
    kept
}

/// The one-shot prompt the headless driver answers. The output format is
/// deliberately rigid so [`parse_distill_output`] never has to guess.
fn distill_prompt(memory_md: &str, recent: &[String], excerpts: &str) -> String {
    let recent_text = if recent.is_empty() {
        "(none)".to_owned()
    } else {
        recent.join("\n")
    };
    format!(
        "You maintain the persistent memory of a software project. It is stored as\n\
         - MEMORY.md: at most {MAX_MEMORY_LINES} lines of durable facts — decisions, \
         user preferences, failed approaches, environment quirks.\n\
         - LOG.txt: an append-only log of one-line notes.\n\n\
         Current MEMORY.md:\n{memory_md}\n\n\
         Recent log lines:\n{recent_text}\n\n\
         New transcript excerpts:\n{excerpts}\n\n\
         Write 0-5 new log lines and the refreshed MEMORY.md.\n\
         - One durable fact per log line, at most {MAX_NOTE_CHARS} characters each.\n\
         - Record decisions, user preferences, failed approaches, and non-obvious facts. \
         Skip status updates, file-by-file narration, and anything already in MEMORY.md.\n\
         - Never record secrets, tokens, passwords, or credentials.\n\
         - Preserve human edits already present in MEMORY.md; keep it under \
         {MAX_MEMORY_LINES} lines.\n\n\
         Reply in exactly this format and nothing else:\n\
         NOTES\n\
         <new log lines, one per line, or NONE>\n\
         MEMORY\n\
         <the complete new MEMORY.md content>",
        memory_md = if memory_md.trim().is_empty() {
            "(empty)"
        } else {
            memory_md
        }
    )
}

/// Split the distiller's rigid reply into (new log lines, new MEMORY.md).
/// Both sections are required; a malformed answer fails the whole pass so a
/// confused model cannot clobber the store.
fn parse_distill_output(output: &str) -> anyhow::Result<(Vec<String>, String)> {
    let notes_start = output
        .find("NOTES")
        .context("distiller output carried no NOTES section")?;
    let after_notes = &output[notes_start + "NOTES".len()..];
    let memory_start = after_notes
        .find("MEMORY")
        .context("distiller output carried no MEMORY section")?;
    let notes_text = after_notes[..memory_start].trim();
    let memory_text = after_notes[memory_start + "MEMORY".len()..].trim();

    let notes = if notes_text.eq_ignore_ascii_case("none") || notes_text.is_empty() {
        Vec::new()
    } else {
        notes_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| line.trim_start_matches("- ").to_owned())
            .collect()
    };
    if memory_text.is_empty() {
        bail!("distiller returned an empty MEMORY section");
    }
    Ok((notes, memory_text.to_owned()))
}

/// Drop or normalize a candidate note. Returns `None` for content that must
/// never reach the store — credentials above all.
fn scrub_note(line: &str) -> Option<String> {
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let lower = collapsed.to_lowercase();
    const SENSITIVE: &[&str] = &[
        "-----begin",
        "sk-",
        "ghp_",
        "gho_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "akia",
        "api_key",
        "api-key",
        "apikey",
        "secret_key",
        "password",
        "passwd",
        "bearer ",
        "private_key",
        "access_token",
    ];
    if SENSITIVE.iter().any(|marker| lower.contains(marker)) {
        return None;
    }
    Some(collapsed.chars().take(MAX_NOTE_CHARS).collect())
}

/// Create the store directory and exclude `.goddard/memory/` from the
/// project's own git, so memory never appears in diffs or commits. Only the
/// store is excluded — `.goddard/commands` and friends stay committable.
/// Non-git projects simply get the directory.
fn ensure_store(project_path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(memory_dir(project_path))?;
    exclude_from_git(project_path);
    Ok(())
}

/// Append `.goddard/memory/` to the repository's `info/exclude` — the local
/// exclude file that never touches the tracked `.gitignore`. Resolves
/// `gitdir:` pointers and then `commondir` so a linked worktree lands the
/// entry in the shared git directory; git never reads a worktree's own
/// `info/exclude`.
fn exclude_from_git(project_path: &Path) {
    let _ = (|| -> anyhow::Result<()> {
        let dotgit = project_path.join(".git");
        let git_dir = if dotgit.is_dir() {
            dotgit
        } else if dotgit.is_file() {
            let contents = std::fs::read_to_string(&dotgit)?;
            let target = contents
                .trim()
                .strip_prefix("gitdir:")
                .map(str::trim)
                .context("unrecognized .git file")?;
            let target = PathBuf::from(target);
            if target.is_absolute() {
                target
            } else {
                project_path.join(target)
            }
        } else {
            return Ok(());
        };
        let git_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
            Ok(common) => {
                let common = PathBuf::from(common.trim());
                if common.is_absolute() {
                    common
                } else {
                    git_dir.join(common)
                }
            }
            Err(_) => git_dir,
        };
        let info = git_dir.join("info");
        std::fs::create_dir_all(&info)?;
        let exclude = info.join("exclude");
        let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
        if existing.lines().any(|line| {
            let line = line.trim().trim_end_matches('/');
            line == ".goddard" || line == ".goddard/memory"
        }) {
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(exclude)?;
        if !existing.is_empty() && !existing.ends_with('\n') {
            writeln!(file)?;
        }
        writeln!(file, ".goddard/memory/")?;
        Ok(())
    })();
}

fn read_memory(store: &Path) -> String {
    std::fs::read_to_string(store.join(MEMORY_FILE)).unwrap_or_default()
}

fn log_tail(store: &Path, count: usize) -> Vec<String> {
    std::fs::read_to_string(store.join(LOG_FILE))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn read_log_lines(store: &Path) -> Vec<String> {
    std::fs::read_to_string(store.join(LOG_FILE))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Pick the log lines worth injecting ahead of `task`. With eval configured,
/// each of the last `RANK_CANDIDATES` lines gets a Noul score against the
/// task; without it (or on failure), the newest lines win — recency is the
/// honest default when relevance is unknown.
fn rank_notes(
    eval: Option<&waku_protocol::eval::EvalSettings>,
    task: &str,
    lines: &[String],
) -> Vec<String> {
    let candidates: Vec<String> = lines
        .iter()
        .rev()
        .take(RANK_CANDIDATES)
        .rev()
        .cloned()
        .collect();
    let Some(settings) = eval else {
        return recent_notes(&candidates);
    };

    let state = json!({ "task": task, "notes": candidates });
    let questions: BTreeMap<String, EvalQuestion> = candidates
        .iter()
        .enumerate()
        .map(|(index, _)| {
            (
                format!("note_{index}"),
                EvalQuestion::Noul {
                    instructions: format!(
                        "Is `notes[{index}]` relevant to the task in `task` — could it \
                         change what the agent should do or avoid? Answer no for generic \
                         facts unrelated to the request."
                    ),
                    criteria: None,
                },
            )
        })
        .collect();
    let mut record = EvalDecisionRecord::empty("memory-rank");
    match crate::eval::evaluate(settings, &state, &questions) {
        Ok(evaluation) => {
            record.model = Some(evaluation.model.clone());
            record.latency_ms = Some(evaluation.latency_ms);
            record.answers = Some(evaluation.answers.clone());
            crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
            let ranked: Vec<String> = candidates
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    evaluation
                        .answers
                        .get(&format!("note_{index}"))
                        .is_some_and(|answer| match answer {
                            waku_protocol::eval::EvalAnswer::Noul { noul } => *noul >= 0.5,
                            _ => false,
                        })
                })
                .take(MAX_INJECTED_NOTES)
                .map(|(_, line)| line.clone())
                .collect();
            if ranked.is_empty() {
                recent_notes(&candidates)
            } else {
                ranked
            }
        }
        Err(error) => {
            record.error = Some(format!("{error:#}"));
            crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
            recent_notes(&candidates)
        }
    }
}

fn recent_notes(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .rev()
        .take(RECENT_NOTES_FALLBACK)
        .rev()
        .cloned()
        .collect()
}

/// The hidden block prepended to a session's first prompt. `None` when both
/// sections would be empty — an empty store should not inject a shell.
fn compose_block(memory_md: &str, notes: &[String], log_path: &Path) -> Option<String> {
    let mut block = String::from(
        "<project-memory>\n\
         This project has persistent memory distilled from earlier sessions.\n",
    );
    if !memory_md.trim().is_empty() {
        block.push_str(&format!("\n## Memory\n{}\n", memory_md.trim_end()));
    }
    if !notes.is_empty() {
        block.push_str(&format!("\n## Notes\n{}\n", notes.join("\n")));
    }
    if memory_md.trim().is_empty() && notes.is_empty() {
        return None;
    }
    block.push_str(&format!(
        "\nFull history lives in {} — grep it when unsure. These notes are \
         context, not ground truth; verify anything that looks stale.\n\
         </project-memory>",
        log_path.display()
    ));
    Some(block)
}

/// Append scrubbed, dated notes to `LOG.txt`. Secret-looking candidates are
/// dropped here as the last line of defense.
fn append_notes(store: &Path, notes: &[String]) -> anyhow::Result<()> {
    let today = chrono::Utc::now().format("%Y-%m-%d");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.join(LOG_FILE))?;
    for note in notes {
        if let Some(clean) = scrub_note(note) {
            writeln!(file, "{today} {clean}")?;
        }
    }
    Ok(())
}

/// Replace `MEMORY.md` atomically; the file is read on every session start,
/// so a torn write must never be observable.
fn write_memory(store: &Path, content: &str) -> anyhow::Result<()> {
    let capped: String = content
        .lines()
        .take(MAX_MEMORY_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let target = store.join(MEMORY_FILE);
    let temporary = store.join(".MEMORY.md.tmp");
    std::fs::write(&temporary, format!("{}\n", capped.trim_end()))?;
    std::fs::rename(temporary, target)?;
    Ok(())
}

fn load_state(store: &Path) -> MemoryState {
    std::fs::read(&store.join(STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_state(store: &Path, state: &MemoryState) -> anyhow::Result<()> {
    let temporary = store.join(".state.json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(temporary, store.join(STATE_FILE))?;
    Ok(())
}

/// Resolve a provider binary the way the daemon does, including user
/// overrides, without holding the backend handle.
fn provider_binary(
    settings: &waku_protocol::settings::DaemonSettings,
    provider: ProviderKind,
) -> anyhow::Result<PathBuf> {
    crate::command_env::refresh_from_default_shell();
    let binary_override = settings
        .provider_binary_overrides
        .get(&provider)
        .map(String::as_str);
    crate::model::provider_probe(provider, binary_override)
        .path
        .ok_or_else(|| anyhow::anyhow!("{} is not installed", provider.display_name()))
}

/// Run one prompt through a provider driver with no session attached and
/// collect its text output. `Ask` mode keeps the run read-only — the prompt
/// tells the model not to touch tools, and any permission request simply
/// stalls until the deadline drops the driver.
fn headless_prompt(
    provider: ProviderKind,
    binary: PathBuf,
    cwd: PathBuf,
    model: Option<String>,
    prompt: String,
) -> anyhow::Result<String> {
    let (wake, _wakes) = smol::channel::unbounded();
    let (sender, receiver) = event_channel(wake);
    let options = DriverStartOptions {
        binary,
        cwd,
        mode: waku_protocol::model::RuntimeMode::Ask,
        model,
        reasoning_effort: None,
        service_tier: None,
        context_window: None,
        agent_preset: None,
        computer_use_enabled: false,
        agent: None,
        subagents: None,
        integrations: Vec::new(),
        provider_cursor: None,
        eval: None,
    };
    let handle = driver::start_local(provider, options, sender)
        .context("could not start the memory distillation driver")?;
    handle.prompt(prompt);

    let deadline = Instant::now() + DISTILL_TIMEOUT;
    let mut text = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            handle.cancel();
            bail!("memory distillation exceeded {}s", DISTILL_TIMEOUT.as_secs());
        }
        match receiver.recv_timeout(remaining.min(Duration::from_secs(30))) {
            Ok(DriverEvent::TextDelta(delta)) => text.push_str(&delta),
            Ok(DriverEvent::TurnFinished { .. }) | Ok(DriverEvent::ProcessExited) => break,
            Ok(DriverEvent::Error(error)) | Ok(DriverEvent::LocalizedError { message: error, .. }) => {
                eprintln!("memory distillation driver reported: {error}");
            }
            Ok(_) => {}
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    if text.trim().is_empty() {
        bail!("memory distillation produced no output");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_notes_from_memory() {
        let output = "Some preamble\nNOTES\nfirst fact\n- second fact\nMEMORY\n# Project memory\n- kept fact\n";
        let (notes, memory) = parse_distill_output(output).unwrap();
        assert_eq!(notes, vec!["first fact", "second fact"]);
        assert_eq!(memory, "# Project memory\n- kept fact");
    }

    #[test]
    fn parse_accepts_none_for_no_new_notes() {
        let (notes, memory) = parse_distill_output("NOTES\nNONE\nMEMORY\nunchanged").unwrap();
        assert!(notes.is_empty());
        assert_eq!(memory, "unchanged");
    }

    #[test]
    fn parse_rejects_missing_sections() {
        assert!(parse_distill_output("just some prose").is_err());
        assert!(parse_distill_output("NOTES\na\nMEMORY\n").is_err());
    }

    #[test]
    fn scrub_drops_credentials_and_collapses_whitespace() {
        assert_eq!(scrub_note("  user   prefers  tabs "), Some("user prefers tabs".into()));
        assert!(scrub_note("the api_key is stored in ~/.config").is_none());
        assert!(scrub_note("token sk-abc123 lives in env").is_none());
        assert!(scrub_note("uses bearer token auth").is_none());
        assert_eq!(scrub_note("decided against JWT sessions"), Some("decided against JWT sessions".into()));
    }

    #[test]
    fn scrub_caps_long_notes() {
        let long = "x".repeat(400);
        assert_eq!(scrub_note(&long).unwrap().chars().count(), MAX_NOTE_CHARS);
    }

    #[test]
    fn tail_fallback_prefers_recent_segments() {
        // Ten ~10k segments exceed the 60k fallback budget, so the oldest
        // are dropped and the result stays in ascending order.
        let segments: Vec<String> = (0..10).map(|i| format!("segment {i} {}", "x".repeat(10_000))).collect();
        let kept = tail_fallback(&segments);
        assert_eq!(kept, vec![5, 6, 7, 8, 9]);
    }

    #[test]
    fn write_memory_caps_and_restores() {
        let store = std::env::temp_dir().join(format!("waku-memory-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&store).unwrap();
        let content: String = (0..80).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        write_memory(&store, &content).unwrap();
        let restored = read_memory(&store);
        assert_eq!(restored.lines().count(), MAX_MEMORY_LINES);
        assert!(restored.starts_with("line 0"));
        std::fs::remove_dir_all(&store).ok();
    }

    #[test]
    fn notes_append_scrubbed_and_dated() {
        let store = std::env::temp_dir().join(format!("waku-memory-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&store).unwrap();
        append_notes(&store, &[
            "uses bun for scripts".to_owned(),
            "the password is hunter2".to_owned(),
        ]).unwrap();
        let lines = log_tail(&store, 10);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("uses bun for scripts"));
        std::fs::remove_dir_all(&store).ok();
    }

    #[test]
    fn exclude_lands_in_git_info() {
        let root = std::env::temp_dir().join(format!("waku-memory-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        exclude_from_git(&root);
        let exclude = std::fs::read_to_string(root.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|line| line == ".goddard/memory/"));
        // Idempotent: a second pass does not duplicate the line.
        exclude_from_git(&root);
        let exclude = std::fs::read_to_string(root.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude.matches(".goddard/memory/").count(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn exclude_resolves_gitdir_pointers() {
        let root = std::env::temp_dir().join(format!("waku-memory-{}", Uuid::new_v4()));
        let gitdir = root.join("real-git");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(root.join(".git"), format!("gitdir: {}", gitdir.display())).unwrap();
        exclude_from_git(&root);
        assert!(gitdir.join("info/exclude").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn exclude_resolves_worktree_commondir() {
        let root = std::env::temp_dir().join(format!("waku-memory-{}", Uuid::new_v4()));
        let common = root.join("main/.git");
        let worktree_git = common.join("worktrees/linked");
        std::fs::create_dir_all(&worktree_git).unwrap();
        std::fs::write(worktree_git.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}", worktree_git.display()),
        )
        .unwrap();
        exclude_from_git(&root);
        let exclude = std::fs::read_to_string(common.join("info/exclude")).unwrap();
        assert!(exclude.lines().any(|line| line == ".goddard/memory/"));
        assert!(!worktree_git.join("info/exclude").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rank_falls_back_to_recent_without_eval() {
        let lines: Vec<String> = (0..40).map(|i| format!("note {i}")).collect();
        let ranked = rank_notes(None, "fix the login bug", &lines);
        assert_eq!(ranked.len(), RECENT_NOTES_FALLBACK);
        assert_eq!(ranked.first().unwrap(), "note 20");
        assert_eq!(ranked.last().unwrap(), "note 39");
    }

    #[test]
    fn compose_block_omits_empty_sections() {
        let log = Path::new("/tmp/proj/.goddard/memory/LOG.txt");
        let block = compose_block("# Facts\n- uses bun", &["2026-01-01 decided x".into()], log)
            .unwrap();
        assert!(block.contains("## Memory\n# Facts\n- uses bun"));
        assert!(block.contains("## Notes\n2026-01-01 decided x"));
        assert!(block.contains(log.to_str().unwrap()));
        let notes_only = compose_block("", &["2026-01-01 y".into()], log).unwrap();
        assert!(!notes_only.contains("## Memory"));
        assert!(notes_only.contains("## Notes"));
        assert!(compose_block("  ", &[], log).is_none());
    }

    #[test]
    fn state_round_trips_watermarks() {
        let store = std::env::temp_dir().join(format!("waku-memory-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&store).unwrap();
        let mut state = MemoryState::default();
        let session = Uuid::new_v4();
        state.sessions.entry(session).or_default().position = 42;
        save_state(&store, &state).unwrap();
        let restored = load_state(&store);
        assert_eq!(restored.sessions[&session].position, 42);
        std::fs::remove_dir_all(&store).ok();
    }
}
