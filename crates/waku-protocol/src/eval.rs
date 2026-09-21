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

impl EvalSettings {
    /// Whether the selected backend is missing the credential its requests
    /// require — mirrors the `required` checks in waku-core's eval request
    /// builder, so a feature can skip a call that can only fail.
    pub fn credential_missing(&self) -> bool {
        let missing = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        };
        match self.backend {
            EvalBackend::TypeSafe => missing(&self.typesafe_api_key),
            EvalBackend::VercelGateway => missing(&self.vercel_api_key),
            EvalBackend::Cloudflare => {
                missing(&self.cloudflare_account_id) || missing(&self.cloudflare_api_token)
            }
        }
    }
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
    /// Cloudflare's Workers AI envelope reports snake_case; TypeSafe and the
    /// Vercel evaluation spec report camelCase.
    #[serde(alias = "input_tokens")]
    pub input_tokens: u64,
    #[serde(alias = "output_tokens")]
    pub output_tokens: u64,
}

/// Token totals for one slice of the eval decision log — the whole log, or
/// one feature's records.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct EvalUsageTotals {
    /// Evaluation calls logged in the slice, whether or not they reported
    /// usage.
    pub calls: u64,
    /// Calls whose record carries backend-reported usage. Below `calls`
    /// when records predate usage reporting or a backend omitted it.
    pub calls_with_usage: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Aggregated token usage across the daemon's eval decision log, read back
/// for the settings pane.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct EvalUsageStats {
    pub totals: EvalUsageTotals,
    /// Totals keyed by the decision record's feature name.
    pub features: BTreeMap<String, EvalUsageTotals>,
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
    /// Client-observed round trip in milliseconds; backends don't report it,
    /// so callers fill it in after parsing.
    #[serde(default)]
    pub latency_ms: u64,
    /// Provider-specific metadata (e.g. TypeSafe's separate `confidence`
    /// statistic), passed through uninterpreted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub provider_metadata: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_missing_mirrors_each_backends_required_fields() {
        let mut settings = EvalSettings::default();
        assert!(settings.credential_missing());
        settings.typesafe_api_key = Some("key".to_owned());
        assert!(!settings.credential_missing());

        settings.backend = EvalBackend::VercelGateway;
        assert!(settings.credential_missing());
        settings.vercel_api_key = Some("key".to_owned());
        // The team id is a routing nicety, not a credential.
        assert!(!settings.credential_missing());

        settings.backend = EvalBackend::Cloudflare;
        assert!(settings.credential_missing());
        settings.cloudflare_account_id = Some("account".to_owned());
        assert!(settings.credential_missing());
        settings.cloudflare_api_token = Some("token".to_owned());
        assert!(!settings.credential_missing());

        // Whitespace alone does not count as configured.
        settings.cloudflare_api_token = Some("   ".to_owned());
        assert!(settings.credential_missing());
    }
}
