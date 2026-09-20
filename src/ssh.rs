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

use std::ffi::OsString;
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
dir=$(mktemp -d "${TMPDIR:-/tmp}/goddard-askpass.XXXXXX") || exit 1
mkfifo "$dir/in" "$dir/out" 2>/dev/null || { rm -rf "$dir"; exit 1; }
printf '%s\n' "$dir" > "%QUEUE%"
printf '%s\n' "$1" > "$dir/in"
cat "$dir/out"
status=$?
rm -rf "$dir"
exit $status
"#;

/// Provision and start the remote daemon, printing `token\nport\nprotocol`
/// on success. `%VERSION%` and `%PROTOCOL%` are substituted with the app's
/// release version and wire protocol when the script runs.
///
/// The token persists at `~/.goddard/daemon-token` so re-connects reuse it.
/// When no binary is on PATH or under `~/.goddard/bin`, or the installed copy
/// was fetched for a different app version, the version-matched tarball is
/// fetched from the release bucket and checksum-verified — exit 8 tells the
/// app to upload its bundled daemon instead. A running daemon whose ready
/// line reports another protocol is stopped and replaced. The daemon is
/// started detached (`nohup`, stdin/out redirected) so it survives the ssh
/// session ending.
const REMOTE_BOOTSTRAP: &str = r#"set -u
app_version="%VERSION%"
want_protocol="%PROTOCOL%"
waku_dir="$HOME/.goddard"
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
bin_dir="$waku_dir/bin"
bin_file="$bin_dir/goddard-daemon"
version_file="$bin_dir/.version"

fetch() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1";
  elif command -v wget >/dev/null 2>&1; then wget -qO- "$1";
  else return 127; fi
}

install_daemon() {
  os=$(uname -s); machine=$(uname -m)
  case "$os/$machine" in
    Linux/x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux/aarch64 | Linux/arm64) target=aarch64-unknown-linux-gnu ;;
    Darwin/arm64) target=aarch64-apple-darwin ;;
    Darwin/x86_64) target=x86_64-apple-darwin ;;
    *) echo "no release mapping for remote platform $os/$machine" >&2; return 1 ;;
  esac
  name="goddard-daemon-$app_version-$target"
  base="${GODDARD_RELEASES_URL:-https://releases.goddardai.org}"
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/goddard-daemon.XXXXXX") || return 1
  if ! fetch "$base/$name.tar.gz" >"$tmp/pkg.tar.gz" 2>/dev/null; then
    rm -rf "$tmp"
    echo "no published daemon artifact $name" >&2
    return 8
  fi
  if fetch "$base/$name.tar.gz.sha256" >"$tmp/pkg.sha256" 2>/dev/null; then
    expected=$(sed -n 's/^\([a-fA-F0-9]*\).*/\1/p' "$tmp/pkg.sha256" | head -1)
    if command -v sha256sum >/dev/null 2>&1; then
      actual=$(sha256sum "$tmp/pkg.tar.gz" | cut -d' ' -f1)
    elif command -v shasum >/dev/null 2>&1; then
      actual=$(shasum -a 256 "$tmp/pkg.tar.gz" | cut -d' ' -f1)
    else
      actual=""
    fi
    if [ -n "$expected" ] && [ -n "$actual" ] && [ "$expected" != "$actual" ]; then
      rm -rf "$tmp"
      echo "checksum mismatch on $name" >&2
      return 1
    fi
  fi
  mkdir -p "$bin_dir" || { rm -rf "$tmp"; return 1; }
  tar -xzf "$tmp/pkg.tar.gz" -C "$bin_dir" || { rm -rf "$tmp"; return 1; }
  chmod 755 "$bin_file" 2>/dev/null || true
  printf '%s' "$app_version" >"$version_file"
  rm -rf "$tmp"
}

ready_protocol() {
  sed -n 's/.*"protocolVersion":\([0-9][0-9]*\).*/\1/p' "$ready_file" 2>/dev/null | head -1
}

