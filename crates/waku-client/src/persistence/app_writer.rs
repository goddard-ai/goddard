//! A single latest-value slot: disk latency never creates a queue of obsolete
//! desktop snapshots. The mutex protects bookkeeping, never serialization or I/O.

use super::*;

type Outcome = Result<(), (io::ErrorKind, String)>;

pub(super) struct Snapshot {
    pub settings: Option<AppSettings>,
    pub state: AppState,
    pub settings_path: PathBuf,
    pub state_path: PathBuf,
}

struct Pending {
    issued: u64,
    durable: u64,
    finished: u64,
    next: Option<(u64, Snapshot)>,
    error: Option<String>,
    outcome: Outcome,
    waiters: Vec<(u64, smol::channel::Sender<Outcome>)>,
}

impl Default for Pending {
    fn default() -> Self {
        Self {
            issued: 0,
            durable: 0,
            finished: 0,
            next: None,
            error: None,
            outcome: Ok(()),
            waiters: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub(super) struct Writer {
    pending: Arc<Mutex<Pending>>,
    wake: crossbeam_channel::Sender<()>,
}

/// A durability barrier for one desktop snapshot. Awaiting it never blocks the
/// UI thread. A newer durable snapshot also satisfies an older barrier.
pub struct AppSaveTicket {
    writer: Writer,
    generation: u64,
}

fn outcome(result: Outcome) -> io::Result<()> {
    result.map_err(|(kind, message)| io::Error::new(kind, message))
}

impl AppSaveTicket {
    pub async fn wait(self) -> io::Result<()> {
        let receiver = {
            let mut pending = self.writer.pending.lock();
            if pending.durable >= self.generation {
                return Ok(());
            }
            if pending.finished >= self.generation {
                return outcome(pending.outcome.clone());
            }
            let (sender, receiver) = smol::channel::bounded(1);
            pending.waiters.retain(|(_, sender)| !sender.is_closed());
            pending.waiters.push((self.generation, sender));
            receiver
        };
        outcome(receiver.recv().await.map_err(|_| {
            io::Error::other("desktop state writer stopped before acknowledging the save")
        })?)
    }
}

impl Writer {
    pub(super) fn start() -> io::Result<Self> {
        // Fingerprints live on the worker; constructing or hashing JSON
        // must never happen in the caller's enqueue path.
        let settings_written = Mutex::new(0);
        let state_written = Mutex::new(0);
        Self::with_writer(move |snapshot| {
            if let Some(settings) = snapshot.settings {
                write_app_file(&settings, &snapshot.settings_path, &settings_written)?;
            }
            write_app_file(&snapshot.state, &snapshot.state_path, &state_written)
        })
    }

    fn with_writer(
        mut write: impl FnMut(Snapshot) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let pending = Arc::new(Mutex::new(Pending {
            outcome: Ok(()),
            ..Pending::default()
        }));
        let (wake, receiver) = crossbeam_channel::bounded(1);
        let shared = Arc::clone(&pending);
        std::thread::Builder::new()
            .name("waku-app-state-writer".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    loop {
                        let Some((generation, snapshot)) = shared.lock().next.take() else {
                            break;
                        };
                        let result =
                            write(snapshot).map_err(|error| (error.kind(), error.to_string()));
                        let mut state = shared.lock();
                        state.finished = generation;
                        if result.is_ok() {
                            state.durable = generation;
                        } else if let Err((_, message)) = &result {
                            state.error = Some(message.clone());
                        }
                        state.outcome = result.clone();
                        state.waiters.retain(|(requested, sender)| {
                            if *requested <= generation {
                                let _ = sender.try_send(result.clone());
                                false
                            } else {
                                !sender.is_closed()
                            }
                        });
                    }
                }
            })?;
        Ok(Self { pending, wake })
    }

    pub(super) fn queue(&self, snapshot: Snapshot) -> io::Result<AppSaveTicket> {
        let generation = {
            let mut pending = self.pending.lock();
            pending.issued = pending
                .issued
                .checked_add(1)
                .ok_or_else(|| io::Error::other("desktop save generation exhausted"))?;
            let generation = pending.issued;
            pending.next = Some((generation, snapshot));
            generation
        };
        match self.wake.try_send(()) {
            Ok(()) | Err(crossbeam_channel::TrySendError::Full(())) => Ok(AppSaveTicket {
                writer: self.clone(),
                generation,
            }),
            Err(crossbeam_channel::TrySendError::Disconnected(())) => {
                Err(io::Error::other("desktop state writer is closed"))
            }
        }
    }

    pub(super) fn ticket(&self) -> AppSaveTicket {
        AppSaveTicket {
            writer: self.clone(),
            generation: self.pending.lock().issued,
        }
    }

    pub(super) fn status(&self) -> (bool, Option<String>) {
        let mut pending = self.pending.lock();
        (pending.finished < pending.issued, pending.error.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(title: &str) -> Snapshot {
        let state = PersistedState::empty().app_state();
        Snapshot {
            settings: None,
            state,
            settings_path: PathBuf::new(),
            state_path: PathBuf::from(title),
        }
    }

    #[test]
    fn delayed_writes_coalesce_without_blocking_enqueue_or_acknowledging_early() {
        let (started, starts) = crossbeam_channel::bounded(1);
        let (release, releases) = crossbeam_channel::bounded(1);
        let (written, writes) = crossbeam_channel::unbounded();
        let writer = Writer::with_writer(move |snapshot| {
            if snapshot.state_path == Path::new("first") {
                started.send(()).unwrap();
                releases.recv().unwrap();
            }
            written.send(snapshot.state_path).unwrap();
            Ok(())
        })
        .unwrap();
        let first = writer.queue(snapshot("first")).unwrap();
        starts
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let superseded = writer.queue(snapshot("obsolete")).unwrap();
        let latest = writer.queue(snapshot("latest")).unwrap();
        assert!(writer.status().0);
        assert_eq!(writer.pending.lock().durable, 0);
        release.send(()).unwrap();
        smol::block_on(first.wait()).unwrap();
        smol::block_on(superseded.wait()).unwrap();
        smol::block_on(latest.wait()).unwrap();
        assert_eq!(
            writes.try_iter().collect::<Vec<_>>(),
            [PathBuf::from("first"), PathBuf::from("latest")]
        );
    }

    #[test]
    fn failed_write_is_reported_and_a_later_save_can_recover() {
        let writer = Writer::with_writer(|snapshot| {
            if snapshot.state_path == Path::new("fail") {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected write failure",
                ))
            } else {
                Ok(())
            }
        })
        .unwrap();
        let ticket = writer.queue(snapshot("fail")).unwrap();
        assert_eq!(
            smol::block_on(ticket.wait()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(writer.status().1.is_some());
        smol::block_on(writer.queue(snapshot("recover")).unwrap().wait()).unwrap();
        assert!(!writer.status().0);
    }
}
