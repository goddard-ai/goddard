//! Persistence and conservative lifecycle rules for temporary user instructions.

use anyhow::{anyhow, ensure};
use uuid::Uuid;
use waku_protocol::boss::{BossState, OutcomeState};
use waku_protocol::instruction_pins::{
    InstructionPin, InstructionPinOperation, InstructionPinResolution, InstructionPinRetirement,
    InstructionPinSource, InstructionPinState,
};
use waku_protocol::model::{truncate_chars, unix_time};

use crate::BossService;

const PIN_TEXT_CAP: usize = 2_000;
const PIN_CONDITION_CAP: usize = 1_000;
const PIN_CONTEXT_CAP: usize = 8_000;
const MAX_CANDIDATES: usize = 32;

fn brief(mut pin: InstructionPin) -> InstructionPin {
    if pin.state == InstructionPinState::Candidate {
        pin.instruction = truncate_chars(&pin.instruction, 250);
    }
    if let Some(source) = &mut pin.source {
        source.quote_truncated |= source.quote.chars().count() > 300;
        source.quote = truncate_chars(&source.quote, 300);
    }
    pin
}

fn active_line(pin: &InstructionPin, project_name: Option<&str>) -> String {
    let scope = pin.project_id.map_or_else(
        || "all projects".into(),
        |id| match project_name {
            Some(name) => format!(
                "project {} ({id})",
                name.chars().take(80).collect::<String>()
            ),
            None => format!("project {id}"),
        },
    );
    format!(
        "- {} (revision {}, {scope}): {}\n  Retire when: {}\n",
        pin.id, pin.revision, pin.instruction, pin.retire_when
    )
}

fn validate_active(state: &BossState) -> anyhow::Result<()> {
    let size: usize = state
        .instruction_pins
        .iter()
        .filter(|pin| pin.state == InstructionPinState::Active)
        .map(|pin| {
            active_line(pin, None).chars().count() + if pin.project_id.is_some() { 83 } else { 0 }
        })
        .sum();
    ensure!(
        size <= PIN_CONTEXT_CAP,
        "active instructions exceed the context budget; shorten or retire an existing pin first"
    );
    Ok(())
}

fn validate_text(instruction: &str, retire_when: &str) -> anyhow::Result<()> {
    ensure!(!instruction.trim().is_empty(), "instruction is required");
    ensure!(
        !retire_when.trim().is_empty(),
        "a retirement condition is required"
    );
    ensure!(
        instruction.chars().count() <= PIN_TEXT_CAP,
        "instruction exceeds {PIN_TEXT_CAP} characters"
    );
    ensure!(
        retire_when.chars().count() <= PIN_CONDITION_CAP,
        "retirement condition exceeds {PIN_CONDITION_CAP} characters"
    );
    Ok(())
}

fn validate_trigger(
    state: &BossState,
    trigger: &Option<InstructionPinRetirement>,
) -> anyhow::Result<()> {
    if let Some(InstructionPinRetirement::OutcomeCompleted { outcome_id }) = trigger {
        ensure!(
            state
                .outcomes
                .iter()
                .any(|outcome| outcome.id == *outcome_id),
            "linked outcome does not exist"
        );
    }
    Ok(())
}

/// Called inside every atomic Boss update. Cancellation, failure, and a
/// finished employee are deliberately insufficient evidence of completion.
pub(crate) fn reconcile(state: &mut BossState, now: u64) {
    for pin in &mut state.instruction_pins {
        if pin.state != InstructionPinState::Active {
            continue;
        }
        let Some(InstructionPinRetirement::OutcomeCompleted { outcome_id }) = pin.retire_on else {
            continue;
        };
        if let Some(outcome) = state.outcomes.iter().find(|outcome| {
            outcome.id == outcome_id
                && outcome.state == OutcomeState::Completed
                && outcome.completed_at.is_some()
                && outcome
                    .evidence
                    .as_deref()
                    .is_some_and(|evidence| !evidence.trim().is_empty())
        }) {
            pin.state = InstructionPinState::Retired;
            pin.revision += 1;
            pin.updated_at = now;
            pin.resolution = Some(InstructionPinResolution {
                at: now,
                evidence: format!(
                    "Linked outcome {outcome_id} completed; evidence: {}",
                    truncate_chars(outcome.evidence.as_deref().unwrap_or_default(), 500)
                ),
                superseded_by: None,
            });
        }
    }
}

