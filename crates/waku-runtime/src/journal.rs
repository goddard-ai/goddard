//! The host's only durable state: an append-only framed record log.
//!
//! Every mutation the host accepts is a record here before it is truth —
//! commands before dispatch, events before publish, fences before
//! termination. Frames are `len | crc32 | payload` so recovery can truncate
//! a torn or corrupted tail at the last valid record instead of trusting
//! it. There is no second store to disagree with: replaying the log
//! rebuilds the whole reconcile-able state.

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

use crate::contract::*;

/// Records are `u32` length + `u32` crc32(payload) + JSON payload.
const FRAME_HEADER: usize = 8;

/// One journaled fact plus the host time it was accepted. `at_ms` also
/// provides the persisted clock floor: recovery takes the maximum seen so a
/// rollback cannot reopen an expired retry window.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub at_ms: u64,
    #[serde(flatten)]
    pub record: JournalRecord,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "rec", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum JournalRecord {
    /// A new runtime instance took ownership of this namespace.
    HostStarted {
        instance: Uuid,
        namespace: String,
    },
    /// A controller acquired the exclusive mutation lease.
    ControllerAcquired {
        epoch: u64,
        retry_window_id: Uuid,
        expires_at_ms: u64,
    },
    /// A persisted, controller-authorized admission created a session.
    Admitted {
        session_id: Uuid,
        generation: u64,
        adapter: String,
        launch: serde_json::Value,
        requested: RequestedConfig,
    },
    SessionStatus {
        session_id: Uuid,
        generation: u64,
        status: SessionStatus,
    },
    /// A fence recorded before any termination attempt — once durable, no
    /// delayed traffic can dispatch in this generation.
    Fence {
        session_id: Uuid,
        generation: u64,
        fence: FenceKind,
    },
    /// Dispatch is held while a cancellation settles.
    DispatchHeld {
        session_id: Uuid,
        generation: u64,
    },
    DispatchPermitted {
        session_id: Uuid,
        generation: u64,
    },
    /// Intent recorded before any contact with the provider.
    Command {
        command_id: Uuid,
        session: Option<SessionRef>,
        action: CanonicalAction,
        retry_window_id: Uuid,
        expires_at_ms: u64,
    },
    Disposition {
        command_id: Uuid,
        disposition: CommandDisposition,
    },
    /// A command whose retry window elapsed. The tombstone keeps the id
    /// rejecting so an old command can never become new work — even after
    /// clock rollback, since expiry is checked against the persisted floor.
    CommandExpired {
        command_id: Uuid,
    },
    /// An ordered provider event, journaled before it can be published.
    Event {
        envelope: Box<EventEnvelope>,
    },
    /// Event payloads at or below `through` were reclaimed after the
    /// controller proved it can reconstruct them. Sequence positions are
    /// preserved; a cursor below the floor is a blocked recovery.
    EventTombstone {
        session_id: Uuid,
        generation: u64,
        through: u64,
    },
    /// Answer-delivery disposition — a separate recorded fact from the
    /// controller's decision and from provider consumption evidence (which
    /// arrives as `RequestAnswered` events). `consumed` preserves the
    /// evidence flag so request state survives compaction.
    RequestDisposition {
        session_id: Uuid,
        generation: u64,
        request_id: String,
        disposition: AnswerDisposition,
        consumed: bool,
        /// Compaction preserves the opening independently of replay
        /// retention. Ordinary disposition writes leave it unchanged.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opened: Option<Box<EventEnvelope>>,
    },
    /// The last requested configuration revision. Requested and effective
    /// config are separate facts; only `ConfigActivated` events move the
    /// effective record.
    ConfigRequest {
        session_id: Uuid,
        revision: u64,
        config: RequestedConfig,
    },
    /// The controller durably applied a sequence; drives reclamation.
    Acknowledged {
        session_id: Uuid,
        generation: u64,
        applied: u64,
    },
    /// A session-scoped grant minted for agent ingress. The token is the
    /// grant's proof — journaled under the same file permissions as the
    /// namespace identity, checked on every request, never renewed.
    GrantIssued {
        session_id: Uuid,
        generation: u64,
        grant_id: Uuid,
        token: String,
        expires_at_ms: u64,
    },
    /// An agent request accepted at scoped ingress, durable before any
    /// forwarding — a crash reconciles the record, it is never lost.
    IngressOpened {
        record: Box<IngressRecord>,
    },
    /// The request was handed to a controller's service — the dispatch
    /// marker separating `Expired` from `Uncertain` after a failure.
    IngressForwarded {
        request_id: Uuid,
    },
    /// The request's recorded outcome — a separate fact from forwarding.
    IngressOutcome {
        request_id: Uuid,
        disposition: IngressDisposition,
    },
    /// A durable resource claim bound to runtime-owned work.
    ClaimRegistered {
        claim: ResourceClaim,
    },
    /// A controller released a claim after reconciling or terminating its
    /// work. Claims never release silently on controller detachment.
    ClaimReleased {
        claim_id: Uuid,
        session_id: Uuid,
        generation: u64,
    },
    /// A crash-recovered namespace completed inventory reconciliation.
    Reconciled {
        epoch: u64,
    },
    /// Clean shutdown. Its absence on open is what makes recovery a crash.
    Drained,
}

impl JournalRecord {
    /// Whether this record carries control-plane truth — fences,
    /// dispositions, terminal facts, controller metadata. Control records
    /// always get emergency-buffer capacity; event payloads share a bounded
    /// pool, so data traffic can never starve cancellation or completion.
    pub fn is_control(&self) -> bool {
        !matches!(self, JournalRecord::Event { .. })
    }
}

