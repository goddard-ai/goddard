//! Inference-provider plumbing inside the daemon: the secret-store handshake
//! between the settings document and the providers' API calls.
//!
//! Credentials never live in `DaemonSettings` — the document's write-only
//! `api_key` slots and the legacy eval credential fields land here, move into
//! the [`SecretStore`], and report back as `credential_configured` flags.
//! Eval calls then run on a hydrated `EvalSettings`: the provider pick plus
//! the credential and config resolved at the boundary.

use anyhow::Context as _;
use waku_protocol::eval::EvalSettings;
use waku_protocol::inference::{InferenceContext, InferenceProvider};
use waku_protocol::settings::DaemonSettings;

use crate::integrations::secrets::SecretStore;

/// Move every write-only credential out of a settings document: each
/// provider entry's `api_key` goes to the secret store (an empty string
/// removes it), and the legacy eval fields — the shape documents and clients
/// predating the provider section still send — migrate into the same places.
/// Then rebuild the daemon-maintained `credential_configured` flags.
///
/// Returns whether the document changed, so startup migration only rewrites
/// `settings.json` when it moved something.
pub fn absorb(settings: &mut DaemonSettings, secrets: &SecretStore) -> bool {
    let mut changed = false;

    // Provider-section writes: `Some(key)` stores, `Some("")` clears.
    for (provider, entry) in settings.inference.iter_mut() {
        if let Some(key) = entry.api_key.take() {
            match key.trim() {
                "" => secrets.remove(&provider.secret_key()),
                key => {
                    if let Err(error) = secrets.store(&provider.secret_key(), key) {
                        eprintln!(
                            "Goddard: could not store the {} credential: {error:#}",
                            provider.display_name()
                        );
                    }
                }
            }
            changed = true;
        }
    }

    // Legacy eval fields: written into this document before the provider
    // section existed, or staged by a client that still sends them.
    if let Some(eval) = settings.eval.as_mut() {
        for (provider, key) in [
            (InferenceProvider::TypeSafe, eval.typesafe_api_key.take()),
            (InferenceProvider::VercelGateway, eval.vercel_api_key.take()),
            (
                InferenceProvider::Cloudflare,
                eval.cloudflare_api_token.take(),
            ),
        ] {
            match key {
                Some(key) if !key.trim().is_empty() => {
                    if let Err(error) = secrets.store(&provider.secret_key(), key.trim()) {
                        eprintln!(
                            "Goddard: could not migrate the {} credential: {error:#}",
                            provider.display_name()
                        );
                    }
                    changed = true;
                }
                Some(_) => changed = true,
                None => {}
            }
        }
        for (provider, field, value) in [
            (
                InferenceProvider::VercelGateway,
                "team_id",
                eval.vercel_team_id.take(),
            ),
            (
                InferenceProvider::Cloudflare,
                "account_id",
                eval.cloudflare_account_id.take(),
            ),
        ] {
            match value {
                Some(value) if !value.trim().is_empty() => {
                    settings
                        .inference
                        .entry(provider)
                        .or_default()
                        .config
                        .insert(field.to_owned(), value.trim().to_owned());
                    changed = true;
                }
                Some(_) => changed = true,
                None => {}
            }
        }
    }

    // Rebuild the daemon-maintained flags. Entries only exist for providers
    // with state to report — a configured credential or non-secret config —
    // so the document stays sparse.
    for provider in InferenceProvider::ALL {
        let configured = secrets
            .read(&provider.secret_key())
            .is_some_and(|key| !key.trim().is_empty());
        match settings.inference.get_mut(&provider) {
            Some(entry) => {
                if entry.credential_configured != configured {
                    entry.credential_configured = configured;
                    changed = true;
                }
            }
            None if configured => {
                settings.inference.insert(
                    provider,
                    waku_protocol::inference::InferenceProviderSettings {
                        credential_configured: true,
                        ..Default::default()
                    },
                );
                changed = true;
            }
            None => {}
        }
    }
    changed
}

/// Fill an eval settings' staging slots from the provider section and the
/// secret store. The object's own values win — a `testEvalConnection` probe
/// stages an unsaved credential this way — so only absent fields hydrate.
pub fn hydrate_eval(eval: &mut EvalSettings, settings: &DaemonSettings, secrets: &SecretStore) {
    let provider = eval.provider;
    let stored = || secrets.read(&provider.secret_key());
    let staged = |slot: &mut Option<String>| {
        *slot = slot.take().filter(|v| !v.trim().is_empty()).or_else(stored)
    };
    let config = settings.inference.get(&provider);
    match provider {
        InferenceProvider::TypeSafe => staged(&mut eval.typesafe_api_key),
        InferenceProvider::VercelGateway => {
            staged(&mut eval.vercel_api_key);
            if eval.vercel_team_id.is_none() {
                eval.vercel_team_id = config
                    .and_then(|entry| entry.config.get("team_id").cloned())
                    .filter(|value| !value.trim().is_empty());
            }
        }
        InferenceProvider::Cloudflare => {
            staged(&mut eval.cloudflare_api_token);
            if eval.cloudflare_account_id.is_none() {
                eval.cloudflare_account_id = config
                    .and_then(|entry| entry.config.get("account_id").cloned())
                    .filter(|value| !value.trim().is_empty());
            }
        }
        // No eval endpoint exists here; the arm exists so a staged probe
        // reports the provider's real failure rather than a missing field.
        InferenceProvider::OpenRouter => {}
    }
}

