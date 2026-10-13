//! Explicit launch/adoption of a private stable runtime. Neither the caller's
//! lifetime nor mutable cargo build lanes own its executable or sessions.

#[cfg(unix)]
mod unix {
    use sha2::{Digest, Sha256};
    use std::os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    };
    use std::{
        fs,
        io::{Read, Write},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::Arc,
        time::{Duration, Instant},
    };
    use waku_runtime::{
        ipc::{self, Endpoint},
        *,
    };

    pub fn main() -> Result<(), HostError> {
        let mut args = std::env::args().skip(1).peekable();
        let operation = if args.peek().is_some_and(|arg| arg.starts_with("--")) {
            "start".into()
        } else {
            args.next().ok_or_else(usage)?
        };
        let mut root = None;
        let mut namespace = None;
        let mut policy = None;
        let mut devin_binary = std::env::var_os("WAKU_RUNTIME_DEVIN_BINARY").map(PathBuf::from);
        while let Some(flag) = args.next() {
            let value = args.next().ok_or_else(usage)?;
            match flag.as_str() {
                "--root" => root = Some(PathBuf::from(value)),
                "--namespace" => namespace = Some(value),
                "--devin-binary" => devin_binary = Some(PathBuf::from(value)),
                "--policy" => {
                    policy = Some(match value.as_str() {
                        "whenIdle" => DrainPolicy::WhenIdle,
                        "forced" => DrainPolicy::Forced,
                        _ => return Err(usage()),
                    })
                }
                _ => return Err(usage()),
            }
        }
        let root = root.ok_or_else(usage)?;
        let namespace = namespace.ok_or_else(usage)?;
        if namespace.is_empty()
            || namespace.len() > 256
            || !root.is_absolute()
            || (operation != "drain" && policy.is_some())
        {
            return Err(usage());
        }
        if devin_binary.is_none() {
            devin_binary = std::env::var_os("PATH")
                .and_then(|paths| {
                    std::env::split_paths(&paths)
                        .map(|dir| dir.join("devin"))
                        .find(|binary| binary.is_file())
                })
                .and_then(|binary| fs::canonicalize(binary).ok());
        }
        if devin_binary
            .as_ref()
            .is_some_and(|binary| !binary.is_absolute() || !binary.is_file())
        {
            return Err(usage());
        }
        ipc::private_root(&root)?;
        let root = fs::canonicalize(root).map_err(io)?;
        match operation.as_str() {
            "start" => {
                let endpoint = start(&root, &namespace, devin_binary.as_deref())?;
                println!(
                    "{}",
                    serde_json::to_string(&endpoint).expect("endpoint serializes")
                );
                Ok(())
            }
            "status" | "drain" => {
                let token = controller_token(&root)?;
                let (mut client, hello) =
                    RuntimeClient::controller(&root, &token, &ClientHello::current())
                        .map_err(client_error)?;
                if hello.namespace != namespace {
                    return Err(HostError::Unauthorized);
                }
                let reply = if operation == "status" {
                    serde_json::to_value(client.status().map_err(client_error)?)
                        .expect("status serializes")
                } else {
                    let grant = client
                        .acquire_control(&ClientHello::current())
                        .map_err(client_error)?;
                    serde_json::to_value(
                        client
                            .drain(grant.epoch, policy.ok_or_else(usage)?)
                            .map_err(client_error)?,
                    )
                    .expect("drain serializes")
                };
                println!("{}", reply);
                Ok(())
            }
            "serve" => {
                let executable =
                    fs::canonicalize(std::env::current_exe().map_err(io)?).map_err(io)?;
                if executable != pin(&root)? {
                    return Err(HostError::Malformed(
                        "use start to launch the pinned executable".into(),
                    ));
                }
                let mut registry = AdapterRegistry::new();
                registry.register_versioned(
                    "codex",
                    env!("CARGO_PKG_VERSION"),
                    Arc::new(waku_runtime_codex::CodexFactory),
                );
                if let Some(binary) = devin_binary {
                    if !binary.is_absolute() || !binary.is_file() {
                        return Err(usage());
                    }
                    registry.register_versioned(
                        "devin",
                        "0.1.0",
                        Arc::new(waku_runtime_devin::DevinFactory::new(binary)),
                    );
                }
                let host = RuntimeHost::open(
                    &root,
                    HostOptions::new(namespace),
                    Arc::new(registry),
                    Box::new(SystemClock),
                )?
                .with_build_identity(format!(
                    "{}:{}",
                    env!("CARGO_PKG_VERSION"),
                    digest(&executable)?
                ));
                let mut server =
                    RuntimeServer::bind(host, &root, Duration::from_millis(5)).map_err(io)?;
                while !server.is_shutdown() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                server.shutdown();
                Ok(())
            }
            _ => Err(usage()),
        }
    }
    fn start(
        root: &Path,
        namespace: &str,
        devin_binary: Option<&Path>,
    ) -> Result<Endpoint, HostError> {
        match adopt(root, namespace, devin_binary.is_some()) {
            Ok(discovery) => return Ok(discovery),
            Err(HostError::Io(_)) => {} // absent/closed endpoint; singleton still fences concurrent start
            Err(error) => return Err(error), // incompatible/foreign/corrupt evidence never causes replacement
        }
        // Verify absence rather than using a stale PID as owner evidence.
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(root.join("runtime.lock"))
            .map_err(io)?;
        lock.try_lock().map_err(|_| HostError::AlreadyOwned)?;
        drop(lock);
        let executable = pin(root)?;
        let mut command = Command::new(&executable);
        command
            .arg("serve")
            .arg("--root")
            .arg(root)
            .arg("--namespace")
            .arg(namespace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(binary) = devin_binary {
            if !binary.is_absolute() || !binary.is_file() {
                return Err(usage());
            }
            command.arg("--devin-binary").arg(binary);
        }
        // SAFETY: setsid is the only pre-exec operation; it allocates nothing
        // and acquires no Rust locks in the forked child.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = command.spawn().map_err(io)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(discovery) = adopt(root, namespace, devin_binary.is_some()) {
                return Ok(discovery);
            }
            if child.try_wait().map_err(io)?.is_some() {
                // A racing launcher may already have acquired the namespace.
                return adopt(root, namespace, devin_binary.is_some());
            }
            if Instant::now() >= deadline {
                return Err(HostError::Io(
                    "runtime launch remains unconfirmed; no automatic replacement".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn adopt(root: &Path, namespace: &str, require_devin: bool) -> Result<Endpoint, HostError> {
        let endpoint = ipc::Endpoint::load(root).map_err(io)?;
        if endpoint.namespace != namespace {
            return Err(HostError::Unauthorized);
        }
        let token = controller_token(root)?;
        let (_, hello) = RuntimeClient::controller(root, &token, &ClientHello::current())
            .map_err(client_error)?;
        if hello.runtime_instance != endpoint.instance || hello.namespace != namespace {
            return Err(HostError::Unauthorized);
        }
        if !hello.adapter_versions.contains_key("codex")
            || (require_devin && !hello.adapter_versions.contains_key("devin"))
        {
            return Err(HostError::Incompatible("live runtime lacks the required provider adapter; explicitly drain before replacing it".into()));
        }
        Ok(endpoint)
    }
    fn client_error(error: ClientError) -> HostError {
        match error {
            ClientError::Host(error) => error,
            ClientError::Io(error) => io(error),
            ClientError::Protocol(detail) => HostError::Malformed(detail),
        }
    }
    fn controller_token(root: &Path) -> Result<String, HostError> {
        let metadata = fs::symlink_metadata(root.join("identity.json")).map_err(io)?;
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.len() > 16 * 1024
        {
            return Err(HostError::Unauthorized);
        }
        let identity: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("identity.json")).map_err(io)?)
                .map_err(|_| HostError::Malformed("runtime identity".into()))?;
        let token = identity["controller"]
            .as_str()
            .ok_or(HostError::Unauthorized)?;
        Ok(token.to_owned())
    }
    fn digest(path: &Path) -> Result<String, HostError> {
        let mut file = fs::File::open(path).map_err(io)?;
        let mut hash = Sha256::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut chunk).map_err(io)?;
            if count == 0 {
                break;
            }
            hash.update(&chunk[..count]);
        }
        Ok(format!("{:x}", hash.finalize()))
    }
    fn pin(root: &Path) -> Result<PathBuf, HostError> {
        let source = std::env::current_exe().map_err(io)?;
        let hash = digest(&source)?;
        let directory = root.join("artifacts").join(&hash);
        ipc::private_root(&directory)?;
        let destination = directory.join("waku-runtime-host");
        if destination.exists() {
            if digest(&destination)? != hash
                || fs::symlink_metadata(&destination)
                    .map_err(io)?
                    .file_type()
                    .is_symlink()
            {
                return Err(HostError::Unauthorized);
            }
            return Ok(destination);
        }
        let temporary = directory.join(format!("pin-{}", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o500)
            .open(&temporary)
            .map_err(io)?;
        let mut input = fs::File::open(source).map_err(io)?;
        std::io::copy(&mut input, &mut file).map_err(io)?;
        file.flush().map_err(io)?;
        file.sync_all().map_err(io)?;
        if digest(&temporary)? != hash {
            return Err(HostError::Io("executable changed while pinning".into()));
        }
        fs::rename(&temporary, &destination).map_err(io)?;
        fs::File::open(&directory)
            .and_then(|f| f.sync_all())
            .map_err(io)?;
        Ok(destination)
    }
    fn usage() -> HostError {
        HostError::Malformed("usage: waku-runtime-host start|status|serve --root ABSOLUTE_PRIVATE_DIRECTORY --namespace ENVIRONMENT_PROJECT_ID; drain also requires --policy whenIdle|forced".into())
    }
    fn io(error: std::io::Error) -> HostError {
        HostError::Io(error.to_string())
    }
}
#[cfg(unix)]
fn main() {
    if let Err(error) = unix::main() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(unix))]
fn main() {
    eprintln!("stable private IPC is not qualified on this platform");
    std::process::exit(1);
}
