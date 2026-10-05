//! Compatibility exports and the daemon settings adapter for Boss rotation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use waku_protocol::model::ProviderKind;

pub use waku_boss::boss_rotation::{
    BossRotationConfig as BossRotationPolicy, BossRotationSettings, RotationIntent, RotationJournal,
};

/// Compatibility configuration with the daemon settings constructor retained.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BossRotationConfig {
    pub context_threshold: f64,
    pub provider_cache_ttl_secs: HashMap<ProviderKind, u64>,
}

impl Default for BossRotationConfig {
    fn default() -> Self {
        let policy = BossRotationPolicy::default();
        Self {
            context_threshold: policy.context_threshold,
            provider_cache_ttl_secs: policy.provider_cache_ttl_secs,
        }
    }
}

impl BossRotationConfig {
    pub fn from_settings(settings: &crate::DaemonSettings) -> Self {
        Self {
            context_threshold: settings.boss_rotation_context_threshold,
            provider_cache_ttl_secs: settings
                .boss_rotation_cache_ttl_secs
                .iter()
                .map(|(provider, ttl)| (*provider, *ttl))
                .collect(),
        }
    }

    pub fn should_rotate(
        &self,
        provider: ProviderKind,
        context_tokens: u64,
        context_window: Option<u64>,
        cache_key_matches: bool,
        last_cache_refresh_at: Option<u64>,
        now: u64,
    ) -> bool {
        BossRotationPolicy {
            context_threshold: self.context_threshold,
            provider_cache_ttl_secs: self.provider_cache_ttl_secs.clone(),
        }
        .should_rotate(
            provider,
            context_tokens,
            context_window,
            cache_key_matches,
            last_cache_refresh_at,
            now,
        )
    }
}
