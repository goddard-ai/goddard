//! SSH transport for remote daemons.
//!
//! Remote hosts connect through the platform `ssh` binary so the user's
//! `~/.ssh/config`, ProxyJump, ssh-agent, GSSAPI, and known_hosts semantics
//! keep working unchanged. Each host gets a ControlMaster socket that
//! multiplexes provisioning commands and the `-L` forward carrying the
//! daemon websocket, so only the first connect pays for authentication.
//!
//! Password and passphrase prompts reach the app through `SSH_ASKPASS`: the
//! helper script announces a per-request fifo pair on the queue fifo, writes
//! the prompt, and blocks reading the answer — which the app supplies after
//! showing a native prompt dialog.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use uuid::Uuid;

/// One pending password/passphrase request from an ssh child. The prompt
/// text comes from ssh itself (for example `user@host's password:`), and
/// the answer is delivered by writing it to `response`.
pub(super) struct SshAskpassRequest {
    pub prompt: String,
    response: PathBuf,
}

impl SshAskpassRequest {
    /// Write the answer (or an empty line on cancel) back to the waiting
    /// askpass helper. Blocking: call from a background task.
    pub(super) fn answer(&self, answer: &str) -> anyhow::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&self.response)
            .context("could not open askpass response fifo")?;
        file.write_all(answer.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }
}

/// The helper ssh invokes for credentials. It announces a private fifo pair
/// on the shared queue, writes the prompt, and prints whatever answer it
/// reads — which is what ssh consumes on stdout. `%QUEUE%` is replaced with
/// the queue fifo's absolute path when the script is written.
const ASKPASS_SCRIPT: &str = r#"#!/bin/sh
dir=$(mktemp -d "${TMPDIR:-/tmp}/waku-askpass.XXXXXX") || exit 1
mkfifo "$dir/in" "$dir/out" 2>/dev/null || { rm -rf "$dir"; exit 1; }
printf '%s\n' "$dir" > "%QUEUE%"
printf '%s\n' "$1" > "$dir/in"
cat "$dir/out"
status=$?
rm -rf "$dir"
exit $status
"#;

/// Provision and start the remote daemon, printing `token\nport` on
/// success. The token persists at `~/.waku/daemon-token` so re-connects
/// reuse it, and the daemon is started detached so it survives the ssh
/// session ending.
const REMOTE_BOOTSTRAP: &str = r#"set -u
waku_dir="$HOME/.waku"
mkdir -p "$waku_dir" 2>/dev/null || { echo "cannot create $waku_dir" >&2; exit 4; }
chmod 700 "$waku_dir" 2>/dev/null || true

token_file="$waku_dir/daemon-token"
if [ ! -f "$token_file" ]; then
  ( umask 077; LC_ALL=C tr -dc 'a-f0-9' < /dev/urandom | head -c 48 > "$token_file" 2>/dev/null ) \
    || { echo "cannot create daemon token" >&2; exit 5; }
fi
token=$(cat "$token_file")

pid_file="$waku_dir/daemon.pid"
ready_file="$waku_dir/daemon.ready"

running=0
if [ -f "$pid_file" ]; then
  pid=$(cat "$pid_file" 2>/dev/null || true)
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    running=1
  fi
fi

if [ "$running" -eq 0 ]; then
  bin=$(command -v waku-daemon 2>/dev/null || true)
  if [ -z "$bin" ] && [ -x "$waku_dir/bin/waku-daemon" ]; then
    bin="$waku_dir/bin/waku-daemon"
  fi
  if [ -z "$bin" ]; then
    echo "waku-daemon binary not found on the remote host" >&2
    exit 3
  fi
  : > "$ready_file" 2>/dev/null || true
  WAKU_DAEMON_TOKEN="$token" nohup "$bin" --bind 127.0.0.1:0 \
    >"$ready_file" 2>>"$waku_dir/daemon.log" </dev/null &
  echo $! > "$pid_file"
fi

i=0
while [ ! -s "$ready_file" ]; do
  i=$((i + 1))
  if [ "$i" -gt 100 ]; then
    echo "waku-daemon did not report ready" >&2
    exit 6
  fi
  sleep 0.1 2>/dev/null || sleep 1
done

