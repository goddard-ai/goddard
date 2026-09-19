//! Model-routing wire types: the classifier names the *kind* of work (a task
//! family and class), while policy — never the model — maps those to a
//! concrete provider and model. The daemon evaluates and resolves; the app
//! supplies the eligible candidate set and applies the decision.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::eval::EvalBackend;
use crate::model::ProviderKind;

/// The twelve task families the classifier chooses between, adapted from the
/// Vercel Labs `fx` router. Classification describes the requested
/// deliverable, never a model choice — the model sees these ids and their
/// descriptions, nothing else.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum TaskFamily {
    /// Produce or modify code: implement a feature, write a script, refactor.
    CodeGeneration,
    /// Diagnose a failure or review existing code: find the bug, explain the
    /// trace, critique the diff.
    DebuggingReview,
    /// Write new prose from scratch: docs, posts, copy, summaries of ideas
    /// the user did not hand over.
    Writing,
    /// Rewrite text the user supplied: tighten, reformat, translate, re-tone.
    EditingRewriting,
    /// Answer a factual question or explain a concept.
    InformationSeeking,
    /// Procedural guidance: how do I do X, what steps get me to Y.
    HowToAdvice,
    /// Interactive teaching where the user explicitly wants to be taught or
    /// quizzed, not just informed.
    Tutoring,
    /// Compute, transform, or reason about numbers and structured data.
    DataMath,
    /// Design a plan, brainstorm options, weigh tradeoffs before acting.
    PlanningIdeation,
    /// Actually use tools to change external state or read live systems, or
    /// the user explicitly asks for that — not merely a task tools could
    /// help with.
    AgenticToolUse,
    /// Produce or describe media: images, audio, video, diagrams-as-assets.
    CreativeMedia,
    /// Conversation, opinions, and anything the other families do not cover.
    ConversationalOther,
}

impl TaskFamily {
    pub const ALL: [TaskFamily; 12] = [
        TaskFamily::CodeGeneration,
        TaskFamily::DebuggingReview,
        TaskFamily::Writing,
        TaskFamily::EditingRewriting,
        TaskFamily::InformationSeeking,
        TaskFamily::HowToAdvice,
        TaskFamily::Tutoring,
        TaskFamily::DataMath,
        TaskFamily::PlanningIdeation,
        TaskFamily::AgenticToolUse,
        TaskFamily::CreativeMedia,
        TaskFamily::ConversationalOther,
    ];

    /// The stable id the classifier answers with and policy JSON keys on.
    pub fn id(&self) -> &'static str {
        match self {
            TaskFamily::CodeGeneration => "code-generation",
            TaskFamily::DebuggingReview => "debugging-review",
            TaskFamily::Writing => "writing",
            TaskFamily::EditingRewriting => "editing-rewriting",
            TaskFamily::InformationSeeking => "information-seeking",
            TaskFamily::HowToAdvice => "how-to-advice",
            TaskFamily::Tutoring => "tutoring",
            TaskFamily::DataMath => "data-math",
            TaskFamily::PlanningIdeation => "planning-ideation",
            TaskFamily::AgenticToolUse => "agentic-tool-use",
            TaskFamily::CreativeMedia => "creative-media",
            TaskFamily::ConversationalOther => "conversational-other",
        }
    }

    /// One-line description shown to the classifier as the option's criteria.
    pub fn description(&self) -> &'static str {
        match self {
            TaskFamily::CodeGeneration => {
                "produce or modify code: implement, script, refactor, generate"
            }
            TaskFamily::DebuggingReview => {
                "diagnose a failure or review existing code: fix the bug, explain the trace, critique the change"
            }
            TaskFamily::Writing => "write new prose from scratch",
            TaskFamily::EditingRewriting => {
                "revise text the user supplied: tighten, reformat, re-tone, translate"
            }
            TaskFamily::InformationSeeking => "answer a factual question or explain a concept",
            TaskFamily::HowToAdvice => "procedural guidance: how to do a thing, step by step",
            TaskFamily::Tutoring => {
                "the user explicitly wants to be taught or quizzed interactively"
            }
            TaskFamily::DataMath => {
                "compute, transform, or reason about numbers and structured data"
            }
            TaskFamily::PlanningIdeation => "plan, brainstorm, or weigh options before acting",
            TaskFamily::AgenticToolUse => {
                "the task must use tools to read live state or change external systems"
            }
            TaskFamily::CreativeMedia => "produce or describe media artifacts",
            TaskFamily::ConversationalOther => {
                "conversation, opinions, or anything no other family covers"
            }
        }
    }
}

/// How much of a model the task deserves. `general` is the unmarked middle;
/// `routine` routes cheap/fast, `demanding` routes to the strongest tier.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, TS,
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

impl TaskClass {
    /// The stable id the classifier answers with and policy JSON keys on.
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
/// unknown — a concrete target for it is accepted but a tier always resolves
/// to that provider's own default.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RouteCandidate {
    pub provider: ProviderKind,
    #[serde(default)]
    pub models: Vec<String>,
}

/// A concrete route: provider plus an optional model (`None` = the
/// provider's own default).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RouteTarget {
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// The outcome of one routing decision — what the session starts on and why.
/// Kept on the session so the UI can explain the route after the fact.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub target: RouteTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<TaskFamily>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<TaskClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_planning: Option<bool>,
    /// Confidence the backend reported for the family answer, 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_confidence: Option<f64>,
    /// Confidence the backend reported for the class answer, 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_confidence: Option<f64>,
    /// Why this target won: "policy", "low-confidence", "eval-failed",
    /// "eval-unconfigured", "default", "target-ineligible", and friends.
    pub reason: String,
    /// Hash of the policy document the decision was made under, matching the
    /// decision log so a route can be traced back to its policy.
    pub policy_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<EvalBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_latency_ms: Option<u64>,
}

/// The routing policy as the settings surface sees it: the class-level
/// targets rendered as dropdowns, where the file lives, and whether the
/// user's document is the one in effect.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RoutePolicyView {
    #[ts(type = "string")]
    pub path: PathBuf,
    /// `false` means the user's document failed validation and the shipped
    /// default is in effect.
    pub valid: bool,
    pub hash: String,
    /// class id -> raw policy target string ("tier:fast", "codex:gpt-5.5").
    pub classes: BTreeMap<String, String>,
    /// The configured default route: "last_used" or a target string.
    pub default: String,
}
