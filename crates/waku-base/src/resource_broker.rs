//! All Goddard daemons transact against one durable, OS-locked host ledger.
//! No lease expiry can make a live process group or resident device disappear.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use waku_protocol::boss::AdmissionBlocker;
use waku_protocol::resources::*;

#[derive(Default, Deserialize, Serialize)]
struct Ledger {
    reservations: Vec<Reservation>,
}

/// The answer to one daemon-owned admission try-grant: the model claim and
/// host set landed under one authority lock, or nothing did.
#[derive(Debug)]
pub struct AdmissionAttempt {
    /// The ticket is granted — fresh or an idempotent retry of a grant
    /// already held. A denied attempt holds nothing.
    pub granted: bool,
    /// Why the ticket could not take capacity this pass.
    pub blockers: Vec<AdmissionBlocker>,
    pub status: ResourceStatus,
}

pub struct Broker {
    root: PathBuf,
}
#[derive(Clone, Default)]
struct Observation {
    devices: Vec<String>,
    /// Device-class prefixes (`ios:`, `android:`) whose inventory probe
    /// failed or was skipped — requests naming them cannot be told apart
    /// from user-owned devices, so they stay parked. A failure in one
    /// class must not blind the other. `linux:` has no inventory probe,
    /// so it can never be blind.
    blind: Vec<String>,
    errors: Vec<String>,
}

/// Exclusive keys that draw on the shared `resident_devices` pool:
/// mobile simulators/emulators plus named Linux VMs. Other exclusive
/// names remain plain locks that consume no resident capacity.
fn resident_device(key: &str) -> bool {
    key.starts_with("ios:") || key.starts_with("android:") || key.starts_with("linux:")
}

/// Resident classes the host inventory probe can report. A `linux:` VM
/// is not observable: its residency is bounded by the reservation's own
/// lifecycle rather than a retained idle-device observation.
fn probed_device(key: &str) -> bool {
    key.starts_with("ios:") || key.starts_with("android:")
}

/// Whether a resident key still proves live capacity after its
/// reservation ends — the inventory still lists it, or its class's probe
/// failed so it is conservatively retained.
fn retained_device(key: &str, observation: &Observation) -> bool {
    observation.devices.iter().any(|device| device == key)
        || (probed_device(key) && !observation.errors.is_empty())
}

impl Broker {
    pub fn host() -> Result<Self> {
        let root = waku_protocol::DaemonSettings::default_path()
            .parent()
            .context("Goddard home unavailable")?
            .join("resource-broker");
        Ok(Self { root })
    }

    /// A broker rooted anywhere — tests keep their own ledger rather than
    /// the host's.
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn operate(&self, task: Uuid, operation: ResourceOperation) -> Result<ResourceStatus> {
        #[cfg(not(unix))]
        bail!("resource workload supervision currently requires a Unix host");
        // A live observation spawns xcrun+ps probes, so it is reserved for
        // the ops that decide from the device inventory: acquisitions that
        // claim devices get a fresh one, and Status polls share a short
        // cache — a waiter's 500 ms poll must not serialize the authority
        // lock behind probe latency. Every other op reports "not probed":
        // an errored observation keeps resident-device claims
        // conservatively and still blocks resident-device grants, which is
        // what `release_admission` already relies on.
        let claims_devices = match &operation {
            ResourceOperation::Acquire { resources, .. }
            | ResourceOperation::Admission { resources, .. } => {
                !resources.exclusive.is_empty() || resources.resident_devices > 0
            }
            _ => false,
        };
        if claims_devices {
            self.with_observation(task, operation, observe)
        } else if matches!(operation, ResourceOperation::Status { .. }) {
            self.transaction(task, operation, cached_observation())
        } else {
            self.transaction(
                task,
                operation,
                Observation {
                    devices: Vec::new(),
                    blind: vec!["ios:".into(), "android:".into()],
                    errors: vec!["device inventory not probed".into()],
                },
            )
        }
    }

    fn transaction(
        &self,
        task: Uuid,
        operation: ResourceOperation,
        observation: Observation,
    ) -> Result<ResourceStatus> {
        self.with_observation(task, operation, || observation)
    }

    fn with_observation(
        &self,
        task: Uuid,
        operation: ResourceOperation,
        observe: impl FnOnce() -> Observation,
    ) -> Result<ResourceStatus> {
        crate::fs_ext::create_private_dir_all(&self.root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("authority.lock"))?;
        crate::fs_ext::restrict_to_owner(&self.root.join("authority.lock"))?;
        lock.lock().context("lock host resource authority")?;
        let path = self.root.join("state.json");
        let mut ledger: Ledger = read_json(&path)?.unwrap_or_default();
        let policy: ResourcePolicy = read_json(&self.root.join("policy.json"))?.unwrap_or_default();
        let observation = observe();
        let now = now();
        // Observation failure retains all previously granted resident claims.
        ledger.reservations.retain_mut(|r| {
            // A reanchored lease answers liveness through its surviving
            // owner, never the transacting daemon. Holder anchors that were
            // the daemon itself (admission tickets) move with the owner;
            // agent-acquired callers keep their own pid's liveness.
            let daemon_anchor = r.owner_pid.unwrap_or(r.daemon_pid);
            let holder_anchor = match r.owner_pid {
                Some(owner) if r.holder_pid == r.daemon_pid => owner,
                _ => r.holder_pid,
            };
            let holder_alive = waku_protocol::pid::is_alive(holder_anchor);
            if !waku_protocol::pid::is_alive(daemon_anchor) || !holder_alive {
                r.cancelled = true;
            }
            if r.granted_at.is_none() {
                return !r.cancelled && !r.released && r.deadline > now && holder_alive;
            }
            // A granted row whose runner died without stopping its
            // workload signals the orphaned group — the row is still
            // retained below while the group is alive, so capacity is
            // never freed early; only signalling is new.
            if !holder_alive
                && let Some(pid) = r.workload_pid
                && group_alive(pid)
            {
                reap_orphaned_workload(r, pid, now);
            }
            let live_work = r.workload_pid.is_some_and(group_alive);
            let live_device = r
                .resources
                .exclusive
                .iter()
                .any(|key| resident_device(key) && retained_device(key, &observation));
            if !live_work && (r.cancelled || r.released) && live_device {
                // A finished workload releases build/input capacity even when its
                // idle simulator service remains resident outside the process group.
                r.resources.native_builds = 0;
                r.resources.desktop_input = 0;
                r.resources.exclusive.retain(|key| {
                    resident_device(key) && retained_device(key, &observation)
                });
                r.resources.resident_devices = r.resources.exclusive.len() as u32;
            }
            live_work
                || live_device
                || (!r.cancelled && !r.released && holder_alive)
        });
        let request_id;
        let mut borrowed = false;
        let mut denied_blockers = Vec::new();
        match operation {
            ResourceOperation::Acquire {
                mut resources,
                purpose,
                holder_pid,
                wait_seconds,
                parent,
            } => {
                validate(&mut resources, &policy)?;
                if purpose.trim().is_empty() || purpose.len() > 512 {
                    bail!("purpose must contain 1–512 bytes");
                }
                if holder_pid <= 1
                    || holder_pid > i32::MAX as u32
                    || !waku_protocol::pid::is_alive(holder_pid)
                {
                    bail!("reservation holder is not alive");
                }
                if wait_seconds > 86400 {
                    bail!("wait_seconds must be at most 86400");
                }
                // A granted parent with an empty set — a model-claim-only
                // admission ticket — holds nothing a subset could borrow,
                // so the request falls through to a top-level acquisition.
                let borrows = match parent {
                    Some(id) => {
                        let r = ledger
                            .reservations
                            .iter()
                            .find(|r| r.id == id && r.task == task)
                            .context("parent reservation is not owned by this task")?;
                        if r.cancelled
                            || r.released
                            || r.granted_at.is_none()
                            || (!r.resources.is_empty() && !subset(&resources, &r.resources))
                        {
                            bail!(
                                "nested reservation must use an active parent's resource subset; acquire the full set at the outermost command"
                            );
                        }
                        (!r.resources.is_empty()).then_some(id)
                    }
                    None => None,
                };
                if let Some(id) = borrows {
                    request_id = Some(id);
                    borrowed = true;
                } else {
                    // Explicit acquisitions by a task that already holds resources cannot wait for expansion.
                    if ledger.reservations.iter().any(|r| {
                        r.task == task
                            && r.granted_at.is_some()
                            && !r.released
                            && !r.resources.is_empty()
                    }) {
                        bail!(
                            "task already holds resources; pass parent for subset reuse or release before acquiring a new set"
                        );
                    }
                    let id = Uuid::new_v4();
                    request_id = Some(id);
                    ledger.reservations.push(Reservation {
                        id,
                        task,
                        purpose,
                        resources,
                        holder_pid,
                        daemon_pid: std::process::id(),
                        owner_pid: None,
                        workload_pid: None,
                        workload_started: None,
                        reap_signalled_at: None,
                        requested_at: now,
                        duration_seconds: 0,
                        deadline: now + u64::from(wait_seconds),
                        granted_at: None,
                        cancelled: false,
                        released: false,
                        admission: None,
                        waiting_on: None,
                    });
                }
            }
            ResourceOperation::Admission {
                id,
                mut resources,
                purpose,
                claim,
            } => {
                if let Some(existing) = ledger
                    .reservations
                    .iter()
                    .find(|r| r.id == id && r.task == task)
                {
                    // A retry after a lost response or a daemon restart:
                    // the ticket's claims are already held — re-issue is a
                    // no-op. A settled id belongs to a stale generation and
                    // must never revive.
                    if existing.cancelled || existing.released {
                        bail!("admission ticket {id} was already released");
                    }
                    request_id = Some(id);
                } else {
                    validate_admission(&mut resources, &policy, &claim)?;
                    if purpose.trim().is_empty() || purpose.len() > 512 {
                        bail!("purpose must contain 1–512 bytes");
                    }
                    denied_blockers = admission_blockers(
                        &resources,
                        &claim,
                        task,
                        &ledger,
                        &policy,
                        &observation,
                    );
                    if denied_blockers.is_empty() {
                        ledger.reservations.push(Reservation {
                            id,
                            task,
                            purpose,
                            resources,
                            holder_pid: std::process::id(),
                            daemon_pid: std::process::id(),
                            owner_pid: None,
                            workload_pid: None,
                            workload_started: None,
                            reap_signalled_at: None,
                            requested_at: now,
                            duration_seconds: 0,
                            // Granted rows ignore the deadline; it exists
                            // only for parked Acquire requests.
                            deadline: now,
                            granted_at: Some(now),
                            cancelled: false,
                            released: false,
                            admission: Some(claim),
                            waiting_on: None,
                        });
                        request_id = Some(id);
                    } else {
                        // Denied admissions hold nothing — the daemon's
                        // queue owns ordering, not the ledger.
                        request_id = Some(id);
                    }
                }
            }
            ResourceOperation::Attach { id, workload_pid } => {
                let r = owned(&mut ledger, task, id)?;
                if r.cancelled || r.released || r.granted_at.is_none() || r.workload_pid.is_some() {
                    bail!("reservation is not available to attach a workload");
                }
                if workload_pid <= 1 || workload_pid > i32::MAX as u32 || !group_alive(workload_pid)
                {
                    bail!("workload process group is not alive");
                }
                r.workload_pid = Some(workload_pid);
                r.workload_started = crate::process_identity::start_time(workload_pid as i32);
                request_id = Some(id);
            }
            ResourceOperation::Reanchor { id, owner_pid } => {
                let r = owned(&mut ledger, task, id)?;
                if r.cancelled || r.released {
                    bail!("reservation is not live");
                }
                if owner_pid <= 1
                    || owner_pid > i32::MAX as u32
                    || !waku_protocol::pid::is_alive(owner_pid)
                {
                    bail!("reservation anchor is not alive");
                }
                r.owner_pid = Some(owner_pid);
                request_id = Some(id);
            }
            ResourceOperation::Release { id } | ResourceOperation::Cancel { id } => {
                let r = owned(&mut ledger, task, id)?;
                r.cancelled = true;
                r.released = true;
                request_id = Some(id);
            }
            ResourceOperation::Status { id } => {
                if let Some(id) = id
                    && let Some(r) = ledger.reservations.iter().find(|r| r.id == id)
                    && r.task != task
                {
                    bail!("reservation is not owned by this task");
                }
                request_id = id;
            }
        }
        schedule(&mut ledger, &policy, &observation, now);
        for reservation in &mut ledger.reservations {
            reservation.duration_seconds =
                now.saturating_sub(reservation.granted_at.unwrap_or(reservation.requested_at));
        }
        let external_devices: Vec<String> = observation
            .devices
            .iter()
            .filter(|device| {
                !ledger
                    .reservations
                    .iter()
                    .any(|r| r.granted_at.is_some() && r.resources.exclusive.contains(device))
            })
            .cloned()
            .collect();
        for index in 0..ledger.reservations.len() {
            let reservation = &ledger.reservations[index];
            let waiting_on = (reservation.granted_at.is_none()
                && !reservation.cancelled
                && !reservation.released)
                .then(|| {
                wait_reason(
                    reservation,
                    index,
                    &ledger,
                    &policy,
                    &observation,
                    &external_devices,
                )
            });
            ledger.reservations[index].waiting_on = waiting_on;
        }
        // Rename under a stable separate lock: a killed writer leaves either old or new complete JSON.
        let temporary = self.root.join("state.next");
        let bytes = serde_json::to_vec(&ledger)?;
        fs::write(&temporary, bytes)?;
        crate::fs_ext::restrict_to_owner(&temporary)?;
        File::open(&temporary)?.sync_all()?;
        fs::rename(&temporary, &path)?;
        File::open(&self.root)?.sync_all()?;
        drop(lock);
        Ok(ResourceStatus {
            policy,
            reservations: ledger.reservations,
            external_devices,
            observation_errors: observation.errors,
            request_id,
            borrowed,
            admission_blockers: denied_blockers,
        })
    }

