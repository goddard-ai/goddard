//! Hosted evaluation-model calls (TypeSafe's Jev today): a shared `state` is
//! scored against typed questions and answers come back as calibrated
//! probabilities rather than generated text. The daemon owns the HTTP calls;
//! these are the wire and settings shapes every eval feature shares.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Which hosted service answers evaluation calls. All three speak the same
/// question/answer contract; only the request envelope and credentials differ.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum EvalBackend {
    /// TypeSafe's own API: `api.typesafe.ai/v1/systemone`, one API key.
    #[default]
    TypeSafe,
    /// Vercel AI Gateway's evaluation endpoint, AI Gateway key or OIDC token.
    VercelGateway,
    /// Cloudflare Workers AI, account id + API token.
    Cloudflare,
}

/// Bring-your-own-key evaluation configuration, stored in daemon settings.
/// `None` fields mean that backend is not configured; the selected `backend`
/// must have its credential present for a call to run.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct EvalSettings {
    pub backend: EvalBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typesafe_api_key: Option<String>,
    /// AI Gateway API key or a Vercel OIDC token; both take the same Bearer slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vercel_api_key: Option<String>,
    /// Optional Vercel team routing header (`x-vercel-ai-gateway-team`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vercel_team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudflare_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudflare_api_token: Option<String>,
}

/// One typed question evaluated against the request's shared `state`. The
/// `type` tag and field names match the evaluation API contract exactly.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EvalQuestion {
    /// A yes/no judgment; the answer is the probability of `true`.
    Noul {
        instructions: String,
        /// Optional descriptions of what each side means.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<BTreeMap<String, String>>,
    },
    /// Pick exactly one of the named options; the answer carries the full
    /// probability distribution over them. A `None` description is a bare
    /// option label (useful for an `other` bucket).
    Choice {
        instructions: String,
        criteria: BTreeMap<String, Option<String>>,
    },
    /// A position on an ordered rubric; `criteria` are the level descriptions
    /// from lowest to highest.
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

/// One question's typed answer. Choice and Score also carry `confidence`, a
/// 0–1 summary of how concentrated the probability distribution is.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EvalAnswer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        probabilities: BTreeMap<String, f64>,
    },
    Score {
        score: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        /// Level index → description, echoed back by the model.
        #[serde(default)]
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct EvalUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// One completed evaluation call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Evaluation {
    /// The versioned model id the backend reports answering with.
    pub model: String,
    pub answers: BTreeMap<String, EvalAnswer>,
    #[serde(default)]
    pub usage: EvalUsage,
    /// Client-observed round trip in milliseconds.
    pub latency_ms: u64,
    /// Provider-specific metadata (e.g. TypeSafe's separate `confidence`
    /// statistic), passed through uninterpreted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub provider_metadata: Option<Value>,
}
