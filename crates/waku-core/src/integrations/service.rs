//! The integration service: catalog + settings state + credential store +
//! the local proxy. Owned by the backend. The proxy binds first, `Inner` is
//! built with its address, then the accept loop takes an `Arc<Inner>` — no
//! cycle.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, anyhow, bail};
use parking_lot::Mutex;
use uuid::Uuid;
use waku_protocol::integrations::{
    IntegrationAuthKind, IntegrationAuthState, IntegrationSetting, IntegrationSnapshot,
};
use waku_protocol::model::ProviderKind;

use super::catalog::{self};
use super::oauth::{self, StoredCredential};
use super::proxy;
use super::secrets::SecretStore;
use crate::EventSink;
use crate::driver::McpServerSpec;
use crate::settings::DaemonSettingsStore;

/// What the proxy needs to forward one request.
pub struct Upstream {
    pub url: String,
    pub auth_header: Option<String>,
}

pub struct Inner {
    settings: Arc<DaemonSettingsStore>,
    secrets: SecretStore,
    pub data_dir: PathBuf,
    proxy_address: SocketAddr,
    proxy_token: String,
    scoped_tokens: Mutex<HashMap<String, (Uuid, Vec<String>)>>,
    http_mcp_capable_providers: Mutex<HashSet<ProviderKind>>,
    http_mcp_capability_path: PathBuf,
}

impl Inner {
    pub fn permits(&self, token: &str, integration: &str) -> bool {
        token == self.proxy_token
            || self
                .scoped_tokens
                .lock()
                .get(token)
                .is_some_and(|(_, grants)| grants.iter().any(|id| id == integration))
    }

    /// Resolve `/mcp/<id>` to its upstream URL and credential header.
    /// `Ok(None)` means the integration is not connected.
    pub fn upstream(&self, id: &str) -> anyhow::Result<Option<Upstream>> {
        let settings = self.settings.get();
        let Some(setting) = settings.integrations.iter().find(|s| s.id == id) else {
            return Ok(None);
        };
        let entry = catalog::find(id).ok_or_else(|| anyhow!("unknown integration {id}"))?;
        let variant = entry
            .variant(&setting.variant_id)
            .unwrap_or_else(|| entry.default_variant());
        let auth_header =
            oauth::access_token(&self.secrets, id)?.map(|token| format!("Bearer {token}"));
        Ok(Some(Upstream {
            url: variant.url.to_owned(),
            auth_header,
        }))
    }
}

#[derive(Clone)]
pub struct IntegrationService {
    inner: Arc<Inner>,
    auth_inflight: Arc<Mutex<HashSet<String>>>,
}