/// A backend-free fallback only creates a review candidate, never a binding
/// instruction. Semantic classification can discover other formulations.
pub fn candidate_hint(prompt: &str) -> bool {
    let mut in_code = false;
    let text = prompt
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code = !in_code;
                return false;
            }
            !in_code && !trimmed.starts_with('>')
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let ongoing = [
        "until ",
        "before ",
        "while ",
        "from now on",
        "always ",
        "never ",
    ];
    let obligation = [
        "do not ", "don't ", "must ", "only ", "wait ", "hold ", "keep ", "always ", "never ",
    ];
    ongoing.iter().any(|word| text.contains(word))
        && obligation.iter().any(|word| text.contains(word))
}

impl BossService {
    pub fn instruction_pin_operation(
        &self,
        caller: Option<Uuid>,
        action: InstructionPinOperation,
        source: Option<InstructionPinSource>,
    ) -> anyhow::Result<Vec<InstructionPin>> {
        ensure!(
            caller.is_none_or(|id| self.is_boss(id)),
            "only the Boss chat or a human can manage instruction pins"
        );
        if let InstructionPinOperation::Show { id } = action {
            return self
                .document()
                .instruction_pins
                .into_iter()
                .find(|pin| pin.id == id)
                .map(|pin| vec![pin])
                .ok_or_else(|| anyhow!("instruction pin does not exist"));
        }
        if let InstructionPinOperation::List { include_retired } = action {
            return Ok(self
                .document()
                .instruction_pins
                .into_iter()
                .filter(|pin| {
                    include_retired
                        || matches!(
                            pin.state,
                            InstructionPinState::Active | InstructionPinState::Candidate
                        )
                })
                .map(brief)
                .collect());
        }
        let now = unix_time();
        let mut changed_id = None;
        self.update(|state| {
            match action {
                InstructionPinOperation::Pin { instruction, retire_when, project_id, retire_on, .. } => {
                    validate_text(&instruction, &retire_when)?;
                    validate_trigger(state, &retire_on)?;
                    ensure!(caller.is_none() || source.is_some(), "Boss pins must cite an original user message");
                    let same_source = |pin: &InstructionPin| source.as_ref().is_some_and(|source| pin.source.as_ref().is_some_and(|prior| prior.message_id == source.message_id && prior.session_id == source.session_id));
                    // A message can contain several obligations. Replaying the
                    // same one is idempotent; revising it requires Edit.
                    if let Some(existing) = state.instruction_pins.iter().filter(|pin| same_source(pin)).find(|pin| pin.state != InstructionPinState::Candidate && pin.instruction == instruction.trim()) {
                        ensure!(existing.state == InstructionPinState::Active && existing.retire_when == retire_when.trim() && existing.project_id == project_id && existing.retire_on == retire_on,
                            "this message already has a pin; edit its current revision or choose another source");
                        changed_id = Some(existing.id);
                        return Ok(());
                    }
                    if let Some(index) = state.instruction_pins.iter().position(|pin| same_source(pin) && pin.state == InstructionPinState::Candidate) {
                        let candidate = &mut state.instruction_pins[index];
                        candidate.instruction = instruction.trim().into();
                        candidate.retire_when = retire_when.trim().into();
                        candidate.project_id = project_id;
                        candidate.retire_on = retire_on;
                        candidate.state = InstructionPinState::Active;
                        candidate.updated_at = now;
                        candidate.revision += 1;
                        changed_id = Some(candidate.id);
                        reconcile(state, now);
                        return validate_active(state);
                    }
                    let id = Uuid::new_v4();
                    changed_id = Some(id);
                    state.instruction_pins.push(InstructionPin {
                        id, revision: 1,
                        instruction: instruction.trim().into(), retire_when: retire_when.trim().into(),
                        project_id, retire_on, source,
                        state: InstructionPinState::Active, created_at: now, updated_at: now, resolution: None,
                    });
                }
                InstructionPinOperation::Edit { id, expected_revision, instruction, retire_when, project_id, retire_on } => {
                    changed_id = Some(id);
                    validate_text(&instruction, &retire_when)?;
                    validate_trigger(state, &retire_on)?;
                    let pin = state.instruction_pins.iter_mut().find(|pin| pin.id == id).ok_or_else(|| anyhow!("instruction pin does not exist"))?;
                    ensure!(pin.revision == expected_revision, "instruction pin changed; reload its current revision");
                    ensure!(matches!(pin.state, InstructionPinState::Active | InstructionPinState::Candidate), "retired instructions cannot be reactivated; create a new pin");
                    pin.instruction = instruction.trim().into(); pin.retire_when = retire_when.trim().into();
                    pin.project_id = project_id; pin.retire_on = retire_on;
                    pin.state = InstructionPinState::Active; pin.updated_at = now; pin.revision += 1;
                }
                InstructionPinOperation::Retire { id, expected_revision, evidence, superseded_by } => {
                    changed_id = Some(id);
                    ensure!(!evidence.trim().is_empty(), "retirement evidence is required");
                    ensure!(evidence.chars().count() <= PIN_CONDITION_CAP, "retirement evidence is too long");
                    if let Some(replacement) = superseded_by {
                        ensure!(replacement != id && state.instruction_pins.iter().any(|pin| pin.id == replacement && pin.state == InstructionPinState::Active), "replacement must name another active instruction pin");
                    }
                    let pin = state.instruction_pins.iter_mut().find(|pin| pin.id == id).ok_or_else(|| anyhow!("instruction pin does not exist"))?;
                    // A retry of an already-confirmed retirement is harmless.
                    if pin.resolution.as_ref().is_some_and(|resolution| resolution.evidence == evidence.trim() && resolution.superseded_by == superseded_by) { return Ok(()); }
                    ensure!(pin.revision == expected_revision, "instruction pin changed; reload its current revision");
                    ensure!(matches!(pin.state, InstructionPinState::Active | InstructionPinState::Candidate), "instruction pin is already retired");
                    pin.state = if superseded_by.is_some() { InstructionPinState::Superseded } else { InstructionPinState::Retired };
                    pin.updated_at = now; pin.revision += 1;
                    pin.resolution = Some(InstructionPinResolution { at: now, evidence: evidence.trim().into(), superseded_by });
                }
                InstructionPinOperation::List { .. } | InstructionPinOperation::Show { .. } => unreachable!(),
            }
            reconcile(state, now);
            validate_active(state)
        })?;
        Ok(self
            .document()
            .instruction_pins
            .into_iter()
            .filter(|pin| Some(pin.id) == changed_id)
            .map(brief)
            .collect())
    }

