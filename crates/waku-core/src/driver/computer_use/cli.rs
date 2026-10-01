//! Task-owned private REPL transport. Providers see only the scoped CLI.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use anyhow::{Context as _, bail};
use base64::Engine as _;
use crossbeam_channel::{Receiver, bounded};
use parking_lot::Mutex;
use serde_json::{Value, json};
use uuid::Uuid;

use super::ComputerUseConfig;
use crate::driver::DriverEventSender;
use crate::model::{ActivityKind, DriverEvent};

fn services() -> &'static Mutex<HashMap<Uuid, Weak<Service>>> {
    static SERVICES: OnceLock<Mutex<HashMap<Uuid, Weak<Service>>>> = OnceLock::new();
    SERVICES.get_or_init(Default::default)
}

pub(crate) fn for_task(task: Uuid) -> anyhow::Result<Arc<Service>> {
    services()
        .lock()
        .get(&task)
        .and_then(Weak::upgrade)
        .context("computer use is unavailable for this task; start an enabled runtime first")
}

struct Connection {
    input: ChildStdin,
    output: Receiver<anyhow::Result<Value>>,
}

pub(crate) struct Service {
    config: ComputerUseConfig,
    cwd: PathBuf,
    blobs: Arc<crate::blob_store::BlobStore>,
    connection: Mutex<Option<Connection>>,
    child: Mutex<Option<Child>>,
    closed: AtomicBool,
    events: Mutex<Option<DriverEventSender>>,
}

impl Service {
    pub(super) fn bind(
        task: Uuid,
        config: ComputerUseConfig,
        cwd: &Path,
        events: DriverEventSender,
        blobs: Arc<crate::blob_store::BlobStore>,
    ) -> Arc<Self> {
        let service = Arc::new(Self {
            config,
            cwd: cwd.to_owned(),
            blobs,
            connection: Mutex::new(None),
            child: Mutex::new(None),
            closed: AtomicBool::new(false),
            events: Mutex::new(Some(events)),
        });
        let mut registry = services().lock();
        registry.retain(|_, service| service.strong_count() > 0);
        registry.insert(task, Arc::downgrade(&service));
        service
    }