stop_daemon() {
  if [ -f "$pid_file" ]; then
    pid=$(cat "$pid_file" 2>/dev/null || true)
    if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; fi
    rm -f "$pid_file"
  fi
  : >"$ready_file" 2>/dev/null || true
}

start_daemon() {
  : >"$ready_file" 2>/dev/null || true
  GODDARD_DAEMON_TOKEN="$token" nohup "$1" --bind 127.0.0.1:0 \
    >"$ready_file" 2>>"$waku_dir/daemon.log" </dev/null &
  echo $! >"$pid_file"
}

bin=$(command -v goddard-daemon 2>/dev/null || true)
if [ -z "$bin" ]; then
  if [ ! -x "$bin_file" ] || \
     [ "$(cat "$version_file" 2>/dev/null || true)" != "$app_version" ]; then
    install_daemon
    rc=$?
    if [ "$rc" -ne 0 ]; then
      echo "goddard-daemon unavailable on the remote host" >&2
      # Exit 8 means "no published artifact" — the app uploads its bundled
      # daemon. Other codes (checksum failure, no fetcher) are hard errors.
      exit "$rc"
    fi
  fi
  bin="$bin_file"
fi

running=0
if [ -f "$pid_file" ]; then
  pid=$(cat "$pid_file" 2>/dev/null || true)
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then running=1; fi
fi
if [ "$running" -eq 1 ]; then
  proto=$(ready_protocol)
  if [ -n "$proto" ] && [ "$proto" != "$want_protocol" ]; then
    # A stale daemon answers with an incompatible wire protocol — stop it and
    # prefer the version-matched copy under ~/.goddard.
    stop_daemon
    running=0
    if [ "$(cat "$version_file" 2>/dev/null || true)" != "$app_version" ]; then
      install_daemon || true
    fi
  fi
fi
if [ "$running" -eq 0 ] && [ -x "$bin_file" ] && \
   [ "$(cat "$version_file" 2>/dev/null || true)" = "$app_version" ]; then
  bin="$bin_file"
fi
if [ "$running" -eq 0 ]; then
  start_daemon "$bin"
fi

i=0
while [ ! -s "$ready_file" ]; do
  i=$((i + 1))
  if [ "$i" -gt 100 ]; then
    echo "goddard-daemon did not report ready" >&2
    exit 6
  fi
  sleep 0.1 2>/dev/null || sleep 1
done

proto=$(ready_protocol)
port=$(sed -n 's/.*"address":"[^"]*:\([0-9][0-9]*\)".*/\1/p' "$ready_file" | head -1)
if [ -z "$port" ]; then
  echo "goddard-daemon reported no bound port" >&2
  exit 7
fi
printf '%s\n%s\n%s\n' "$token" "$port" "$proto"
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
    std::fs::create_dir_all(&dir).context("could not create ~/.goddard/ssh")?;
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

/// How an ssh invocation may authenticate. `Batch` forbids interaction
/// outright — `BatchMode` plus askpass disabled — so background work can
/// never raise a prompt. `Interactive` wires the askpass helper and caps
/// password retries at one, so a single cancel ends the attempt instead of
/// re-prompting; it only runs on explicit user intent.
#[derive(Clone, Copy)]
pub(super) enum SshAuth {
    Batch,
    Interactive,
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