/// The eval settings runnable calls need: provider pick resolved, credential
/// and config hydrated from the store. `None` when the pick cannot serve
/// evaluations or still lacks a required field — the same "degrade to the
/// default" contract `credential_missing` enforced on the old layout.
pub fn resolve_eval(settings: &DaemonSettings, secrets: &SecretStore) -> Option<EvalSettings> {
    let mut eval = settings.eval.clone()?;
    if !eval.provider.supports(InferenceContext::Eval) {
        return None;
    }
    hydrate_eval(&mut eval, settings, secrets);
    (!eval.credential_missing()).then_some(eval)
}

/// One provider's stored credential, for clients that run provider traffic
/// themselves (the native app's voice briefings).
pub fn read_credential(secrets: &SecretStore, provider: InferenceProvider) -> Option<String> {
    secrets
        .read(&provider.secret_key())
        .filter(|key| !key.trim().is_empty())
}

/// Store or clear one provider's credential — the app-side migration path
/// hands the daemon a key it found in app state this way.
pub fn write_credential(
    secrets: &SecretStore,
    provider: InferenceProvider,
    key: &str,
) -> anyhow::Result<()> {
    if key.trim().is_empty() {
        secrets.remove(&provider.secret_key());
        Ok(())
    } else {
        secrets
            .store(&provider.secret_key(), key.trim())
            .with_context(|| format!("could not store the {} credential", provider.display_name()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::inference::InferenceProviderSettings;
    use waku_protocol::settings::DaemonSettings;

    fn store() -> (std::path::PathBuf, SecretStore) {
        let dir = std::env::temp_dir().join(format!("waku-inference-{}", uuid::Uuid::new_v4()));
        let secrets = SecretStore::file_only(dir.clone());
        (dir, secrets)
    }

    #[test]
    fn absorb_moves_submitted_keys_into_the_store() {
        let (_dir, secrets) = store();
        let mut settings = DaemonSettings::default();
        settings.inference.insert(
            InferenceProvider::VercelGateway,
            InferenceProviderSettings {
                api_key: Some("vck_test".to_owned()),
                ..Default::default()
            },
        );
        assert!(absorb(&mut settings, &secrets));
        // The key left the document and the flag reports the store.
        let entry = &settings.inference[&InferenceProvider::VercelGateway];
        assert!(entry.api_key.is_none());
        assert!(entry.credential_configured);
        assert_eq!(
            secrets
                .read(&InferenceProvider::VercelGateway.secret_key())
                .as_deref(),
            Some("vck_test")
        );
    }

    #[test]
    fn absorb_empty_key_clears_the_stored_credential() {
        let (_dir, secrets) = store();
        secrets
            .store(&InferenceProvider::TypeSafe.secret_key(), "ts_test")
            .unwrap();
        let mut settings = DaemonSettings::default();
        settings.inference.insert(
            InferenceProvider::TypeSafe,
            InferenceProviderSettings {
                api_key: Some(String::new()),
                credential_configured: true,
                ..Default::default()
            },
        );
        assert!(absorb(&mut settings, &secrets));
        assert!(
            secrets
                .read(&InferenceProvider::TypeSafe.secret_key())
                .is_none()
        );
        assert!(!settings.inference[&InferenceProvider::TypeSafe].credential_configured);
    }

    #[test]
    fn absorb_migrates_legacy_eval_fields() {
        let (_dir, secrets) = store();
        let mut settings = DaemonSettings::default();
        settings.eval = Some(EvalSettings {
            provider: InferenceProvider::Cloudflare,
            cloudflare_api_token: Some("cf_tok".to_owned()),
            cloudflare_account_id: Some("acct_1".to_owned()),
            ..Default::default()
        });
        assert!(absorb(&mut settings, &secrets));
        let eval = settings.eval.as_ref().unwrap();
        assert!(eval.cloudflare_api_token.is_none());
        assert!(eval.cloudflare_account_id.is_none());
        assert_eq!(
            secrets
                .read(&InferenceProvider::Cloudflare.secret_key())
                .as_deref(),
            Some("cf_tok")
        );
        let entry = &settings.inference[&InferenceProvider::Cloudflare];
        assert!(entry.credential_configured);
        assert_eq!(
            entry.config.get("account_id").map(String::as_str),
            Some("acct_1")
        );
        // A second pass is a no-op — migration is idempotent.
        assert!(!absorb(&mut settings, &secrets));
    }

    #[test]
    fn absorb_overwrites_a_client_sent_configured_flag() {
        let (_dir, secrets) = store();
        let mut settings = DaemonSettings::default();
        settings.inference.insert(
            InferenceProvider::OpenRouter,
            InferenceProviderSettings {
                credential_configured: true,
                ..Default::default()
            },
        );
        assert!(absorb(&mut settings, &secrets));
        assert!(!settings.inference[&InferenceProvider::OpenRouter].credential_configured);
    }

    #[test]
    fn resolve_eval_hydrates_and_rejects_unservable_providers() {
        let (_dir, secrets) = store();
        secrets
            .store(&InferenceProvider::VercelGateway.secret_key(), "vck_test")
            .unwrap();
        let mut settings = DaemonSettings::default();
        settings.eval = Some(EvalSettings {
            provider: InferenceProvider::VercelGateway,
            ..Default::default()
        });
        absorb(&mut settings, &secrets);

        let eval = resolve_eval(&settings, &secrets).unwrap();
        assert_eq!(eval.vercel_api_key.as_deref(), Some("vck_test"));

        settings.eval.as_mut().unwrap().provider = InferenceProvider::OpenRouter;
        assert!(resolve_eval(&settings, &secrets).is_none());
    }
}
