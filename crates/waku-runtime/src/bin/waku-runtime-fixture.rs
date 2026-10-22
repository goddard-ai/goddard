//! Explicit fake-host fixture for IPC qualification tests.
//! The independently pinned production launcher lives in waku-runtime-host.

use std::process::ExitCode;

#[cfg(unix)]
use std::{path::PathBuf, sync::Arc, time::Duration};
#[cfg(unix)]
use waku_runtime::{
    adapter::AdapterFactory,
    fake::FakeFactory,
    host::{HostOptions, RuntimeHost, SystemClock},
    registry::AdapterRegistry,
    server::RuntimeServer,
};

#[cfg(unix)]
fn run() -> ExitCode {
    let mut root: Option<PathBuf> = None;
    let mut namespace = "default".to_string();
    let mut pump_ms = 10u64;
    let mut fake = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--namespace" => namespace = args.next().unwrap_or(namespace),
            "--pump-ms" => pump_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(pump_ms),
            // Deterministic-harness seam: real provider adapters register
            // here as their stages land; the fake is explicit opt-in only.
            "--enable-fake-adapter" => fake = true,
            other => {
                eprintln!("waku-runtime-host: unknown argument {other}");
                eprintln!(
                    "usage: waku-runtime-host --root PATH [--namespace NAME] [--pump-ms N] [--enable-fake-adapter]"
                );
                return ExitCode::from(2);
            }
        }
    }
    let Some(root) = root else {
        eprintln!("usage: waku-runtime-host --root PATH [--namespace NAME] [--pump-ms N]");
        return ExitCode::from(2);
    };
    let mut registry = AdapterRegistry::new();
    if fake {
        registry.register(
            "fake",
            Arc::new(FakeFactory::new()) as Arc<dyn AdapterFactory>,
        );
    }
    let host = match RuntimeHost::open(
        &root,
        HostOptions::new(namespace),
        Arc::new(registry),
        Box::new(SystemClock),
    ) {
        Ok(host) => host,
        Err(error) => {
            eprintln!("waku-runtime-host: {error}");
            return ExitCode::FAILURE;
        }
    };
    let _server = match RuntimeServer::bind(host, &root, Duration::from_millis(pump_ms)) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("waku-runtime-host: {error}");
            return ExitCode::FAILURE;
        }
    };
    // Parked forever: only an explicit drain command or a signal ends the
    // instance — never a parent's exit.
    loop {
        std::thread::park();
    }
}

#[cfg(unix)]
fn main() -> ExitCode {
    run()
}

#[cfg(not(unix))]
fn main() -> ExitCode {
    eprintln!("waku-runtime-host: unix-only transport in this stage");
    ExitCode::FAILURE
}