    /// `ssh` with the control socket pinned and its authentication mode
    /// fixed. Every invocation goes through this so a prompt can neither
    /// fall back to a terminal that does not exist nor fire from a
    /// background attempt.
    fn command(&self, auth: SshAuth) -> Command {
        let mut control_path = OsString::from("ControlPath=");
        control_path.push(self.control_path());
        let mut command = Command::new("ssh");
        command
            .arg("-o")
            .arg(control_path)
            .arg("-o")
            .arg("ControlPersist=600")
            .arg("-o")
            .arg("ConnectTimeout=15")
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());
        match auth {
            SshAuth::Batch => {
                command
                    .arg("-o")
                    .arg("BatchMode=yes")
                    .env("SSH_ASKPASS_REQUIRE", "never")
                    .env_remove("SSH_ASKPASS")
                    .env_remove("DISPLAY");
            }
            SshAuth::Interactive => {
                command
                    .arg("-o")
                    .arg("NumberOfPasswordPrompts=1")
                    // First contact trusts on use: there is no tty to answer
                    // a host-key prompt, so an unknown key would fail every
                    // attempt. Batch attempts keep the user's strict default.
                    .arg("-o")
                    .arg("StrictHostKeyChecking=accept-new")
                    .env("SSH_ASKPASS", askpass_script_path())
                    .env("SSH_ASKPASS_REQUIRE", "force")
                    .env("DISPLAY", "goddard:0");
            }
        }
        command
    }

    /// Establish or reuse the master connection. `-Nf` daemonizes the
    /// master after authentication succeeds, so a non-zero exit here means
    /// auth or reachability failed and stderr carries why. `ControlMaster`
    /// is set through `-o` only: adding `-M` on top of it promotes the
    /// master to confirmation mode, where every later session and forward
    /// is refused with "Permission denied".
    fn ensure_master(&self, auth: SshAuth) -> anyhow::Result<()> {
        if self.master_alive() {
            return Ok(());
        }
        let output = self
            .command(auth)
            .arg("-o")
            .arg("ControlMaster=yes")
            .arg("-Nf")
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

    /// Whether the control master is up. `-O` queries the socket — it never
    /// authenticates, so it always runs in batch mode.
    pub(super) fn master_alive(&self) -> bool {
        self.control_path().exists()
            && self
                .command(SshAuth::Batch)
                .arg("-O")
                .arg("check")
                .arg(&self.destination)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }

    /// Run a script on the remote through the master connection, returning
    /// its exit code and captured output.
    fn run_remote_status(
        &self,
        auth: SshAuth,
        script: &str,
    ) -> anyhow::Result<(i32, String, String)> {
        self.ensure_master(auth)?;
        let mut child = self
            .command(auth)
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
        Ok((
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    /// Ensure the remote daemon is running: install or upgrade the binary
    /// when needed, then return its token plus bound loopback port. The
    /// bootstrap's exit 8 means the remote platform has no published
    /// artifact — the caller uploads the bundled binary and retries.
    fn provision(&self, auth: SshAuth) -> anyhow::Result<(String, u16)> {
        let script = REMOTE_BOOTSTRAP
            .replace("%VERSION%", env!("CARGO_PKG_VERSION"))
            .replace("%PROTOCOL%", &waku_client::PROTOCOL_VERSION.to_string());
        let (code, stdout, stderr) = self.run_remote_status(auth, &script)?;
        if code == 8 {
            self.upload_daemon(auth)?;
            let (code, stdout, stderr) = self.run_remote_status(auth, &script)?;
            if code != 0 {
                bail!(
                    "remote provisioning on {} failed after binary upload: {}",
                    self.destination,
                    stderr.trim()
                );
            }
            return Self::parse_provision(&stdout);
        }
        if code != 0 {
            bail!(
                "remote provisioning on {} failed: {}",
                self.destination,
                stderr.trim()
            );
        }
        Self::parse_provision(&stdout)
    }

    /// Parse the bootstrap's `token\nport\nprotocol` output and refuse a
    /// wire-incompatible daemon.
    fn parse_provision(stdout: &str) -> anyhow::Result<(String, u16)> {
        let mut lines = stdout.lines();
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
        let protocol = lines
            .next()
            .and_then(|line| line.trim().parse::<u32>().ok());
        if let Some(protocol) = protocol {
            if protocol != waku_client::PROTOCOL_VERSION {
                bail!(
                    "remote goddard-daemon speaks protocol {protocol}, this build requires {}",
                    waku_client::PROTOCOL_VERSION
                );
            }
        }
        Ok((token, port))
    }

    /// Push the app's bundled daemon binary to `~/.goddard/bin`. Only viable
    /// when the remote shares this machine's OS and architecture.
    fn upload_daemon(&self, auth: SshAuth) -> anyhow::Result<()> {
        self.ensure_master(auth)?;
        let uname = self.remote_uname(auth)?;
        if !remote_matches_local(&uname) {
            bail!(
                "remote platform {uname} does not match this machine — install goddard-daemon there manually"
            );
        }
        let binary = crate::daemon::daemon_executable_path()
            .context("could not locate the bundled goddard-daemon to upload")?;
        let bytes = std::fs::read(&binary)
            .with_context(|| format!("could not read {}", binary.display()))?;
        let mut child = self
            .command(auth)
            .arg(&self.destination)
            .arg(format!(
                "mkdir -p \"$HOME/.goddard/bin\" \
                 && cat >\"$HOME/.goddard/bin/goddard-daemon.new\" \
                 && chmod 755 \"$HOME/.goddard/bin/goddard-daemon.new\" \
                 && mv \"$HOME/.goddard/bin/goddard-daemon.new\" \"$HOME/.goddard/bin/goddard-daemon\" \
                 && printf '%s' '{}' >\"$HOME/.goddard/bin/.version\"",
                env!("CARGO_PKG_VERSION")
            ))
            .stdin(Stdio::piped())
            .spawn()
            .context("could not upload goddard-daemon over ssh")?;
        child
            .stdin
            .take()
            .context("ssh stdin was not piped")?
            .write_all(&bytes)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "daemon upload to {} failed: {}",
                self.destination,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// `uname -s`/`uname -m` on the remote, for the upload platform check.
    fn remote_uname(&self, auth: SshAuth) -> anyhow::Result<String> {
        let child = self
            .command(auth)
            .arg(&self.destination)
            .arg("uname -sm")
            .spawn()
            .context("could not run uname over ssh")?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "uname on {} failed: {}",
                self.destination,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Point a local ephemeral port at the remote daemon's loopback port on
    /// the existing master. Fails fast when the forward cannot bind.
    fn add_forward(&self, auth: SshAuth, local_port: u16, remote_port: u16) -> anyhow::Result<()> {
        let output = self
            .command(auth)
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
    pub(super) fn connect(
        &self,
        auth: SshAuth,
    ) -> anyhow::Result<(u16, waku_client::DaemonSupervisor)> {
        self.ensure_master(auth)?;
        let (token, remote_port) = self.provision(auth)?;
        let local_port = free_local_port()?;
        self.add_forward(auth, local_port, remote_port)?;
        let supervisor =
            waku_client::DaemonSupervisor::connect(&format!("127.0.0.1:{local_port}"), token)?;
        Ok((local_port, supervisor))
    }

    /// Re-establish the forward after the master restarts. The local port
    /// must stay stable so the supervisor's reconnect target never changes.
    pub(super) fn restore_forward(&self, auth: SshAuth, local_port: u16) -> anyhow::Result<()> {
        self.ensure_master(auth)?;
        let (_, remote_port) = self.provision(auth)?;
        self.add_forward(auth, local_port, remote_port)
    }

    /// Tear down the master connection, closing every forward on it.
    pub(super) fn shutdown(&self) {
        let _ = self
            .command(SshAuth::Batch)
            .arg("-O")
            .arg("exit")
            .arg(&self.destination)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Whether the remote's `uname -sm` output matches this machine's platform
/// — the upload fallback only makes sense when it does.
fn remote_matches_local(uname: &str) -> bool {
    let mut parts = uname.split_whitespace();
    let (Some(system), Some(machine)) = (parts.next(), parts.next()) else {
        return false;
    };
    let os = match system {
        "Linux" => "linux",
        "Darwin" => "macos",
        _ => return false,
    };
    let arch = match machine {
        "x86_64" | "amd64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        _ => return false,
    };
    os == std::env::consts::OS && arch == std::env::consts::ARCH
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
