//! Client pairing: a nearby device asks the daemon for a token, a human
//! approves on an already-connected client, and the new device joins as a
//! full client. Carried wholesale on every change like `FriendsState` —
//! the document is a handful of pending requests and paired clients.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Whole pairing document — pending requests plus every client whose
/// minted token the daemon still honors.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PairingState {
    /// Devices asking for a token right now, awaiting a local decision.
    pub pending: Vec<PairRequestInfo>,
    /// Devices whose minted tokens authenticate as full clients.
    pub clients: Vec<PairedClientInfo>,
}

/// One unanswered pair request.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PairRequestInfo {
    /// Identifies the pending request for `respondPairRequest`.
    pub request_id: Uuid,
    /// Self-reported device name shown on the approval prompt.
    pub device_name: String,
    /// How the request arrived — `ws` for a direct socket, `link` for the
    /// iroh LAN channel.
    pub transport: String,
    pub at_ms: u64,
}

/// A device the daemon minted a token for. The token itself never leaves
/// the store — this record is what clients render.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PairedClientInfo {
    /// Identifies the client for `revokePairedClient`.
    pub client_id: Uuid,
    pub name: String,
    pub added_at_ms: u64,
}