    /// Validate a declared set against the current policy without holding
    /// or granting anything — submission-time checks use it; a later
    /// policy change never turns a granted reservation invalid.
    pub fn validate_set(&self, resources: &ResourceSet) -> Result<()> {
        validate_set(&mut resources.clone(), &self.policy()?)
    }

    /// The host capacity policy the ledger currently transacts under.
    pub fn policy(&self) -> Result<ResourcePolicy> {
        crate::fs_ext::create_private_dir_all(&self.root)?;
        Ok(read_json(&self.root.join("policy.json"))?.unwrap_or_default())
    }

    /// Replace the host capacity policy under the authority lock — the
    /// same serialized writer the ledger uses, so a policy write can never
    /// interleave with a grant decision.
    pub fn set_policy(&self, policy: &ResourcePolicy) -> Result<()> {
        crate::fs_ext::create_private_dir_all(&self.root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("authority.lock"))?;
        crate::fs_ext::restrict_to_owner(&self.root.join("authority.lock"))?;
        lock.lock().context("lock host resource authority")?;
        let path = self.root.join("policy.json");
        let temporary = self.root.join("policy.next");
        let bytes = serde_json::to_vec(policy)?;
        fs::write(&temporary, bytes)?;
        crate::fs_ext::restrict_to_owner(&temporary)?;
        File::open(&temporary)?.sync_all()?;
        fs::rename(&temporary, &path)?;
        File::open(&self.root)?.sync_all()?;
        drop(lock);
        Ok(())
    }

    /// Daemon-owned admission try-grant: claim a model slot and the
    /// declared host set atomically under the authority lock. A denied
    /// attempt writes no ledger entry — ordering stays in the daemon's
    /// queue — while retrying an already-granted `id` answers granted, so
    /// restart reconciliation re-issues tickets idempotently.
    pub fn try_admission(
        &self,
        task: Uuid,
        id: Uuid,
        resources: ResourceSet,
        purpose: String,
        claim: AdmissionClaim,
    ) -> Result<AdmissionAttempt> {
        // Device claims need the real inventory — an empty observation
        // would hide user-owned devices and over-grant. Model-only and
        // counted-pool tickets never consult it.
        let observe = if resources.exclusive.is_empty() && resources.resident_devices == 0 {
            Observation::default
        } else {
            observe
        };
        let status = self.with_observation(
            task,
            ResourceOperation::Admission {
                id,
                resources,
                purpose,
                claim,
            },
            observe,
        )?;
        let granted = status
            .reservations
            .iter()
            .any(|r| r.id == id && r.granted_at.is_some());
        Ok(AdmissionAttempt {
            granted,
            blockers: status.admission_blockers.clone(),
            status,
        })
    }

    /// Release an admission ticket's claims — `Release` semantics under
    /// the ticket's owning task id. Missing or already-settled
    /// reservations are a no-op so recovery can call it unconditionally.
    pub fn release_admission(&self, task: Uuid, id: Uuid) {
        let _ = self.transaction(
            task,
            ResourceOperation::Release { id },
            Observation {
                devices: vec![],
                blind: vec!["ios:".into(), "android:".into()],
                errors: vec!["admission release".into()],
            },
        );
    }

    /// Cancel exact request IDs captured at runtime exit. A new runtime of the
    /// same task cannot be affected by this delayed worker.
    pub fn cancel_owned_async(task: Uuid, ids: Vec<Uuid>) {
        let _ = std::thread::Builder::new()
            .name("resource-owner-exit".into())
            .spawn(move || {
                if let Ok(broker) = Self::host() {
                    for id in ids {
                        let _ = broker.transaction(
                            task,
                            ResourceOperation::Cancel { id },
                            Observation {
                                devices: vec![],
                                blind: vec!["ios:".into(), "android:".into()],
                                errors: vec!["lifecycle cleanup".into()],
                            },
                        );
                    }
                }
            });
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
            format!("invalid broker file {} (left intact)", path.display())
        })?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn owned(ledger: &mut Ledger, task: Uuid, id: Uuid) -> Result<&mut Reservation> {
    ledger
        .reservations
        .iter_mut()
        .find(|r| r.id == id && r.task == task)
        .context("reservation is not owned by this task")
}
fn subset(a: &ResourceSet, b: &ResourceSet) -> bool {
    a.exclusive.iter().all(|key| b.exclusive.contains(key))
        && a.resident_devices <= b.resident_devices
        && a.native_builds <= b.native_builds
        && a.desktop_input <= b.desktop_input
}
fn validate(r: &mut ResourceSet, policy: &ResourcePolicy) -> Result<()> {
    validate_set(r, policy)?;
    if r.exclusive.is_empty() && r.native_builds == 0 && r.desktop_input == 0 {
        bail!("request must name at least one resource");
    }
    Ok(())
}

/// An admission's host set is allowed to be empty — the model claim alone
/// is a valid ticket — but anything declared still has to be a legal,
/// in-policy set.
fn validate_admission(
    r: &mut ResourceSet,
    policy: &ResourcePolicy,
    claim: &AdmissionClaim,
) -> Result<()> {
    validate_set(r, policy)?;
    if claim.provider.trim().is_empty() || claim.model.trim().is_empty() {
        bail!("admission claims need a resolved provider and model");
    }
    if claim.live_limit > claim.hard_cap {
        bail!("admission claim live limit cannot exceed its hard cap");
    }
    Ok(())
}

fn validate_set(r: &mut ResourceSet, policy: &ResourcePolicy) -> Result<()> {
    for key in &mut r.exclusive {
        if let Some(id) = key.strip_prefix("ios:") {
            *key = format!(
                "ios:{}",
                Uuid::parse_str(id)
                    .context("iOS resource must name a simulator UUID")?
                    .to_string()
                    .to_uppercase()
            );
        } else if key == "android:" {
            bail!("Android resource must name an AVD");
        } else if key == "linux:" {
            bail!("Linux resource must name a VM");
        }
    }
    r.exclusive.sort();
    r.exclusive.dedup();
    if r.exclusive.len() > 32
        || r.exclusive
            .iter()
            .any(|key| key.trim().is_empty() || key.len() > 256)
    {
        bail!("exclusive resource names must contain 1–256 bytes (at most 32 names)");
    }
    let devices = r.exclusive.iter().filter(|s| resident_device(s)).count() as u32;
    if devices != r.resident_devices {
        bail!(
            "resident_devices must equal the number of ios:<UDID>/android:<AVD>/linux:<name> exclusive resources"
        );
    }
    if r.resident_devices > policy.resident_devices
        || r.native_builds > policy.native_builds
        || r.desktop_input > policy.desktop_input
    {
        bail!("request exceeds host capacity policy");
    }
    Ok(())
}

