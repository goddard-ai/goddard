//! Read-only physical-writer checks after daemon replacement. These are
//! necessary preconditions; the executor still needs final workspace admission.
use super::*;
use waku_runtime::contract::{CommandDisposition, HostStatus};

pub(crate) fn require_quiet_workspace_writers(
    backend: &WakuBackend,
    path: &Path,
) -> anyhow::Result<()> {
    let store = backend.task_store();
    anyhow::ensure!(
        !store.stable_writers_have_unknown_links()?,
        "workspace cleanup deferred: a surviving stable link has unknown writer origin"
    );
    let target = dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let mut hosts: HashMap<(PathBuf, Uuid), (RuntimeClient, HostStatus)> = HashMap::new();
    let mut after = None;
    loop {
        let page = store.stable_writer_page(after.as_deref())?;
        if page.is_empty() {
            return Ok(());
        }
        after = page.last().map(|origin| origin.key());
        for origin in page {
            let workspace =
                dunce::canonicalize(&origin.workspace).unwrap_or(origin.workspace.clone());
            if !(workspace.starts_with(&target) || target.starts_with(&workspace)) {
                continue;
            }
            let key = (origin.root.clone(), origin.runtime_instance);
            if !hosts.contains_key(&key) {
                // Retain at most eight host inventories/connections while
                // traversing arbitrarily many small durable metadata pages.
                if hosts.len() == 8 {
                    hosts.clear();
                }
                anyhow::ensure!(
                    origin.root.canonicalize()? == origin.root,
                    "workspace cleanup deferred: recorded provider root changed"
                );
                let credentials = load_credentials(&origin.root)?;
                let (mut client, hello) = RuntimeClient::controller(
                    &origin.root,
                    &credentials.controller,
                    &ClientHello::current(),
                )
                .map_err(link_error)?;
                anyhow::ensure!(
                    hello.runtime_instance == origin.runtime_instance,
                    "workspace cleanup deferred: recorded provider host changed"
                );
                // Status is credential-authenticated but does not acquire or
                // replace the live daemon's control epoch, nor admit a provider.
                let status = client.status().map_err(link_error)?;
                anyhow::ensure!(
                    status.runtime_instance == origin.runtime_instance && status.persistence_ok,
                    "workspace cleanup deferred: provider host truth is unavailable"
                );
                hosts.insert(key.clone(), (client, status));
            }
            let (client, status) = hosts.get_mut(&key).expect("host was inserted");
            let sref = SessionRef {
                session_id: origin.runtime_session_id,
                generation: origin.generation,
            };
            let quiet = match status
                .sessions
                .iter()
                .find(|session| session.session == sref)
            {
                Some(session) => matches!(
                    session.status,
                    SessionStatus::Terminated | SessionStatus::Exited
                ),
                // Absence alone is not evidence. A rejected original admission
                // proves there was no owned writer, including a lost reply.
                None => client
                    .command(origin.runtime_session_id)
                    .map_err(link_error)?
                    .is_some_and(|receipt| {
                        matches!(receipt.disposition, CommandDisposition::Rejected { .. })
                    }),
            };
            anyhow::ensure!(
                quiet,
                "workspace cleanup deferred: a durable provider writer is active or its stop is unconfirmed"
            );
        }
    }
}
