//! Cooperative host resource reservations; identities come from daemon authentication.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ResourceSet {
    #[serde(default)]
    pub exclusive: Vec<String>,
    #[serde(default)]
    pub resident_devices: u32,
    #[serde(default)]
    pub native_builds: u32,
    #[serde(default)]
    pub desktop_input: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceOperation {
    Acquire {
        resources: ResourceSet,
        purpose: String,
        holder_pid: u32,
        #[serde(default = "default_wait")]
        wait_seconds: u32,
        #[serde(default)]
        parent: Option<Uuid>,
    },
    Attach {
        id: Uuid,
        workload_pid: u32,
    },
    Release {
        id: Uuid,
    },
    Cancel {
        id: Uuid,
    },
    Status {
        #[serde(default)]
        id: Option<Uuid>,
    },
}
pub fn default_wait() -> u32 {
    600
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ResourcePolicy {
    pub resident_devices: u32,
    pub native_builds: u32,
    pub desktop_input: u32,
}
impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            resident_devices: 1,
            native_builds: 1,
            desktop_input: 1,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct Reservation {
    pub id: Uuid,
    pub task: Uuid,
    pub purpose: String,
    pub resources: ResourceSet,
    pub holder_pid: u32,
    pub daemon_pid: u32,
    pub workload_pid: Option<u32>,
    pub requested_at: u64,
    #[serde(default)]
    pub duration_seconds: u64,
    pub deadline: u64,
    pub granted_at: Option<u64>,
    pub cancelled: bool,
    pub released: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
pub struct ResourceStatus {
    pub policy: ResourcePolicy,
    pub reservations: Vec<Reservation>,
    /// Running devices not attributed to a reservation. Never stopped by Goddard.
    pub external_devices: Vec<String>,
    pub observation_errors: Vec<String>,
    pub request_id: Option<Uuid>,
    pub borrowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_cannot_supply_task_identity_and_defaults_are_bounded() {
        let op: ResourceOperation = serde_json::from_value(serde_json::json!({
            "action":"acquire", "resources":{"native_builds":1}, "purpose":"test", "holder_pid":42
        }))
        .unwrap();
        assert!(matches!(
            op,
            ResourceOperation::Acquire {
                wait_seconds: 600,
                parent: None,
                ..
            }
        ));
        assert!(serde_json::from_value::<ResourceOperation>(serde_json::json!({
            "action":"acquire", "resources":{"native_builds":1}, "purpose":"test", "holder_pid":42, "task":"forged"
        })).is_err());
    }
    #[test]
    fn resource_command_and_status_round_trip_the_wire() {
        let command = crate::Command::AgentResources {
            operation: ResourceOperation::Status { id: None },
        };
        let json = serde_json::to_value(command).unwrap();
        assert_eq!(json["type"], "agentResources");
        assert_eq!(json["operation"]["action"], "status");
        serde_json::from_value::<crate::Command>(json).unwrap();
        let response = crate::ResponsePayload::AgentResources {
            status: ResourceStatus::default(),
        };
        serde_json::from_value::<crate::ResponsePayload>(serde_json::to_value(response).unwrap())
            .unwrap();
    }
}
