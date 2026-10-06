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
    errors: Vec<String>,
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
            if !waku_protocol::pid::is_alive(r.daemon_pid)
                || !waku_protocol::pid::is_alive(r.holder_pid)
            {
                r.cancelled = true;
            }
            if r.granted_at.is_none() {
                return !r.cancelled
                    && !r.released
                    && r.deadline > now
                    && waku_protocol::pid::is_alive(r.holder_pid);
            }
            let live_work = r.workload_pid.is_some_and(group_alive);
            let live_device = r
                .resources
                .exclusive
                .iter()
                .any(|key| observation.devices.contains(key))
                || (r.resources.resident_devices > 0 && !observation.errors.is_empty());
            if !live_work && (r.cancelled || r.released) && live_device {
                // A finished workload releases build/input capacity even when its
                // idle simulator service remains resident outside the process group.
                r.resources.native_builds = 0;
                r.resources.desktop_input = 0;
                r.resources.exclusive.retain(|key| {
                    (key.starts_with("ios:") || key.starts_with("android:"))
                        && (!observation.errors.is_empty() || observation.devices.contains(key))
                });
                r.resources.resident_devices = r.resources.exclusive.len() as u32;
            }
            live_work
                || live_device
                || (!r.cancelled && !r.released && waku_protocol::pid::is_alive(r.holder_pid))
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
                    if ledger
                        .reservations
                        .iter()
                        .any(|r| {
                            r.task == task
                                && r.granted_at.is_some()
                                && !r.released
                                && !r.resources.is_empty()
                        })
                    {
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
                        workload_pid: None,
                        requested_at: now,
                        duration_seconds: 0,
                        deadline: now + u64::from(wait_seconds),
                        granted_at: None,
                        cancelled: false,
                        released: false,
                        admission: None,
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
                            workload_pid: None,
                            requested_at: now,
                            duration_seconds: 0,
                            // Granted rows ignore the deadline; it exists
                            // only for parked Acquire requests.
                            deadline: now,
                            granted_at: Some(now),
                            cancelled: false,
                            released: false,
                            admission: Some(claim),
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
        let external_devices = observation
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
    let devices = r
        .exclusive
        .iter()
        .filter(|s| s.starts_with("ios:") || s.starts_with("android:"))
        .count() as u32;
    if devices != r.resident_devices {
        bail!(
            "resident_devices must equal the number of ios:<UDID>/android:<AVD> exclusive resources"
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
    } else if starves_earlier_waiter(request, ledger) {
        blockers.push(AdmissionBlocker::HostResources {
            detail: "an earlier resource request is waiting for the same capacity".into(),
        });
    }
    blockers
}

/// Whether granting `request` now would leapfrog a parked `Acquire` that
/// needs the same capacity. A pending request counts as sharing when it
/// names an identical exclusive key or draws on the same counted pool.
fn starves_earlier_waiter(request: &ResourceSet, ledger: &Ledger) -> bool {
    ledger
        .reservations
        .iter()
        .filter(|r| r.granted_at.is_none() && !r.cancelled && !r.released)
        .any(|waiting| {
            let waiting = &waiting.resources;
            waiting
                .exclusive
                .iter()
                .any(|key| request.exclusive.contains(key))
                || (waiting.resident_devices > 0 && request.resident_devices > 0)
                || (waiting.native_builds > 0 && request.native_builds > 0)
                || (waiting.desktop_input > 0 && request.desktop_input > 0)
        })
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
    let external: Vec<_> = observation
        .devices
        .iter()
        .filter(|key| !held.iter().any(|r| r.resources.exclusive.contains(key)))
        .collect();
    // An external device conflicts only with a request for that same
    // exclusive key. It is user-owned, so it does not consume our shared
    // resident-device capacity.
    if request.resident_devices > 0
        && (!observation.errors.is_empty()
            || external.iter().any(|key| request.exclusive.contains(key)))
    {
        return true;
    }
    let claims = held.iter().map(|r| &r.resources).chain(
        ledger.reservations[..queued_before]
            .iter()
            .filter(|r| r.granted_at.is_none() && !r.cancelled && !r.released)
            .map(|r| &r.resources),
    );
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

pub fn waiting_title(r: &Reservation, status: &ResourceStatus) -> String {
    let label = if r.resources.resident_devices > 0 {
        r.resources.exclusive.join(", ")
    } else if r.resources.native_builds > 0 {
        "native build".into()
    } else {
        "desktop input".into()
    };
    let owner = status.reservations.iter().find(|h| {
        h.id != r.id
            && h.granted_at.is_some()
            && (h
                .resources
                .exclusive
                .iter()
                .any(|key| r.resources.exclusive.contains(key))
                || (r.resources.resident_devices > 0 && h.resources.resident_devices > 0)
                || (r.resources.native_builds > 0 && h.resources.native_builds > 0)
                || (r.resources.desktop_input > 0 && h.resources.desktop_input > 0))
    });
    match owner {
        Some(owner) => format!("Waiting for {label}—held by task {}", owner.task),
        None if !status.external_devices.is_empty() && r.resources.resident_devices > 0 => {
            format!("Waiting for {label}—user-owned device running")
        }
        None => format!("Waiting for {label}—queued for host capacity"),
    }
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
                    observation
                        .errors
                        .push("iOS device inventory unavailable".into());
                }
            }
            Err(_) => observation
                .errors
                .push("iOS device inventory unavailable; resident acquisitions blocked".into()),
        }
    }
    #[cfg(unix)]
    match probe("/bin/ps", &["-axo", "comm=,args="]) {
        Ok(bytes) => observation
            .devices
            .extend(android_devices(&String::from_utf8_lossy(&bytes))),
        Err(_) => observation
            .errors
            .push("Android process inventory unavailable; resident acquisitions blocked".into()),
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
        assert_eq!(
            status.observation_errors,
            ["device inventory not probed"]
        );
        let status = h
            .broker
            .operate(task, ResourceOperation::Release { id: id(&status) })
            .unwrap();
        assert_eq!(
            status.observation_errors,
            ["device inventory not probed"]
        );
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

    #[test]
    fn invalid_policy_or_ledger_fails_closed_and_unknown_inventory_blocks_resident_only() {
        let h = Harness::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let errors = || Observation {
            devices: vec![],
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
