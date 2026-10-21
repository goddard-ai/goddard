//! Real daemon/controller with a scripted external provider and synthetic data.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const MUTATE: &str = "ACCEPTANCE_WRITE_TASK";

struct ScriptedProvider {
    prompts: Mutex<Vec<String>>,
    mutations: Mutex<Vec<PathBuf>>,
    target: PathBuf,
}
impl driver::DriverControl for ScriptedProvider {
    fn prompt(&self, text: String) {
        if text.contains(MUTATE) {
            // An independent observable side effect, not just a prompt count.
            std::fs::write(&self.target, "scripted task change").unwrap();
            self.mutations.lock().push(self.target.clone());
        }
        self.prompts.lock().push(text);
    }
    fn cancel(&self) {}
    fn respond(&self, _: String, _: String) {}
    fn rollback(
        &self,
        _: usize,
    ) -> anyhow::Result<Option<waku_protocol::model::ProviderResumeCursor>> {
        Ok(None)
    }
}

#[test]
fn plan_acceptance_provider_workspace_and_launch_fault_matrix() {
    let mut cases = 0;
    for provider in [ProviderKind::Codex, ProviderKind::Devin] {
        for workspace in ["direct", "worktree", "cross-project"] {
            for fault in [
                "none",
                "unconfirmed",
                "launch",
                "auth-after-launch",
                "provider-switch",
                "workspace-switch",
                "revision-switch",
            ] {
                let root = std::env::temp_dir().join(format!("plan-acceptance-{}", Uuid::new_v4()));
                let (mut backend, session) = runtime_generation_launch_fixture(&root, true);
                runtime_generation_configure_launch(&mut backend, &root);
                let plan = boss_plan("plans/acceptance.md", &[]);
                backend.boss.add_plan(plan.clone()).unwrap();
                // Both reported failing sizes plus the complete approval range
                // cross the real launch preflight for each supported adapter.
                let bytes = match workspace {
                    "direct" => 33_290,
                    "worktree" => 40_009,
                    _ => 256 * 1024,
                };
                let header = "# Reviewed fixture\n";
                let approved_text = format!("{header}{}", "x".repeat(bytes - header.len()));
                let file = backend.boss.plan_document_path(&plan.plan_file).unwrap();
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, &approved_text).unwrap();
                let reviewed = review_plan_fixture(&backend, &plan.plan_file);
                backend
                    .boss
                    .finalize_plan(&plan.plan_file, None, 1, &reviewed)
                    .unwrap();
                backend
                    .boss
                    .set_employee_plan(session, Some(Some(plan.plan_file)), None)
                    .unwrap();
                let cwd = if workspace == "direct" {
                    root.join("repo")
                } else {
                    root.join(workspace)
                };
                std::fs::create_dir_all(cwd.join("plans")).unwrap();
                std::fs::write(
                    cwd.join("plans/acceptance.md"),
                    "PROJECT_LOOKALIKE_SENTINEL",
                )
                .unwrap();
                {
                    let mut state = backend.task_state.lock();
                    let mut project = state.projects[0].clone();
                    if workspace == "cross-project" {
                        project.id = Uuid::new_v4();
                        project.path = cwd.clone();
                        state.projects.push(project.clone());
                    }
                    let target = state.session_mut(session).unwrap();
                    backend.task_store.hydrate(target).unwrap();
                    target.provider = provider;
                    if workspace == "worktree" {
                        target.workspace = SessionWorkspace::Worktree {
                            path: cwd.clone(),
                            name: "fixture".into(),
                            branch: None,
                            base_branch: None,
                            adopted_by: None,
                            borrowed: false,
                        };
                    } else if workspace == "cross-project" {
                        target.project_id = project.id;
                    }
                    state.mark_session_dirty(session);
                    backend.task_store.save(&mut state).unwrap();
                }
                let mut settings = backend.settings.get();
                settings.provider_binary_overrides.insert(
                    provider,
                    root.join("controlled-provider").display().to_string(),
                );
                backend.settings.replace(settings).unwrap();
                let script = Arc::new(ScriptedProvider {
                    prompts: Mutex::new(Vec::new()),
                    mutations: Mutex::new(Vec::new()),
                    target: cwd.join("task-result"),
                });
                let starts = Arc::new(AtomicUsize::new(0));
                let launches = starts.clone();
                let raw = script.clone();
                let agent = backend.agent.clone();
                // Retain event transport so the controlled runtime does not exit.
                let transport = Arc::new(Mutex::new(None));
                let sender = transport.clone();
                backend.runtime_test_start = Some(Arc::new(move |_, options, events| {
                    assert_eq!(
                        dunce::canonicalize(&options.cwd).unwrap(),
                        dunce::canonicalize(raw.target.parent().unwrap()).unwrap()
                    );
                    launches.fetch_add(1, Ordering::SeqCst);
                    if fault == "launch" {
                        anyhow::bail!("injected provider launch failure");
                    }
                    if fault == "auth-after-launch" {
                        agent.revoke_session(options.agent.as_ref().unwrap().task_id);
                    }
                    *sender.lock() = Some(events);
                    Ok(DriverHandle::from_control(raw.clone()))
                }));
                let runtime = backend.ensure_agent_runtime(session, &EventSink::detached());
                assert_eq!(
                    starts.load(Ordering::SeqCst),
                    1,
                    "{provider:?}/{workspace}/{fault}"
                );
                if fault == "launch" {
                    assert!(runtime.is_err());
                } else if let Ok((_, driver)) = runtime {
                    // Identity changes land before the first submission so no
                    // execution input is authorized for the stale attempt.
                    match fault {
                        "provider-switch" | "workspace-switch" => {
                            let mut state = backend.task_state.lock();
                            let target = state.session_mut(session).unwrap();
                            if fault == "provider-switch" {
                                target.provider = if provider == ProviderKind::Codex {
                                    ProviderKind::Devin
                                } else {
                                    ProviderKind::Codex
                                };
                            } else {
                                let moved = root.join("replacement-workspace");
                                std::fs::create_dir_all(&moved).unwrap();
                                target.workspace = SessionWorkspace::Worktree {
                                    path: moved,
                                    name: "replacement".into(),
                                    branch: None,
                                    base_branch: None,
                                    adopted_by: None,
                                    borrowed: false,
                                };
                            }
                            state.mark_session_dirty(session);
                            backend.task_store.save(&mut state).unwrap();
                        }
                        "revision-switch" => {
                            let replacement = boss_plan("plans/replacement.md", &[]);
                            backend.boss.add_plan(replacement.clone()).unwrap();
                            let reviewed = review_plan_fixture(&backend, &replacement.plan_file);
                            backend
                                .boss
                                .finalize_plan(&replacement.plan_file, None, 2, &reviewed)
                                .unwrap();
                            let accepted = backend.boss.employee(session).unwrap().plan_binding;
                            assert!(
                                backend
                                    .boss
                                    .set_employee_plan(
                                        session,
                                        Some(Some(replacement.plan_file)),
                                        None
                                    )
                                    .is_err()
                            );
                            assert_eq!(
                                backend.boss.employee(session).unwrap().plan_binding,
                                accepted
                            );
                        }
                        _ => {}
                    }
                    driver.prompt(MUTATE.into());
                    let gate = backend.sessions.lock()[&session].plan_gate.clone().unwrap();
                    match fault {
                        "none" | "unconfirmed" | "revision-switch" => {
                            // The complete input carries exact approved bytes
                            // and the brief together — no review turn, no
                            // ack line, no lookalike content.
                            let prompts = script.prompts.lock();
                            assert_eq!(prompts.len(), 1);
                            assert!(prompts[0].contains(&approved_text));
                            assert!(prompts[0].contains(MUTATE));
                            assert!(!prompts[0].contains("PLAN-ACK "));
                            assert!(!prompts[0].contains("PROJECT_LOOKALIKE_SENTINEL"));
                            drop(prompts);
                            assert!(!gate.delivered());
                            if fault != "unconfirmed" {
                                gate.observe(&DriverEvent::TurnStarted);
                                assert!(gate.delivered());
                                // Delivered follow-ups revalidate but resend
                                // only the prompt, not the approved bytes.
                                driver.prompt(MUTATE.into());
                                let prompts = script.prompts.lock();
                                assert_eq!(prompts.len(), 2);
                                assert!(!prompts[1].contains("# Reviewed fixture\n"));
                            }
                        }
                        _ => {
                            // Blocked before submission: the gate held the
                            // brief and the provider saw nothing.
                            assert!(script.prompts.lock().is_empty(), "{fault}");
                            assert!(!gate.delivered(), "{fault}");
                            let failure = backend
                                .boss
                                .employee(session)
                                .unwrap()
                                .plan_readiness
                                .and_then(|r| r.failure)
                                .unwrap();
                            assert!(
                                matches!(
                                    failure.kind,
                                    waku_protocol::boss::PlanFailureKind::RuntimeAuthentication
                                        | waku_protocol::boss::PlanFailureKind::BindingChanged
                                ),
                                "{fault}: {failure:?}"
                            );
                        }
                    }
                }
                // Physical mutation evidence is separate from authorization:
                // only submissions the adapter actually received can mutate.
                let expected = match fault {
                    "none" | "revision-switch" => 2,
                    "unconfirmed" => 1,
                    _ => 0,
                };
                assert_eq!(
                    script.mutations.lock().len(),
                    expected,
                    "{provider:?}/{workspace}/{fault}"
                );
                backend.shutdown();
                drop(backend);
                std::fs::remove_dir_all(root).unwrap();
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 42);
    eprintln!(
        "plan acceptance runtime matrix: {cases} passed; Codex/Devin x direct/worktree/cross-project x success/unconfirmed delivery/launch failure/auth revoked/provider switch/workspace switch/revision switch rejected"
    );
}

#[test]
fn plan_acceptance_ordinary_no_plan_work_keeps_unstaged_dispatch() {
    for managed in [false, true] {
        for provider in [ProviderKind::Codex, ProviderKind::Devin] {
            let root = std::env::temp_dir().join(format!("no-plan-acceptance-{}", Uuid::new_v4()));
            let (mut backend, session) = runtime_generation_launch_fixture(&root, managed);
            runtime_generation_configure_launch(&mut backend, &root);
            {
                let mut state = backend.task_state.lock();
                state.session_mut(session).unwrap().provider = provider;
                state.mark_session_dirty(session);
                backend.task_store.save(&mut state).unwrap();
            }
            let mut settings = backend.settings.get();
            settings.provider_binary_overrides.insert(
                provider,
                root.join("controlled-provider").display().to_string(),
            );
            backend.settings.replace(settings).unwrap();
            let raw = Arc::new(ScriptedProvider {
                prompts: Mutex::new(Vec::new()),
                mutations: Mutex::new(Vec::new()),
                target: root.join("repo/ordinary-result"),
            });
            let script = raw.clone();
            let transport = Arc::new(Mutex::new(None));
            let sender = transport.clone();
            backend.runtime_test_start = Some(Arc::new(move |_, _, events| {
                *sender.lock() = Some(events);
                Ok(DriverHandle::from_control(script.clone()))
            }));
            let (_, driver) = backend
                .ensure_agent_runtime(session, &EventSink::detached())
                .unwrap();
            assert!(backend.sessions.lock()[&session].plan_gate.is_none());
            driver.prompt(MUTATE.into());
            assert_eq!(raw.mutations.lock().len(), 1);
            assert!(raw.target.exists());
            backend.shutdown();
            drop(backend);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    eprintln!("plan acceptance ordinary work: 4 passed; no plan gate or acknowledgement required");
}
