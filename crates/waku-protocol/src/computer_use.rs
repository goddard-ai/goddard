use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Clamp a requested Computer Use enablement to what the daemon's settings
/// allow. The feature is experimental, so the daemon ANDs the user's enable
/// flag with the Computer Use experiment opt-in before any driver or helper
/// starts.
pub const fn resolve_enabled(requested: bool, experiment_enabled: bool) -> bool {
    requested && experiment_enabled
}

/// Identifies an approval issued by Goddard's REPL rather than by a provider.
/// The opaque request id travels through the existing permission event path.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ComputerApprovalId {
    pub nonce: String,
    pub scope: String,
    pub app_name: String,
    pub bundle_id: Option<String>,
}

impl ComputerApprovalId {
    const PREFIX: &'static str = "goddard-computer-use:";

    pub fn encode(&self) -> String {
        format!(
            "{}{}",
            Self::PREFIX,
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(self).expect("Computer Approval ID is serializable"))
        )
    }

    pub fn decode(value: &str) -> Option<Self> {
        let encoded = value.strip_prefix(Self::PREFIX)?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn app_grant(&self) -> Option<ComputerAppGrant> {
        self.bundle_id
            .as_ref()
            .is_some_and(|id| !id.is_empty() && self.scope == format!("app:{id}"))
            .then(|| ComputerAppGrant {
                bundle_id: self.bundle_id.clone().unwrap_or_default(),
                app_name: self.app_name.clone(),
                verified: true,
            })
    }
}

#[derive(Clone, Debug)]
pub struct ComputerToolRequest {
    pub call_id: String,
    pub tool: String,
    pub arguments: Value,
}

impl ComputerToolRequest {
    pub fn summary(&self) -> String {
        if self.tool != "use" {
            return match self.tool.as_str() {
                "status" => "Check computer-use access".into(),
                _ => self.tool.clone(),
            };
        }
        let actions = self
            .arguments
            .get("actions")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if actions.is_empty() {
            return "Inspect the window".into();
        }
        let mut labels = actions
            .iter()
            .filter_map(|action| action.get("type").and_then(Value::as_str))
            .map(action_label)
            .collect::<Vec<_>>();
        labels.dedup();
        format!("{} {}", labels.join(", "), plural(actions.len(), "action"))
    }
}

fn action_label(action: &str) -> &'static str {
    match action {
        "click" | "double_click" => "Click",
        "move" => "Move the pointer",
        "drag" => "Drag",
        "scroll" => "Scroll",
        "type" => "Type text",
        "keypress" => "Press keys",
        "wait" => "Wait",
        _ => "Interact",
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComputerPermissions {
    pub screen_recording: bool,
    pub accessibility: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComputerTarget {
    pub window_id: u64,
    pub bundle_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub app_name: String,
    pub window_title: String,
    pub width: u32,
    pub height: u32,
}

impl ComputerTarget {
    pub fn grant_key(&self) -> String {
        self.bundle_id.clone()
    }

    pub fn persistable(&self) -> bool {
        !self.bundle_id.trim().is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComputerAppGrant {
    pub bundle_id: String,
    pub app_name: String,
    /// Old app grants predate enforcement and must not authorize the REPL.
    #[serde(default)]
    pub verified: bool,
}

impl ComputerAppGrant {
    pub fn key(&self) -> String {
        self.bundle_id.clone()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ComputerUsePhase {
    AwaitingApproval,
    Running,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseState {
    pub target: Option<ComputerTarget>,
    pub phase: ComputerUsePhase,
    pub visible: bool,
    pub image_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ComputerAppGrant, ComputerApprovalId, resolve_enabled};

    #[test]
    fn computer_use_requires_the_experiment_opt_in() {
        assert!(!resolve_enabled(false, true));
        assert!(!resolve_enabled(true, false));
        assert!(resolve_enabled(true, true));
    }

    #[test]
    fn computer_approval_ids_round_trip_and_only_apps_persist() {
        let approval = ComputerApprovalId {
            nonce: "nonce".into(),
            scope: "app:com.example.Editor".into(),
            app_name: "Editor".into(),
            bundle_id: Some("com.example.Editor".into()),
        };
        let decoded = ComputerApprovalId::decode(&approval.encode()).unwrap();
        let grant = decoded.app_grant().unwrap();
        assert_eq!(grant.bundle_id, "com.example.Editor");
        assert!(grant.verified);
        assert!(ComputerApprovalId::decode("provider-request").is_none());

        for scope in ["browser", "clipboard", "desktop"] {
            let mut temporary = approval.clone();
            temporary.scope = scope.into();
            assert!(temporary.app_grant().is_none());
        }
    }

    #[test]
    fn old_app_grants_do_not_authorize_the_repl() {
        let old: ComputerAppGrant =
            serde_json::from_str(r#"{"bundleId":"com.example.Editor","appName":"Editor"}"#)
                .unwrap();
        assert!(!old.verified);
    }
}
