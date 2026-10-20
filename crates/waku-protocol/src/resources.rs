//! Cooperative host resource reservations; identities come from daemon authentication.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, TS)]
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
impl ResourceSet {
    /// Whether the set names any host capacity at all. An admission
    /// ticket may carry an empty set — it claims a model slot only — so
    /// such a reservation holds nothing a nested request could borrow.
    pub fn is_empty(&self) -> bool {
        self.exclusive.is_empty()
            && self.resident_devices == 0
            && self.native_builds == 0
            && self.desktop_input == 0
    }
}

/// A daemon-owned model-slot claim an admission reservation carries.
/// Namespaced by stable daemon (boss) identity so one host broker
/// arbitrates model capacity for every daemon's queue. Limits arrive
/// with the claim: the daemon's boss-set policy supplies them per attempt,
/// so a policy change takes effect on the next try without re-writing the
/// ledger.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct AdmissionClaim {
    /// Stable daemon identity the claim is namespaced under — the Boss
    /// document's identity id, not a process id.
    pub daemon: Uuid,
    pub provider: String,
    pub model: String,
    /// Normal concurrent cap for this (daemon, provider, model) pair.
    pub live_limit: u32,
    /// Absolute cap reachable only when `allow_burst` is set.
    pub hard_cap: u32,
    #[serde(default)]
    pub allow_burst: bool,
    /// A human force-start may leapfrog parked `Acquire` waiters sharing
    /// its capacity — they keep waiting rather than lose anything held.
    /// Granted reservations and policy capacity are never bypassed.
    #[serde(default)]
    pub skip_wait_order: bool,
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
    /// Daemon-owned atomic check-and-grant: the model claim and the
    /// declared `resources` are granted under one authority lock or not
    /// at all. Unlike `Acquire` this never parks — a denied admission
    /// leaves no ledger entry, so ordering stays with the daemon's own
    /// queue, and an empty `resources` set is valid. `id` is the
    /// daemon's stable ticket key: retrying a granted id is a no-op, so
    /// restart recovery re-issues the admission idempotently.
    Admission {
        id: Uuid,
        resources: ResourceSet,
        purpose: String,
        claim: AdmissionClaim,
    },
    Attach {
        id: Uuid,
        workload_pid: u32,
    },
    /// Daemon-internal lease transfer: rebind the reservation's daemon and
    /// daemon-held liveness anchors to a surviving owner process (the
    /// stable runtime host) so a daemon reload cannot silently release work
    /// the runtime still owns. Agent-supplied callers never send this.
    Reanchor {
        id: Uuid,
        owner_pid: u32,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
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
    /// When set, the surviving process that owns this reservation's
    /// liveness instead of the transacting daemon — the runtime host for
    /// runtime-owned sessions. `holder_pid` anchors that matched the old
    /// daemon move with it; agent-held callers keep their own pid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_pid: Option<u32>,
    pub workload_pid: Option<u32>,
    pub requested_at: u64,
    #[serde(default)]
    pub duration_seconds: u64,
    pub deadline: u64,
    pub granted_at: Option<u64>,
    pub cancelled: bool,
    pub released: bool,
    /// The model-slot claim this reservation holds, when it came through
    /// `Admission`. Ledger entries written before admission tickets
    /// deserialize as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<AdmissionClaim>,
    /// Why a parked reservation cannot grant yet — a named device running
    /// outside the ledger, a failed inventory probe, an earlier
    /// reservation's claims, or held capacity. The scheduler recomputes
    /// it every transaction; it is diagnostics, never input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_on: Option<String>,
}
impl Reservation {
    /// The activity-card title for a parked reservation. The broker's
    /// scheduler stamps `waiting_on` every transaction, so rows written
    /// by older daemons are the only ones that fall back to the generic
    /// text.
    pub fn waiting_title(&self) -> String {
        let label = if self.resources.resident_devices > 0 {
            self.resources.exclusive.join(", ")
        } else if self.resources.native_builds > 0 {
            "native build".into()
        } else {
            "desktop input".into()
        };
        match self.waiting_on.as_deref() {
            Some(reason) => format!("Waiting for {label}—{reason}"),
            None => format!("Waiting for {label}—queued for host capacity"),
        }
    }
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
    /// Why a denied `Admission` could not take capacity this pass —
    /// empty for granted admissions and every other operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admission_blockers: Vec<crate::boss::AdmissionBlocker>,
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
    fn waiting_title_renders_the_stamped_reason_or_the_queued_fallback() {
        let mut reservation = Reservation {
            id: Uuid::new_v4(),
            task: Uuid::new_v4(),
            purpose: "simulated test".into(),
            resources: ResourceSet {
                exclusive: vec!["ios:00000000-0000-0000-0000-000000000001".into()],
                resident_devices: 1,
                ..Default::default()
            },
            holder_pid: 0,
            daemon_pid: 0,
            owner_pid: None,
            workload_pid: None,
            requested_at: 0,
            duration_seconds: 0,
            deadline: 0,
            granted_at: None,
            cancelled: false,
            released: false,
            admission: None,
            waiting_on: Some(
                "device ios:00000000-0000-0000-0000-000000000001 is running outside Goddard"
                    .into(),
            ),
        };
        assert_eq!(
            reservation.waiting_title(),
            "Waiting for ios:00000000-0000-0000-0000-000000000001—device \
             ios:00000000-0000-0000-0000-000000000001 is running outside Goddard"
        );
        reservation.waiting_on = None;
        assert_eq!(
            reservation.waiting_title(),
            "Waiting for ios:00000000-0000-0000-0000-000000000001—queued for host capacity"
        );
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