    pub fn capture_instruction_candidate(
        &self,
        source: InstructionPinSource,
    ) -> anyhow::Result<()> {
        self.update(|state| {
            if state.instruction_pins.iter().any(|pin| {
                pin.source.as_ref().is_some_and(|prior| {
                    prior.session_id == source.session_id && prior.message_id == source.message_id
                })
            }) {
                return Ok(());
            }
            ensure!(
                state
                    .instruction_pins
                    .iter()
                    .filter(|pin| pin.state == InstructionPinState::Candidate)
                    .count()
                    < MAX_CANDIDATES,
                "instruction candidates need review before more can be captured"
            );
            let now = unix_time();
            state.instruction_pins.push(InstructionPin {
                id: Uuid::new_v4(),
                revision: 1,
                instruction: truncate_chars(&source.quote, PIN_TEXT_CAP),
                retire_when: String::new(),
                project_id: None,
                retire_on: None,
                source: Some(source),
                state: InstructionPinState::Candidate,
                created_at: now,
                updated_at: now,
                resolution: None,
            });
            Ok(())
        })
    }

    /// All applicable obligations stay visible; a mutation exceeding the
    /// budget refuses atomically rather than silently omitting a pin.
    pub fn instruction_pin_context(&self, projects: &[waku_protocol::model::Project]) -> String {
        let state = self.document();
        let mut body = String::new();
        for pin in &state.instruction_pins {
            if pin.state == InstructionPinState::Active {
                let name = pin.project_id.and_then(|id| {
                    projects
                        .iter()
                        .find(|project| project.id == id)
                        .map(|project| project.name.as_str())
                });
                body.push_str(&active_line(pin, name));
            }
        }
        if body.is_empty() && !state.instruction_pins.is_empty() {
            // Omitting an empty snapshot would leave an older provider
            // context's active instructions in force after human release.
            body.push_str("Active pins: none. No prior pin snapshot remains binding.\n");
        }
        let candidates: Vec<_> = state
            .instruction_pins
            .iter()
            .filter(|pin| pin.state == InstructionPinState::Candidate)
            .collect();
        if !candidates.is_empty() {
            body.push_str("Candidates awaiting review (not binding instructions):\n");
            for pin in candidates.iter().take(4) {
                body.push_str(&format!(
                    "- {} (revision {}): {}\n",
                    pin.id,
                    pin.revision,
                    truncate_chars(&pin.instruction.replace('\n', " "), 250)
                ));
            }
            if candidates.len() > 4 {
                body.push_str(&format!(
                    "{} more candidates; inspect with `goddard-agent boss instructions list`.\n",
                    candidates.len() - 4
                ));
            }
        }
        if body.is_empty() {
            return body;
        }
        format!(
            "<goddard-instruction-pins>\nThis is the current instruction-pin snapshot and supersedes all earlier pin snapshots. Only pins listed as active here remain binding. Temporary user obligations complement personas and memory buckets. Apply active pins within their stated scope; keep them active until their retirement condition has evidence. Carry applicable constraints when delegating work. Review candidates with `goddard-agent boss instructions show ID`, then activate with `instructions edit ID --revision N --text INSTRUCTION --retire-when CONDITION`. Dismiss or retire with `instructions retire ID --revision N --evidence EVIDENCE`. A changed instruction may supersede an older pin.\n{body}</goddard-instruction-pins>"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> InstructionPinSource {
        InstructionPinSource {
            session_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            quote: "Do not resume chains until I confirm the daemon restart.".into(),
            quote_truncated: false,
        }
    }

    #[test]
    fn pin_creation_activates_a_candidate_and_supports_multiple_obligations() {
        let root = std::env::temp_dir().join(format!("instruction-pin-source-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let source = source();
        service
            .capture_instruction_candidate(source.clone())
            .unwrap();
        let candidate_id = service.document().instruction_pins[0].id;
        let create = |instruction: &str| InstructionPinOperation::Pin {
            instruction: instruction.into(),
            retire_when: "The user confirms the restart".into(),
            project_id: None,
            retire_on: None,
            source_session_id: Some(source.session_id),
            source_message_id: Some(source.message_id),
        };
        let activated = service
            .instruction_pin_operation(None, create("Hold summon chains"), Some(source.clone()))
            .unwrap();
        assert_eq!(activated[0].id, candidate_id);
        assert_eq!(activated[0].state, InstructionPinState::Active);
        let before = service.document().revision;
        service
            .instruction_pin_operation(None, create("Hold summon chains"), Some(source.clone()))
            .unwrap();
        assert_eq!(service.document().revision, before);
        service
            .instruction_pin_operation(None, create("Hold scheduled repairs"), Some(source.clone()))
            .unwrap();
        service.capture_instruction_candidate(source).unwrap();
        assert_eq!(service.document().instruction_pins.len(), 2);
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linked_completion_retires_with_evidence_but_cancellation_keeps_the_hold() {
        let root = std::env::temp_dir().join(format!("instruction-pin-outcome-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let outcome = service
            .create_outcome(None, "Repair daemon", "Verified fix")
            .unwrap();
        service
            .instruction_pin_operation(
                None,
                InstructionPinOperation::Pin {
                    instruction: "Hold new assignments while the daemon repair is open".into(),
                    retire_when: "The daemon repair outcome is completed with evidence".into(),
                    project_id: None,
                    retire_on: Some(InstructionPinRetirement::OutcomeCompleted {
                        outcome_id: outcome.id,
                    }),
                    source_session_id: None,
                    source_message_id: None,
                },
                None,
            )
            .unwrap();
        service
            .set_outcome_state(None, outcome.id, OutcomeState::Cancelled, None)
            .unwrap();
        assert_eq!(
            service.document().instruction_pins[0].state,
            InstructionPinState::Active
        );
        service
            .set_outcome_state(None, outcome.id, OutcomeState::Open, None)
            .unwrap();
        assert!(
            service
                .set_outcome_state(None, outcome.id, OutcomeState::Completed, None)
                .is_err()
        );
        assert_eq!(
            service.document().instruction_pins[0].state,
            InstructionPinState::Active
        );
        service
            .set_outcome_state(
                None,
                outcome.id,
                OutcomeState::Completed,
                Some("Fix landed and regression checks passed".into()),
            )
            .unwrap();
        let pin = service.document().instruction_pins[0].clone();
        assert_eq!(pin.state, InstructionPinState::Retired);
        assert!(
            pin.resolution
                .unwrap()
                .evidence
                .contains("regression checks passed")
        );
        let context = service.instruction_pin_context(&[]);
        assert!(context.contains("Active pins: none"));
        assert!(!context.contains("Hold new assignments"));
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overflow_refuses_atomically_and_quoted_material_is_not_a_candidate() {
        let root = std::env::temp_dir().join(format!("instruction-pin-budget-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        for index in 0..4 {
            service
                .instruction_pin_operation(
                    None,
                    InstructionPinOperation::Pin {
                        instruction: format!("{index}: {}", "x".repeat(1_700)),
                        retire_when: "The user releases this constraint".into(),
                        project_id: None,
                        retire_on: None,
                        source_session_id: None,
                        source_message_id: None,
                    },
                    None,
                )
                .unwrap();
        }
        let before = service.document();
        assert!(
            service
                .instruction_pin_operation(
                    None,
                    InstructionPinOperation::Pin {
                        instruction: "x".repeat(1_700),
                        retire_when: "The user releases it".into(),
                        project_id: None,
                        retire_on: None,
                        source_session_id: None,
                        source_message_id: None,
                    },
                    None
                )
                .is_err()
        );
        assert_eq!(service.document().revision, before.revision);
        assert_eq!(service.document().instruction_pins, before.instruction_pins);
        assert!(
            service
                .instruction_pin_operation(
                    Some(Uuid::new_v4()),
                    InstructionPinOperation::List {
                        include_retired: true
                    },
                    None
                )
                .is_err()
        );
        assert!(candidate_hint(
            "Do not resume chains until I confirm the restart."
        ));
        assert!(!candidate_hint(
            "> Never resume chains until confirmed.\nComment: please explain this proposal."
        ));
        assert!(!candidate_hint(
            "```\nnever resume until confirmed\n```\nExplain the code."
        ));
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn candidates_survive_restart_activation_and_retirement_with_evidence() {
        let root = std::env::temp_dir().join(format!("instruction-pins-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let source = source();
        service
            .capture_instruction_candidate(source.clone())
            .unwrap();
        service
            .capture_instruction_candidate(source.clone())
            .unwrap();
        let candidate = service.document().instruction_pins[0].clone();
        assert_eq!(service.document().instruction_pins.len(), 1);
        assert!(service.instruction_pin_context(&[]).contains("not binding"));
        drop(service);
        let service = BossService::open(root.clone()).unwrap();
        service
            .instruction_pin_operation(
                None,
                InstructionPinOperation::Edit {
                    id: candidate.id,
                    expected_revision: 1,
                    instruction: source.quote.clone(),
                    retire_when: "The user confirms the daemon restart".into(),
                    project_id: None,
                    retire_on: None,
                },
                None,
            )
            .unwrap();
        assert!(
            service
                .instruction_pin_context(&[])
                .contains("Retire when: The user confirms")
        );
        let retire = InstructionPinOperation::Retire {
            id: candidate.id,
            expected_revision: 2,
            evidence: "".into(),
            superseded_by: None,
        };
        assert!(
            service
                .instruction_pin_operation(None, retire, None)
                .is_err()
        );
        assert_eq!(
            service.document().instruction_pins[0].state,
            InstructionPinState::Active
        );
        let retire = InstructionPinOperation::Retire {
            id: candidate.id,
            expected_revision: 2,
            evidence: "User confirmed the restart in message 7".into(),
            superseded_by: None,
        };
        service
            .instruction_pin_operation(None, retire.clone(), None)
            .unwrap();
        service
            .instruction_pin_operation(None, retire, None)
            .unwrap();
        let context = service.instruction_pin_context(&[]);
        assert!(context.contains("Active pins: none"));
        assert!(!context.contains(&source.quote));
        let stale = InstructionPinOperation::Edit {
            id: candidate.id,
            expected_revision: 2,
            instruction: "Resume chains".into(),
            retire_when: "When complete".into(),
            project_id: None,
            retire_on: None,
        };
        assert!(
            service
                .instruction_pin_operation(None, stale, None)
                .is_err()
        );
        drop(service);
        let service = BossService::open(root.clone()).unwrap();
        assert_eq!(
            service.document().instruction_pins[0].state,
            InstructionPinState::Retired
        );
        assert!(service.document().instruction_pins[0].resolution.is_some());
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }
}