port=$(sed -n 's/.*"address":"[^"]*:\([0-9][0-9]*\)".*/\1/p' "$ready_file" | head -1)
if [ -z "$port" ]; then
  echo "waku-daemon reported no bound port" >&2
  exit 7
fi
printf '%s\n%s\n' "$token" "$port"
"#;

/// State directory shared by control sockets and the askpass queue.
fn ssh_dir() -> PathBuf {
    waku_client::persistence::StateStore::default_path().with_file_name("ssh")
}

/// The queue fifo askpass helpers write their request directories to.
fn askpass_queue_path() -> PathBuf {
    ssh_dir().join("askpass-queue")
}

fn askpass_script_path() -> PathBuf {
    ssh_dir().join("askpass.sh")
}

/// Write the askpass helper and create the queue fifo. Idempotent; the
/// script is refreshed on every call so an updated app replaces it.
pub(super) fn prepare_askpass() -> anyhow::Result<()> {
    let dir = ssh_dir();
    std::fs::create_dir_all(&dir).context("could not create ~/.waku/ssh")?;
    let script = ASKPASS_SCRIPT.replace("%QUEUE%", &askpass_queue_path().to_string_lossy());
    std::fs::write(askpass_script_path(), script).context("could not write askpass helper")?;
    let mut permissions = std::fs::metadata(askpass_script_path())?.permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o700);
    std::fs::set_permissions(askpass_script_path(), permissions)?;

    let queue = askpass_queue_path();
    if !queue.exists() {
        let path = queue.to_string_lossy().into_owned();
        let status = Command::new("mkfifo").arg(&path).status()?;
        if !status.success() {
            bail!("mkfifo {path} failed");
        }
    }
    Ok(())
}

/// Read askpass request directories off the queue fifo forever, forwarding
/// each prompt. FIFO opens block until a writer arrives, so the loop idles
/// between requests and resumes on the next one.
pub(super) fn askpass_responder_loop(sender: smol::channel::Sender<SshAskpassRequest>) {
    let queue = askpass_queue_path();
    loop {
        let Ok(file) = std::fs::File::open(&queue) else {
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(dir) = line else { break };
            let dir = PathBuf::from(dir.trim());
            let mut prompt = String::new();
            let read = std::fs::File::open(dir.join("in"))
                .and_then(|file| BufReader::new(file).read_line(&mut prompt));
            if read.is_err() {
                continue;
            }
            let _ = sender.send_blocking(SshAskpassRequest {
                prompt: prompt.trim_end().to_owned(),
                response: dir.join("out"),
            });
        }
    }
}

/// One host's ControlMaster-managed ssh channel.
#[derive(Clone)]
pub(super) struct SshTransport {
    host: Uuid,
    destination: String,
}

impl SshTransport {
    pub(super) fn new(host: Uuid, destination: &str) -> Self {
        Self {
            host,
            destination: destination.to_owned(),
        }
    }

    fn control_path(&self) -> PathBuf {
        ssh_dir().join(format!("{}.ctl", self.host))
    }

