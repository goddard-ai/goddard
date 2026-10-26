use super::*;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Output};
use std::time::{Duration, Instant};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("goddard-ssh-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join(".goddard")).unwrap();
        let fixture = Self { root };
        // Downloads are an external boundary. No test contacts the release
        // service or uses the developer's installed daemon or SSH config.
        fixture.tool("curl", "#!/bin/sh\nexit 22\n");
        fixture
    }

    fn tool(&self, name: &str, script: &str) {
        let path = self.root.join("bin").join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", &self.root)
            .env("TMPDIR", &self.root)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .env("GODDARD_RELEASES_URL", "https://fixture.invalid")
            .env_remove("ENV")
            .env_remove("BASH_ENV");
        command
    }

    fn daemon(&self, protocol: u32) {
        self.tool(
            "goddard-daemon",
            &format!(
                r#"#!/bin/sh
printf 'started\n' >> "$HOME/starts"
printf '%s\n' '{{"address":"127.0.0.1:45678","protocolVersion":{protocol}}}'
exec /bin/sleep 60
"#
            ),
        );
    }

    fn bootstrap(&self) -> Output {
        let script = REMOTE_BOOTSTRAP
            .replace("%VERSION%", env!("CARGO_PKG_VERSION"))
            .replace("%PROTOCOL%", &waku_client::PROTOCOL_VERSION.to_string());
        let mut child = self
            .command("sh")
            .arg("-s")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn rerun_isolated(&self) {
        let output = self
            .command(std::env::current_exe().unwrap().to_str().unwrap())
            .arg("--exact")
            .arg(std::thread::current().name().unwrap())
            .arg("--nocapture")
            .env("GODDARD_SSH_TEST_CHILD", "1")
            .env("GODDARD_SSH_TEST_ROOT", &self.root)
            .env("GODDARD_DATA_DIR", &self.root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only fixture daemons write this private pid file.
        if let Ok(pid) = fs::read_to_string(self.root.join(".goddard/daemon.pid")) {
            let _ = Command::new("kill")
                .arg(pid.trim())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fifo(path: &Path) -> File {
    assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .unwrap()
}

fn read_line_before(file: &mut File) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut line = Vec::new();
    loop {
        let mut byte = [0];
        match file.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => return String::from_utf8(line).unwrap(),
            Ok(1) => line.push(byte[0]),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("fifo read failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "helper never published its prompt"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_before(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("helper did not finish");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn bootstrap_reuses_a_live_compatible_daemon_and_its_private_token() {
    let fixture = Fixture::new();
    fixture.daemon(waku_client::PROTOCOL_VERSION);
    let first = fixture.bootstrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_reply =
        SshTransport::parse_provision(std::str::from_utf8(&first.stdout).unwrap()).unwrap();
    assert_eq!(first_reply.0.len(), 48);
    let pid = fs::read(fixture.root.join(".goddard/daemon.pid")).unwrap();
    let second = fixture.bootstrap();
    assert!(second.status.success());
    assert_eq!(
        SshTransport::parse_provision(std::str::from_utf8(&second.stdout).unwrap()).unwrap(),
        first_reply
    );
    assert_eq!(
        fs::read(fixture.root.join(".goddard/daemon.pid")).unwrap(),
        pid
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("starts")).unwrap(),
        "started\n"
    );
    assert_eq!(
        fs::metadata(fixture.root.join(".goddard/daemon-token"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn incompatible_daemon_propagates_upgrade_failures_instead_of_starting_stale_code() {
    for expected_exit in [8, 1] {
        let fixture = Fixture::new();
        let stale_protocol = waku_client::PROTOCOL_VERSION + 1;
        fixture.daemon(stale_protocol);
        if expected_exit == 1 {
            assert!(
                Command::new("tar")
                    .arg("-czf")
                    .arg(fixture.root.join("release.tar.gz"))
                    .arg("-C")
                    .arg(fixture.root.join("bin"))
                    .arg("goddard-daemon")
                    .status()
                    .unwrap()
                    .success()
            );
            fixture.tool("curl", "#!/bin/sh\ncase \"$*\" in\n *.sha256) printf '%064d\\n' 0 ;;\n *) /bin/cat \"$HOME/release.tar.gz\" ;;\nesac\n");
        }
        let mut old_daemon = Command::new("sleep").arg("60").spawn().unwrap();
        fs::write(
            fixture.root.join(".goddard/daemon.pid"),
            old_daemon.id().to_string(),
        )
        .unwrap();
        fs::write(
            fixture.root.join(".goddard/daemon.ready"),
            format!(r#"{{"address":"127.0.0.1:45678","protocolVersion":{stale_protocol}}}"#),
        )
        .unwrap();
        let result = fixture.bootstrap();
        let _ = old_daemon.kill();
        let _ = old_daemon.wait();
        assert_eq!(
            result.status.code(),
            Some(expected_exit),
            "failed upgrades must request upload for missing releases and abort for checksum mismatches: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!fixture.root.join("starts").exists());
        assert!(!fixture.root.join(".goddard/bin/goddard-daemon").exists());
    }
}

#[test]
fn askpass_accepts_an_immediate_answer_or_cancel_without_losing_it() {
    for answer in ["  pass\\word with spaces  ", ""] {
        let fixture = Fixture::new();
        let queue_path = fixture.root.join("queue");
        let mut queue = fifo(&queue_path);
        // Delay the external reader in the original helper. Publishing the
        // prompt must guarantee that the response FIFO can accept an answer.
        fixture.tool("cat", "#!/bin/sh\nwhile [ ! -f \"$HOME/read-response\" ]; do /bin/sleep 0.01; done\nexec /bin/cat \"$@\"\n");
        let script = ASKPASS_SCRIPT.replace("%QUEUE%", &queue_path.to_string_lossy());
        let child = fixture
            .command("sh")
            .arg("-c")
            .arg(script)
            .arg("askpass")
            .arg("Password:")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request_dir = PathBuf::from(read_line_before(&mut queue));
        let mut prompt = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(request_dir.join("in"))
            .unwrap();
        assert_eq!(read_line_before(&mut prompt), "Password:");
        let request = SshAskpassRequest {
            prompt: "Password:".into(),
            response: request_dir.join("out"),
        };
        let delivered = request.answer(answer);
        fs::write(fixture.root.join("read-response"), "").unwrap();
        if delivered.is_err() {
            // Release the broken helper before reporting the regression.
            let mut response = OpenOptions::new()
                .write(true)
                .open(&request.response)
                .unwrap();
            response.write_all(b"\n").unwrap();
        }
        let output = wait_before(child);
        assert!(
            delivered.is_ok(),
            "an immediate answer failed: {delivered:?}"
        );
        assert!(output.status.success());
        assert_eq!(output.stdout, format!("{answer}\n").as_bytes());
        assert!(
            !request_dir.exists(),
            "helper left its credential FIFOs behind"
        );
    }
}

#[test]
fn tunnel_repair_replaces_a_stale_forward_on_the_same_local_port() {
    // PATH and state-directory overrides belong to a child process so parallel
    // tests and the developer's SSH master never observe fixture state.
    if std::env::var_os("GODDARD_SSH_TEST_CHILD").is_none() {
        let fixture = Fixture::new();
        fixture.tool(
            "ssh",
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> "$GODDARD_SSH_TEST_ROOT/calls"
for arg do
  case "$arg" in ControlPath=*) control="${{arg#ControlPath=}}" ;; esac
done
case "$*" in
  *'-O check'*) exit 0 ;;
  *'-Nf'*)
    if [ -f "$GODDARD_SSH_TEST_ROOT/fail-auth" ]; then echo 'Permission denied' >&2; exit 255; fi
    : > "$control"; exit 0 ;;
  *'sh -s'*)
    /bin/cat >/dev/null
    if [ -f "$GODDARD_SSH_TEST_ROOT/fail-provision" ]; then echo 'provision failed' >&2; exit 9; fi
    printf 'fixture-token\n5000\n{}\n'; exit 0 ;;
  *'-O cancel'*)
    if [ ! -f "$GODDARD_SSH_TEST_ROOT/forward" ]; then echo 'forward not found' >&2; exit 1; fi
    rm -f "$GODDARD_SSH_TEST_ROOT/forward"; exit 0 ;;
  *'-O forward'*)
    if [ -f "$GODDARD_SSH_TEST_ROOT/forward" ]; then
      echo 'local port is already forwarded' >&2; exit 1
    fi
    : > "$GODDARD_SSH_TEST_ROOT/forward"; exit 0 ;;
esac
echo 'unexpected ssh command' >&2
exit 1
"#,
                waku_client::PROTOCOL_VERSION
            ),
        );
        fixture.rerun_isolated();
        return;
    }
    let root = PathBuf::from(std::env::var_os("GODDARD_SSH_TEST_ROOT").unwrap());
    let transport = SshTransport::new(Uuid::new_v4(), "fixture-host");
    prepare_control_dir(transport.control_path().parent().unwrap()).unwrap();
    fs::write(transport.control_path(), "fixture master").unwrap();
    transport.add_forward(SshAuth::Batch, 31337, 4000).unwrap();
    // A daemon restart changes its ephemeral port while SSH remains alive.
    transport.restore_forward(SshAuth::Batch, 31337).unwrap();
    let calls = fs::read_to_string(root.join("calls")).unwrap();
    assert!(calls.contains("-O cancel -L 127.0.0.1:31337:127.0.0.1:4000"));
    assert!(calls.contains("-O forward -L 127.0.0.1:31337:127.0.0.1:5000"));

    // Losing the master removes its forwards. Recovery must reuse the local
    // endpoint without trying to cancel a forward on the new master.
    fs::remove_file(transport.control_path()).unwrap();
    fs::remove_file(root.join("forward")).unwrap();
    fs::write(root.join("fail-provision"), "").unwrap();
    assert!(transport.restore_forward(SshAuth::Batch, 31337).is_err());
    fs::remove_file(root.join("fail-provision")).unwrap();
    transport.restore_forward(SshAuth::Batch, 31337).unwrap();
    let calls = fs::read_to_string(root.join("calls")).unwrap();
    assert_eq!(calls.matches("-O cancel").count(), 1);

    // An authentication failure stops repair before any new forward can be
    // installed, preserving the useful cause for the app's connection error.
    fs::remove_file(transport.control_path()).unwrap();
    fs::remove_file(root.join("forward")).unwrap();
    fs::write(root.join("fail-auth"), "").unwrap();
    let error = transport
        .restore_forward(SshAuth::Batch, 31337)
        .unwrap_err();
    assert!(format!("{error:#}").contains("Permission denied"));
    assert!(!root.join("forward").exists());
}

#[test]
fn ssh_host_picker_accepts_unicode_comments_without_treating_comments_as_hosts() {
    if std::env::var_os("GODDARD_SSH_TEST_CHILD").is_none() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.root.join(".ssh")).unwrap();
        fs::write(fixture.root.join(".ssh/config"), "# 🖥️ Servers\nHost work home # server aliases\n  HostName example.invalid\nHost *.internal !excluded\n").unwrap();
        fixture.rerun_isolated();
        return;
    }
    assert_eq!(ssh_config_hosts(), ["work", "home"]);
}
