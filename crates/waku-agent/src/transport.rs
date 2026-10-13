//! Opt-in stable ingress. An invalid descriptor never falls back to a daemon bearer.
use std::time::Duration;

use anyhow::{Context as _, bail};
use uuid::Uuid;
use waku_client::DaemonClient;
use waku_protocol::{Command, ResponsePayload};

pub enum AgentClient {
    Daemon(DaemonClient),
    #[cfg(unix)]
    Runtime(stable::Bridge),
}

impl AgentClient {
    pub fn connect() -> anyhow::Result<Self> {
        if let Some(path) = std::env::var_os("GODDARD_RUNTIME_INGRESS") {
            #[cfg(unix)]
            return stable::Bridge::open(std::path::Path::new(&path)).map(Self::Runtime);
            #[cfg(not(unix))]
            {
                let _ = path;
                bail!("stable agent ingress is unsupported on this platform");
            }
        }
        let address = std::env::var(waku_protocol::DAEMON_ADDRESS_ENV)
            .context("GODDARD_DAEMON_ADDRESS is not set; this session has no agent surface")?;
        let token = std::env::var(waku_protocol::AGENT_TOKEN_ENV)
            .context("GODDARD_AGENT_TOKEN is not set; this session has no agent surface")?;
        DaemonClient::connect(&address, token)
            .map(Self::Daemon)
            .context("could not reach the Goddard daemon")
    }

    pub fn request(
        &self,
        task: Uuid,
        runtime: Uuid,
        command: Command,
    ) -> anyhow::Result<ResponsePayload> {
        match self {
            Self::Daemon(client) => client.request(task, runtime, command),
            #[cfg(unix)]
            Self::Runtime(client) => client.request(task, command, Some(Duration::from_secs(120))),
        }
    }

    pub fn request_with_timeout(
        &self,
        task: Uuid,
        runtime: Uuid,
        command: Command,
        timeout: Option<Duration>,
    ) -> anyhow::Result<ResponsePayload> {
        match self {
            Self::Daemon(client) => client.request_with_timeout(task, runtime, command, timeout),
            #[cfg(unix)]
            Self::Runtime(client) => client.request(task, command, timeout),
        }
    }
}

#[cfg(unix)]
mod stable {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use waku_runtime::client::RuntimeClient;
    use waku_runtime::contract::{IngressDisposition, IngressKind, IngressRequest, SessionGrant};

    const MAX_RECORD_BYTES: u64 = 1024 * 1024;
    const MAX_RECORDS: usize = 1024;
    const MAX_RETAINED_BYTES: u64 = 16 * 1024 * 1024;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Descriptor {
        version: u32,
        root: PathBuf,
        task_id: Uuid,
        grant: SessionGrant,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Intent {
        task_id: Uuid,
        session_id: Uuid,
        generation: u64,
        request: IngressRequest,
    }

    pub struct Bridge {
        descriptor: Descriptor,
        records: PathBuf,
    }

    fn private(file: &File) -> anyhow::Result<()> {
        let meta = file.metadata()?;
        // SAFETY: geteuid has no pointer arguments or side effects.
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            bail!("stable ingress state must be owned by this user and private");
        }
        Ok(())
    }

