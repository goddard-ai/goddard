//! Inference providers: hosted token-inference services — TypeSafe, the
//! Vercel AI Gateway, Cloudflare Workers AI, OpenRouter — that features draw
//! on through one shared credential store. These are not coding-agent
//! harnesses (`crate::model::ProviderKind`): they answer API calls rather
//! than running sessions, so their setup is an API key plus a little
//! non-secret config, managed in one place and consumed by any feature that
//! needs a given [`InferenceContext`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A hosted inference provider. Each variant carries the static spec — which
/// contexts it can serve and which non-secret config its requests need — that
/// the settings pane and the daemon's request builders share.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS,
)]
#[serde(rename_all = "camelCase")]
pub enum InferenceProvider {
    /// TypeSafe's own API: `api.typesafe.ai` — evaluation calls only.
    #[default]
    TypeSafe,
    /// Vercel AI Gateway: evaluation, chat completions, and speech endpoints.
    VercelGateway,
    /// Cloudflare Workers AI: `ai/run` evaluation and chat models.
    Cloudflare,
    /// OpenRouter: chat completions plus an OpenAI-compatible speech endpoint.
    OpenRouter,
}

/// The kind of inference a caller needs. A feature's picker filters the
/// provider rail to the providers serving its context, and a provider's
/// model list shows only models valid under it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InferenceContext {
    /// Structured evaluation calls — the Jev contract (`state` plus typed
    /// `questions` returning calibrated probabilities). Served by a fixed
    /// model per provider, so this context never offers model rows.
    Eval,
    /// Chat-style text generation.
    Text,
    /// Text-to-speech.
    Speech,
}

/// One inference provider's settings entry in [`crate::settings::DaemonSettings`].
///
/// `api_key` is write-only in effect: a client submits a key to configure it
/// and the daemon moves the value into its secret store before the document
/// persists or broadcasts, so an emitted document always carries `None` —
/// status arrives as `credential_configured`. Submitting an empty string
/// removes the stored credential.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct InferenceProviderSettings {
    /// Client → daemon credential write. The daemon absorbs the value into
    /// its secret store on receipt, so this is only ever `Some` in a
    /// document a client is sending, never in one it received.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Daemon-maintained: whether the secret store holds this provider's
    /// credential. Client-sent values are discarded on update.
    pub credential_configured: bool,
    /// Non-secret provider config, keyed by the ids in
    /// [`InferenceProvider::required_config`] and
    /// [`InferenceProvider::optional_config`].
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, String>,
}

impl InferenceProvider {
    pub const ALL: [Self; 4] = [
        Self::TypeSafe,
        Self::VercelGateway,
        Self::Cloudflare,
        Self::OpenRouter,
    ];

    /// Stable id for logs, storage keys, and search filters.
    pub fn id(self) -> &'static str {
        match self {
            Self::TypeSafe => "typesafe",
            Self::VercelGateway => "vercel",
            Self::Cloudflare => "cloudflare",
            Self::OpenRouter => "openrouter",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::TypeSafe => "TypeSafe",
            Self::VercelGateway => "Vercel AI Gateway",
            Self::Cloudflare => "Cloudflare Workers AI",
            Self::OpenRouter => "OpenRouter",
        }
    }

    /// Where the user creates the credential.
    pub fn keys_url(self) -> &'static str {
        match self {
            Self::TypeSafe => "https://typesafe.ai",
            Self::VercelGateway => "https://vercel.com/docs/ai-gateway",
            Self::Cloudflare => {
                "https://developers.cloudflare.com/workers-ai/get-started/rest-api/"
            }
            Self::OpenRouter => "https://openrouter.ai/settings/keys",
        }
    }

    /// The contexts this provider can serve.
    pub fn contexts(self) -> &'static [InferenceContext] {
        match self {
            Self::TypeSafe => &[InferenceContext::Eval],
            Self::VercelGateway => &[
                InferenceContext::Eval,
                InferenceContext::Text,
                InferenceContext::Speech,
            ],
            Self::Cloudflare => &[InferenceContext::Eval, InferenceContext::Text],
            Self::OpenRouter => &[InferenceContext::Text, InferenceContext::Speech],
        }
    }

    pub fn supports(self, context: InferenceContext) -> bool {
        self.contexts().contains(&context)
    }

    /// `config` keys a request cannot be built without — a provider with any
    /// of these missing counts as unconfigured even when its credential is
    /// stored.
    pub fn required_config(self) -> &'static [&'static str] {
        match self {
            Self::Cloudflare => &["account_id"],
            _ => &[],
        }
    }

    /// `config` keys that refine requests but are not required.
    pub fn optional_config(self) -> &'static [&'static str] {
        match self {
            Self::VercelGateway => &["team_id"],
            _ => &[],
        }
    }

    /// The account name under which the daemon's secret store holds this
    /// provider's credential (keychain service `ai.goddard.integrations`,
    /// or the `secrets/` fallback file). Kept slash-free — the file
    /// fallback stores each key as one flat file.
    pub fn secret_key(self) -> String {
        format!("inference.{}", self.id())
    }
}

impl InferenceProviderSettings {
    /// Whether requests for this provider can be built: the credential is
    /// stored and every required `config` key carries a non-blank value.
    pub fn ready(&self, provider: InferenceProvider) -> bool {
        self.credential_configured
            && provider.required_config().iter().all(|field| {
                self.config
                    .get(*field)
                    .is_some_and(|value| !value.trim().is_empty())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_requires_credential_and_required_config() {
        let mut entry = InferenceProviderSettings::default();
        assert!(!entry.ready(InferenceProvider::TypeSafe));
        entry.credential_configured = true;
        assert!(entry.ready(InferenceProvider::TypeSafe));

        let mut entry = InferenceProviderSettings {
            credential_configured: true,
            ..Default::default()
        };
        assert!(!entry.ready(InferenceProvider::Cloudflare));
        entry
            .config
            .insert("account_id".to_owned(), "   ".to_owned());
        assert!(!entry.ready(InferenceProvider::Cloudflare));
        entry
            .config
            .insert("account_id".to_owned(), "acct".to_owned());
        assert!(entry.ready(InferenceProvider::Cloudflare));
    }

    #[test]
    fn api_key_is_write_only_in_effect() {
        // The field serializes only on the client's write path — a document
        // the daemon emitted has already been absorbed, so it carries `None`
        // and omits the key entirely.
        let stored = InferenceProviderSettings {
            credential_configured: true,
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&stored).unwrap(),
            serde_json::json!({"credentialConfigured": true})
        );
        let staged: InferenceProviderSettings =
            serde_json::from_value(serde_json::json!({"apiKey": "new"})).unwrap();
        assert_eq!(staged.api_key.as_deref(), Some("new"));
    }
}
