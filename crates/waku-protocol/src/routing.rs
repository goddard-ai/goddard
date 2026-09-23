//! Model-routing wire types: the classifier names the *kind* of work (a task
//! class), while the user's class map — never the model — maps that to a
//! concrete provider, model, and reasoning effort. The daemon evaluates and
//! resolves; the app supplies the eligible candidate set and applies the
//! decision.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::eval::EvalBackend;
use crate::model::ProviderKind;

/// Difficulty tier for automatic model routing: easy, medium, or hard.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS,
)]
#[serde(rename_all = "camelCase")]
pub enum TaskClass {
    /// Mechanical, low-risk, or single-step work.
    #[serde(rename = "easy", alias = "routine")]
    Routine,
    /// The default: ordinary tasks that benefit from a solid model.
    #[default]
    #[serde(rename = "medium", alias = "general")]
    General,
    /// Subtle, high-stakes, or long-horizon work where mistakes are costly.
    #[serde(rename = "hard", alias = "demanding")]
    Demanding,
}

/// Every class the classifier may answer, in route order.
pub const ALL_TASK_CLASSES: [TaskClass; 3] =
    [TaskClass::Routine, TaskClass::General, TaskClass::Demanding];

impl TaskClass {
    /// The stable id the classifier answers with and the class map keys on.
    pub fn id(&self) -> &'static str {
        match self {
            TaskClass::Routine => "easy",
            TaskClass::General => "medium",
            TaskClass::Demanding => "hard",
        }
    }

    /// The tier a task drops to once its plan exists — planning absorbed the
    /// hard reasoning, so implementation runs one class cheaper.
    pub fn implementation_class(&self) -> TaskClass {
        match self {
            TaskClass::Demanding => TaskClass::General,
            TaskClass::General | TaskClass::Routine => TaskClass::Routine,
        }
    }
}

/// Where a task sits in a plan-then-execute lifecycle. Phase routing derives
/// it from the tool stream and settle evaluations — descriptive session
/// state the provider is never told about.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SessionPhase {
    /// Reads, searches, and plan tools only so far — no durable edits.
    Planning,
    /// The task committed to implementation: a real file change ran, or an
    /// evaluation judged planning done.
    Executing,
}

/// What one streamed activity says about the planning→implementation
/// boundary. The classifier is deliberately conservative: ambiguous work
/// waits for the turn-settle evaluation rather than flipping on a guess.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhaseSignal {
    /// A durable edit to a non-doc path — unambiguous execution. Flips
    /// Planning → Executing immediately, no evaluation needed.
    Committing,
    /// Could be either side of the boundary: a shell command, or a write
    /// whose target reads as the plan document itself. Defers to the
    /// settle evaluation.
    Ambiguous,
    /// Reads, searches, plan tools, failures — planning evidence. Never
    /// flips the phase.
    Planning,
}

/// One provider the router may pick, with the model ids the app knows are
/// available on it. An empty `models` means the provider's catalog is
/// unknown — a configured model for it is accepted sight unseen.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RouteCandidate {
    pub provider: ProviderKind,
    #[serde(default)]
    pub models: Vec<String>,
}

/// A concrete route: provider plus an optional model (`None` = the
/// provider's own default) and an optional reasoning effort (`None` = the
/// user's remembered traits for the resolved model).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RouteTarget {
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// The user's per-class route: which provider/model/effort a task of this
/// class starts on. Stored in daemon settings; an absent class leaves the
/// route on the `last_used` default.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct RouteClassTarget {
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// The class map: `TaskClass` → the user's configured target.
pub type RouteClassMap = BTreeMap<TaskClass, RouteClassTarget>;

/// The outcome of one routing decision — what the session starts on and why.
/// Kept on the session so the UI can explain the route after the fact.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub target: RouteTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<TaskClass>,
    /// Confidence the backend reported for the class answer, 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_confidence: Option<f64>,
    /// The intake evaluation judged this task worth a planning phase: it
    /// started on the hardest-class entry and may downshift a class tier
    /// once planning ends. `false` when the eval skipped the question.
    #[serde(default, skip_serializing_if = "crate::model::is_false")]
    pub phased: bool,
    /// Why this target won: "class-map", "class-unmapped",
    /// "low-class-confidence", "eval-failed", "eval-unconfigured",
    /// "model-ineligible", "provider-ineligible", and friends.
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<EvalBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_latency_ms: Option<u64>,
}