/// Why one admission try-grant cannot take capacity under the authority
/// lock — the model claim against other granted claims, and the host set
/// against the ledger, policy, and live device inventory. Granting an
/// admission that would leave an earlier parked `Acquire` still blocked
/// on the same resources also denies: admissions never starve the
/// broker's FIFO clients.
///
/// The requesting task's own granted claims stay out of the count: a
/// daemon re-admission under a fresh id is a swap that releases the old
/// reservation on grant, so counting it against itself could never
/// succeed under a saturated cap.
fn admission_blockers(
    request: &ResourceSet,
    claim: &AdmissionClaim,
    task: Uuid,
    ledger: &Ledger,
    policy: &ResourcePolicy,
    observation: &Observation,
) -> Vec<AdmissionBlocker> {
    let mut blockers = Vec::new();
    let used = ledger
        .reservations
        .iter()
        .filter(|r| {
            r.task != task
                && r.granted_at.is_some()
                && !r.released
                && !r.cancelled
                && r.admission.as_ref().is_some_and(|held| {
                    held.daemon == claim.daemon
                        && held.provider == claim.provider
                        && held.model == claim.model
                })
        })
        .count() as u32;
    let cap = if claim.allow_burst {
        claim.hard_cap
    } else {
        claim.live_limit
    };
    if used >= cap {
        blockers.push(AdmissionBlocker::ModelLimit { used, limit: cap });
    }
    if blocked(request, ledger, policy, observation, Some(task), 0) {
        blockers.push(AdmissionBlocker::HostResources {
            detail: "host capacity is occupied".into(),
        });
    } else if !claim.skip_wait_order && starves_earlier_waiter(request, ledger) {
        blockers.push(AdmissionBlocker::HostResources {
            detail: "an earlier resource request is waiting for the same capacity".into(),
        });
    }
    blockers
}

/// Whether two sets contend for the same capacity — an identical
/// exclusive key or a draw on the same counted pool.
fn shares_capacity(a: &ResourceSet, b: &ResourceSet) -> bool {
    a.exclusive.iter().any(|key| b.exclusive.contains(key))
        || (a.resident_devices > 0 && b.resident_devices > 0)
        || (a.native_builds > 0 && b.native_builds > 0)
        || (a.desktop_input > 0 && b.desktop_input > 0)
}

/// Whether the request names a device that cannot grant this pass — it
/// is running outside the ledger (`external`), or its class's inventory
/// probe failed (`blind`) so we cannot prove it free. Only the named
/// class's failure blinds a request: an Android probe error never stalls
/// an iOS claim, nor vice versa.
fn device_unavailable(
    request: &ResourceSet,
    external: &[String],
    observation: &Observation,
) -> bool {
    if request.resident_devices == 0 {
        return false;
    }
    request.exclusive.iter().any(|key| {
        external.contains(key)
            || observation
                .blind
                .iter()
                .any(|class| key.starts_with(class.as_str()))
    })
}

/// The portion of a parked waiter's set that still reserves capacity —
/// counted pools and exclusive keys it could actually grant on. A claim
/// on a device that cannot grant this pass (running outside the ledger,
/// or hidden by a failed probe) is released so a satisfiable request
/// behind it bypasses instead of starving; the waiter's counted pools
/// keep their place in line since capacity protection is what stops a
/// flood of small requests from starving the head forever.
fn claimable_while_waiting(
    resources: &ResourceSet,
    external: &[String],
    observation: &Observation,
) -> ResourceSet {
    let mut claimable = resources.clone();
    let before = claimable.exclusive.len();
    claimable.exclusive.retain(|key| {
        !external.contains(key)
            && !observation
                .blind
                .iter()
                .any(|class| key.starts_with(class.as_str()))
    });
    let dropped = (before - claimable.exclusive.len()) as u32;
    claimable.resident_devices = claimable.resident_devices.saturating_sub(dropped);
    claimable
}

/// Whether granting `request` now would leapfrog a parked `Acquire` that
/// needs the same capacity. A pending request counts as sharing when it
/// names an identical exclusive key or draws on the same counted pool.
fn starves_earlier_waiter(request: &ResourceSet, ledger: &Ledger) -> bool {
    ledger
        .reservations
        .iter()
        .filter(|r| r.granted_at.is_none() && !r.cancelled && !r.released)
        .any(|waiting| shares_capacity(&waiting.resources, request))
}
fn schedule(ledger: &mut Ledger, policy: &ResourcePolicy, observation: &Observation, now: u64) {
    for index in 0..ledger.reservations.len() {
        let r = &ledger.reservations[index];
        if r.granted_at.is_some() || r.cancelled || r.released {
            continue;
        }
        // Earlier waiters reserve their full sets in the capacity calculation,
        // but do not prevent later requests from using spare or unrelated pools.
        // They still hold nothing until their atomic set can actually grant.
        if blocked(&r.resources, ledger, policy, observation, None, index) {
            continue;
        }
        ledger.reservations[index].granted_at = Some(now);
    }
}
/// Whether `request` cannot grant now. `exclude` names a task whose own
/// granted claims should not count — a re-admission swap releases them
/// on grant, so they are not part of the capacity the request competes
/// for. `queued_before` also protects the capacity needed by earlier
/// waiters, allowing later requests to use only the remaining capacity.
fn blocked(
    request: &ResourceSet,
    ledger: &Ledger,
    policy: &ResourcePolicy,
    observation: &Observation,
    exclude: Option<Uuid>,
    queued_before: usize,
) -> bool {
    let held: Vec<_> = ledger
        .reservations
        .iter()
        .filter(|r| r.granted_at.is_some() && Some(r.task) != exclude)
        .collect();
    let external: Vec<String> = observation
        .devices
        .iter()
        .filter(|key| !held.iter().any(|r| r.resources.exclusive.contains(*key)))
        .cloned()
        .collect();
    // An external device conflicts only with a request for that same
    // exclusive key. It is user-owned, so it does not consume our shared
    // resident-device capacity.
    if device_unavailable(request, &external, observation) {
        return true;
    }
    // Earlier waiters keep only the claims they could spend this pass —
    // a head parked on an unavailable device yields the device slot so
    // satisfiable requests behind it grant instead of starving.
    let reduced: Vec<ResourceSet> = ledger.reservations[..queued_before]
        .iter()
        .filter(|r| r.granted_at.is_none() && !r.cancelled && !r.released)
        .map(|r| claimable_while_waiting(&r.resources, &external, observation))
        .collect();
    let claims = held.iter().map(|r| &r.resources).chain(reduced.iter());
    request.exclusive.iter().any(|key| {
        claims
            .clone()
            .any(|resources| resources.exclusive.contains(key))
    }) || request.resident_devices > 0
        && (u64::from(request.resident_devices)
            + claims
                .clone()
                .map(|resources| u64::from(resources.resident_devices))
                .sum::<u64>()
            > u64::from(policy.resident_devices))
        || request.native_builds > 0
            && (u64::from(request.native_builds)
                + claims
                    .clone()
                    .map(|resources| u64::from(resources.native_builds))
                    .sum::<u64>()
                > u64::from(policy.native_builds))
        || request.desktop_input > 0
            && (u64::from(request.desktop_input)
                + claims
                    .map(|resources| u64::from(resources.desktop_input))
                    .sum::<u64>()
                > u64::from(policy.desktop_input))
}

/// Why a parked reservation cannot grant this pass — the named device's
/// availability first since it is what a user can fix, then the granted
/// row holding an identical exclusive key, then the earlier reservation
/// whose claims stand ahead, then each exhausted capacity pool with the
/// reservations holding it. Stamped on the reservation as `waiting_on`
/// so `resource status` names the wait instead of a bare queue position.
fn wait_reason(
    reservation: &Reservation,
    position: usize,
    ledger: &Ledger,
    policy: &ResourcePolicy,
    observation: &Observation,
    external: &[String],
) -> String {
    let resources = &reservation.resources;
    if resources.resident_devices > 0 {
        if resources.exclusive.iter().any(|key| {
            observation
                .blind
                .iter()
                .any(|class| key.starts_with(class.as_str()))
        }) {
            return "device inventory could not be probed".into();
        }
        if let Some(device) = resources
            .exclusive
            .iter()
            .find(|key| external.contains(key))
        {
            return format!("device {device} is running outside Goddard");
        }
    }
    if let Some((key, holder)) = resources.exclusive.iter().find_map(|key| {
        ledger
            .reservations
            .iter()
            .find(|held| held.granted_at.is_some() && held.resources.exclusive.contains(key))
            .map(|held| (key, held))
    }) {
        return format!(
            "device {key} held by {}",
            holder_label(holder, observation)
        );
    }
    if let Some(earlier) = ledger.reservations[..position].iter().find(|earlier| {
        earlier.granted_at.is_none()
            && !earlier.cancelled
            && !earlier.released
            && shares_capacity(&earlier.resources, resources)
    }) {
        return format!("queued behind an earlier reservation ({})", earlier.id);
    }
    let mut exhausted = Vec::new();
    let mut report = |name: &str, requested: u32, limit: u32, count: fn(&ResourceSet) -> u32| {
        if requested == 0 {
            return;
        }
        let holders: Vec<&Reservation> = ledger
            .reservations
            .iter()
            .filter(|held| held.granted_at.is_some() && count(&held.resources) > 0)
            .collect();
        let used: u64 = holders
            .iter()
            .map(|held| u64::from(count(&held.resources)))
            .sum();
        if u64::from(requested) + used > u64::from(limit) {
            let names = holders
                .iter()
                .map(|held| holder_label(held, observation))
                .collect::<Vec<_>>()
                .join(", ");
            exhausted.push(format!("{name} {used}/{limit} held by {names}"));
        }
    };
    report(
        "resident devices",
        resources.resident_devices,
        policy.resident_devices,
        |r| r.resident_devices,
    );
    report(
        "native builds",
        resources.native_builds,
        policy.native_builds,
        |r| r.native_builds,
    );
    report(
        "desktop input",
        resources.desktop_input,
        policy.desktop_input,
        |r| r.desktop_input,
    );
    if !exhausted.is_empty() {
        return exhausted.join("; ");
    }
    "host capacity".into()
}

/// The `task <id>` handle a wait reason gives a granted row, extended for
/// settled rows that still hold capacity: a released row retained by its
/// live workload process group, or by a device the inventory still lists.
fn holder_label(r: &Reservation, observation: &Observation) -> String {
    let mut label = format!("task {}", r.task);
    if r.cancelled || r.released {
        if let Some(pid) = r.workload_pid.filter(|pid| group_alive(*pid)) {
            label.push_str(&format!(
                " (released; workload process group {pid} still running)"
            ));
        } else if let Some(key) = r
            .resources
            .exclusive
            .iter()
            .find(|key| resident_device(key) && retained_device(key, observation))
        {
            label.push_str(&format!(" (released; device {key} retained)"));
        }
    }
    label
}