    fn read<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| anyhow::anyhow!("stable ingress state cannot be opened"))?;
        private(&file)?;
        if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_RECORD_BYTES {
            bail!("stable ingress state is not a bounded regular file");
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            bail!("stable ingress state is oversized");
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            anyhow::anyhow!("stable ingress state is invalid; reconcile before retrying")
        })
    }

    fn now() -> anyhow::Result<u64> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis()
            .try_into()?)
    }

    impl Bridge {
        pub fn open(path: &Path) -> anyhow::Result<Self> {
            let descriptor: Descriptor = read(path)?;
            if descriptor.version != 1 {
                bail!("unsupported stable ingress descriptor version");
            }
            let records = path.with_extension("requests");
            match std::fs::DirBuilder::new().mode(0o700).create(&records) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => bail!("stable ingress request storage cannot be created"),
            }
            Ok(Self {
                descriptor,
                records,
            })
        }

        fn intent(
            &self,
            task: Uuid,
            command: Command,
            timeout: Option<Duration>,
        ) -> anyhow::Result<Intent> {
            if task != self.descriptor.task_id {
                bail!("stable ingress task binding mismatch");
            }
            let id = match std::env::var_os("GODDARD_RUNTIME_REQUEST_ID") {
                Some(value) => value
                    .to_str()
                    .context("invalid stable request identity")?
                    .parse::<Uuid>()?,
                None => Uuid::new_v4(),
            };
            let directory = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
                .open(&self.records)
                .map_err(|_| anyhow::anyhow!("stable ingress request storage cannot be opened"))?;
            private(&directory)?;
            // SAFETY: the live directory File owns this valid fd. Drop releases the lock.
            if unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX) } != 0 {
                bail!("stable ingress request storage cannot be locked");
            }
            let path = self.records.join(format!("{id}.json"));
            let subject = serde_json::to_value(command)?;
            if path.try_exists()? {
                let old: Intent = read(&path)?;
                if old.task_id != task
                    || old.session_id != self.descriptor.grant.session_id
                    || old.generation != self.descriptor.grant.generation
                    || old.request.request_id != id
                    || old.request.class != IngressKind::Cli
                    || old.request.subject != subject
                {
                    bail!("stable ingress request identity payload or owner mismatch");
                }
                return Ok(old);
            }
            let entries = std::fs::read_dir(&self.records)?
                .take(MAX_RECORDS)
                .collect::<Result<Vec<_>, _>>()?;
            if entries.len() >= MAX_RECORDS {
                bail!("stable ingress request retention is full; reconcile before new work");
            }
            let budget = timeout
                .unwrap_or(Duration::from_secs(600))
                .min(Duration::from_secs(600));
            let intent = Intent {
                task_id: task,
                session_id: self.descriptor.grant.session_id,
                generation: self.descriptor.grant.generation,
                request: IngressRequest {
                    request_id: id,
                    class: IngressKind::Cli,
                    subject,
                    deadline_ms: now()?
                        .checked_add(budget.as_millis().try_into()?)
                        .context("stable request deadline overflow")?,
                },
            };
            let bytes = serde_json::to_vec(&intent)?;
            if bytes.len() as u64 > MAX_RECORD_BYTES {
                bail!("stable ingress request is oversized");
            }
            let retained = entries.iter().try_fold(0u64, |total, entry| {
                Ok::<_, std::io::Error>(
                    total.saturating_add(std::fs::symlink_metadata(entry.path())?.len()),
                )
            })?;
            if retained.saturating_add(bytes.len() as u64) > MAX_RETAINED_BYTES {
                bail!("stable ingress request byte retention is full; reconcile before new work");
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|_| {
                    anyhow::anyhow!("stable ingress request intent cannot be persisted")
                })?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            directory.sync_all()?;
            Ok(intent)
        }

        pub fn request(
            &self,
            task: Uuid,
            command: Command,
            timeout: Option<Duration>,
        ) -> anyhow::Result<ResponsePayload> {
            let intent = self.intent(task, command, timeout)?;
            let id = intent.request.request_id;
            let mut client = RuntimeClient::agent(&self.descriptor.root, &self.descriptor.grant)
                .map_err(|_| anyhow::anyhow!("stable ingress authentication unavailable; retry request {id} with the same identity"))?;
            // Repeating this original intent only queries/returns the host's existing truth.
            // Lost transport acceptance is never retried under a fresh identity.
            let remaining = intent.request.deadline_ms.saturating_sub(now()?);
            client.set_request_timeout(Duration::from_millis(if remaining == 0 {
                10_000
            } else {
                remaining.min(10_000)
            }))?;
            // Expired intents may query retained outcomes, but cannot enter
            // ingress as new work when the original acceptance is unknown.
            let accepted = if remaining == 0 {
                client.ingress_query(id)
            } else {
                client.ingress(intent.request.clone())
            };
            let mut record = accepted.map_err(|_| {
                anyhow::anyhow!("stable ingress acceptance unconfirmed or deadline expired; reconcile request {id}")
            })?;
            loop {
                match record.disposition {
                    IngressDisposition::Completed { result } => {
                        return serde_json::from_value(result)
                            .context("stable ingress returned an invalid product response");
                    }
                    IngressDisposition::Failed { detail }
                    | IngressDisposition::Refused { detail } => bail!("{detail}"),
                    IngressDisposition::Uncertain { .. } => bail!(
                        "stable ingress result uncertain; reconcile request {id}; do not resend the operation"
                    ),
                    IngressDisposition::Expired => {
                        bail!("stable ingress request {id} expired before dispatch")
                    }
                    IngressDisposition::Pending | IngressDisposition::Forwarded => {}
                }
                let remaining = intent.request.deadline_ms.saturating_sub(now()?);
                if remaining == 0 {
                    bail!(
                        "stable ingress deadline reached; query request {id}; acceptance may be uncertain"
                    );
                }
                client.set_request_timeout(Duration::from_millis(remaining.min(10_000)))?;
                record = client.ingress_wait(id, remaining.min(1000)).map_err(|_| {
                    anyhow::anyhow!("stable ingress response unconfirmed; reconcile request {id}")
                })?;
            }
        }
    }
}
