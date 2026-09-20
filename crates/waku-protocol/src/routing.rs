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

/// How much of a model the task deserves. `general` is the unmarked middle;
/// `routine` routes cheap/fast, `demanding` routes to the strongest model.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS,
)]
#[serde(rename_all = "camelCase")]
pub enum TaskClass {
    /// Mechanical, low-risk, or single-step work.
    Routine,
    /// The default: ordinary tasks that benefit from a solid model.
    #[default]
    General,
    /// Subtle, high-stakes, or long-horizon work where mistakes are costly.
    Demanding,
}

/// Every class the classifier may answer, in route order.
pub const ALL_TASK_CLASSES: [TaskClass; 3] =
    [TaskClass::Routine, TaskClass::General, TaskClass::Demanding];

impl TaskClass {
    /// The stable id the classifier answers with and the class map keys on.
    pub fn id(&self) -> &'static str {
        match self {
            TaskClass::Routine => "routine",
            TaskClass::General => "general",
            TaskClass::Demanding => "demanding",
        }
    }
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
    /// Why this target won: "class-map", "class-unmapped",
    /// "low-class-confidence", "eval-failed", "eval-unconfigured",
    /// "model-ineligible", "provider-ineligible", and friends.
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<EvalBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_latency_ms: Option<u64>,
}