    fn connect(&self) -> anyhow::Result<Connection> {
        let mut child_slot = self.child.lock();
        if self.closed.load(Ordering::Acquire) {
            bail!("computer-use runtime has closed");
        }
        let mut command = crate::command_env::command(&self.config.repl_path);
        command
            .command_mut()
            .current_dir(&self.cwd)
            .env("GODDARD_COMPUTER_USE_SERVER", &self.config.server_path)
            .env(
                "GODDARD_COMPUTER_USE_PROCESS_DIRECTORY",
                &self.config.process_directory,
            )
            // A shared provider server's routing variables must not select a
            // different kernel inside this task-owned private subprocess.
            .env_remove("GODDARD_COMPUTER_USE_SESSIONS_DIRECTORY");
        let mut command = crate::command_env::guard_command(command.into_inner());
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = crate::command_env::spawn(&mut command)
            .context("could not start the computer-use kernel")?;
        let input = child.stdin.take().context("kernel stdin unavailable")?;
        let output = child.stdout.take().context("kernel stdout unavailable")?;
        *child_slot = Some(child);
        let (sender, receiver) = bounded(1);
        if let Err(error) = std::thread::Builder::new()
            .name("goddard-computer-use-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(output);
                loop {
                    // Bound private output as well as the daemon wire message.
                    let mut line = Vec::new();
                    let result = std::io::Read::take(&mut reader, 32 * 1024 * 1024 + 1)
                        .read_until(b'\n', &mut line);
                    let result = match result {
                        Ok(0) => break,
                        Ok(_) if line.len() > 32 * 1024 * 1024 => {
                            let _ =
                                sender.send(Err(anyhow::anyhow!("kernel response exceeds 32 MB")));
                            break;
                        }
                        Ok(_) => serde_json::from_slice(&line).map_err(Into::into),
                        Err(error) => Err(error.into()),
                    };
                    if sender.send(result).is_err() {
                        break;
                    }
                }
            })
        {
            if let Some(mut child) = child_slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err(error.into());
        }
        Ok(Connection {
            input,
            output: receiver,
        })
    }

    pub(crate) fn call(
        &self,
        code: Option<&str>,
        timeout_ms: Option<u64>,
        title: Option<&str>,
    ) -> anyhow::Result<Value> {
        let timeout_ms = timeout_ms.unwrap_or(300_000);
        if !(1..=300_000).contains(&timeout_ms) {
            bail!("timeout_ms must be between 1 and 300000");
        }
        if code.is_some_and(|code| code.len() > 1024 * 1024) {
            bail!("JavaScript exceeds 1 MB");
        }
        let mut connection = self.connection.lock();
        if self.closed.load(Ordering::Acquire) {
            bail!("computer-use runtime has closed");
        }
        if self
            .config
            .process_directory
            .join("computer-use-disabled")
            .exists()
        {
            bail!("computer use is disabled");
        }
        if connection.is_none() {
            *connection = Some(self.connect()?);
        }
        let id = Uuid::new_v4().to_string();
        let mut arguments = match code {
            Some(code) => json!({"code": code, "timeout_ms": timeout_ms}),
            None => json!({}),
        };
        if code.is_some() {
            if let Some(title) = title {
                arguments["title"] = json!(title);
            }
        }
        let request = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
            "name": if code.is_some() { "js" } else { "js_reset" }, "arguments": arguments,
        }});
        let transport = connection.as_mut().unwrap();
        let response = (|| -> anyhow::Result<Value> {
            serde_json::to_writer(&mut transport.input, &request)?;
            transport.input.write_all(b"\n")?;
            transport.input.flush()?;
            let response = transport
                .output
                .recv_timeout(Duration::from_millis(timeout_ms + 10_000))
                .context("computer-use kernel stopped or timed out; kernel state was lost")??;
            if response["id"] != id {
                bail!("unexpected computer-use response id");
            }
            if let Some(error) = response.get("error") {
                bail!("computer-use kernel: {error}");
            }
            response
                .get("result")
                .cloned()
                .context("kernel returned no result")
        })();
        let mut result = match response {
            Ok(result) => result,
            Err(error) => {
                // Never replay an action after a lost response: it may have
                // already changed the user's app. The next call gets a fresh kernel.
                super::stop_registered_processes(
                    &self.config.process_directory,
                    &self.config.server_path,
                );
                connection.take();
                if let Some(mut child) = self.child.lock().take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(error);
            }
        };
        if self.closed.load(Ordering::Acquire) {
            bail!("computer-use runtime closed during execution");
        }
        let images = materialize_images(&mut result, &self.blobs)?;
        let activity = super::super::activity::tool_activity(
            Some(id),
            ActivityKind::Tool,
            title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or("Computer use")
                .to_owned(),
            Some(&arguments),
            Some(&result),
            None,
            result["isError"] == true,
            true,
        )
        .with_image_urls(images);
        if let Some(events) = self.events.lock().as_ref() {
            let _ = events.send(DriverEvent::RichActivity(activity));
        }
        Ok(result)
    }

    pub(crate) fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        super::stop_registered_processes(&self.config.process_directory, &self.config.server_path);
        self.events.lock().take();
        if let Some(mut child) = self.child.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn materialize_images(
    result: &mut Value,
    blobs: &crate::blob_store::BlobStore,
) -> anyhow::Result<Vec<String>> {
    let mut references = Vec::new();
    let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) else {
        return Ok(references);
    };
    for item in content {
        if item["type"] != "image" {
            continue;
        }
        let mime = item["mimeType"].as_str().unwrap_or("image/png");
        if !mime.starts_with("image/") {
            bail!("invalid screenshot MIME type: {mime}");
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(item["data"].as_str().context("image missing data")?)?;
        let reference = blobs.store_image_bytes(mime, &bytes)?;
        let path = blobs
            .path_for(&reference)
            .context("could not resolve screenshot path")?;
        *item = json!({"type":"image", "path": path, "mimeType": mime});
        references.push(reference);
    }
    Ok(references)
}