    /// `ssh` with the control socket and askpass environment pinned. Every
    /// invocation goes through this so a prompt can never fall back to a
    /// terminal that does not exist.
    fn command(&self) -> Command {
        let mut command = Command::new("ssh");
        command
            .arg("-o")
            .arg("ControlPath")
            .arg(self.control_path())
            .arg("-o")
            .arg("ControlPersist=600")
            .arg("-o")
            .arg("ConnectTimeout=15")
            .env("SSH_ASKPASS", askpass_script_path())
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "goddard:0")
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());
        command
    }

    /// Establish or reuse the master connection. `-MNf` daemonizes the
    /// master after authentication succeeds, so a non-zero exit here means
    /// auth or reachability failed and stderr carries why.
    fn ensure_master(&self) -> anyhow::Result<()> {
        if self.master_alive() {
            return Ok(());
        }
        let output = self
            .command()
            .arg("-o")
            .arg("ControlMaster=yes")
            .arg("-MNf")
            .arg(&self.destination)
            .output()
            .context("could not run ssh")?;
        if !output.status.success() {
            bail!(
                "ssh to {} failed: {}",
                self.destination,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Whether the control master is up.
    pub(super) fn master_alive(&self) -> bool {
        self.control_path().exists()
            && self
                .command()
                .arg("-O")
                .arg("check")
                .arg(&self.destination)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }

    /// Run a script on the remote through the master connection.
    fn run_remote(&self, script: &str) -> anyhow::Result<String> {
        self.ensure_master()?;
        let mut child = self
            .command()
            .arg(&self.destination)
            .arg("sh -s")
            .stdin(Stdio::piped())
            .spawn()
            .context("could not run remote command over ssh")?;
        child
            .stdin
            .take()
            .context("ssh stdin was not piped")?
            .write_all(script.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "remote provisioning on {} failed: {}",
                self.destination,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("remote provisioning returned non-utf8 output")
    }

    /// Ensure the remote daemon is running and return its token plus the
    /// port it bound on the remote loopback.
    fn provision(&self) -> anyhow::Result<(String, u16)> {
        let output = self.run_remote(REMOTE_BOOTSTRAP)?;
        let mut lines = output.lines();
        let token = lines
            .next()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .context("remote provisioning returned no token")?
            .to_owned();
        let port = lines
            .next()
            .and_then(|line| line.trim().parse::<u16>().ok())
            .context("remote provisioning returned no port")?;
        Ok((token, port))
    }

    /// Point a local ephemeral port at the remote daemon's loopback port on
    /// the existing master. Fails fast when the forward cannot bind.
    fn add_forward(&self, local_port: u16, remote_port: u16) -> anyhow::Result<()> {
        let output = self
            .command()
            .arg("-O")
            .arg("forward")
            .arg("-L")
            .arg(format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"))
            .arg(&self.destination)
            .output()
            .context("could not run ssh forward")?;
        if !output.status.success() {
            bail!(
                "ssh port forward to {} failed: {}",
                self.destination,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Ensure master + provisioning + forward and connect a supervisor to
    /// the forwarded daemon. Returns the local port the supervisor dialed —
    /// `restore_forward` rebinds it after the master restarts.
    pub(super) fn connect(&self) -> anyhow::Result<(u16, waku_client::DaemonSupervisor)> {
        self.ensure_master()?;
        let (token, remote_port) = self.provision()?;
        let local_port = free_local_port()?;
        self.add_forward(local_port, remote_port)?;
        let supervisor =
            waku_client::DaemonSupervisor::connect(&format!("127.0.0.1:{local_port}"), token)?;
        Ok((local_port, supervisor))
    }

    /// Re-establish the forward after the master restarts. The local port
    /// must stay stable so the supervisor's reconnect target never changes.
    pub(super) fn restore_forward(&self, local_port: u16) -> anyhow::Result<()> {
        self.ensure_master()?;
        let (_, remote_port) = self.provision()?;
        self.add_forward(local_port, remote_port)
    }

    /// Tear down the master connection, closing every forward on it.
    pub(super) fn shutdown(&self) {
        let _ = self
            .command()
            .arg("-O")
            .arg("exit")
            .arg(&self.destination)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// `Host` aliases declared in `~/.ssh/config`, minus wildcard stanzas.
/// Read once per editor open; `Include`d files are not followed.
pub(super) fn ssh_config_hosts() -> Vec<String> {
    let Some(path) = dirs::home_dir().map(|home| home.join(".ssh").join("config")) else {
        return Vec::new();
    };
    let Ok(config) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    config
        .lines()
        .flat_map(|line| {
            let line = line.trim();
            // `Host` is case-insensitive and must be a whole word —
            // `HostName` is a different directive.
            if line.len() <= 4
                || !line[..4].eq_ignore_ascii_case("host")
                || !line.as_bytes()[4].is_ascii_whitespace()
            {
                return Vec::new();
            }
            line[4..]
                .split_whitespace()
                .filter(|pattern| {
                    !pattern.is_empty()
                        && !pattern.starts_with('!')
                        && !pattern.starts_with('#')
                        && !pattern.contains('*')
                        && !pattern.contains('?')
                })
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

/// A free loopback port. There is an inherent race between releasing the
/// probe socket and ssh binding it; `-O forward` fails fast on a lost race
/// and the caller retries the whole attempt.
fn free_local_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}
