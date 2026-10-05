//! Store-independent policy and durable intent records for Boss rotation.
//!
//! Rotation stays dormant unless `enabled` is explicitly set. The daemon owns
//! provider session creation; this module makes the trigger and the journal
//! decision deterministic and restart-readable.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::model::ProviderKind;

/// Snapshot of the daemon settings used by Boss rotation policy.
pub trait BossRotationSettings {
    fn boss_rotation_enabled(&self) -> bool;
    fn boss_rotation_context_threshold(&self) -> f64;
    fn boss_rotation_cache_ttls(&self) -> Vec<(ProviderKind, u64)>;
}

const DEFAULT_CACHE_TTL_SECS: u64 = 5 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BossRotationConfig {
    pub enabled: bool,
    /// Rotate once context reaches this fraction of the reported window.
    pub context_threshold: f64,
    /// Provider cache TTLs in seconds. A value of zero means no prompt cache.
    pub provider_cache_ttl_secs: HashMap<ProviderKind, u64>,
}

impl Default for BossRotationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            context_threshold: 0.8,
            provider_cache_ttl_secs: HashMap::new(),
        }
    }
}

impl BossRotationConfig {
    pub fn from_settings(settings: &impl BossRotationSettings) -> Self {
        Self {
            enabled: settings.boss_rotation_enabled(),
            context_threshold: settings.boss_rotation_context_threshold(),
            provider_cache_ttl_secs: settings.boss_rotation_cache_ttls().into_iter().collect(),
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
        if !self.enabled || !self.context_threshold.is_finite() {
            return false;
        }
        let Some(window) = context_window.filter(|window| *window > 0) else {
            return false;
        };
        if context_tokens as f64 / window as f64 <= self.context_threshold.clamp(0.0, 1.0) {
            return false;
        }
        let ttl = self
            .provider_cache_ttl_secs
            .get(&provider)
            .copied()
            .unwrap_or(DEFAULT_CACHE_TTL_SECS);
        if ttl == 0 {
            return true;
        }
        let Some(refreshed) = last_cache_refresh_at.filter(|at| *at <= now) else {
            return true;
        };
        !cache_key_matches || now.saturating_sub(refreshed) >= ttl
    }
}

/// A replayable rotation intent. `new_session_id` is only published after the
/// caller has durably created and initialized the provider session.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationIntent {
    pub boss_id: Uuid,
    pub old_session_id: Uuid,
    pub old_generation: u64,
    pub new_session_id: Uuid,
    pub created_at: u64,
    pub committed_at: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RotationJournal {
    pub generation: u64,
    pub active_session_id: Option<Uuid>,
    pub intent: Option<RotationIntent>,
}

impl RotationJournal {
    /// Persist an intent before staging the new session. Existing unresolved
    /// intents are rejected so recovery cannot silently lose a generation.
    pub fn begin(
        &mut self,
        boss_id: Uuid,
        old_session_id: Uuid,
        new_session_id: Uuid,
        now: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.intent.is_none(), "a Boss rotation is already pending");
        anyhow::ensure!(
            self.active_session_id == Some(old_session_id),
            "Boss session pointer changed before rotation"
        );
        self.intent = Some(RotationIntent {
            boss_id,
            old_session_id,
            old_generation: self.generation,
            new_session_id,
            created_at: now,
            committed_at: None,
        });
        Ok(())
    }

    /// Complete the compare-and-swap after session initialization succeeds.
    pub fn commit(&mut self, now: u64) -> anyhow::Result<Uuid> {
        let intent = self
            .intent
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no Boss rotation is pending"))?;
        anyhow::ensure!(
            self.generation == intent.old_generation
                && self.active_session_id == Some(intent.old_session_id),
            "Boss rotation lost its generation compare-and-swap"
        );
        intent.committed_at = Some(now);
        let next = intent.new_session_id;
        self.active_session_id = Some(next);
        self.generation = self.generation.saturating_add(1);
        self.intent = None;
        Ok(next)
    }

    /// Abandon an uncommitted staging attempt. The old pointer stays active.
    pub fn abort(&mut self) {
        self.intent = None;
    }

    pub fn persist(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        let temp = temporary_path(path);
        fs::write(&temp, bytes)?;
        fs::rename(temp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("rotation-{}.tmp", Uuid::new_v4().simple()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_requires_opt_in_threshold_and_cold_cache() {
        let provider = ProviderKind::Codex;
        let mut config = BossRotationConfig::default();
        assert!(!config.should_rotate(provider, 90, Some(100), true, None, 500));
        config.enabled = true;
        assert!(!config.should_rotate(provider, 79, Some(100), false, None, 500));
        assert!(!config.should_rotate(provider, 90, Some(100), true, Some(400), 500));
        assert!(config.should_rotate(provider, 90, Some(100), true, Some(200), 500));
        assert!(config.should_rotate(provider, 90, Some(100), false, Some(400), 500));
    }

    #[test]
    fn provider_can_disable_cache_or_override_ttl() {
        let provider = ProviderKind::Codex;
        let mut config = BossRotationConfig {
            enabled: true,
            ..Default::default()
        };
        config.provider_cache_ttl_secs.insert(provider, 0);
        assert!(config.should_rotate(provider, 90, Some(100), true, Some(499), 500));
        config.provider_cache_ttl_secs.insert(provider, 30);
        assert!(!config.should_rotate(provider, 90, Some(100), true, Some(480), 500));
        assert!(config.should_rotate(provider, 90, Some(100), true, Some(470), 500));
    }

    #[test]
    fn journal_keeps_old_pointer_until_commit_and_survives_restart() {
        let root = std::env::temp_dir().join(format!("boss-rotation-{}", Uuid::new_v4()));
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let boss = Uuid::new_v4();
        let mut journal = RotationJournal {
            active_session_id: Some(old),
            ..Default::default()
        };
        journal.begin(boss, old, next, 10).unwrap();
        journal.persist(&root).unwrap();
        let mut recovered = RotationJournal::load(&root).unwrap();
        assert_eq!(recovered.active_session_id, Some(old));
        assert!(recovered.intent.is_some());
        assert_eq!(recovered.commit(20).unwrap(), next);
        assert_eq!(recovered.generation, 1);
        assert_eq!(recovered.active_session_id, Some(next));
        recovered.persist(&root).unwrap();
        assert_eq!(
            RotationJournal::load(&root).unwrap().active_session_id,
            Some(next)
        );
        fs::remove_file(root).ok();
    }
}