/// Stop an orphaned workload process group: the `resource run` runner
/// normally SIGTERMs its own group before exiting (waku-agent), but a
/// holder that dies uncatchably — provider teardown — never gets the
/// chance, and the orphaned group would pin its capacity forever. The
/// broker SIGTERMs the group, then escalates to SIGKILL about two
/// seconds later on a subsequent transaction.
///
/// Every signal is fenced by the group leader's kernel birth stamp
/// recorded at `Attach`: a leader whose stamp differs, or a row written
/// before stamps existed, is never signalled. When the leader is gone
/// but the group still lives, POSIX never reassigns a group ID while
/// the group exists, so the group is provably the orphan. Capacity is
/// still held until the group actually exits — signalling never frees
/// it early.
#[cfg(unix)]
fn reap_orphaned_workload(r: &mut Reservation, pid: u32, now: u64) {
    // Legacy rows carry no recorded stamp: without a fence there is no
    // safe signal, so the orphan is left alone.
    if r.workload_started.is_none() {
        return;
    }
    match crate::process_identity::start_time(pid as i32) {
        // A live leader must be the exact process that was attached.
        Some(stamp) if Some(stamp) == r.workload_started => {}
        // The leader is gone or its stamp unreadable. If the leader
        // still exists the PID may have been reused — no signal.
        _ if waku_protocol::pid::is_alive(pid) => return,
        // Leader gone, group alive: the group ID still names our orphan.
        _ => {}
    }
    match r.reap_signalled_at {
        None => {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
            r.reap_signalled_at = Some(now);
        }
        Some(signalled) if now >= signalled + 2 => unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        },
        _ => {}
    }
}
#[cfg(not(unix))]
fn reap_orphaned_workload(r: &mut Reservation, pid: u32, now: u64) {
    let _ = (r, pid, now);
}

