//! Jev-triggered follow-ups. A completed turn becomes eligible in the runtime
//! forwarder, then its first settled client snapshot supplies the transcript
//! for evaluation. Claims are persisted before evaluation, so replayed saves
//! and daemon restarts cannot send the same follow-up twice.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::auto_prompts::{AutoPromptRule, turn_state};
use waku_protocol::eval::{EvalAnswer, EvalQuestion};
use waku_protocol::model::{AgentSession, MessageRole, TurnStatus};

use crate::daemon::WakuBackend;
use crate::server::EventSink;

const AUTO_PROMPT_PREFIX: &str = "[Auto prompt: ";

#[derive(Clone, Copy, Deserialize, Serialize)]
struct AutoPromptHistoryEntry {
    session_id: Uuid,
    selected_rule: Option<Uuid>,
}

pub struct AutoPromptService {
    path: PathBuf,
    handled: Mutex<HashMap<Uuid, AutoPromptHistoryEntry>>,
    eligible: Mutex<HashSet<Uuid>>,
    nonhuman_turns: Mutex<HashMap<Uuid, Uuid>>,
    backend: Mutex<Weak<WakuBackend>>,
    events: Mutex<Option<EventSink>>,
}

impl AutoPromptService {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let handled = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(handled) => handled,
                Err(error) => {
                    let backup = path.with_extension(format!("json.corrupt-{}", Uuid::new_v4()));
                    fs::rename(&path, &backup)?;
                    eprintln!(
                        "moved invalid auto prompt history to {}: {error}",
                        backup.display()
                    );
                    HashMap::new()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            handled: Mutex::new(handled),
            eligible: Mutex::new(HashSet::new()),
            nonhuman_turns: Mutex::new(HashMap::new()),
            backend: Mutex::new(Weak::new()),
            events: Mutex::new(None),
        })
    }

    pub fn start(&self, backend: &Arc<WakuBackend>) {
        *self.backend.lock() = Arc::downgrade(backend);
    }

    pub fn set_event_source(&self, events: EventSink) {
        *self.events.lock() = Some(events);
    }

    pub fn note_turn_finished(&self, session_id: Uuid, success: bool) {
        if success {
            self.eligible.lock().insert(session_id);
        } else {
            self.eligible.lock().remove(&session_id);
            self.nonhuman_turns.lock().remove(&session_id);
        }
    }

    pub fn note_nonhuman_turn(&self, session_id: Uuid, turn_id: Uuid) {
        self.nonhuman_turns.lock().insert(session_id, turn_id);
    }

    pub fn consider(self: &Arc<Self>, sessions: &[AgentSession]) {
        for session in sessions {
            if !self.eligible.lock().contains(&session.id) {
                continue;
            }
            let Some(turn) = session
                .turns
                .last()
                .filter(|turn| turn.status == TurnStatus::Completed)
            else {
                continue;
            };
            let turn_id = turn.id;
            let nonhuman = {
                let mut turns = self.nonhuman_turns.lock();
                if turns.get(&session.id) == Some(&turn_id) {
                    turns.remove(&session.id);
                    true
                } else {
                    false
                }
            };
            if nonhuman {
                self.eligible.lock().remove(&session.id);
                continue;
            }
            let Some(prompt) = session.messages.iter().find(|message| {
                message.turn_id == Some(turn.id) && message.role == MessageRole::User
            }) else {
                continue;
            };
            // Daemon-origin follow-ups are visibly labeled; their answer can
            // never recursively trigger another rule.
            if prompt.content.starts_with(AUTO_PROMPT_PREFIX) || prompt.sent_by_task.is_some() {
                self.eligible.lock().remove(&session.id);
                continue;
            }
            let Some(backend) = self.backend.lock().upgrade() else {
                continue;
            };
            let settings = backend.auto_prompt_settings();
            let rules: Vec<_> = settings
                .auto_prompts
                .into_iter()
                .filter(AutoPromptRule::valid_for_dispatch)
                .collect();
            self.eligible.lock().remove(&session.id);
            if rules.is_empty() || settings.eval.is_none_or(|eval| eval.credential_missing()) {
                continue;
            }
            let session = session.clone();
            let service = self.clone();
            let _ = std::thread::Builder::new()
                .name(format!("goddard-auto-prompt-{}", session.id))
                .spawn(move || {
                    if let Err(error) = service.claim(session.id, turn_id) {
                        eprintln!("could not claim auto prompt turn {turn_id}: {error:#}");
                        return;
                    }
                    service.evaluate_and_dispatch(session, turn_id, rules);
                });
        }
    }

    fn claim(&self, session_id: Uuid, turn_id: Uuid) -> anyhow::Result<()> {
        let mut handled = self.handled.lock();
        if handled.contains_key(&turn_id) {
            anyhow::bail!("turn already handled");
        }
        handled.insert(
            turn_id,
            AutoPromptHistoryEntry {
                session_id,
                selected_rule: None,
            },
        );
        self.write_history(&handled)
    }

    fn record_selection(
        &self,
        session_id: Uuid,
        turn_id: Uuid,
        rule_id: Uuid,
    ) -> anyhow::Result<()> {
        let mut handled = self.handled.lock();
        let Some(entry) = handled
            .get_mut(&turn_id)
            .filter(|entry| entry.session_id == session_id)
        else {
            anyhow::bail!("auto prompt turn was superseded");
        };
        entry.selected_rule = Some(rule_id);
        self.write_history(&handled)
    }

    fn write_history(&self, handled: &HashMap<Uuid, AutoPromptHistoryEntry>) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec(handled)?)?;
        fs::rename(temporary, &self.path)?;
        Ok(())
    }

    fn evaluate_and_dispatch(
        &self,
        session: AgentSession,
        turn_id: Uuid,
        rules: Vec<AutoPromptRule>,
    ) {
        let Some(backend) = self.backend.lock().upgrade() else {
            return;
        };
        let Some(eval) = backend.auto_prompt_settings().eval else {
            return;
        };
        let Some(events) = self.events.lock().clone() else {
            return;
        };
        let state = turn_state(&session, turn_id);
        let questions: BTreeMap<_, _> = rules
            .iter()
            .flat_map(|rule| {
                rule.questions.iter().map(move |question| {
                    (
                        format!("{}:{}", rule.id, question.id),
                        EvalQuestion::Noul {
                            instructions: question.instructions.clone(),
                            criteria: None,
                        },
                    )
                })
            })
            .collect();
        let started = std::time::Instant::now();
        let result = crate::eval::evaluate(&eval, &state, &questions);
        let mut record = crate::eval::EvalDecisionRecord::empty("auto-prompt");
        record.backend = Some(eval.backend);
        record.latency_ms = Some(started.elapsed().as_millis() as u64);
        record.state = Some(state);
        record.questions = Some(questions);
        record.model = result
            .as_ref()
            .ok()
            .map(|evaluation| evaluation.model.clone());
        record.usage = result
            .as_ref()
            .ok()
            .map(|evaluation| evaluation.usage.clone());
        record.answers = result
            .as_ref()
            .ok()
            .map(|evaluation| evaluation.answers.clone());
        record.error = result.as_ref().err().map(|error| error.to_string());
        crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
        let evaluation = match result {
            Ok(evaluation) => evaluation,
            Err(error) => {
                eprintln!(
                    "auto prompt evaluation failed for task {}: {error:#}",
                    session.id
                );
                return;
            }
        };
        let winner = rules
            .iter()
            .filter_map(|rule| {
                let mut weighted = 0.0;
                let mut total = 0.0;
                for question in &rule.questions {
                    let key = format!("{}:{}", rule.id, question.id);
                    let Some(EvalAnswer::Noul { noul }) = evaluation.answers.get(&key) else {
                        return None;
                    };
                    if !noul.is_finite() || !(0.0..=1.0).contains(noul) {
                        return None;
                    }
                    let weight = question.weight?;
                    weighted += weight * *noul;
                    total += weight;
                }
                let score = weighted / total;
                (score >= rule.threshold?).then_some((score, rule))
            })
            .max_by(|(left, _), (right, _)| left.total_cmp(right));
        let Some((_, rule)) = winner else {
            return;
        };
        if !backend
            .auto_prompt_settings()
            .auto_prompts
            .iter()
            .any(|current| current == rule)
        {
            return;
        }
        // A newer human turn supersedes this judgment. The queue handles a
        // turn that starts between this check and dispatch without steering.
        if !backend.auto_prompt_turn_is_latest(session.id, turn_id) {
            return;
        }
        if let Err(error) = self.record_selection(session.id, turn_id, rule.id) {
            eprintln!("could not record auto prompt rule decision for {turn_id}: {error:#}");
            return;
        }
        let name = rule.name.replace(['\n', '\r'], " ");
        let prompt = format!("{AUTO_PROMPT_PREFIX}{name}]\n\n{}", rule.prompt);
        if let Err(error) = backend.queue_auto_prompt(session.id, turn_id, prompt, &events) {
            eprintln!(
                "could not send auto prompt for task {}: {error:#}",
                session.id
            );
        }
    }
}