/// Durability failures. `Injected` is the deterministic fault-injection
/// seam the harness uses to prove conservative behavior on disk failure.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("journal io: {0}")]
    Io(#[from] io::Error),
    #[error("journal write failed (injected fault)")]
    Injected,
}

/// Shared fault flag: when set, every append fails. Tests flip it to prove
/// the host stops accepting work and keeps control paths alive.
pub type JournalFault = Arc<AtomicBool>;

pub struct Journal {
    path: PathBuf,
    file: File,
    /// Bytes currently on disk — the accounting basis for journal budgets.
    bytes: u64,
    /// Time floor carried across recovery; never moves backward.
    floor_ms: u64,
    /// The tail was torn/corrupt and truncated on open.
    tail_corrupt: bool,
    fault: Option<JournalFault>,
    #[cfg(test)]
    pub(crate) fail_on: Option<fn(&JournalRecord) -> bool>,
}

impl Journal {
    /// Open (or create) the journal, replay valid frames, and truncate any
    /// corrupt tail. The returned frames are the recovery fold input.
    pub fn open(
        path: &Path,
        fault: Option<JournalFault>,
    ) -> Result<(Self, Vec<Frame>), JournalError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;

        let mut frames = Vec::new();
        let mut valid = 0usize;
        let mut floor = 0u64;
        let mut tail_corrupt = false;
        loop {
            if raw.len() - valid < FRAME_HEADER {
                tail_corrupt |= raw.len() - valid > 0;
                break;
            }
            let len = u32::from_be_bytes(raw[valid..valid + 4].try_into().unwrap()) as usize;
            let crc = u32::from_be_bytes(raw[valid + 4..valid + 8].try_into().unwrap());
            let end = valid + FRAME_HEADER + len;
            if end > raw.len() {
                tail_corrupt = true;
                break;
            }
            let payload = &raw[valid + FRAME_HEADER..end];
            if crc32fast::hash(payload) != crc {
                tail_corrupt = true;
                break;
            }
            match serde_json::from_slice::<Frame>(payload) {
                Ok(frame) => {
                    floor = floor.max(frame.at_ms);
                    frames.push(frame);
                    valid = end;
                }
                // A frame that fails to decode cannot be trusted or skipped
                // safely — everything after it is suspect.
                Err(_) => {
                    tail_corrupt = true;
                    break;
                }
            }
        }
        if tail_corrupt {
            file.set_len(valid as u64)?;
            file.seek(SeekFrom::Start(valid as u64))?;
        } else {
            file.seek(SeekFrom::End(0))?;
        }
        Ok((
            Self {
                path: path.to_path_buf(),
                file,
                bytes: valid as u64,
                floor_ms: floor,
                tail_corrupt,
                fault,
                #[cfg(test)]
                fail_on: None,
            },
            frames,
        ))
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn floor_ms(&self) -> u64 {
        self.floor_ms
    }

    pub fn tail_corrupt(&self) -> bool {
        self.tail_corrupt
    }

    /// Append one record durably. Fails rather than pretending: the caller
    /// treats any error as "not accepted" and closes new work accordingly.
    pub fn append(&mut self, at_ms: u64, record: JournalRecord) -> Result<(), JournalError> {
        #[cfg(test)]
        if self.fail_on.is_some_and(|predicate| predicate(&record)) {
            return Err(JournalError::Injected);
        }
        if self
            .fault
            .as_ref()
            .is_some_and(|f| f.load(Ordering::SeqCst))
        {
            return Err(JournalError::Injected);
        }
        let frame = Frame { at_ms, record };
        let payload = serde_json::to_vec(&frame).expect("journal records serialize");
        let mut framed = Vec::with_capacity(FRAME_HEADER + payload.len());
        framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        framed.extend_from_slice(&crc32fast::hash(&payload).to_be_bytes());
        framed.extend_from_slice(&payload);
        self.file.write_all(&framed)?;
        self.file.sync_data()?;
        self.bytes += framed.len() as u64;
        self.floor_ms = self.floor_ms.max(at_ms);
        Ok(())
    }

    /// Rewrite the log as `records` plus nothing else, atomically. Used to
    /// reclaim journal space once retained payloads are proven
    /// reconstructible. A crash mid-rewrite leaves the old file or the new
    /// one — both are valid journals.
    pub fn rewrite(&mut self, at_ms: u64, frames: Vec<Frame>) -> Result<(), JournalError> {
        if self
            .fault
            .as_ref()
            .is_some_and(|f| f.load(Ordering::SeqCst))
        {
            return Err(JournalError::Injected);
        }
        let tmp = self.path.with_extension("rewrite");
        let mut bytes = 0u64;
        {
            let mut tmp_file = File::create(&tmp)?;
            for frame in &frames {
                let payload = serde_json::to_vec(frame).expect("journal records serialize");
                tmp_file.write_all(&(payload.len() as u32).to_be_bytes())?;
                tmp_file.write_all(&crc32fast::hash(&payload).to_be_bytes())?;
                tmp_file.write_all(&payload)?;
                bytes += FRAME_HEADER as u64 + payload.len() as u64;
            }
            tmp_file.sync_data()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        self.file = OpenOptions::new().append(true).open(&self.path)?;
        self.bytes = bytes;
        self.floor_ms = self.floor_ms.max(at_ms);
        Ok(())
    }
}

/// Mint a credential. Two UUIDs of entropy, stored — never derived.
pub fn mint_credential() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