#[cfg(unix)]
fn group_alive(pid: u32) -> bool {
    if pid <= 1 || pid > i32::MAX as u32 {
        return false;
    }
    let result = unsafe { libc::kill(-(pid as i32), 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
#[cfg(not(unix))]
fn group_alive(pid: u32) -> bool {
    waku_protocol::pid::is_alive(pid)
}

// Read-only probes are bounded, run on request workers, and never stop/boot devices.
fn probe(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().context("probe stdout unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(1024 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait()? {
            let bytes = reader
                .join()
                .map_err(|_| anyhow::anyhow!("device probe reader failed"))??;
            if !status.success() {
                bail!("device probe failed");
            }
            return Ok(bytes);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("device probe timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
/// Status pollers share one fresh-enough probe per process — the cache is
/// evaluated before the authority lock, so waiting callers cannot queue
/// xcrun+ps subprocesses inside it.
fn cached_observation() -> Observation {
    const TTL: Duration = Duration::from_secs(2);
    static CACHE: parking_lot::Mutex<Option<(Instant, Observation)>> =
        parking_lot::Mutex::new(None);
    let mut cache = CACHE.lock();
    if let Some((probed_at, observation)) = cache.as_ref()
        && probed_at.elapsed() < TTL
    {
        return observation.clone();
    }
    let observation = observe();
    *cache = Some((Instant::now(), observation.clone()));
    observation
}

fn observe() -> Observation {
    let mut observation = Observation::default();
    #[cfg(target_os = "macos")]
    if Path::new("/usr/bin/xcrun").exists() {
        match probe(
            "/usr/bin/xcrun",
            &["simctl", "list", "devices", "booted", "--json"],
        )
        .and_then(|bytes| Ok(serde_json::from_slice::<serde_json::Value>(&bytes)?))
        {
            Ok(value) => {
                if let Some(devices) = value.get("devices").and_then(|v| v.as_object()) {
                    for device in devices.values().filter_map(|v| v.as_array()).flatten() {
                        if device.get("state").and_then(|v| v.as_str()) == Some("Booted")
                            && let Some(id) = device.get("udid").and_then(|v| v.as_str())
                        {
                            observation.devices.push(format!("ios:{id}"));
                        }
                    }
                } else {
                    observation.blind.push("ios:".into());
                    observation
                        .errors
                        .push("iOS device inventory unavailable".into());
                }
            }
            Err(_) => {
                observation.blind.push("ios:".into());
                observation
                    .errors
                    .push("iOS device inventory unavailable; resident acquisitions blocked".into());
            }
        }
    }
    #[cfg(unix)]
    match probe("/bin/ps", &["-axo", "comm=,args="]) {
        Ok(bytes) => observation
            .devices
            .extend(android_devices(&String::from_utf8_lossy(&bytes))),
        Err(_) => {
            observation.blind.push("android:".into());
            observation.errors.push(
                "Android process inventory unavailable; resident acquisitions blocked".into(),
            );
        }
    }
    observation.devices.sort();
    observation.devices.dedup();
    observation
}
fn android_devices(processes: &str) -> Vec<String> {
    processes
        .lines()
        .filter_map(|line| {
            let words: Vec<_> = line.split_whitespace().collect();
            let executable = Path::new(*words.first()?).file_name()?.to_str()?;
            if executable != "emulator" && !executable.starts_with("qemu-system-") {
                return None;
            }
            let avd = words
                .windows(2)
                .find(|pair| pair[0] == "-avd")
                .map(|pair| pair[1])
                .or_else(|| words.iter().find_map(|word| word.strip_prefix('@')));
            // Unknown emulator still consumes capacity; never assume it is ours.
            Some(format!("android:{}", avd.unwrap_or("unknown")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::{Child, Command},
        sync::{Arc, Barrier},
    };

    struct Harness {
        broker: Broker,
    }
    impl Harness {
        fn new() -> Self {
            Self {
                broker: Broker {
                    root: std::env::temp_dir()
                        .join(format!("goddard-broker-test-{}", Uuid::new_v4())),
                },
            }
        }
        fn op(&self, task: Uuid, op: ResourceOperation) -> ResourceStatus {
            self.broker
                .transaction(task, op, Observation::default())
                .unwrap()
        }
        fn acquire(&self, task: Uuid, resources: ResourceSet) -> ResourceStatus {
            self.op(task, acquisition(resources, None))
        }
        fn mutate(&self, f: impl FnOnce(&mut Ledger)) {
            let path = self.broker.root.join("state.json");
            let mut ledger: Ledger = read_json(&path).unwrap().unwrap();
            f(&mut ledger);
            fs::write(path, serde_json::to_vec(&ledger).unwrap()).unwrap();
        }
    }
    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.broker.root);
        }
    }
    fn acquisition(resources: ResourceSet, parent: Option<Uuid>) -> ResourceOperation {
        ResourceOperation::Acquire {
            resources,
            purpose: "simulated test".into(),
            holder_pid: std::process::id(),
            wait_seconds: 60,
            parent,
        }
    }
    fn build() -> ResourceSet {
        ResourceSet {
            native_builds: 1,
            ..Default::default()
        }
    }
    fn ios() -> ResourceSet {
        ResourceSet {
            exclusive: vec!["ios:00000000-0000-0000-0000-000000000001".into()],
            resident_devices: 1,
            ..Default::default()
        }
    }
    fn android() -> ResourceSet {
        ResourceSet {
            exclusive: vec!["android:Pixel_Test".into()],
            resident_devices: 1,
            ..Default::default()
        }
    }
    fn linux() -> ResourceSet {
        ResourceSet {
            exclusive: vec!["linux:gtk-proof-vm".into()],
            resident_devices: 1,
            native_builds: 1,
            ..Default::default()
        }
    }
    fn id(s: &ResourceStatus) -> Uuid {
        s.request_id.unwrap()
    }
    fn granted(s: &ResourceStatus, id: Uuid) -> bool {
        s.reservations
            .iter()
            .any(|r| r.id == id && r.granted_at.is_some())
    }
    struct Workload(Child);
    impl Workload {
        #[cfg(unix)]
        fn start() -> Self {
            use std::os::unix::process::CommandExt;
            Self(
                Command::new("/bin/sleep")
                    .arg("30")
                    .process_group(0)
                    .spawn()
                    .unwrap(),
            )
        }
        fn stop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    impl Drop for Workload {
        fn drop(&mut self) {
            self.stop();
        }
    }

    #[test]
    fn operate_probes_devices_only_for_status_and_device_claims() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        // Device-free acquire, release, and cancel never spawn xcrun+ps —
        // they report "not probed" instead of a live inventory.
        let status = h.broker.operate(task, acquisition(build(), None)).unwrap();
        assert_eq!(status.observation_errors, ["device inventory not probed"]);
        let status = h
            .broker
            .operate(task, ResourceOperation::Release { id: id(&status) })
            .unwrap();
        assert_eq!(status.observation_errors, ["device inventory not probed"]);
        // Status still runs the real (cached) observation — its errors are
        // genuine probe failures, never the skip marker.
        let status = h
            .broker
            .operate(task, ResourceOperation::Status { id: None })
            .unwrap();
        assert!(
            !status
                .observation_errors
                .iter()
                .any(|error| error == "device inventory not probed")
        );
    }

    #[test]
    fn independent_project_daemons_share_one_atomic_fifo_authority() {
        let h = Harness::new();
        let barrier = Arc::new(Barrier::new(9));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let root = h.broker.root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    // Separate broker instances model daemons belonging to unrelated projects/worktrees.
                    barrier.wait();
                    Broker { root }
                        .transaction(
                            Uuid::new_v4(),
                            acquisition(build(), None),
                            Observation::default(),
                        )
                        .unwrap()
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        let status = h.op(Uuid::new_v4(), ResourceOperation::Status { id: None });
        assert_eq!(status.reservations.len(), 8);
        assert_eq!(
            status
                .reservations
                .iter()
                .filter(|r| r.granted_at.is_some())
                .count(),
            1
        );
        let first = &status.reservations[0];
        assert!(first.granted_at.is_some());
        h.op(first.task, ResourceOperation::Release { id: first.id });
        let after = h.op(Uuid::new_v4(), ResourceOperation::Status { id: None });
        assert_eq!(after.reservations[0].id, status.reservations[1].id);
        assert!(after.reservations[0].granted_at.is_some());
    }

    #[test]
    fn atomic_sets_do_not_hold_partial_resources_and_fifo_prevents_starvation() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let first = h.acquire(a, build());
        let second = h.acquire(
            b,
            ResourceSet {
                native_builds: 1,
                desktop_input: 1,
                ..Default::default()
            },
        );
        let third = h.acquire(
            c,
            ResourceSet {
                desktop_input: 1,
                ..Default::default()
            },
        );
        assert!(!granted(&second, id(&second)));
        assert!(!granted(&third, id(&third)));
        h.op(a, ResourceOperation::Release { id: id(&first) });
        let status = h.op(
            b,
            ResourceOperation::Status {
                id: Some(id(&second)),
            },
        );
        assert!(granted(&status, id(&second)));
        assert!(!granted(&status, id(&third)));
    }

    #[test]
    fn blocked_device_waiter_leaves_spare_build_capacity_without_losing_fifo_priority() {
        let h = Harness::new();
        h.op(Uuid::new_v4(), ResourceOperation::Status { id: None });
        fs::write(
            h.broker.root.join("policy.json"),
            serde_json::to_vec(&ResourcePolicy {
                native_builds: 3,
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let device_task = Uuid::new_v4();
        let observation = || Observation {
            devices: ios().exclusive,
            blind: vec![],
            errors: vec![],
        };
        let device = h
            .broker
            .transaction(
                device_task,
                acquisition(
                    ResourceSet {
                        native_builds: 1,
                        ..ios()
                    },
                    None,
                ),
                observation(),
            )
            .unwrap();
        assert!(!granted(&device, id(&device)));
        let build_task = Uuid::new_v4();
        let native = h
            .broker
            .transaction(
                build_task,
                acquisition(
                    ResourceSet {
                        native_builds: 2,
                        ..Default::default()
                    },
                    None,
                ),
                observation(),
            )
            .unwrap();
        assert!(granted(&native, id(&native)));
        assert!(!granted(&native, id(&device)));
        let later_task = Uuid::new_v4();
        let later = h
            .broker
            .transaction(later_task, acquisition(build(), None), observation())
            .unwrap();
        // The last slot remains available for the earlier atomic request.
        assert!(!granted(&later, id(&later)));
        let status = h.op(device_task, ResourceOperation::Status { id: None });
        assert!(granted(&status, id(&device)));
        assert!(!granted(&status, id(&later)));
        h.op(build_task, ResourceOperation::Release { id: id(&native) });
        let status = h.op(later_task, ResourceOperation::Status { id: None });
        assert!(granted(&status, id(&later)));
    }

    #[test]
    fn device_blocked_head_yields_the_device_slot_and_names_its_wait() {
        let h = Harness::new();
        h.op(Uuid::new_v4(), ResourceOperation::Status { id: None });
        fs::write(
            h.broker.root.join("policy.json"),
            serde_json::to_vec(&ResourcePolicy {
                resident_devices: 1,
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let taken_device = "ios:00000000-0000-0000-0000-000000000042".to_string();
        let observation = || Observation {
            devices: vec![taken_device.clone()],
            blind: vec![],
            errors: vec![],
        };
        let head_task = Uuid::new_v4();
        // The head parks on a device running outside the ledger — it
        // cannot grant this pass no matter what capacity is free.
        let head = h
            .broker
            .transaction(
                head_task,
                acquisition(
                    ResourceSet {
                        exclusive: vec![taken_device.clone()],
                        resident_devices: 1,
                        ..Default::default()
                    },
                    None,
                ),
                observation(),
            )
            .unwrap();
        assert!(!granted(&head, id(&head)));
        // A satisfiable request for a different device bypasses it —
        // before the fix the head's resident-device claim starved it.
        let behind = h
            .broker
            .transaction(
                Uuid::new_v4(),
                acquisition(
                    ResourceSet {
                        exclusive: vec!["ios:00000000-0000-0000-0000-000000000099".into()],
                        resident_devices: 1,
                        ..Default::default()
                    },
                    None,
                ),
                observation(),
            )
            .unwrap();
        assert!(granted(&behind, id(&behind)));
        let status = h
            .broker
            .transaction(
                head_task,
                ResourceOperation::Status { id: None },
                observation(),
            )
            .unwrap();
        let head = status
            .reservations
            .iter()
            .find(|r| r.id == id(&head))
            .unwrap();
        assert_eq!(
            head.waiting_on.as_deref(),
            Some(format!("device {taken_device} is running outside Goddard").as_str())
        );
    }

    #[test]
    fn cancellation_timeout_and_dead_queue_holders_let_next_request_progress() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let first = h.acquire(a, build());
        let second = h.acquire(b, build());
        let third = h.acquire(c, build());
        assert!(
            h.broker
                .transaction(
                    a,
                    ResourceOperation::Cancel { id: id(&second) },
                    Observation::default()
                )
                .is_err()
        );
        h.op(b, ResourceOperation::Cancel { id: id(&second) });
        h.mutate(|ledger| {
            ledger
                .reservations
                .iter_mut()
                .find(|r| r.id == id(&third))
                .unwrap()
                .deadline = 0
        });
        let status = h.op(a, ResourceOperation::Status { id: None });
        assert_eq!(status.reservations.len(), 1);
        assert_eq!(status.reservations[0].id, id(&first));
        let fourth = h.acquire(b, build());
        h.mutate(|ledger| {
            ledger
                .reservations
                .iter_mut()
                .find(|r| r.id == id(&fourth))
                .unwrap()
                .holder_pid = u32::MAX / 2
        });
        assert_eq!(
            h.op(a, ResourceOperation::Status { id: None })
                .reservations
                .len(),
            1
        );
    }

    #[test]
    #[cfg(unix)]
    fn owner_death_or_expired_deadline_never_reassigns_live_workload() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = h.acquire(a, build());
        let mut workload = Workload::start();
        h.op(
            a,
            ResourceOperation::Attach {
                id: id(&first),
                workload_pid: workload.0.id(),
            },
        );
        h.mutate(|ledger| {
            let r = &mut ledger.reservations[0];
            r.holder_pid = u32::MAX / 2;
            r.daemon_pid = u32::MAX / 2;
            r.deadline = 0;
        });
        // A brand-new broker instance models crash/restart recovery from disk.
        let second = Broker {
            root: h.broker.root.clone(),
        }
        .transaction(b, acquisition(build(), None), Observation::default())
        .unwrap();
        assert!(!granted(&second, id(&second)));
        assert!(second.reservations[0].cancelled);
        workload.stop();
        let status = h.op(b, ResourceOperation::Status { id: None });
        assert!(granted(&status, id(&second)));
    }

    #[test]
    #[cfg(unix)]
    fn release_keeps_capacity_until_registered_process_group_exits() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = h.acquire(a, build());
        let mut workload = Workload::start();
        h.op(
            a,
            ResourceOperation::Attach {
                id: id(&first),
                workload_pid: workload.0.id(),
            },
        );
        h.op(a, ResourceOperation::Release { id: id(&first) });
        let second = h.acquire(b, build());
        assert!(!granted(&second, id(&second)));
        // A live holder owns its group: release never signals it.
        let retained = second
            .reservations
            .iter()
            .find(|r| r.id == id(&first))
            .unwrap();
        assert!(retained.reap_signalled_at.is_none());
        assert!(workload.0.try_wait().unwrap().is_none());
        workload.stop();
        assert!(granted(
            &h.op(b, ResourceOperation::Status { id: None }),
            id(&second)
        ));
    }

    #[test]
    fn cross_platform_resident_capacity_counts_retained_idle_but_not_user_owned_devices() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = h.acquire(a, ios());
        let observed = || Observation {
            devices: ios().exclusive,
            blind: vec![],
            errors: vec![],
        };
        h.broker
            .transaction(a, ResourceOperation::Release { id: id(&first) }, observed())
            .unwrap();
        let second = h
            .broker
            .transaction(b, acquisition(android(), None), observed())
            .unwrap();
        assert!(!granted(&second, id(&second)));
        assert!(second.external_devices.is_empty());
        let status = h.op(b, ResourceOperation::Status { id: None }); // device has shut down
        assert!(granted(&status, id(&second)));
        let external_host = Harness::new();
        let user_device = external_host
            .broker
            .transaction(a, acquisition(android(), None), observed())
            .unwrap();
        assert!(granted(&user_device, id(&user_device)));
        assert_eq!(user_device.external_devices, ios().exclusive);
        // The unrelated Android claim is admitted despite the external iOS
        // device. Native builds remain an independent pool.
        let native = external_host
            .broker
            .transaction(b, acquisition(build(), None), observed())
            .unwrap();
        assert!(granted(&native, id(&native)));

        let same_device_host = Harness::new();
        let same_device = same_device_host
            .broker
            .transaction(a, acquisition(ios(), None), observed())
            .unwrap();
        assert!(!granted(&same_device, id(&same_device)));
    }

    /// A brokered Linux VM is a named resident device: it validates like
    /// ios:/android:, draws on the same shared resident pool, and keeps
    /// its exclusive-key ownership.
    #[test]
    fn linux_vm_reservations_share_the_resident_pool_and_exclusive_ownership() {
        let h = Harness::new();
        h.op(Uuid::new_v4(), ResourceOperation::Status { id: None });
        fs::write(
            h.broker.root.join("policy.json"),
            serde_json::to_vec(&ResourcePolicy {
                resident_devices: 2,
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        // The exact GTK-proof set: named VM, one resident slot, one build.
        let first = h.acquire(a, linux());
        assert!(granted(&first, id(&first)));
        // A differently named VM takes the second resident slot; the same
        // name is exclusive even with spare resident capacity.
        let other_vm = h.acquire(
            b,
            ResourceSet {
                exclusive: vec!["linux:other-vm".into()],
                resident_devices: 1,
                ..Default::default()
            },
        );
        assert!(granted(&other_vm, id(&other_vm)));
        let same_vm = h.acquire(c, linux());
        assert!(!granted(&same_vm, id(&same_vm)));
        // Ownership: another task cannot release or even poll a foreign id.
        assert!(
            h.broker
                .transaction(
                    c,
                    ResourceOperation::Release { id: id(&first) },
                    Observation::default()
                )
                .is_err()
        );
        h.op(a, ResourceOperation::Release { id: id(&first) });
        let status = h.op(c, ResourceOperation::Status { id: None });
        assert!(granted(&status, id(&same_vm)));
    }

    /// The resident slot is global: a held Linux VM leaves no room for a
    /// mobile resident claim, and vice versa.
    #[test]
    fn linux_vm_residency_contends_with_mobile_devices_on_one_pool() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let vm = h.acquire(a, linux());
        assert!(granted(&vm, id(&vm)));
        let mobile = h.acquire(b, ios());
        assert!(!granted(&mobile, id(&mobile)));
        h.op(a, ResourceOperation::Release { id: id(&vm) });
        let status = h.op(b, ResourceOperation::Status { id: None });
        assert!(granted(&status, id(&mobile)));
    }

    /// With no inventory probe a `linux:` claim lives and dies with its
    /// reservation: release frees it even while mobile inventory probes
    /// are failing, while an observed `linux:` key retains and excludes
    /// exactly like a mobile device.
    #[test]
    fn linux_vm_residency_ends_with_the_reservation_unless_observed() {
        let probe_failed = || Observation {
            devices: vec![],
            blind: vec!["ios:".into(), "android:".into()],
            errors: vec!["probe failed".into()],
        };
        // Mobile claims stay conservatively retained across a probe
        // failure; an unprobed Linux VM cannot outlive its reservation.
        let mobile_host = Harness::new();
        let a = Uuid::new_v4();
        let first = mobile_host.acquire(a, ios());
        mobile_host
            .broker
            .transaction(
                a,
                ResourceOperation::Release { id: id(&first) },
                probe_failed(),
            )
            .unwrap();
        let next = mobile_host
            .broker
            .transaction(Uuid::new_v4(), acquisition(ios(), None), probe_failed())
            .unwrap();
        assert!(!granted(&next, id(&next)));

        let linux_host = Harness::new();
        let first = linux_host.acquire(a, linux());
        linux_host
            .broker
            .transaction(
                a,
                ResourceOperation::Release { id: id(&first) },
                probe_failed(),
            )
            .unwrap();
        let next = linux_host
            .broker
            .transaction(Uuid::new_v4(), acquisition(linux(), None), probe_failed())
            .unwrap();
        assert!(granted(&next, id(&next)));

        // An observation that does report the VM retains its residency and
        // its name exactly like a still-running mobile device.
        let observed_host = Harness::new();
        let vm_running = || Observation {
            devices: vec!["linux:gtk-proof-vm".into()],
            blind: vec![],
            errors: vec![],
        };
        let first = observed_host.acquire(a, linux());
        observed_host
            .broker
            .transaction(
                a,
                ResourceOperation::Release { id: id(&first) },
                vm_running(),
            )
            .unwrap();
        let same_vm = observed_host
            .broker
            .transaction(Uuid::new_v4(), acquisition(linux(), None), vm_running())
            .unwrap();
        assert!(!granted(&same_vm, id(&same_vm)));
        let status = observed_host.op(a, ResourceOperation::Status { id: None });
        assert!(granted(&status, id(&same_vm)));

        // A VM reported running outside the ledger blocks its own name.
        let external_host = Harness::new();
        let external = external_host
            .broker
            .transaction(Uuid::new_v4(), acquisition(linux(), None), vm_running())
            .unwrap();
        assert!(!granted(&external, id(&external)));
        assert_eq!(external.external_devices, ["linux:gtk-proof-vm"]);
    }

    /// A registered process group keeps a released Linux VM claim until
    /// it exits — the same lifecycle gate mobile workloads get.
    #[test]
    #[cfg(unix)]
    fn linux_vm_release_keeps_capacity_until_registered_process_group_exits() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = h.acquire(a, linux());
        let mut workload = Workload::start();
        h.op(
            a,
            ResourceOperation::Attach {
                id: id(&first),
                workload_pid: workload.0.id(),
            },
        );
        h.op(a, ResourceOperation::Release { id: id(&first) });
        let second = h.acquire(b, linux());
        assert!(!granted(&second, id(&second)));
        workload.stop();
        assert!(granted(
            &h.op(b, ResourceOperation::Status { id: None }),
            id(&second)
        ));
    }

    #[test]
    fn linux_vm_names_validate_like_mobile_devices() {
        let h = Harness::new();
        for resources in [
            // Bare prefix names no VM.
            ResourceSet {
                exclusive: vec!["linux:".into()],
                resident_devices: 1,
                ..Default::default()
            },
            // A resident key still needs its resident_devices count.
            ResourceSet {
                exclusive: vec!["linux:gtk-proof-vm".into()],
                ..Default::default()
            },
            // The count must come from named resident devices.
            ResourceSet {
                resident_devices: 1,
                ..Default::default()
            },
            // The shared pool cap applies to Linux VMs too.
            ResourceSet {
                exclusive: vec!["linux:gtk-proof-vm".into(), "linux:other-vm".into()],
                resident_devices: 2,
                ..Default::default()
            },
        ] {
            assert!(
                h.broker
                    .transaction(Uuid::new_v4(), acquisition(resources, None), Observation::default())
                    .is_err()
            );
        }
        // The exact rejected GTK-proof set is now a valid acquisition.
        let status = h.acquire(Uuid::new_v4(), linux());
        assert!(granted(&status, id(&status)));
    }

    #[test]
    fn nested_subset_borrows_without_releasing_parent_and_expansion_is_rejected() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        let first = h.acquire(
            task,
            ResourceSet {
                native_builds: 1,
                desktop_input: 1,
                ..Default::default()
            },
        );
        let borrowed = h.op(task, acquisition(build(), Some(id(&first))));
        assert_eq!(id(&first), id(&borrowed));
        assert!(borrowed.borrowed);
        assert_eq!(borrowed.reservations.len(), 1);
        assert!(
            h.broker
                .transaction(
                    task,
                    acquisition(ios(), Some(id(&first))),
                    Observation::default()
                )
                .is_err()
        );
        assert!(
            h.broker
                .transaction(
                    Uuid::new_v4(),
                    acquisition(build(), Some(id(&first))),
                    Observation::default()
                )
                .is_err()
        );
        assert!(
            h.broker
                .transaction(task, acquisition(build(), None), Observation::default())
                .is_err()
        );
    }

    /// An employee admitted with an empty declared set holds only a model
    /// claim: the ticket id its session inherits is not a resource parent,
    /// so first-use `resource` calls acquire their own set top-level
    /// instead of borrowing a subset that does not exist.
    #[test]
    fn model_only_admission_passes_inherited_acquires_through_to_top_level() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        let claim = || AdmissionClaim {
            daemon: Uuid::new_v4(),
            provider: "codex".into(),
            model: "gpt-5.5".into(),
            live_limit: u32::MAX,
            hard_cap: u32::MAX,
            allow_burst: false,
            skip_wait_order: false,
        };
        let ticket = h
            .broker
            .try_admission(
                task,
                Uuid::new_v4(),
                ResourceSet::default(),
                "ticket".into(),
                claim(),
            )
            .unwrap();
        assert!(ticket.granted);
        let ticket_id = ticket
            .status
            .reservations
            .iter()
            .find(|r| r.granted_at.is_some())
            .unwrap()
            .id;

        // The inherited ticket id passes through to a real acquisition.
        let first = h.op(task, acquisition(build(), Some(ticket_id)));
        let first_id = id(&first);
        assert_ne!(first_id, ticket_id);
        assert!(!first.borrowed);
        assert!(granted(&first, first_id));

        // Runs nested under the real reservation still borrow its subset,
        // and a second top-level set still cannot expand mid-hold.
        let nested = h.op(task, acquisition(build(), Some(first_id)));
        assert!(nested.borrowed);
        assert_eq!(id(&nested), first_id);
        assert!(
            h.broker
                .transaction(
                    task,
                    acquisition(
                        ResourceSet {
                            desktop_input: 1,
                            ..Default::default()
                        },
                        Some(ticket_id),
                    ),
                    Observation::default()
                )
                .is_err()
        );

        // An empty ticket also does not block a plain top-level acquire.
        let other = Uuid::new_v4();
        h.broker
            .try_admission(
                other,
                Uuid::new_v4(),
                ResourceSet::default(),
                "ticket".into(),
                claim(),
            )
            .unwrap();
        let direct = h.op(
            other,
            acquisition(
                ResourceSet {
                    desktop_input: 1,
                    ..Default::default()
                },
                None,
            ),
        );
        assert!(granted(&direct, id(&direct)));
    }

    /// A re-admission under a fresh id is the daemon's swap: the
    /// requesting task's own held claims — model slot and host set —
    /// stay out of the count, while every other task still sees the
    /// full pool.
    #[test]
    fn a_readmission_ignores_the_requesting_tasks_own_claims() {
        let h = Harness::new();
        let claim = || AdmissionClaim {
            daemon: Uuid::new_v4(),
            provider: "codex".into(),
            model: "gpt-5.5".into(),
            live_limit: 1,
            hard_cap: 1,
            allow_burst: false,
            skip_wait_order: false,
        };
        let task = Uuid::new_v4();
        let first = h
            .broker
            .try_admission(task, Uuid::new_v4(), build(), "first".into(), claim())
            .unwrap();
        assert!(first.granted);

        // Same task, fresh id, same claim + set: without the self
        // exclusion both the model lane (1/1) and the build pool (1/1)
        // would report full forever.
        let swap_id = Uuid::new_v4();
        let swap = h
            .broker
            .try_admission(task, swap_id, build(), "swap".into(), claim())
            .unwrap();
        assert!(
            swap.granted,
            "the swap re-admission blocked on its own claims: {:?}",
            swap.blockers
        );

        // Another task sees the pool as full as it is: both of the first
        // task's reservations count against it until one releases.
        let other = h
            .broker
            .try_admission(
                Uuid::new_v4(),
                Uuid::new_v4(),
                build(),
                "other".into(),
                claim(),
            )
            .unwrap();
        assert!(!other.granted);
        h.broker.release_admission(task, swap_id);
    }

    /// A force-start claim leapfrogs a parked waiter's fairness hold —
    /// the waiter loses only its place in line, never a resource it
    /// holds — while an ordinary admission still honors the FIFO shield.
    #[test]
    fn skip_wait_order_admission_leapfrogs_a_parked_waiter() {
        let h = Harness::new();
        // Desktop input is held elsewhere, so a wider request parks with
        // a claim on the still-free build pool.
        let held = h.acquire(
            Uuid::new_v4(),
            ResourceSet {
                desktop_input: 1,
                ..Default::default()
            },
        );
        assert!(granted(&held, id(&held)));
        let waiter = h.acquire(
            Uuid::new_v4(),
            ResourceSet {
                native_builds: 1,
                desktop_input: 1,
                ..Default::default()
            },
        );
        let waiter_id = id(&waiter);
        assert!(!granted(&waiter, waiter_id));

        let claim = |skip_wait_order| AdmissionClaim {
            daemon: Uuid::new_v4(),
            provider: "codex".into(),
            model: "gpt-5.5".into(),
            live_limit: u32::MAX,
            hard_cap: u32::MAX,
            allow_burst: false,
            skip_wait_order,
        };
        // An ordinary admission honors the waiter's place in line.
        let queued = h
            .broker
            .try_admission(Uuid::new_v4(), Uuid::new_v4(), build(), "queued".into(), claim(false))
            .unwrap();
        assert!(!queued.granted);
        assert!(
            queued.blockers.iter().any(|blocker| matches!(
                blocker,
                AdmissionBlocker::HostResources { detail }
                    if detail.contains("earlier resource request")
            )),
            "expected the FIFO shield as the reason: {:?}",
            queued.blockers
        );
        // The force claim takes the free build slot over the waiter's
        // order; the waiter stays parked, its held set untouched.
        let forced = h
            .broker
            .try_admission(Uuid::new_v4(), Uuid::new_v4(), build(), "forced".into(), claim(true))
            .unwrap();
        assert!(
            forced.granted,
            "a force claim leapfrogs the waiter: {:?}",
            forced.blockers
        );
        let status = h
            .broker
            .operate(Uuid::new_v4(), ResourceOperation::Status { id: None })
            .unwrap();
        let parked = status
            .reservations
            .iter()
            .find(|r| r.id == waiter_id)
            .unwrap();
        assert!(parked.granted_at.is_none() && !parked.released);
    }

    #[test]
    fn admission_state_ignores_unknown_claim_fields() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        let reservation = Uuid::new_v4();
        let claim = AdmissionClaim {
            daemon: Uuid::new_v4(),
            provider: "codex".into(),
            model: "gpt-5.5".into(),
            live_limit: 2,
            hard_cap: 2,
            allow_burst: false,
            skip_wait_order: false,
        };
        let admitted = h
            .broker
            .try_admission(task, reservation, build(), "test".into(), claim)
            .unwrap();
        assert!(admitted.granted);

        let path = h.broker.root.join("state.json");
        let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        state["reservations"][0]["admission"]["newer_admission_field"] =
            serde_json::json!("ignored by older daemons");
        fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();

        let loaded = h
            .broker
            .transaction(
                task,
                ResourceOperation::Status { id: None },
                Observation::default(),
            )
            .unwrap();
        let loaded_claim = loaded.reservations[0].admission.as_ref().unwrap();
        assert_eq!(loaded_claim.provider, "codex");
        assert!(!loaded_claim.skip_wait_order);
    }

    #[test]
    fn invalid_policy_or_ledger_fails_closed_and_unknown_inventory_blocks_resident_only() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let errors = || Observation {
            devices: vec![],
            blind: vec!["ios:".into(), "android:".into()],
            errors: vec!["probe failed".into()],
        };
        let native = h
            .broker
            .transaction(a, acquisition(build(), None), errors())
            .unwrap();
        assert!(granted(&native, id(&native)));
        let resident = h
            .broker
            .transaction(b, acquisition(ios(), None), errors())
            .unwrap();
        assert!(!granted(&resident, id(&resident)));
        fs::write(h.broker.root.join("policy.json"), b"broken").unwrap();
        assert!(
            h.broker
                .transaction(
                    a,
                    ResourceOperation::Status { id: None },
                    Observation::default()
                )
                .is_err()
        );
        fs::remove_file(h.broker.root.join("policy.json")).unwrap();
        fs::write(h.broker.root.join("state.json"), b"broken").unwrap();
        assert!(
            h.broker
                .transaction(
                    a,
                    ResourceOperation::Status { id: None },
                    Observation::default()
                )
                .is_err()
        );
        assert_eq!(
            fs::read(h.broker.root.join("state.json")).unwrap(),
            b"broken"
        );
    }

    #[test]
    fn android_inventory_recognizes_avds_and_unknown_emulators_without_claiming_them() {
        assert_eq!(
            android_devices(
                "/sdk/emulator /sdk/emulator -avd Pixel\nqemu-system-aarch64 qemu-system-aarch64 @Tablet\nqemu-system-x86_64 qemu-system-x86_64\n/bin/sh sh -c emulator"
            ),
            vec!["android:Pixel", "android:Tablet", "android:unknown"]
        );
    }

    #[test]
    fn capacity_policy_and_exclusive_device_names_are_validated() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        fs::create_dir_all(&h.broker.root).unwrap();
        fs::write(
            h.broker.root.join("policy.json"),
            serde_json::to_vec(&ResourcePolicy {
                native_builds: 2,
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let a = h.acquire(task, build());
        let b = h.acquire(Uuid::new_v4(), build());
        assert!(granted(&a, id(&a)));
        assert!(granted(&b, id(&b)));
        assert!(
            h.broker
                .transaction(
                    Uuid::new_v4(),
                    acquisition(
                        ResourceSet {
                            native_builds: 3,
                            ..Default::default()
                        },
                        None
                    ),
                    Observation::default()
                )
                .is_err()
        );
        assert!(
            h.broker
                .transaction(
                    Uuid::new_v4(),
                    acquisition(
                        ResourceSet {
                            exclusive: vec!["ios:bad".into()],
                            ..Default::default()
                        },
                        None
                    ),
                    Observation::default()
                )
                .is_err()
        );
    }

    #[test]
    #[cfg(unix)]
    fn reanchored_lease_survives_daemon_death_and_ends_with_its_owner() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        let first = h.acquire(task, build());
        let id = id(&first);
        // The lease moves its liveness anchor onto a surviving owner —
        // a live helper process stands in for the runtime host.
        let mut owner = Workload::start();
        h.op(
            task,
            ResourceOperation::Reanchor {
                id,
                owner_pid: owner.0.id(),
            },
        );
        // The transacting daemon dies — the lease stays live on its owner.
        h.mutate(|ledger| {
            let row = ledger
                .reservations
                .iter_mut()
                .find(|r| r.id == id)
                .unwrap();
            assert_eq!(row.owner_pid, Some(owner.0.id()));
            row.daemon_pid = u32::MAX / 2;
        });
        let status = h.op(task, ResourceOperation::Status { id: None });
        let row = status.reservations.iter().find(|r| r.id == id).unwrap();
        assert!(row.granted_at.is_some() && !row.cancelled && !row.released);

        // A lease whose owner also dies ends honestly.
        owner.stop();
        assert!(h
            .op(task, ResourceOperation::Status { id: None })
            .reservations
            .iter()
            .all(|r| r.id != id));
    }

    #[test]
    #[cfg(unix)]
    fn reanchor_moves_daemon_held_rows_but_keeps_agent_holders() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        // A daemon-held (admission-style) row — holder is the daemon itself.
        let held = h.acquire(task, build());
        let held_id = id(&held);
        // An agent-held row whose CLI outlives the daemon.
        let mut workload = Workload::start();
        let agent_task = Uuid::new_v4();
        let agent = h.op(
            agent_task,
            ResourceOperation::Acquire {
                resources: build(),
                purpose: "simulated test".into(),
                holder_pid: workload.0.id(),
                wait_seconds: 60,
                parent: None,
            },
        );
        let agent_id = id(&agent);
        h.op(
            task,
            ResourceOperation::Reanchor {
                id: held_id,
                owner_pid: std::process::id(),
            },
        );
        h.op(
            agent_task,
            ResourceOperation::Reanchor {
                id: agent_id,
                owner_pid: std::process::id(),
            },
        );
        // Now the transacting daemon dies: the daemon-held row answers
        // liveness through its surviving owner, and the agent-held row
        // keeps its own CLI as the anchor.
        h.mutate(|ledger| {
            for row in ledger
                .reservations
                .iter_mut()
                .filter(|r| r.id == held_id || r.id == agent_id)
            {
                row.daemon_pid = u32::MAX / 2;
            }
        });
        let status = h.op(task, ResourceOperation::Status { id: None });
        assert!(status
            .reservations
            .iter()
            .any(|r| r.id == held_id && !r.cancelled));
        assert!(status
            .reservations
            .iter()
            .any(|r| r.id == agent_id && !r.cancelled));
        // Agent holder death still ends its own row even under a live owner.
        workload.stop();
        let status = h.op(task, ResourceOperation::Status { id: None });
        assert!(status.reservations.iter().all(|r| r.id != agent_id));
        assert!(status
            .reservations
            .iter()
            .any(|r| r.id == held_id && !r.cancelled));
    }

    #[test]
    fn reanchor_refuses_dead_owners_foreign_tasks_and_dead_rows() {
        let h = Harness::new();
        let task = Uuid::new_v4();
        let first = h.acquire(task, build());
        let id = id(&first);
        // A dead pid can never become a surviving owner.
        assert!(h
            .broker
            .transaction(
                task,
                ResourceOperation::Reanchor {
                    id,
                    owner_pid: u32::MAX / 2
                },
                Observation::default()
            )
            .is_err());
        // Ownership fencing — no other task may touch the lease.
        assert!(h
            .broker
            .transaction(
                Uuid::new_v4(),
                ResourceOperation::Reanchor {
                    id,
                    owner_pid: std::process::id()
                },
                Observation::default()
            )
            .is_err());
        // Cancelled or released rows stay dead.
        h.op(task, ResourceOperation::Cancel { id });
        assert!(h
            .broker
            .transaction(
                task,
                ResourceOperation::Reanchor {
                    id,
                    owner_pid: std::process::id()
                },
                Observation::default()
            )
            .is_err());
    }

    /// A wait reason names the real blockers: the granted row holding an
    /// identical exclusive key, or each exhausted pool with every
    /// reservation retaining it — including released rows whose workload
    /// group or device still occupies capacity.
    #[test]
    #[cfg(unix)]
    fn wait_reason_names_the_exclusive_holder_and_every_pool_retainer() {
        const IOS: &str = "ios:91D40099-A056-497D-A572-ADDDFD6A8F23";
        // The inventory reports B's simulator only after B is granted.
        let observed = || Observation {
            devices: vec![IOS.into()],
            blind: vec![],
            errors: vec![],
        };
        let h = Harness::new();
        h.broker
            .set_policy(&ResourcePolicy {
                resident_devices: 2,
                native_builds: 3,
                desktop_input: 1,
            })
            .unwrap();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let set = |exclusive: &str, resident: u32, builds: u32| ResourceSet {
            exclusive: vec![exclusive.into()],
            resident_devices: resident,
            native_builds: builds,
            desktop_input: 0,
        };
        let acquire = |task: Uuid, resources: ResourceSet, observation: Observation| {
            h.broker
                .transaction(task, acquisition(resources, None), observation)
                .unwrap()
        };
        let op = |task: Uuid, operation: ResourceOperation| {
            h.broker.transaction(task, operation, observed()).unwrap()
        };
        let row = |status: &ResourceStatus, id: Uuid| {
            status
                .reservations
                .iter()
                .find(|r| r.id == id)
                .unwrap()
                .clone()
        };

        let first = acquire(
            a,
            set("android:octane-tuneful-conch", 1, 1),
            Observation::default(),
        );
        let a_id = id(&first);
        assert!(granted(&first, a_id));
        let mut workload = Workload::start();
        h.broker
            .transaction(
                a,
                ResourceOperation::Attach {
                    id: a_id,
                    workload_pid: workload.0.id(),
                },
                Observation::default(),
            )
            .unwrap();
        // B's iOS claim grants while the device has not been observed.
        let second = acquire(b, set(IOS, 1, 0), Observation::default());
        let b_id = id(&second);
        assert!(granted(&second, b_id));
        // A's holder stays alive (this test process), so the released row
        // is retained by its workload group; B's by the booted simulator.
        op(a, ResourceOperation::Release { id: a_id });
        op(b, ResourceOperation::Release { id: b_id });

        let parked = acquire(c, set("android:905d3a9e", 1, 0), observed());
        let parked_id = id(&parked);
        assert!(!granted(&parked, parked_id));
        let expected_pool = format!(
            "resident devices 2/2 held by task {a} (released; workload process \
             group {} still running), task {b} (released; device {IOS} retained)",
            workload.0.id()
        );
        assert_eq!(
            row(&parked, parked_id).waiting_on.as_deref(),
            Some(expected_pool.as_str())
        );
        op(c, ResourceOperation::Cancel { id: parked_id });
        // A Linux VM draws on the same resident pool and names it identically.
        let vm = acquire(c, set("linux:webkitgtk", 1, 0), observed());
        let vm_id = id(&vm);
        assert!(!granted(&vm, vm_id));
        assert_eq!(
            row(&vm, vm_id).waiting_on.as_deref(),
            Some(expected_pool.as_str())
        );
        op(c, ResourceOperation::Cancel { id: vm_id });
        // A physical device is a plain lock: no resident claim, it grants.
        let phone = acquire(
            c,
            ResourceSet {
                exclusive: vec!["device:905d3a9e".into()],
                ..Default::default()
            },
            observed(),
        );
        let phone_id = id(&phone);
        assert!(granted(&phone, phone_id));
        op(c, ResourceOperation::Release { id: phone_id });

        // The identical exclusive key names its own holder, not the pool.
        let again = acquire(b, set(IOS, 1, 0), observed());
        let again_id = id(&again);
        assert!(!granted(&again, again_id));
        assert_eq!(
            row(&again, again_id).waiting_on.as_deref(),
            Some(format!("device {IOS} held by task {b} (released; device {IOS} retained)").as_str())
        );
        workload.stop();
    }

    /// A holder that dies uncatchably never stops its workload's process
    /// group — the broker SIGTERMs the orphan, keeps the row until the
    /// group exits, and only then frees the capacity.
    #[test]
    #[cfg(unix)]
    fn orphaned_workload_is_terminated_after_its_holder_dies() {
        use std::os::unix::process::ExitStatusExt;
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = h.acquire(a, build());
        let first_id = id(&first);
        let mut workload = Workload::start();
        let attached = h.op(
            a,
            ResourceOperation::Attach {
                id: first_id,
                workload_pid: workload.0.id(),
            },
        );
        assert!(
            attached
                .reservations
                .iter()
                .find(|r| r.id == first_id)
                .unwrap()
                .workload_started
                .is_some()
        );
        // The runner died without stopping its group.
        h.mutate(|l| l.reservations[0].holder_pid = u32::MAX / 2);
        let second = h.acquire(b, build());
        let second_id = id(&second);
        assert!(!granted(&second, second_id));
        let row = second
            .reservations
            .iter()
            .find(|r| r.id == first_id)
            .unwrap();
        assert!(row.cancelled);
        assert!(row.reap_signalled_at.is_some());
        assert_eq!(workload.0.wait().unwrap().signal(), Some(libc::SIGTERM));
        let status = h.op(b, ResourceOperation::Status { id: None });
        assert!(granted(&status, second_id));
    }

    /// An orphan that ignores SIGTERM is escalated to SIGKILL once the
    /// grace period has elapsed.
    #[test]
    #[cfg(unix)]
    fn orphan_ignoring_term_is_killed_after_grace() {
        use std::os::unix::process::{CommandExt, ExitStatusExt};
        let h = Harness::new();
        let a = Uuid::new_v4();
        let first = h.acquire(a, build());
        let first_id = id(&first);
        // SIG_IGN survives exec, so this orphan ignores the TERM grace.
        let mut workload = Workload(
            Command::new("/bin/sh")
                .args(["-c", "trap '' TERM; exec /bin/sleep 30"])
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        h.op(
            a,
            ResourceOperation::Attach {
                id: first_id,
                workload_pid: workload.0.id(),
            },
        );
        h.mutate(|l| l.reservations[0].holder_pid = u32::MAX / 2);
        let signalled = h.op(a, ResourceOperation::Status { id: None });
        let row = signalled
            .reservations
            .iter()
            .find(|r| r.id == first_id)
            .unwrap();
        assert!(row.reap_signalled_at.is_some());
        assert!(workload.0.try_wait().unwrap().is_none());
        // Backdate the TERM so the next transaction sees the grace elapsed.
        h.mutate(|l| l.reservations[0].reap_signalled_at = Some(0));
        h.op(a, ResourceOperation::Status { id: None });
        assert_eq!(workload.0.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    /// Without a birth stamp matching the group leader, the broker never
    /// signals: a mismatched stamp means a reused PID, and a missing
    /// stamp means a row written before the fence existed.
    #[test]
    #[cfg(unix)]
    fn unfenced_workloads_are_never_signalled() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = h.acquire(a, build());
        let first_id = id(&first);
        let mut workload = Workload::start();
        h.op(
            a,
            ResourceOperation::Attach {
                id: first_id,
                workload_pid: workload.0.id(),
            },
        );
        // A stamp that is not this leader's: signalling stays fenced.
        h.mutate(|l| {
            let r = &mut l.reservations[0];
            r.holder_pid = u32::MAX / 2;
            r.workload_started = Some(1);
        });
        let second = h.acquire(b, build());
        let second_id = id(&second);
        assert!(!granted(&second, second_id));
        let row = second
            .reservations
            .iter()
            .find(|r| r.id == first_id)
            .unwrap();
        assert!(row.reap_signalled_at.is_none());
        assert!(workload.0.try_wait().unwrap().is_none());
        // Rows written before stamps existed are never signalled either.
        h.mutate(|l| {
            let r = &mut l.reservations[0];
            r.workload_started = None;
        });
        let status = h.op(b, ResourceOperation::Status { id: None });
        let row = status
            .reservations
            .iter()
            .find(|r| r.id == first_id)
            .unwrap();
        assert!(row.reap_signalled_at.is_none());
        assert!(!granted(&status, second_id));
        assert!(workload.0.try_wait().unwrap().is_none());
        workload.stop();
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn retained_idle_device_releases_build_and_desktop_capacity() {
        let root = std::env::temp_dir().join(format!("goddard-broker-retain-{}", Uuid::new_v4()));
        let broker = Broker { root: root.clone() };
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let device = "ios:00000000-0000-0000-0000-000000000001".to_string();
        let observation = || Observation {
            devices: vec![device.clone()],
            blind: vec![],
            errors: vec![],
        };
        let first = broker
            .transaction(
                a,
                ResourceOperation::Acquire {
                    resources: ResourceSet {
                        exclusive: vec![device.clone()],
                        resident_devices: 1,
                        native_builds: 1,
                        desktop_input: 1,
                    },
                    purpose: "mixed phase".into(),
                    holder_pid: std::process::id(),
                    wait_seconds: 60,
                    parent: None,
                },
                Observation::default(),
            )
            .unwrap();
        broker
            .transaction(
                a,
                ResourceOperation::Release {
                    id: first.request_id.unwrap(),
                },
                observation(),
            )
            .unwrap();
        let next = broker
            .transaction(
                b,
                ResourceOperation::Acquire {
                    resources: ResourceSet {
                        native_builds: 1,
                        desktop_input: 1,
                        ..Default::default()
                    },
                    purpose: "next build".into(),
                    holder_pid: std::process::id(),
                    wait_seconds: 60,
                    parent: None,
                },
                observation(),
            )
            .unwrap();
        assert!(
            next.reservations
                .iter()
                .find(|r| r.id == next.request_id.unwrap())
                .unwrap()
                .granted_at
                .is_some()
        );
        let retained = next
            .reservations
            .iter()
            .find(|r| r.id == first.request_id.unwrap())
            .unwrap();
        assert_eq!(retained.resources.resident_devices, 1);
        assert_eq!(retained.resources.native_builds, 0);
        assert_eq!(retained.resources.desktop_input, 0);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn process_worker() {
        let Some(root) = std::env::var_os("GODDARD_TEST_BROKER_ROOT") else {
            return;
        };
        let broker = Broker {
            root: PathBuf::from(root),
        };
        let task: Uuid = std::env::var("GODDARD_TEST_BROKER_TASK")
            .unwrap()
            .parse()
            .unwrap();
        broker
            .transaction(
                task,
                ResourceOperation::Acquire {
                    resources: ResourceSet {
                        native_builds: 1,
                        ..Default::default()
                    },
                    purpose: format!("project {}", std::env::current_dir().unwrap().display()),
                    holder_pid: std::process::id(),
                    wait_seconds: 60,
                    parent: None,
                },
                Observation::default(),
            )
            .unwrap();
        fs::write(broker.root.join(format!("ready-{task}")), b"").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !broker.root.join("done").exists() {
            assert!(Instant::now() < deadline, "cross-process harness timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn separate_processes_in_different_project_directories_share_authority() {
        let root = std::env::temp_dir().join(format!("goddard-broker-process-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut children = Vec::new();
        for index in 0..4 {
            let project = root.join(format!("project-{index}"));
            fs::create_dir(&project).unwrap();
            let task = Uuid::new_v4();
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "resource_broker::process_tests::process_worker"])
                .current_dir(project)
                .env("GODDARD_TEST_BROKER_ROOT", &root)
                .env("GODDARD_TEST_BROKER_TASK", task.to_string())
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            children.push((task, child));
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while children
            .iter()
            .any(|(task, _)| !root.join(format!("ready-{task}")).exists())
        {
            if Instant::now() >= deadline {
                for (_, child) in &mut children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                panic!("cross-process acquisitions timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let broker = Broker { root: root.clone() };
        let status = broker
            .transaction(
                Uuid::new_v4(),
                ResourceOperation::Status { id: None },
                Observation::default(),
            )
            .unwrap();
        // Clean child workloads before assertions so even a failed assertion leaves no processes.
        fs::write(root.join("done"), b"").unwrap();
        for (_, mut child) in children {
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(status.reservations.len(), 4);
        assert_eq!(
            status
                .reservations
                .iter()
                .filter(|r| r.granted_at.is_some())
                .count(),
            1
        );
        assert_eq!(
            broker
                .transaction(
                    Uuid::new_v4(),
                    ResourceOperation::Status { id: None },
                    Observation::default()
                )
                .unwrap()
                .reservations
                .len(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }
}