#[cfg(test)]
pub(crate) fn bind_for_test(
    task: Uuid,
    repl_path: PathBuf,
    directory: &Path,
    events: DriverEventSender,
    blobs: Arc<crate::blob_store::BlobStore>,
) -> Arc<Service> {
    Service::bind(
        task,
        ComputerUseConfig {
            server_path: directory.join("native"),
            repl_path,
            skill_path: directory.join("SKILL.md"),
            process_directory: directory.to_owned(),
        },
        directory,
        events,
        blobs,
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    struct Fixture {
        service: Arc<Service>,
        events: crossbeam_channel::Receiver<DriverEvent>,
        root: PathBuf,
        task: Uuid,
    }
    impl Fixture {
        fn new(repl: Option<PathBuf>) -> Self {
            let task = Uuid::new_v4();
            let root = std::env::temp_dir().join(format!("goddard-cli-test-{task}"));
            crate::fs_ext::create_private_dir_all(&root).unwrap();
            let repl = repl.unwrap_or_else(|| {
                let path = root.join("kernel");
                std::fs::write(&path, include_str!("../fixtures/computer_use_kernel.py")).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
                path
            });
            let native = root.join("native");
            std::fs::write(&native, include_str!("../fixtures/computer_use_native.py")).unwrap();
            std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
            let (events, receiver) = crate::driver::test_event_channel();
            let service = bind_for_test(
                task,
                repl,
                &root,
                events,
                Arc::new(crate::blob_store::BlobStore::new(root.join("blobs"))),
            );
            Self {
                service,
                root,
                task,
                events: receiver,
            }
        }
        fn call(&self, code: &str) -> Value {
            self.service
                .call(Some(code), Some(2000), Some("CLI regression"))
                .unwrap()
        }
        fn await_started(&self) {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !self.root.join("started").exists() {
                assert!(std::time::Instant::now() < deadline, "kernel did not start");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.service.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn cli_service_reuses_its_process_isolates_tasks_and_retains_image_references() {
        let a = Fixture::new(None);
        let b = Fixture::new(None);
        assert!(Arc::ptr_eq(&for_task(a.task).unwrap(), &a.service));
        assert!(for_task(Uuid::new_v4()).is_err());
        let first = a.call("image");
        let path = PathBuf::from(first["content"][1]["path"].as_str().unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic-image-bytes");
        assert!(first["content"][1].get("data").is_none());
        let second = a.call("text");
        assert_eq!(second["_meta"]["pid"], first["_meta"]["pid"]);
        assert_eq!(second["content"][0]["text"], "2");
        assert_eq!(b.call("text")["content"][0]["text"], "1");
        assert_eq!(a.call("error")["isError"], true);
        assert_eq!(a.call("text")["content"][0]["text"], "4");
        a.service.call(None, None, None).unwrap();
        assert_eq!(a.call("text")["content"][0]["text"], "1");
        let activity = a
            .events
            .try_iter()
            .find_map(|event| match event {
                DriverEvent::RichActivity(item) if !item.image_urls.is_empty() => Some(item),
                _ => None,
            })
            .unwrap();
        assert!(crate::blob_store::is_blob_reference(
            &activity.image_urls[0]
        ));
        assert_eq!(
            a.service.blobs.path_for(&activity.image_urls[0]).unwrap(),
            path
        );
        a.service.shutdown();
        assert!(
            path.exists(),
            "retained transcript image survives kernel teardown"
        );
        assert!(a.service.call(Some("text"), None, None).is_err());
    }

    #[test]
    fn cli_service_rejects_disabled_calls_and_never_replays_a_lost_response() {
        let fixture = Fixture::new(None);
        std::fs::write(fixture.root.join("computer-use-disabled"), b"").unwrap();
        assert!(
            fixture
                .service
                .call(Some("text"), None, None)
                .unwrap_err()
                .to_string()
                .contains("disabled")
        );
        assert!(!fixture.root.join("executions").exists());
        std::fs::remove_file(fixture.root.join("computer-use-disabled")).unwrap();
        assert!(
            fixture
                .service
                .call(Some("disconnect"), Some(100), None)
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("executions")).unwrap(),
            "disconnect\n"
        );
        assert_eq!(fixture.call("text")["content"][0]["text"], "1");
    }

    #[test]
    fn cli_service_cancellation_and_teardown_do_not_wait_for_the_execution_lock() {
        let fixture = Fixture::new(None);
        let service = fixture.service.clone();
        let worker = std::thread::spawn(move || service.call(Some("block"), Some(300_000), None));
        fixture.await_started();
        std::fs::write(fixture.root.join("cancel-kernel"), b"").unwrap();
        assert_eq!(worker.join().unwrap().unwrap()["isError"], true);
        std::fs::remove_file(fixture.root.join("started")).unwrap();
        std::fs::remove_file(fixture.root.join("cancel-kernel")).unwrap();
        let service = fixture.service.clone();
        let worker = std::thread::spawn(move || service.call(Some("block"), Some(300_000), None));
        fixture.await_started();
        let started = std::time::Instant::now();
        fixture.service.shutdown();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(worker.join().unwrap().is_err());
    }

    /// Uses the actual QuickJS engine, with no access to apps or screenshots.
    #[test]
    #[ignore = "set GODDARD_TEST_JS_REPL to the packaged private kernel executable"]
    fn cli_service_against_the_packaged_javascript_engine() {
        let repl = std::env::var_os("GODDARD_TEST_JS_REPL").expect("packaged kernel path");
        let a = Fixture::new(Some(repl.clone().into()));
        let b = Fixture::new(Some(repl.into()));
        assert_eq!(
            a.call("var value = 41; jsRepl.write(value);")["content"][0]["text"],
            "41"
        );
        assert_eq!(
            a.call("value += 1; jsRepl.write(value);")["content"][0]["text"],
            "42"
        );
        assert_eq!(
            b.call("jsRepl.write(typeof value);")["content"][0]["text"],
            "undefined"
        );
        assert_eq!(a.call("throw new Error('expected');")["isError"], true);
        assert_eq!(a.call("jsRepl.write(value);")["content"][0]["text"], "42");
        let image = a.call("jsRepl.setResponseMeta({test:true}); await jsRepl.emitImage('data:image/png;base64,aGVsbG8=');");
        let image = image["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "image")
            .unwrap();
        assert_eq!(
            std::fs::read(image["path"].as_str().unwrap()).unwrap(),
            b"hello"
        );
        a.service.call(None, None, None).unwrap();
        assert_eq!(
            a.call("jsRepl.write(typeof value);")["content"][0]["text"],
            "undefined"
        );
        assert_eq!(
            a.service
                .call(Some("while (true) {}"), Some(20), None)
                .unwrap()["isError"],
            true
        );
        a.service.shutdown();
    }
    #[test]
    #[ignore = "set GODDARD_TEST_JS_REPL to the packaged private kernel executable"]
    fn cli_service_keeps_native_approval_enforcement() {
        let fixture = Fixture::new(Some(
            std::env::var_os("GODDARD_TEST_JS_REPL").unwrap().into(),
        ));
        let (events, received) = crate::driver::test_event_channel();
        let monitor =
            super::super::ComputerUsePreviewMonitor::start(fixture.root.clone(), events).unwrap();
        assert_eq!(
            fixture.call("await setupComputerUseRuntime({ globals: globalThis });")["isError"],
            false
        );
        for (decision, failed) in [("deny", true), ("task", false)] {
            let service = fixture.service.clone();
            let worker = std::thread::spawn(move || {
                service.call(
                    Some("jsRepl.write(await cua.clipboard_read());"),
                    Some(5000),
                    None,
                )
            });
            let DriverEvent::Permission { request_id, .. } =
                received.recv_timeout(Duration::from_secs(3)).unwrap()
            else {
                panic!("must request clipboard access");
            };
            assert!(super::super::respond_approval(&request_id, decision));
            assert_eq!(worker.join().unwrap().unwrap()["isError"], failed);
            if failed {
                assert!(!fixture.root.join("native-executions").exists());
            }
        }
        assert_eq!(
            fixture.call("jsRepl.write(await cua.clipboard_read());")["isError"],
            false
        );
        assert!(
            received.try_recv().is_err(),
            "the task-scoped grant is reused"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("native-executions")).unwrap(),
            "clipboard_read\nclipboard_read\n"
        );
        monitor.stop();
    }
}
