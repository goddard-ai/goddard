//! A generic host attachment for control-plane tests. It directly constructs
//! LinkShared; it does not qualify or change any production launch schema.

use super::*;
use waku_runtime::adapter::AdapterFactory;
use waku_runtime::fake::FakeFactory;
use waku_runtime::host::{Budgets, HostOptions, RuntimeHost, SystemClock};
use waku_runtime::registry::AdapterRegistry;
use waku_runtime::server::RuntimeServer;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct ConfirmationFixture {
    pub(crate) driver: DriverHandle,
    pub(crate) link: StableLink,
    pub(crate) factory: FakeFactory,
    host: Option<RuntimeServer>,
    _events: crossbeam_channel::Receiver<DriverEvent>,
    _scratch: Scratch,
}

impl ConfirmationFixture {
    pub(crate) fn new(backend: &Arc<WakuBackend>, task: Uuid, runtime: Uuid) -> Self {
        let root = std::env::temp_dir().join(format!("stable-confirmation-{}", Uuid::new_v4()));
        waku_runtime::ipc::private_root(&root).unwrap();
        let scratch = Scratch(root.clone());
        let factory = FakeFactory::new();
        let mut registry = AdapterRegistry::new();
        registry.register(
            "claude",
            Arc::new(factory.clone()) as Arc<dyn AdapterFactory>,
        );
        let host = RuntimeHost::open(
            &root,
            HostOptions {
                namespace: STABLE_NAMESPACE.into(),
                budgets: Budgets::default(),
                journal_fault: None,
            },
            Arc::new(registry),
            Box::new(SystemClock),
        )
        .unwrap();
        let credential = host.credentials().controller.clone();
        let host = RuntimeServer::bind(host, &root, Duration::from_millis(5)).unwrap();
        let (mut client, hello) =
            RuntimeClient::controller(&root, &credential, &ClientHello::current()).unwrap();
        let grant = client.acquire_control(&ClientHello::current()).unwrap();
        let epoch = grant.epoch;
        let retry_window_id = grant.retry_window_id;
        let initial = waku_runtime::contract::RequestedConfig {
            provider: Some("claude".into()),
            model: Some("initial-model".into()),
            effort: None,
            options: Value::Null,
        };
        client
            .execute(ActionEnvelope {
                namespace: STABLE_NAMESPACE.into(),
                runtime_instance: hello.runtime_instance,
                epoch,
                retry_window_id,
                command_id: runtime,
                session: None,
                action: CanonicalAction::AdmitSession {
                    session_id: runtime,
                    adapter: "claude".into(),
                    launch: json!({"fixture": "control-plane-only"}),
                    requested: initial,
                },
            })
            .unwrap();
        let controller = Arc::new(ControllerLease {
            instance: hello.runtime_instance,
            grant,
            host_pid: std::process::id(),
            routes: Mutex::new(HashMap::new()),
            service_gate: Mutex::new(()),
        });
        let (sender, received) = crate::driver::test_event_channel();
        let shared = Arc::new(LinkShared {
            controller: controller.clone(),
            provider: ProviderKind::Claude,
            mode: crate::model::RuntimeMode::Ask,
            task_id: task,
            runtime_id: runtime,
            sref: SessionRef {
                session_id: runtime,
                generation: 1,
            },
            namespace: STABLE_NAMESPACE.into(),
            instance: hello.runtime_instance,
            epoch,
            retry_window_id,
            marker_epoch: Uuid::new_v4(),
            credential,
            root,
            client: Mutex::new(client),
            store: backend.task_store().clone(),
            sender,
            service: Arc::new(BackendIngress {
                backend: backend.weak_self(),
                task_id: task,
                runtime_id: runtime,
            }),
            active_turn: Mutex::new(None),
            gate: Mutex::new(DispatchGate::default()),
            config_revision: Mutex::new(0),
            applied_configuration: Mutex::new(None),
            configuration_changed: Condvar::new(),
            input_schemas: Mutex::new(HashMap::new()),
            agent_delivery: crate::driver::AgentSurfaceDelivery::Absent,
            agent_descriptor: None,
            stop: Arc::new(AtomicBool::new(false)),
            threads: Mutex::new(Vec::new()),
        });
        controller
            .routes
            .lock()
            .unwrap()
            .insert(runtime, Arc::downgrade(&shared));
        let link = StableLink {
            shared: shared.clone(),
            initial_configuration_confirmed: true,
            _lifetime: Arc::new(LinkLifetime(shared.stop.clone())),
        };
        spawn_pump(&shared, ReconciledRequests::default());
        let driver = DriverHandle::from_control(Arc::new(link.clone())).with_initial_options(
            crate::driver::SessionOptions {
                mode: crate::model::RuntimeMode::Ask,
                model: Some("initial-model".into()),
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
            },
        );
        Self {
            driver,
            link,
            factory,
            host: Some(host),
            _events: received,
            _scratch: scratch,
        }
    }

    pub(crate) fn stop(&self) {
        let _confirmed = self.link.shared.applied_configuration.lock().unwrap();
        self.link.shared.stop.store(true, Ordering::Release);
        self.link.shared.configuration_changed.notify_all();
    }
}

impl Drop for ConfirmationFixture {
    fn drop(&mut self) {
        self.stop();
        let workers = std::mem::take(&mut *self.link.shared.threads.lock().unwrap());
        for worker in workers {
            let _ = worker.join();
        }
        self.host.take();
    }
}
