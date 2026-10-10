//! Compatibility exports and the daemon settings adapter for Boss rotation.

use serde::{Deserialize, Serialize};

pub use waku_boss::boss_rotation::{
    BossRotationConfig as BossRotationPolicy, RotationIntent, RotationJournal,
};

/// Compatibility configuration with the daemon settings constructor retained.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BossRotationConfig {
    pub context_threshold: f64,
}

const MAX_CONTEXT_THRESHOLD: f64 = 0.75;

impl Default for BossRotationConfig {
    fn default() -> Self {
        let policy = BossRotationPolicy::default();
        Self {
            context_threshold: policy.context_threshold,
        }
    }
}

impl BossRotationConfig {
    pub fn from_settings(settings: &crate::DaemonSettings) -> Self {
        Self {
            // Older persisted settings carry the previous 0.8 default. Keep
            // rotation ahead of provider compaction even before they are
            // rewritten; lower configured thresholds remain effective.
            context_threshold: if settings.boss_rotation_context_threshold.is_finite() {
                settings
                    .boss_rotation_context_threshold
                    .min(MAX_CONTEXT_THRESHOLD)
            } else {
                settings.boss_rotation_context_threshold
            },
        }
    }

    pub fn should_rotate(&self, context_tokens: u64, context_window: Option<u64>) -> bool {
        BossRotationPolicy {
            context_threshold: self.context_threshold,
        }
        .should_rotate(context_tokens, context_window)
    }
}
