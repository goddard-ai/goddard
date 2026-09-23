use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{AgentSession, MessageRole};

/// A focused yes/no judgment about a completed task turn.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
pub struct AutoPromptQuestion {
    pub id: Uuid,
    pub instructions: String,
    /// Relative importance in the rule's weighted average. `None` is filled
    /// by the setup suggestion before the rule can be enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

/// An authorized follow-up that Goddard may send after a human turn settles.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
pub struct AutoPromptRule {
    pub id: Uuid,
    pub name: String,
    pub prompt: String,
    pub questions: Vec<AutoPromptQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(default)]
    pub enabled: bool,
}

impl AutoPromptRule {
    pub fn valid_for_dispatch(&self) -> bool {
        self.enabled && self.valid_for_dispatch_without_enabled()
    }

    pub fn valid_for_dispatch_without_enabled(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.prompt.trim().is_empty()
            && !self.questions.is_empty()
            && self.questions.iter().all(|question| {
                !question.instructions.trim().is_empty()
                    && question
                        .weight
                        .is_some_and(|weight| weight.is_finite() && weight > 0.0)
            })
            && self
                .threshold
                .is_some_and(|threshold| threshold.is_finite() && (0.0..=1.0).contains(&threshold))
    }
}

/// The bounded turn state shared by preview and daemon dispatch.
pub fn turn_state(session: &AgentSession, turn_id: Uuid) -> serde_json::Value {
    let prompt = session
        .messages
        .iter()
        .filter(|message| message.turn_id == Some(turn_id) && message.role == MessageRole::User)
        .map(|message| message.visible_content())
        .collect::<Vec<_>>()
        .join("\n\n");
    let response = session
        .messages
        .iter()
        .filter(|message| {
            message.turn_id == Some(turn_id) && message.role == MessageRole::Assistant
        })
        .map(|message| message.visible_content())
        .collect::<Vec<_>>()
        .join("\n\n");
    let head = |text: &str, limit: usize| text.chars().take(limit).collect::<String>();
    let tail = |text: &str, limit: usize| {
        text.chars()
            .rev()
            .take(limit)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    };
    let failed_activities = session
        .transcript_blocks
        .iter()
        .filter(|block| block.turn_id == Some(turn_id))
        .flat_map(|block| block.activities.iter())
        .filter(|activity| activity.failed)
        .collect::<Vec<_>>();
    json!({
        "prompt": head(&prompt, 2400),
        "response": tail(&response, 4000),
        "provider": session.provider.display_name(),
        "toolErrors": failed_activities[failed_activities.len().saturating_sub(8)..]
            .iter()
            .map(|activity| json!({
                "tool": activity.tool_name,
                "title": activity.title,
                "outputTail": activity.output.as_deref().map(|output| tail(output, 400)),
            }))
            .collect::<Vec<_>>(),
        "filesChanged": session.turns.iter().find(|turn| turn.id == turn_id)
            .and_then(|turn| turn.checkpoint.as_ref())
            .map(|checkpoint| checkpoint.files.iter().take(30).map(|file| &file.path).collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}
