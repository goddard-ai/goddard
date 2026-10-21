//! The runtime's IPC wire vocabulary.
//!
//! Every message is a contract type — the journal record payload, the wire
//! frame, and the harness vocabulary are the same definitions, so no owner
//! invents a parallel envelope. The protocol is strictly request/response:
//! the server pushes nothing; waits are long-polls the client renews.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::contract::*;

/// The first frame on every connection — it decides the role, and every
/// request afterwards is authorized by what this proved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connect {
    pub namespace: String,
    pub auth: ConnectAuth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "role",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConnectAuth {
    /// The product daemon — full control authority.
    Controller {
        credential: String,
        client: ClientHello,
    },
    /// A daemon's service endpoint for forwarded agent traffic. It binds
    /// the epoch it serves so a detached or replaced controller cannot
    /// answer new work — the host still fences every call by epoch.
    Service {
        credential: String,
        epoch: u64,
        client: ClientHello,
    },
    /// Narrow local authority: status and emergency stop while no daemon
    /// control exists.
    Emergency { credential: String },
    /// Session-scoped agent ingress — the grant is the whole authority; it
    /// can never acquire controller power or another session's traffic.
    Agent { grant: SessionGrant },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WireRequest {
    Hello {
        client: ClientHello,
    },
    AcquireControl {
        client: ClientHello,
    },
    Reconcile {
        epoch: u64,
    },
    Execute {
        envelope: Box<ActionEnvelope>,
    },
    Command {
        command_id: Uuid,
    },
    /// Ordered replay from a durable applied cursor. `wait_ms` long-polls
    /// for new events; `0` returns immediately.
    Events {
        epoch: u64,
        cursor: EventCursor,
        #[serde(default)]
        wait_ms: u64,
    },
    Acknowledge {
        epoch: u64,
        cursor: EventCursor,
    },
    Status,
    Drain {
        epoch: u64,
        policy: DrainPolicy,
    },
    EmergencyStop {
        session_id: Uuid,
    },
    IssueGrant {
        epoch: u64,
        session: SessionRef,
        ttl_ms: u64,
    },
    RegisterClaim {
        epoch: u64,
        claim: ResourceClaim,
    },
    ReleaseClaim {
        epoch: u64,
        claim_id: Uuid,
    },
    /// The daemon service asking for forwarded agent traffic.
    /// `wait_ms` long-polls; `0` returns immediately.
    ServiceDrain {
        epoch: u64,
        #[serde(default)]
        wait_ms: u64,
    },
    /// The daemon service reporting one forwarded outcome.
    ServiceResult {
        epoch: u64,
        request_id: Uuid,
        outcome: ForwardedOutcome,
    },
    /// An agent submitting one request under its session grant.
    Ingress {
        request: IngressRequest,
    },
    /// An agent reading one of its own requests.
    IngressQuery {
        request_id: Uuid,
    },
    /// An agent waiting for its request to reach an outcome.
    /// `wait_ms` long-polls; `0` returns immediately.
    IngressWait {
        request_id: Uuid,
        #[serde(default)]
        wait_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "response",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WireResponse {
    Hello {
        hello: Box<Hello>,
    },
    Control {
        grant: ControlGrant,
    },
    Inventory {
        inventory: Box<Inventory>,
    },
    Receipt {
        receipt: CommandReceipt,
    },
    Command {
        receipt: Option<CommandReceipt>,
    },
    Page {
        page: Box<EventPage>,
    },
    Ok,
    Status {
        status: Box<HostStatus>,
    },
    SessionStatus {
        status: SessionStatus,
    },
    Drain {
        report: DrainReport,
    },
    Grant {
        grant: Box<SessionGrant>,
    },
    Forwarded {
        requests: Vec<ForwardedRequest>,
    },
    Ingress {
        record: Box<IngressRecord>,
    },
    /// Authorization, fencing, and precondition truth — never transport
    /// failure. A torn connection is an IO error at the client instead.
    Error {
        error: HostError,
    },
}
