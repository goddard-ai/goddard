use super::*;

/// Collect the runtimes safe to evict right now — a runtime is only
/// reclaimable when its task can come back: nothing mid-turn, parked, or
/// queued for delivery, and the session either never produced provider
/// state or holds a resume cursor to rebuild it. `under_pressure` comes
/// from the OS's memory-pressure signal and bypasses only the idle-age
/// cutoff — eligibility rules (`runtime_evictable`) still gate every
/// runtime, and a `runtime_idle_timeout_secs` of `0` still disables
/// eviction outright. Entries are removed here; hub retirement and process
/// teardown are the caller's job, off this lock.
pub(super) fn reap_idle_runtimes(
    sessions: &Mutex<HashMap<Uuid, RuntimeEntry>>,
    task_state: &Mutex<PersistedState>,
    settings: &DaemonSettingsStore,
    agent: &crate::agent::AgentState,
    under_pressure: bool,
) -> Vec<(Uuid, Uuid, DriverHandle)> {
    let timeout = match settings.get().runtime_idle_timeout_secs {
        Some(0) => return Vec::new(),
        Some(secs) => std::time::Duration::from_secs(secs),
        None => DEFAULT_RUNTIME_IDLE_TIMEOUT,
    };
    let cutoff = std::time::Instant::now() - timeout;
    let state = task_state.lock();
    let mut sessions = sessions.lock();
    let evictable = sessions
        .iter()
        .filter(|(session_id, entry)| {
            (under_pressure || entry.last_active <= cutoff)
                && runtime_evictable(&state, **session_id, entry, agent)
        })
        .map(|(session_id, _)| *session_id)
        .collect::<Vec<_>>();
    evictable
        .into_iter()
        .filter_map(|session_id| {
            sessions
                .remove(&session_id)
                .map(|entry| (session_id, entry.runtime_id, entry.driver))
        })
        .collect()
}

/// Retire one runtime the reaper claimed: shut the provider down, release
/// its agent bookkeeping and workspace-index registration, then tell attached
/// clients the runtime ended. The notification must run before
/// `end_session_runtime` clears the hub's routing — afterwards the event
/// would be dropped as stale — and without it a client keeps its driver
/// handle, so the next prompt vanishes into a runtime the daemon no
/// longer has: runtime commands are fire-and-forget and carry no response.
pub(super) fn evict_idle_runtime(
    session_id: Uuid,
    runtime_id: Uuid,
    driver: DriverHandle,
    agent: &crate::agent::AgentState,
    repo_maps: &Arc<(Mutex<RepoMaps>, Condvar)>,
    events: &EventSink,
) {
    driver.begin_shutdown();
    // The scoped credential was valid only while the provider process
    // carrying it lived; the turn bookkeeping and pending steers die with
    // it too.
    agent.clear_session(session_id);
    {
        let mut maps = repo_maps.0.lock();
        maps.sessions.remove(&session_id);
    }
    let sink = events.for_session(session_id, runtime_id);
    sink.notify_runtime_ended();
    sink.end_session_runtime();
    drop_detached(driver);
}

/// Whether killing this task's provider process loses nothing the next
/// prompt cannot rebuild. Busy is checked twice — the persisted status and
/// the forwarder's turn bookkeeping — because either can be the fresher
/// signal when they disagree. Resumability comes from the runtime entry,
/// not the catalog row: a skeletonized session reads `provider_cursor:
/// None` until its next hydrate.
pub(super) fn runtime_evictable(
    state: &PersistedState,
    session_id: Uuid,
    entry: &RuntimeEntry,
    agent: &crate::agent::AgentState,
) -> bool {
    let Some(session) = state.sessions.iter().find(|s| s.id == session_id) else {
        // No catalog row claims this runtime — eviction only helps.
        return true;
    };
    if session.status.is_busy()
        || agent.has_open_turn(session_id)
        || agent.has_queued(session_id)
        || (session.detail_loaded && !session.queued_messages.is_empty())
    {
        return false;
    }
    entry.resumable || !session.has_started()
}