impl IntegrationService {
    pub fn new(settings: Arc<DaemonSettingsStore>, data_dir: PathBuf) -> anyhow::Result<Self> {
        // Mint the per-install proxy bearer once; it persists in settings so
        // provider config files survive daemon restarts.
        let token = {
            let current = settings.get().integrations_proxy_token.clone();
            if !current.is_empty() {
                current
            } else {
                let minted = format!("gmi{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
                let mut document = settings.get();
                document.integrations_proxy_token = minted.clone();
                settings.replace(document)?;
                minted
            }
        };
        let http_mcp_capability_path = data_dir.join("integration-http-capable-providers.json");
        let http_mcp_capable_providers = load_http_mcp_capable_providers(&http_mcp_capability_path);
        let listener = proxy::bind()?;
        let inner = Arc::new(Inner {
            settings,
            secrets: SecretStore::new(data_dir.clone()),
            data_dir,
            proxy_address: listener.local_addr()?,
            proxy_token: token,
            scoped_tokens: Mutex::new(HashMap::new()),
            http_mcp_capable_providers: Mutex::new(http_mcp_capable_providers),
            http_mcp_capability_path,
        });
        proxy::run(listener, inner.clone());
        Ok(Self {
            inner,
            auth_inflight: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Base URL agents use for one connected integration.
    pub fn endpoint_url(&self, id: &str) -> String {
        format!("http://{}/mcp/{id}", self.inner.proxy_address)
    }

    pub fn proxy_token(&self) -> &str {
        &self.inner.proxy_token
    }

    /// The catalog joined with the user's current configuration.
    pub fn snapshots(&self) -> Vec<IntegrationSnapshot> {
        let settings = self.inner.settings.get();
        catalog::catalog()
            .iter()
            .map(|entry| IntegrationSnapshot {
                info: entry.info(),
                configured: settings
                    .integrations
                    .iter()
                    .find(|s| s.id == entry.id)
                    .cloned(),
            })
            .collect()
    }

    /// Integrations a provider launch should receive, in the uniform
    /// `goddard_<id>` + local-URL shape. Empty while the experiment is off.
    pub(crate) fn launch_mcp_servers(&self, provider: ProviderKind) -> Vec<McpServerSpec> {
        if super::deliver::uses_file_sync(provider) && !super::deliver::uses_acp(provider) {
            return Vec::new();
        }
        let settings = self.inner.settings.get();
        if !settings.integrations_enabled {
            return Vec::new();
        }
        settings
            .integrations
            .iter()
            .filter(|setting| setting.providers.contains(&provider))
            .map(|setting| {
                McpServerSpec::http(
                    super::server_name(&setting.id),
                    self.endpoint_url(&setting.id),
                    self.inner.proxy_token.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn scoped_mcp_servers(&self, task: Uuid, grants: &[String]) -> Vec<McpServerSpec> {
        let settings = self.inner.settings.get();
        self.revoke_task(task);
        let token = format!("gms{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.inner.scoped_tokens.lock().insert(
            token.clone(),
            (
                task,
                if settings.integrations_enabled {
                    grants.to_vec()
                } else {
                    Vec::new()
                },
            ),
        );
        // Override every managed entry, including denied ones inherited from
        // provider files. The proxy denies endpoints outside this token's grants.
        settings
            .integrations
            .iter()
            .map(|setting| {
                McpServerSpec::http(
                    super::server_name(&setting.id),
                    self.endpoint_url(&setting.id),
                    token.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn revoke_task(&self, task: Uuid) {
        self.inner
            .scoped_tokens
            .lock()
            .retain(|_, (owner, _)| *owner != task);
    }

    pub(crate) fn http_mcp_supported(&self, provider: ProviderKind) -> bool {
        self.inner
            .http_mcp_capable_providers
            .lock()
            .contains(&provider)
    }

    pub(crate) fn record_http_mcp_capability(
        &self,
        provider: ProviderKind,
        supported: bool,
    ) -> anyhow::Result<()> {
        if !super::deliver::uses_acp(provider) {
            return Ok(());
        }
        let provider_ids = {
            let mut capable = self.inner.http_mcp_capable_providers.lock();
            let changed = if supported {
                capable.insert(provider)
            } else {
                capable.remove(&provider)
            };
            if !changed {
                return Ok(());
            }
            let mut ids = capable
                .iter()
                .map(|provider| provider.id().to_owned())
                .collect::<Vec<_>>();
            ids.sort();
            ids
        };
        let persisted = super::deliver::write_atomic_json(
            &self.inner.http_mcp_capability_path,
            &serde_json::json!(provider_ids),
        );
        super::deliver::sync_file_provider(provider, &self.inner.settings.get(), self);
        persisted
    }

    /// Record a connection choice. For API-key entries the key is stored and
    /// the integration is immediately Connected; for OAuth entries the flow
    /// starts in the background and `auth` flips when it lands.
    pub fn connect(
        &self,
        id: &str,
        variant_id: &str,
        providers: Vec<ProviderKind>,
        api_key: Option<String>,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        let entry = catalog::find(id).ok_or_else(|| anyhow!("unknown integration {id}"))?;
        let variant = entry
            .variant(variant_id)
            .or_else(|| entry.variants.first())
            .ok_or_else(|| anyhow!("integration {id} has no variants"))?;
        let auth = if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
            let credential = serde_json::to_string(&StoredCredential::ApiKey { key })?;
            self.inner.secrets.store(id, &credential)?;
            IntegrationAuthState::Connected
        } else {
            match entry.auth {
                IntegrationAuthKind::ApiKey => bail!("{} requires an API key", entry.name),
                _ => IntegrationAuthState::NeedsAuth,
            }
        };
        self.update_settings(|settings| {
            settings.integrations.retain(|s| s.id != id);
            settings.integrations.push(IntegrationSetting {
                id: id.to_owned(),
                variant_id: variant.id.to_owned(),
                providers,
                auth,
            });
        })?;
        events.settings_changed(self.inner.settings.get());
        if auth == IntegrationAuthState::NeedsAuth {
            self.begin_auth(id, events)?;
        }
        Ok(())
    }

    /// Re-point an existing connection at a new provider set.
    pub fn set_providers(&self, id: &str, providers: Vec<ProviderKind>) -> anyhow::Result<()> {
        self.update_settings(|settings| {
            if let Some(setting) = settings.integrations.iter_mut().find(|s| s.id == id) {
                setting.providers = providers;
            }
        })
    }

    /// Forget the integration entirely: settings entry and stored credential.
    /// Provider config cleanup is the delivery layer's job.
    pub fn disconnect(&self, id: &str) -> anyhow::Result<()> {
        self.update_settings(|settings| {
            settings.integrations.retain(|s| s.id != id);
        })?;
        self.inner.secrets.remove(id);
        Ok(())
    }

    /// Start the OAuth browser flow on a background thread. On success the
    /// credential lands in the secret store, `auth` flips to Connected, and
    /// every client hears it through `SettingsChanged`.
    pub fn begin_auth(&self, id: &str, events: &EventSink) -> anyhow::Result<()> {
        if !self.auth_inflight.lock().insert(id.to_owned()) {
            return Ok(());
        }
        let inner = self.inner.clone();
        let inflight = self.auth_inflight.clone();
        let id_owned = id.to_owned();
        let events = events.clone();
        std::thread::Builder::new()
            .name(format!("goddard-mcp-auth-{id}"))
            .spawn(move || {
                let outcome = (|| {
                    let entry = catalog::find(&id_owned)
                        .ok_or_else(|| anyhow!("unknown integration {id_owned}"))?;
                    let variant_id = inner
                        .settings
                        .get()
                        .integrations
                        .iter()
                        .find(|s| s.id == id_owned)
                        .map(|s| s.variant_id.clone())
                        .unwrap_or_else(|| entry.default_variant().id.to_owned());
                    let variant = entry
                        .variant(&variant_id)
                        .unwrap_or_else(|| entry.default_variant());
                    oauth::authorize(&inner.secrets, &inner.data_dir, entry, variant)
                })();
                inflight.lock().remove(&id_owned);
                match outcome {
                    Ok(_) => {
                        let mut document = inner.settings.get();
                        if let Some(setting) =
                            document.integrations.iter_mut().find(|s| s.id == id_owned)
                        {
                            setting.auth = IntegrationAuthState::Connected;
                        }
                        if let Err(error) = inner.settings.replace(document) {
                            eprintln!("goddard-mcp: could not persist auth state: {error:#}");
                        }
                    }
                    Err(error) => {
                        eprintln!("goddard-mcp: authorization for {id_owned} failed: {error:#}");
                    }
                }
                // The request that started the flow already answered; every
                // client needs this change.
                events
                    .with_source_subscriber(u64::MAX)
                    .settings_changed(inner.settings.get());
            })
            .context("could not start the authorization thread")?;
        Ok(())
    }

    fn update_settings(
        &self,
        mutate: impl FnOnce(&mut waku_protocol::DaemonSettings),
    ) -> anyhow::Result<()> {
        let mut document = self.inner.settings.get();
        mutate(&mut document);
        self.inner.settings.replace(document)?;
        Ok(())
    }
}

fn load_http_mcp_capable_providers(path: &Path) -> HashSet<ProviderKind> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return HashSet::new(),
        Err(error) => {
            eprintln!(
                "goddard-mcp: could not read HTTP capability cache {}: {error}",
                path.display()
            );
            return HashSet::new();
        }
    };
    let ids = match serde_json::from_slice::<Vec<String>>(&bytes) {
        Ok(ids) => ids,
        Err(error) => {
            eprintln!(
                "goddard-mcp: could not parse HTTP capability cache {}: {error}",
                path.display()
            );
            return HashSet::new();
        }
    };
    let ids = ids.into_iter().collect::<HashSet<_>>();
    ProviderKind::ALL
        .iter()
        .copied()
        .filter(|provider| super::deliver::uses_acp(*provider) && ids.contains(provider.id()))
        .collect()
}

#[cfg(test)]
mod boss_tests {
    use super::*;

    #[test]
    fn boss_scoped_mcp_credentials_limit_endpoints_and_expire() {
        let root = std::env::temp_dir().join(format!("boss-mcp-{}", Uuid::new_v4()));
        let settings = Arc::new(DaemonSettingsStore::open(root.join("settings.json")).unwrap());
        let mut document = settings.get();
        document.integrations_enabled = true;
        settings.replace(document).unwrap();
        let service = IntegrationService::new(settings.clone(), root.clone()).unwrap();
        let task = Uuid::new_v4();
        service.scoped_mcp_servers(task, &["linear".into()]);
        let token = service
            .inner
            .scoped_tokens
            .lock()
            .keys()
            .next()
            .unwrap()
            .clone();
        assert!(service.inner.permits(&token, "linear"));
        assert!(!service.inner.permits(&token, "github"));
        assert!(!service.inner.permits("unknown", "linear"));
        assert!(service.inner.permits(service.proxy_token(), "github"));
        service.revoke_task(task);
        assert!(!service.inner.permits(&token, "linear"));
        let mut document = settings.get();
        document.integrations_enabled = false;
        settings.replace(document).unwrap();
        service.scoped_mcp_servers(task, &["linear".into()]);
        let disabled_token = service
            .inner
            .scoped_tokens
            .lock()
            .keys()
            .next()
            .unwrap()
            .clone();
        assert!(!service.inner.permits(&disabled_token, "linear"));
        let _ = std::fs::remove_dir_all(root);
    }
}
