//! Store-independent policy and durable intent records for Boss rotation.
//!
//! Rotation defaults on: a settled Boss session rotates once its context
//! crosses the configured threshold. The daemon owns provider session
//! creation; this module makes the trigger and the journal decision
//! deterministic and restart-readable.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BossRotationConfig {
    /// Rotate once context reaches this fraction of the reported window.
    pub context_threshold: f64,
}

impl Default for BossRotationConfig {
    fn default() -> Self {
        Self {
            context_threshold: 0.75,
        }
    }
}

impl BossRotationConfig {
    pub fn should_rotate(&self, context_tokens: u64, context_window: Option<u64>) -> bool {
        if !self.context_threshold.is_finite() {
            return false;
        }
        let Some(window) = context_window.filter(|window| *window > 0) else {
            return false;
        };
        context_tokens as f64 / window as f64 > self.context_threshold.clamp(0.0, 1.0)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RotationJournal {
    pub generation: u64,
    pub active_session_id: Option<Uuid>,
    pub intent: Option<RotationIntent>,
    /// Completed swaps retained for diagnostics and transcript provenance.
    pub rotations: Vec<RotationIntent>,
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
            reason: None,
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
        self.rotations.push(intent.clone());
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
    fn trigger_requires_threshold_crossing_only() {
        let config = BossRotationConfig::default();
        assert!(!config.should_rotate(74, Some(100)));
        assert!(!config.should_rotate(75, Some(100)));
        assert!(config.should_rotate(76, Some(100)));
        assert!(!config.should_rotate(90, None));
        assert!(!config.should_rotate(90, Some(0)));
        let disabled = BossRotationConfig {
            context_threshold: f64::NAN,
        };
        assert!(!disabled.should_rotate(100, Some(100)));
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
