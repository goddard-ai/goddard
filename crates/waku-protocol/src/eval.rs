//! Hosted evaluation-model calls (TypeSafe's Jev today): a shared `state` is
//! scored against typed questions and answers come back as calibrated
//! probabilities rather than generated text. The daemon owns the HTTP calls;
//! these are the wire and settings shapes every eval feature shares.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::inference::{InferenceContext, InferenceProvider, InferenceProviderSettings};

/// Bring-your-own-key evaluation configuration, stored in daemon settings.
/// `provider` selects among the inference providers serving the `eval`
/// context; credentials and non-secret provider config live in
/// [`crate::settings::DaemonSettings::inference`].
///
/// The remaining `Option<String>` fields are write-only-in-effect staging
/// slots, like [`InferenceProviderSettings::api_key`]: a client sends them
/// (an unsaved credential staged for `testEvalConnection`, or a document
/// written before the provider section existed) and the daemon's absorb
/// pass moves them into the secret store and provider `config` before the
/// document persists or broadcasts, so a received document always reads
/// `None`. Internally the daemon hydrates them back from the store.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct EvalSettings {
    #[serde(alias = "backend")]
    pub provider: InferenceProvider,
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
    /// Whether a hydrated settings object still lacks a field its provider's
    /// requests require. Only meaningful after the daemon has hydrated the
    /// staging slots from the secret store — a settings document as received
    /// always reads missing.
    pub fn credential_missing(&self) -> bool {
        let missing = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        };
        match self.provider {
            InferenceProvider::TypeSafe => missing(&self.typesafe_api_key),
            InferenceProvider::VercelGateway => missing(&self.vercel_api_key),
            InferenceProvider::Cloudflare => {
                missing(&self.cloudflare_account_id) || missing(&self.cloudflare_api_token)
            }
            InferenceProvider::OpenRouter => true,
        }
    }

    /// Whether eval calls can run for the selected provider, judged from the
    /// daemon-maintained provider section — the client-side counterpart of
    /// `credential_missing`, which only sees hydrated values.
    pub fn ready(
        &self,
        inference: &BTreeMap<InferenceProvider, InferenceProviderSettings>,
    ) -> bool {
        self.provider.supports(InferenceContext::Eval)
            && inference
                .get(&self.provider)
                .is_some_and(|entry| entry.ready(self.provider))
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
    /// Session starts each task class routed: `route` records whose reason
    /// applied a class map. Counts the applied class, including older
    /// planning overrides that launched on Hard rather than the answered class.
    pub route_class_counts: BTreeMap<crate::routing::TaskClass, u64>,
    /// Mid-session routes through each provider's own class map —
    /// provider → class → count — recorded by `route-class` records.
    pub provider_route_class_counts:
        BTreeMap<crate::model::ProviderKind, BTreeMap<crate::routing::TaskClass, u64>>,
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

/// The settled-turn ending classification, shared between the app's status
/// markers and daemon-side settle hooks so the same turn reads identically
/// whichever surface evaluates it.
pub const TURN_ENDING_QUESTION: &str = "ending";
/// The bare fallback bucket on the ending question.
pub const TURN_ENDING_OTHER: &str = "other";
/// The information-only ending: the prompt asked for information and the
/// response delivered it without making changes.
pub const TURN_ENDING_ANSWERED: &str = "answered";
/// The bar `answered` must clear — the existing status-marker threshold.
pub const TURN_ENDING_ANSWERED_THRESHOLD: f64 = 0.60;

pub const ENDING_COMPLETE: &str = "The prompt asked for a change or action, the work was carried \
    out end to end, and the final response reports it done — judge the ask \
    as the `prompt` plus any `priorPrompts` still in play. An ask satisfied \
    entirely by information is `answered`; a reported 'no work needed' is \
    `nothing-to-do`; a declined or redirected ask is `pushed-back`.";
pub const ENDING_ANSWERED: &str = "The prompt asked for information — a question, an explanation, \
    or an investigation — and the response delivers that answer, findings, \
    or analysis without making changes. If the prompt also requested work \
    that was carried out, choose `complete` instead; if the response mainly \
    restates the request and waits for confirmation to do the work, choose \
    `aligned`.";
pub const ENDING_NOTHING_TO_DO: &str = "The prompt asked for work, but the assistant investigated and \
    reported that none was needed — the request was already satisfied, the \
    problem does not reproduce, or there is nothing to change.";
pub const ENDING_PUSHED_BACK: &str = "The assistant declined the request as asked or redirected it — \
    refused, disputed the premise, or recommended a different approach — \
    without asking the user anything. If it asked for approval, a decision, \
    or details first, choose `awaiting-input`.";
pub const ENDING_AWAITING_INPUT: &str = "The requested work remains unfinished because the assistant \
    asks the user for a go-ahead, a decision, missing details, or an action \
    only the user can take before continuing. A rhetorical question after \
    completed work does not count. A reply whose substance is replaying \
    the request to confirm shared understanding is `aligned` instead.";
pub const ENDING_ALIGNED: &str = "The assistant's reply restates the user's request — it played \
    back its understanding or recapped what it intends to do — and stops \
    there, waiting for a green light to carry out the work; the only pending \
    input is confirmation. A question needing more than a yes — a decision, \
    missing details, or approval of a specific proposed action — is \
    `awaiting-input`; a reply that delivers the requested information is \
    `answered`.";
pub const ENDING_PARTIAL: &str = "The turn stopped before the work was finished — cut off by a \
    context or step limit (`contextUsage` near its `window` is direct \
    evidence), truncated output, or an explicit promise to continue later.";

/// `(option id, rubric text)` pairs for [`turn_ending_question`], in the
/// app's status-marker order.
pub const TURN_ENDING_OPTIONS: &[(&str, &str)] = &[
    ("complete", ENDING_COMPLETE),
    (TURN_ENDING_ANSWERED, ENDING_ANSWERED),
    ("nothing-to-do", ENDING_NOTHING_TO_DO),
    ("pushed-back", ENDING_PUSHED_BACK),
    ("awaiting-input", ENDING_AWAITING_INPUT),
    ("aligned", ENDING_ALIGNED),
    ("partial", ENDING_PARTIAL),
];

/// The `ending` Choice question every settled-turn evaluation asks.
pub fn turn_ending_question() -> EvalQuestion {
    EvalQuestion::Choice {
        instructions: "Which best describes how this turn ended? Judge the closing \
            `response` against the `prompt` and any `priorPrompts` still in play; \
            `toolSequence`, `toolErrors`, and `filesChanged` carry what the turn \
            actually did. `answered` is only for asks satisfied entirely by \
            information — a prompt that requested work ends `complete` or \
            `partial` even when the response also explains."
            .to_owned(),
        criteria: TURN_ENDING_OPTIONS
            .iter()
            .map(|(id, text)| (id.to_string(), Some(text.to_string())))
            .chain([(TURN_ENDING_OTHER.to_string(), None)])
            .collect(),
    }
}

/// The winning option's probability for an `ending` answer.
pub fn turn_ending_cleared(answer: &EvalAnswer, option: &str, threshold: f64) -> bool {
    matches!(
        answer,
        EvalAnswer::Choice {
            choice,
            probabilities,
            ..
        } if choice == option && probabilities.get(option).is_some_and(|p| *p >= threshold)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_missing_mirrors_each_providers_required_fields() {
        let mut settings = EvalSettings::default();
        assert!(settings.credential_missing());
        settings.typesafe_api_key = Some("key".to_owned());
        assert!(!settings.credential_missing());

        settings.provider = InferenceProvider::VercelGateway;
        assert!(settings.credential_missing());
        settings.vercel_api_key = Some("key".to_owned());
        // The team id is a routing nicety, not a credential.
        assert!(!settings.credential_missing());

        settings.provider = InferenceProvider::Cloudflare;
        assert!(settings.credential_missing());
        settings.cloudflare_account_id = Some("account".to_owned());
        assert!(settings.credential_missing());
        settings.cloudflare_api_token = Some("token".to_owned());
        assert!(!settings.credential_missing());

        // Whitespace alone does not count as configured.
        settings.cloudflare_api_token = Some("   ".to_owned());
        assert!(settings.credential_missing());

        // OpenRouter cannot serve evaluations at all.
        settings.provider = InferenceProvider::OpenRouter;
        assert!(settings.credential_missing());
    }

    #[test]
    fn staged_fields_serialize_for_the_daemon_and_backend_alias_reads() {
        // A client's write path carries staged fields so a probe can test an
        // unsaved key; a daemon-emitted document has already had them
        // absorbed into the secret store and reads `None`.
        let settings = EvalSettings {
            provider: InferenceProvider::VercelGateway,
            vercel_api_key: Some("key".to_owned()),
            vercel_team_id: Some("team".to_owned()),
            ..Default::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "provider": "vercelGateway",
                "vercelApiKey": "key",
                "vercelTeamId": "team",
            })
        );
        let stored = EvalSettings {
            provider: InferenceProvider::VercelGateway,
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&stored).unwrap(),
            serde_json::json!({"provider": "vercelGateway"})
        );

        let parsed: EvalSettings =
            serde_json::from_str(r#"{"backend": "cloudflare", "cloudflareApiToken": "t"}"#)
                .unwrap();
        assert_eq!(parsed.provider, InferenceProvider::Cloudflare);
        assert_eq!(parsed.cloudflare_api_token.as_deref(), Some("t"));
    }

    #[test]
    fn readiness_uses_the_provider_section() {
        let settings = EvalSettings {
            provider: InferenceProvider::Cloudflare,
            ..Default::default()
        };
        let mut inference = BTreeMap::new();
        assert!(!settings.ready(&inference));
        inference.insert(
            InferenceProvider::Cloudflare,
            InferenceProviderSettings {
                credential_configured: true,
                config: BTreeMap::from([("account_id".to_owned(), "acct".to_owned())]),
                ..Default::default()
            },
        );
        assert!(settings.ready(&inference));

        // A provider that cannot serve eval is never ready.
        let settings = EvalSettings {
            provider: InferenceProvider::OpenRouter,
            ..Default::default()
        };
        inference.insert(
            InferenceProvider::OpenRouter,
            InferenceProviderSettings {
                credential_configured: true,
                ..Default::default()
            },
        );
        assert!(!settings.ready(&inference));
    }
}
