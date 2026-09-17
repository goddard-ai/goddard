use super::*;

fn workspace_ack(
    workspace: &waku_client::WorkspaceClient,
    operation: waku_client::WorkspaceOperation,
) -> anyhow::Result<()> {
    match workspace.request(operation)? {
        waku_client::WorkspaceResult::Ack => Ok(()),
        _ => anyhow::bail!("the daemon returned an invalid workspace response"),
    }
}

fn workspace_has_ref(
    workspace: &waku_client::WorkspaceClient,
    cwd: &Path,
    git_ref: &str,
) -> anyhow::Result<bool> {
    match workspace.request(waku_client::WorkspaceOperation::HasRef {
        cwd: cwd.to_path_buf(),
        git_ref: git_ref.to_owned(),
    })? {
        waku_client::WorkspaceResult::Bool { value } => Ok(value),
        _ => anyhow::bail!("the daemon returned an invalid checkpoint response"),
    }
}

fn start_driver(mut request: DriverStartRequest, cwd: PathBuf) -> anyhow::Result<PreparedDriver> {
    request.options.cwd = cwd;
    let (event_tx, events) = driver::event_channel(request.event_wake);
    let handle = driver::start_remote(
        request.daemon,
        request.session_id,
        request.provider,
        request.options,
        event_tx,
    )?;
    Ok(PreparedDriver { handle, events })
}

/// What a remote connect attempt produced: the supervisor plus, for ssh
/// hosts, the transport whose tunnel and forward keep the socket alive.
pub(super) struct RemoteDaemonLink {
    supervisor: waku_client::DaemonSupervisor,
    #[cfg(unix)]
    ssh: Option<SshLink>,
}

/// A live ssh channel: the ControlMaster-managed transport and the local
/// port its `-L` forward owns. The port must stay fixed so the supervisor's
/// reconnects keep working after the master restarts.
#[cfg(unix)]
#[derive(Clone)]
pub(super) struct SshLink {
    transport: crate::ssh::SshTransport,
    local_port: u16,
}

/// One connect attempt against a remote host: over ssh when the record has
/// a destination, direct websocket otherwise. Blocking — run on the
/// background executor.
fn connect_remote_supervisor(
    host: &waku_client::persistence::RemoteHost,
) -> anyhow::Result<RemoteDaemonLink> {
    #[cfg(unix)]
    if let Some(destination) = host.ssh_destination.as_deref() {
        let transport = crate::ssh::SshTransport::new(host.id, destination);
        let (local_port, supervisor) = transport.connect()?;
        return Ok(RemoteDaemonLink {
            supervisor,
            ssh: Some(SshLink {
                transport,
                local_port,
            }),
        });
    }
    #[cfg(not(unix))]
    if host.ssh_destination.is_some() {
        anyhow::bail!("ssh remotes are not supported on this platform");
    }
    let supervisor = waku_client::DaemonSupervisor::connect(&host.address, host.token.clone())?;
    Ok(RemoteDaemonLink {
        supervisor,
        #[cfg(unix)]
        ssh: None,
    })
}

/// A password/passphrase request ssh is waiting on, presented as a modal.
#[cfg(unix)]
pub(super) struct SshPrompt {
    /// The prompt text ssh sent the askpass helper, e.g. `host's password:`.
    pub prompt: String,
    request: crate::ssh::SshAskpassRequest,
    /// Created by the dialog's render path, which owns a `Window`.
    pub input: Option<Entity<crate::input::TextInput>>,
}

fn attach_driver(
    daemon: waku_client::DaemonSupervisor,
    session_id: Uuid,
    event_wake: smol::channel::Sender<()>,
) -> anyhow::Result<Option<(AgentSession, PreparedDriver)>> {
    let Some(session) = waku_client::persistence::hydrate_session(&daemon, session_id)? else {
        return Ok(None);
    };
    let client = daemon.client();
    let response = client.request(session_id, Uuid::nil(), waku_client::Command::AttachSession)?;
    let waku_client::ResponsePayload::SessionRuntime {
        runtime_id,
        supports_steer,
    } = response
    else {
        anyhow::bail!("Goddard daemon returned an invalid runtime attachment response");
    };
    let Some(runtime_id) = runtime_id else {
        return Ok(None);
    };
    let (event_tx, events) = driver::event_channel(event_wake);
    let handle = driver::attach_remote(
        daemon,
        client,
        session_id,
        runtime_id,
        supports_steer,
        session.runtime_event_cursor,
        event_tx,
    )?;
    Ok(Some((session, PreparedDriver { handle, events })))
}

fn load_remote_task_state(
    client: &waku_client::DaemonClient,
) -> anyhow::Result<RemoteTaskStateSnapshot> {
    let response = client.request(
        Uuid::nil(),
        Uuid::nil(),
        waku_client::Command::LoadTaskState,
    )?;
    let waku_client::ResponsePayload::TaskState {
        projects,
        mut sessions,
        ..
    } = response
    else {
        anyhow::bail!("Goddard daemon returned an invalid task-state response");
    };
    for session in &mut sessions {
        session.detail_loaded = false;
    }
    Ok(RemoteTaskStateSnapshot { projects, sessions })
}

/// The running turn and the user message that opened it — the identity other
/// clients adopt when the daemon publishes this submission to the runtime.
pub(super) fn submitted_prompt_identity(session: &AgentSession) -> (Option<Uuid>, Option<Uuid>) {
    let Some(turn_id) = session.active_turn_id() else {
        return (None, None);
    };
    let message_id = session
        .messages
        .iter()
        .find(|message| message.turn_id == Some(turn_id) && message.role == MessageRole::User)
        .map(|message| message.id);
    (Some(turn_id), message_id)
}

pub(super) fn session_has_active_provider_turn(session: &AgentSession) -> bool {
    session.is_busy()
        && session
            .turns
            .last()
            .is_some_and(|turn| turn.status == TurnStatus::Running && turn.provider_turn_started)
}

/// Merge the daemon's list-only session projection into the desktop catalog.
///
/// Existing rows may already contain a hydrated transcript, so only list
/// metadata is copied from the projection. A locally attached runtime remains
/// authoritative for transient status and timestamps until its own events are
/// drained.
/// `owns` scopes deletions to the merging daemon's rows: a remote host's
/// session must never vanish because the *local* daemon's snapshot lacks it.
pub(super) fn merge_remote_session_catalog(
    local: &mut Vec<AgentSession>,
    remote: Vec<AgentSession>,
    owns: impl Fn(Uuid) -> bool,
    has_local_runtime: impl Fn(Uuid) -> bool,
) -> Vec<Uuid> {
    let remote_ids = remote
        .iter()
        .map(|session| session.id)
        .collect::<HashSet<_>>();
    let removed = local
        .iter()
        .filter(|session| {
            session.has_started() && owns(session.id) && !remote_ids.contains(&session.id)
        })
        .map(|session| session.id)
        .collect::<Vec<_>>();
    local.retain(|session| {
        !session.has_started() || !owns(session.id) || remote_ids.contains(&session.id)
    });

    for remote in remote {
        if let Some(local) = local.iter_mut().find(|session| session.id == remote.id) {
            local.title = remote.title;
            local.auto_title = remote.auto_title;
            local.project_id = remote.project_id;
            local.provider = remote.provider;
            local.model = remote.model;
            local.created_at = remote.created_at;
            local.last_reply_at = remote.last_reply_at;
            local.archived_at = remote.archived_at;
            // A hydrated session's workspace is at least as fresh as the
            // stored column the projection carries — and may hold an unsaved
            // move — so only skeletons adopt it.
            if !local.detail_loaded {
                local.workspace = remote.workspace;
            }
            if !has_local_runtime(local.id) {
                local.status = remote.status;
                local.updated_at = remote.updated_at;
            }
        } else {
            local.push(remote);
        }
    }

    removed
}

/// Perform every blocking operation between accepting a submission and
/// starting its provider. This function is called only from the background
/// executor; the UI thread owns applying the returned workspace afterward.
fn prepare_submission(
    workspace_client: waku_client::WorkspaceClient,
    project: Project,
    workspace: SessionWorkspace,
    driver_start: Option<anyhow::Result<DriverStartRequest>>,
    session_id: Uuid,
    turn_count: usize,
) -> anyhow::Result<PreparedSubmission> {
    let mut worktree_restored = false;
    let workspace = match workspace {
        SessionWorkspace::NewWorktree { base_branch } => {
            if project.is_projectless() {
                anyhow::bail!("a projectless task cannot create a Git worktree");
            }
            let created =
                match workspace_client.request(waku_client::WorkspaceOperation::CreateWorktree {
                    project_path: project.path.clone(),
                    name: None,
                    base_ref: base_branch.clone(),
                })? {
                    waku_client::WorkspaceResult::WorktreeCreated { worktree } => worktree,
                    _ => anyhow::bail!("the daemon returned an invalid worktree response"),
                };
            SessionWorkspace::Worktree {
                path: created.path,
                name: created.name,
                branch: None,
                base_branch,
            }
        }
        SessionWorkspace::Worktree {
            path,
            name,
            branch,
            base_branch,
        } => {
            // The directory can vanish between visits — archived tasks
            // outlive their worktrees once archive cleanup removes them.
            // The daemon no-ops while it exists, so this doubles as the
            // existence check on the daemon host. Restoration prefers the
            // worktree's branch, then the snapshot archive cleanup left,
            // then the latest checkpoint, so a continued turn lands on the
            // state the transcript shows.
            let archive_ref = checkpoint::archive_ref(session_id);
            let archived = workspace_client
                .request(waku_client::WorkspaceOperation::HasRef {
                    cwd: project.path.clone(),
                    git_ref: archive_ref.clone(),
                })
                .is_ok_and(|result| {
                    matches!(result, waku_client::WorkspaceResult::Bool { value: true })
                });
            let base_ref = if archived {
                Some(archive_ref.clone())
            } else {
                Some(checkpoint::checkpoint_ref(
                    session_id,
                    turn_count.saturating_sub(1),
                ))
            };
            let ensured =
                match workspace_client.request(waku_client::WorkspaceOperation::EnsureWorktree {
                    project_path: project.path.clone(),
                    path: path.clone(),
                    branch: branch.clone(),
                    base_ref,
                })? {
                    waku_client::WorkspaceResult::WorktreeEnsured { created, branch } => {
                        (created, branch)
                    }
                    _ => anyhow::bail!("the daemon returned an invalid worktree response"),
                };
            // The archive snapshot is single-use: once the worktree exists
            // again the ref is stale, and keeping it would shadow newer
            // checkpoints the next time the directory disappears.
            if archived {
                let _ = workspace_client.request(waku_client::WorkspaceOperation::DeleteRef {
                    cwd: project.path.clone(),
                    git_ref: archive_ref,
                });
            }
            worktree_restored = ensured.0;
            SessionWorkspace::Worktree {
                path,
                name,
                // A branch that no longer exists comes back detached; the
                // reported checkout replaces the stale persisted one.
                branch: if ensured.0 { ensured.1 } else { branch },
                base_branch,
            }
        }
        workspace => {
            // The daemon no-ops while the directory exists, so this doubles
            // as the existence check on the daemon host — the role
            // `EnsureWorktree` plays above. Archive cleanup may have zipped
            // a projectless workspace away; restore brings it back.
            if project.is_projectless() && !crate::projectless::is_legacy_root_path(&project.path) {
                match workspace_client.request(
                    waku_client::WorkspaceOperation::RestoreProjectlessWorkspace {
                        path: project.path.clone(),
                    },
                )? {
                    waku_client::WorkspaceResult::Bool { .. } => {}
                    _ => anyhow::bail!("the daemon returned an invalid projectless response"),
                }
            }
            workspace
        }
    };
    let project_path = workspace.path().unwrap_or(&project.path);

    // Every turn gets its own immutable starting snapshot. Reusing the prior
    // response's ending ref would attribute branch switches or terminal edits
    // made between turns to the next response.
    let checkpoint_warning = workspace_ack(
        &workspace_client,
        waku_client::WorkspaceOperation::CaptureTurnStart {
            cwd: project_path.to_path_buf(),
            session_id,
            turn_count,
        },
    )
    .err()
    .map(|error| tr!("errors.capture_pre_turn_checkpoint", error = error));

    // Process startup can synchronously resolve executables, bind sockets,
    // and spawn children. It belongs behind the same animated preparation
    // boundary as Git work, otherwise the last spinner frame visibly freezes
    // just before Stop appears.
    let driver = driver_start.map(|request| {
        request.and_then(|request| start_driver(request, project_path.to_path_buf()))
    });

    Ok(PreparedSubmission {
        workspace,
        checkpoint_warning,
        worktree_restored,
        driver,
    })
}

/// Everything a past-message resend needs after the UI accepts it.
///
/// The request owns only thread-safe snapshots. Git, provider RPCs, process
/// startup, and native transcript reads all happen in
/// [`perform_message_rewind`] on the background executor.
struct MessageRewindRequest {
    workspace_client: waku_client::WorkspaceClient,
    session_id: Uuid,
    provider: ProviderKind,
    provider_cursor: Option<ProviderResumeCursor>,
    session_title: String,
    /// Cursor has no native branch API, so its background helper needs the
    /// retained visible transcript. Other providers avoid cloning a long task
    /// on the click path entirely.
    cursor_source: Option<AgentSession>,
    project_path: PathBuf,
    retained_turn_count: usize,
    previous_turn_count: usize,
    rollback_turns: usize,
    provider_turn_count: usize,
    provider_resume_at: Option<String>,
    binary: Option<PathBuf>,
    driver: Option<DriverHandle>,
    driver_start: Option<DriverStartRequest>,
}

struct PreparedMessageRewind {
    provider_rewind_cursor: Option<ProviderResumeCursor>,
    claude_fork: Option<waku_client::provider_session::ProviderSessionFork>,
    prepared_driver: Option<PreparedDriver>,
    reset_native_session: bool,
    cleanup_error: Option<String>,
}

fn perform_message_rewind(
    mut request: MessageRewindRequest,
) -> Result<PreparedMessageRewind, String> {
    let session_id = request.session_id;
    let turn_start_ref =
        checkpoint::turn_start_ref(session_id, request.retained_turn_count.saturating_add(1));
    let retained_ref = checkpoint::checkpoint_ref(session_id, request.retained_turn_count);
    let restore_ref = if workspace_has_ref(
        &request.workspace_client,
        &request.project_path,
        &turn_start_ref,
    )
    .map_err(|error| error.to_string())?
    {
        turn_start_ref
    } else {
        retained_ref
    };
    if !workspace_has_ref(
        &request.workspace_client,
        &request.project_path,
        &restore_ref,
    )
    .map_err(|error| error.to_string())?
    {
        return Err(tr!("session.pre_turn_checkpoint_missing"));
    }

    let safety_ref = format!("refs/waku/revert-backup-{session_id}-{}", Uuid::new_v4());
    workspace_ack(
        &request.workspace_client,
        waku_client::WorkspaceOperation::CaptureRef {
            cwd: request.project_path.clone(),
            git_ref: safety_ref.clone(),
        },
    )
    .map_err(|error| tr!("errors.create_rewind_snapshot", error = error))?;
    if let Err(error) = workspace_ack(
        &request.workspace_client,
        waku_client::WorkspaceOperation::RestoreRef {
            cwd: request.project_path.clone(),
            git_ref: restore_ref.clone(),
        },
    ) {
        return Err(
            match workspace_ack(
                &request.workspace_client,
                waku_client::WorkspaceOperation::RestoreRef {
                    cwd: request.project_path.clone(),
                    git_ref: safety_ref.clone(),
                },
            ) {
                Ok(()) => {
                    let _ = workspace_ack(
                        &request.workspace_client,
                        waku_client::WorkspaceOperation::DeleteRef {
                            cwd: request.project_path.clone(),
                            git_ref: safety_ref.clone(),
                        },
                    );
                    tr!("errors.restore_checkpoint", error = error)
                }
                Err(restore_error) => tr!(
                    "errors.restore_checkpoint_and_safety",
                    error = error,
                    restore_error = restore_error,
                    safety_ref = safety_ref
                ),
            },
        );
    }

    let provider_rewind = perform_provider_rewind(&mut request);
    let (provider_rewind_cursor, claude_fork, prepared_driver) = match provider_rewind {
        Ok(rewind) => rewind,
        Err(error) => {
            return Err(
                match workspace_ack(
                    &request.workspace_client,
                    waku_client::WorkspaceOperation::RestoreRef {
                        cwd: request.project_path.clone(),
                        git_ref: safety_ref.clone(),
                    },
                ) {
                    Ok(()) => {
                        let _ = workspace_ack(
                            &request.workspace_client,
                            waku_client::WorkspaceOperation::DeleteRef {
                                cwd: request.project_path.clone(),
                                git_ref: safety_ref.clone(),
                            },
                        );
                        tr!("errors.rollback_rejected_workspace_restored", error = error)
                    }
                    Err(restore_error) => tr!(
                        "errors.rollback_and_safety_failed",
                        error = error,
                        restore_error = restore_error,
                        safety_ref = safety_ref
                    ),
                },
            );
        }
    };

    let _ = workspace_ack(
        &request.workspace_client,
        waku_client::WorkspaceOperation::DeleteRef {
            cwd: request.project_path.clone(),
            git_ref: safety_ref,
        },
    );
    let cleanup_error = workspace_ack(
        &request.workspace_client,
        waku_client::WorkspaceOperation::DeleteTurnRefsAfter {
            cwd: request.project_path.clone(),
            session_id,
            retained_turn_count: request.retained_turn_count,
            previous_turn_count: request.previous_turn_count,
        },
    )
    .err()
    .map(|error| error.to_string());

    Ok(PreparedMessageRewind {
        provider_rewind_cursor,
        claude_fork,
        prepared_driver,
        reset_native_session: request.rollback_turns > 0
            && request.retained_turn_count == 0
            && matches!(
                request.provider,
                ProviderKind::Claude
                    | ProviderKind::Copilot
                    | ProviderKind::Cursor
                    | ProviderKind::Grok
            ),
        cleanup_error,
    })
}

type ProviderRewindResult = (
    Option<ProviderResumeCursor>,
    Option<waku_client::provider_session::ProviderSessionFork>,
    Option<PreparedDriver>,
);

fn perform_provider_rewind(
    request: &mut MessageRewindRequest,
) -> anyhow::Result<ProviderRewindResult> {
    let provider = request.provider;
    let reset_native_session = request.rollback_turns > 0
        && request.retained_turn_count == 0
        && matches!(
            provider,
            ProviderKind::Claude
                | ProviderKind::Copilot
                | ProviderKind::Cursor
                | ProviderKind::Grok
        );
    if request.rollback_turns == 0 || reset_native_session {
        return Ok((None, None, None));
    }

    match provider {
        ProviderKind::Claude => {
            let Some(ProviderResumeCursor::Claude {
                session_id: native_session_id,
                ..
            }) = request.provider_cursor.as_ref()
            else {
                anyhow::bail!(tr!(
                    "errors.provider_native_cursor_unavailable",
                    provider = "Claude"
                ));
            };
            let fork = request.workspace_client.fork_provider_session(
                waku_client::provider_session::ProviderSessionForkRequest::Claude {
                    session_id: native_session_id.clone(),
                    resume_at: request.provider_resume_at.clone(),
                    turn_count: request.provider_turn_count,
                    title: tr!(
                        "session.rewind_title",
                        title = request.session_title.as_str()
                    ),
                },
            )?;
            Ok((None, Some(fork), None))
        }
        ProviderKind::OpenCode => {
            let cursor = if let Some(driver) = request.driver.as_ref() {
                driver.rollback(request.rollback_turns)?.ok_or_else(|| {
                    anyhow::anyhow!("OpenCode returned no cursor for the rewound session")
                })?
            } else {
                let Some(ProviderResumeCursor::OpenCode {
                    session_id: native_session_id,
                }) = request.provider_cursor.as_ref()
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_cursor_unavailable",
                        provider = "OpenCode"
                    ));
                };
                let binary = request.binary.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(tr!("errors.provider_not_found", provider = "OpenCode"))
                })?;
                request
                    .workspace_client
                    .fork_provider_session(
                        waku_client::provider_session::ProviderSessionForkRequest::OpenCode {
                            binary: binary.to_owned(),
                            cwd: request.project_path.clone(),
                            session_id: native_session_id.clone(),
                            turn_count: request.provider_turn_count,
                        },
                    )?
                    .cursor
            };
            Ok((Some(cursor), None, None))
        }
        ProviderKind::OpenCode2 => {
            let cursor = if let Some(driver) = request.driver.as_ref() {
                driver.rollback(request.rollback_turns)?.ok_or_else(|| {
                    anyhow::anyhow!("OpenCode 2 returned no cursor for the rewound session")
                })?
            } else {
                let Some(ProviderResumeCursor::OpenCode2 {
                    session_id: native_session_id,
                    ..
                }) = request.provider_cursor.as_ref()
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_cursor_unavailable",
                        provider = "OpenCode 2"
                    ));
                };
                let binary = request.binary.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(tr!("errors.provider_not_found", provider = "OpenCode 2"))
                })?;
                request
                    .workspace_client
                    .fork_provider_session(
                        waku_client::provider_session::ProviderSessionForkRequest::OpenCode2 {
                            binary: binary.to_owned(),
                            session_id: native_session_id.clone(),
                            turn_count: request.provider_turn_count,
                        },
                    )?
                    .cursor
            };
            Ok((Some(cursor), None, None))
        }
        ProviderKind::Amp => {
            let Some(ProviderResumeCursor::Amp {
                thread_id: native_thread_id,
                fork_context,
            }) = request.provider_cursor.as_ref()
            else {
                anyhow::bail!(tr!(
                    "errors.provider_native_thread_cursor_unavailable",
                    provider = "Amp"
                ));
            };
            let binary = request.binary.as_deref().ok_or_else(|| {
                anyhow::anyhow!(tr!("errors.provider_not_found", provider = "Amp"))
            })?;
            let cursor = request
                .workspace_client
                .fork_provider_session(
                    waku_client::provider_session::ProviderSessionForkRequest::Amp {
                        binary: binary.to_owned(),
                        cwd: request.project_path.clone(),
                        thread_id: native_thread_id.clone(),
                        fork_context: fork_context.clone(),
                        turn_count: request.provider_turn_count,
                    },
                )?
                .cursor;
            Ok((Some(cursor), None, None))
        }
        ProviderKind::Cursor => {
            let source = request.cursor_source.as_ref().ok_or_else(|| {
                anyhow::anyhow!(tr!(
                    "errors.provider_waku_task_unavailable",
                    provider = "Cursor"
                ))
            })?;
            Ok((
                Some(
                    request
                        .workspace_client
                        .fork_provider_session(
                            waku_client::provider_session::ProviderSessionForkRequest::Cursor {
                                source: source.clone(),
                                turn_count: request.retained_turn_count,
                            },
                        )?
                        .cursor,
                ),
                None,
                None,
            ))
        }
        ProviderKind::Grok => {
            let Some(ProviderResumeCursor::Grok {
                session_id: native_session_id,
            }) = request.provider_cursor.as_ref()
            else {
                anyhow::bail!(tr!(
                    "errors.provider_native_cursor_unavailable",
                    provider = "Grok"
                ));
            };
            let binary = request.binary.as_deref().ok_or_else(|| {
                anyhow::anyhow!(tr!("errors.provider_not_found", provider = "Grok Build"))
            })?;
            let cursor = request
                .workspace_client
                .fork_provider_session(
                    waku_client::provider_session::ProviderSessionForkRequest::Grok {
                        binary: binary.to_owned(),
                        cwd: request.project_path.clone(),
                        session_id: native_session_id.clone(),
                        turn_count: request.provider_turn_count,
                    },
                )?
                .cursor;
            Ok((Some(cursor), None, None))
        }
        ProviderKind::Codex | ProviderKind::DeepSeek | ProviderKind::OhMyPi | ProviderKind::Pi => {
            let mut prepared_driver = None;
            let driver = if let Some(driver) = request.driver.as_ref() {
                driver.clone()
            } else {
                let start = request.driver_start.take().ok_or_else(|| {
                    anyhow::anyhow!(tr!(
                        "errors.provider_not_found",
                        provider = provider.display_name()
                    ))
                })?;
                let prepared = start_driver(start, request.project_path.clone())?;
                let driver = prepared.handle.clone();
                prepared_driver = Some(prepared);
                driver
            };
            let cursor = driver.rollback(request.rollback_turns)?;
            Ok((cursor, None, prepared_driver))
        }
        ProviderKind::Copilot => {
            let Some(ProviderResumeCursor::Copilot {
                session_id: native_session_id,
            }) = request.provider_cursor.as_ref()
            else {
                anyhow::bail!(tr!(
                    "errors.provider_native_cursor_unavailable",
                    provider = "GitHub Copilot"
                ));
            };
            let binary = request.binary.as_deref().ok_or_else(|| {
                anyhow::anyhow!(tr!("errors.provider_not_found", provider = "GitHub Copilot"))
            })?;
            let cursor = request
                .workspace_client
                .fork_provider_session(
                    waku_client::provider_session::ProviderSessionForkRequest::Copilot {
                        binary: binary.to_owned(),
                        cwd: request.project_path.clone(),
                        session_id: native_session_id.clone(),
                        turn_count: request.provider_turn_count,
                        title: tr!(
                            "session.rewind_title",
                            title = request.session_title.as_str()
                        ),
                    },
                )?
                .cursor;
            Ok((Some(cursor), None, None))
        }
        // Unreachable through the UI, which hides rewinding for providers that
        // answer `supports_conversation_rollback` with false.
        ProviderKind::Devin | ProviderKind::Droid | ProviderKind::Fx | ProviderKind::Kimi => {
            Err(anyhow::anyhow!(tr!(
                "errors.provider_turn_branching_unsupported",
                provider = provider.display_name()
            )))
        }
    }
}

/// Everything a response fork needs after the click has been accepted.
///
/// The session is a point-in-time snapshot: provider branching may take long
/// enough for the user to navigate elsewhere, but the resulting task must
/// still end at the response they chose. Provider RPCs, process startup,
/// native transcript I/O, and Git ref copying are all performed by
/// [`perform_response_fork`] on the background executor.
struct ResponseForkRequest {
    workspace_client: waku_client::WorkspaceClient,
    source: AgentSession,
    source_workspace_path: PathBuf,
    fork_title: String,
    turn_count: usize,
    provider_turn_count: usize,
    turns_to_remove: usize,
    binary: Option<PathBuf>,
    driver: Option<DriverHandle>,
    driver_start: Option<DriverStartRequest>,
}

fn numbered_title_suffix(title: &str) -> Option<(&str, usize)> {
    let (base, suffix) = title.rsplit_once(" (")?;
    let number = suffix.strip_suffix(')')?.parse().ok()?;
    (!base.is_empty() && number >= 2).then_some((base, number))
}

fn next_response_fork_title<'a>(
    source_title: &str,
    existing_titles: impl IntoIterator<Item = &'a str>,
) -> String {
    let existing_titles = existing_titles.into_iter().collect::<Vec<_>>();
    let base = numbered_title_suffix(source_title)
        .filter(|(base, _)| existing_titles.iter().any(|title| title == base))
        .map_or(source_title, |(base, _)| base);
    let highest_number = existing_titles
        .iter()
        .filter_map(|title| {
            if *title == base {
                Some(1)
            } else {
                numbered_title_suffix(title)
                    .filter(|(candidate_base, _)| *candidate_base == base)
                    .map(|(_, number)| number)
            }
        })
        .max()
        .unwrap_or(1);
    format!("{base} ({})", highest_number.saturating_add(1).max(2))
}

struct PreparedResponseFork {
    forked: AgentSession,
    prepared_driver: Option<PreparedDriver>,
    checkpoint_warning: Option<String>,
}

type ProviderForkResult = (
    ProviderResumeCursor,
    Option<HashMap<String, String>>,
    Option<PreparedDriver>,
);

fn fork_response_with_driver(
    request: &mut ResponseForkRequest,
) -> anyhow::Result<(ProviderResumeCursor, Option<PreparedDriver>)> {
    let provider = request.source.provider;
    let mut prepared_driver = None;
    let driver = if let Some(driver) = request.driver.as_ref() {
        driver.clone()
    } else {
        let start = request.driver_start.take().ok_or_else(|| {
            anyhow::anyhow!(tr!(
                "errors.provider_not_found",
                provider = provider.display_name()
            ))
        })?;
        let prepared = start_driver(start, request.source_workspace_path.clone())?;
        let driver = prepared.handle.clone();
        prepared_driver = Some(prepared);
        driver
    };
    Ok((driver.fork(request.turns_to_remove)?, prepared_driver))
}

fn perform_response_fork(mut request: ResponseForkRequest) -> Result<PreparedResponseFork, String> {
    let provider = request.source.provider;
    let native_fork = (|| -> anyhow::Result<ProviderForkResult> {
        match provider {
            ProviderKind::Claude => {
                let ProviderResumeCursor::Claude {
                    session_id: native_session_id,
                    ..
                } = request.source.provider_cursor.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(tr!(
                        "errors.provider_native_session_unavailable",
                        provider = "Claude"
                    ))
                })?
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_session_unavailable",
                        provider = "Claude"
                    ));
                };
                let resume_at = request
                    .source
                    .turns
                    .get(request.turn_count.saturating_sub(1))
                    .and_then(|turn| turn.provider_resume_at.clone());
                let fork = request.workspace_client.fork_provider_session(
                    waku_client::provider_session::ProviderSessionForkRequest::Claude {
                        session_id: native_session_id.clone(),
                        resume_at,
                        turn_count: request.provider_turn_count,
                        title: request.fork_title.clone(),
                    },
                )?;
                Ok((fork.cursor, Some(fork.message_ids), None))
            }
            ProviderKind::Codex => {
                if !matches!(
                    request.source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Codex { .. })
                ) {
                    anyhow::bail!(tr!(
                        "errors.provider_native_thread_unavailable",
                        provider = "Codex"
                    ));
                }
                let (cursor, prepared_driver) = fork_response_with_driver(&mut request)?;
                Ok((cursor, None, prepared_driver))
            }
            ProviderKind::DeepSeek => {
                if !matches!(
                    request.source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::DeepSeek { .. })
                ) {
                    anyhow::bail!(tr!(
                        "errors.provider_native_session_unavailable",
                        provider = "DeepSeek Harness"
                    ));
                }
                let (cursor, prepared_driver) = fork_response_with_driver(&mut request)?;
                Ok((cursor, None, prepared_driver))
            }
            ProviderKind::Cursor => Ok((
                request
                    .workspace_client
                    .fork_provider_session(
                        waku_client::provider_session::ProviderSessionForkRequest::Cursor {
                            source: request.source.clone(),
                            turn_count: request.turn_count,
                        },
                    )?
                    .cursor,
                None,
                None,
            )),
            ProviderKind::Amp => {
                let Some(ProviderResumeCursor::Amp {
                    thread_id: native_thread_id,
                    fork_context,
                }) = request.source.provider_cursor.as_ref()
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_thread_unavailable",
                        provider = "Amp"
                    ));
                };
                let binary = request.binary.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(tr!("errors.provider_not_installed", provider = "Amp"))
                })?;
                Ok((
                    request
                        .workspace_client
                        .fork_provider_session(
                            waku_client::provider_session::ProviderSessionForkRequest::Amp {
                                binary: binary.to_owned(),
                                cwd: request.source_workspace_path.clone(),
                                thread_id: native_thread_id.clone(),
                                fork_context: fork_context.clone(),
                                turn_count: request.provider_turn_count,
                            },
                        )?
                        .cursor,
                    None,
                    None,
                ))
            }
            ProviderKind::OpenCode => {
                let Some(ProviderResumeCursor::OpenCode {
                    session_id: native_session_id,
                }) = request.source.provider_cursor.as_ref()
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_session_unavailable",
                        provider = "OpenCode"
                    ));
                };
                let binary = request.binary.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(tr!("errors.provider_not_installed", provider = "OpenCode"))
                })?;
                Ok((
                    request
                        .workspace_client
                        .fork_provider_session(
                            waku_client::provider_session::ProviderSessionForkRequest::OpenCode {
                                binary: binary.to_owned(),
                                cwd: request.source_workspace_path.clone(),
                                session_id: native_session_id.clone(),
                                turn_count: request.provider_turn_count,
                            },
                        )?
                        .cursor,
                    None,
                    None,
                ))
            }
            ProviderKind::OpenCode2 => {
                let Some(ProviderResumeCursor::OpenCode2 {
                    session_id: native_session_id,
                    ..
                }) = request.source.provider_cursor.as_ref()
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_session_unavailable",
                        provider = "OpenCode 2"
                    ));
                };
                let binary = request.binary.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(tr!(
                        "errors.provider_not_installed",
                        provider = "OpenCode 2"
                    ))
                })?;
                Ok((
                    request
                        .workspace_client
                        .fork_provider_session(
                            waku_client::provider_session::ProviderSessionForkRequest::OpenCode2 {
                                binary: binary.to_owned(),
                                session_id: native_session_id.clone(),
                                turn_count: request.provider_turn_count,
                            },
                        )?
                        .cursor,
                    None,
                    None,
                ))
            }
            ProviderKind::Grok => {
                let Some(ProviderResumeCursor::Grok {
                    session_id: native_session_id,
                }) = request.source.provider_cursor.as_ref()
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_session_unavailable",
                        provider = "Grok"
                    ));
                };
                let binary = request.binary.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(tr!(
                        "errors.provider_not_installed",
                        provider = "Grok Build"
                    ))
                })?;
                Ok((
                    request
                        .workspace_client
                        .fork_provider_session(
                            waku_client::provider_session::ProviderSessionForkRequest::Grok {
                                binary: binary.to_owned(),
                                cwd: request.source_workspace_path.clone(),
                                session_id: native_session_id.clone(),
                                turn_count: request.provider_turn_count,
                            },
                        )?
                        .cursor,
                    None,
                    None,
                ))
            }
            ProviderKind::Pi => {
                if !matches!(
                    request.source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Pi {
                        session_file: Some(_),
                        ..
                    })
                ) {
                    anyhow::bail!(tr!(
                        "errors.provider_session_file_unavailable",
                        provider = "Pi"
                    ));
                }
                let (cursor, prepared_driver) = fork_response_with_driver(&mut request)?;
                Ok((cursor, None, prepared_driver))
            }
            ProviderKind::OhMyPi => {
                if !matches!(
                    request.source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::OhMyPi {
                        session_file: Some(_),
                        ..
                    })
                ) {
                    anyhow::bail!(tr!(
                        "errors.provider_session_file_unavailable",
                        provider = "Oh My Pi"
                    ));
                }
                let (cursor, prepared_driver) = fork_response_with_driver(&mut request)?;
                Ok((cursor, None, prepared_driver))
            }
            ProviderKind::Copilot => {
                let Some(ProviderResumeCursor::Copilot {
                    session_id: native_session_id,
                }) = request.source.provider_cursor.as_ref()
                else {
                    anyhow::bail!(tr!(
                        "errors.provider_native_session_unavailable",
                        provider = "GitHub Copilot"
                    ));
                };
                let binary = request.binary.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(tr!("errors.provider_not_installed", provider = "GitHub Copilot"))
                })?;
                Ok((
                    request
                        .workspace_client
                        .fork_provider_session(
                            waku_client::provider_session::ProviderSessionForkRequest::Copilot {
                                binary: binary.to_owned(),
                                cwd: request.source_workspace_path.clone(),
                                session_id: native_session_id.clone(),
                                turn_count: request.provider_turn_count,
                                title: request.fork_title.clone(),
                            },
                        )?
                        .cursor,
                    None,
                    None,
                ))
            }
            // Unreachable through the UI, which hides branching for providers
            // that answer `supports_conversation_fork` with false.
            ProviderKind::Devin | ProviderKind::Droid | ProviderKind::Fx | ProviderKind::Kimi => {
                anyhow::bail!(tr!(
                    "errors.provider_turn_branching_unsupported",
                    provider = provider.display_name()
                ))
            }
        }
    })();
    let (provider_cursor, claude_message_ids, prepared_driver) =
        native_fork.map_err(|error| tr!("errors.fork_task", error = error))?;
    let Some(mut forked) =
        request
            .source
            .fork_through_turn(request.turn_count, provider_cursor, &request.fork_title)
    else {
        return Err(tr!("session.response_cannot_copy"));
    };
    if let Some(message_ids) = claude_message_ids {
        for turn in &mut forked.turns {
            if let Some(message_id) = turn.provider_resume_at.as_mut()
                && let Some(remapped) = message_ids.get(message_id)
            {
                *message_id = remapped.clone();
            }
        }
    }

    let fork_id = forked.id;
    for turn in &mut forked.turns {
        if let Some(checkpoint) = turn.checkpoint.as_mut() {
            checkpoint.git_ref = checkpoint::checkpoint_ref(fork_id, checkpoint.turn_count);
        }
    }
    let checkpoint_warning = workspace_ack(
        &request.workspace_client,
        waku_client::WorkspaceOperation::CopySessionRefs {
            cwd: request.source_workspace_path.clone(),
            source_session_id: request.source.id,
            target_session_id: fork_id,
            through_turn_count: request.turn_count,
        },
    )
    .err()
    .map(|error| error.to_string());

    Ok(PreparedResponseFork {
        forked,
        prepared_driver,
        checkpoint_warning,
    })
}

impl Waku {
    /// Follow one daemon's client stream and mirror its task state and
    /// settings into the merged catalog. Runs once per connected daemon —
    /// the local supervisor at launch plus one per remote host — and ends
    /// when the supervisor's client channel closes, which is how a removed
    /// host's worker stops.
    pub(super) fn start_task_state_sync(
        &self,
        key: waku_client::DaemonKey,
        supervisor: waku_client::DaemonSupervisor,
    ) {
        let clients = supervisor.subscribe_clients();
        let results = self.task_state_sync_tx.clone();
        let settings_updates = self.daemon_settings_tx.clone();
        let friends_updates = self.friends_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        std::thread::Builder::new()
            .name(format!("waku-task-state-sync-{key:?}"))
            .spawn(move || {
                let Ok(mut client) = clients.recv() else {
                    return;
                };
                loop {
                    while let Ok(newer) = clients.try_recv() {
                        client = newer;
                    }
                    let revisions = client.subscribe_task_state();
                    let settings = client.subscribe_settings();
                    let friends = client.subscribe_friends();
                    // Seed the document before broadcasts arrive — a client
                    // connecting after the last change sees no event until
                    // something mutates friends state again.
                    if let Ok(waku_client::ResponsePayload::Friends { state }) = client.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::GetFriends,
                    ) {
                        if friends_updates.send(state).is_err() {
                            return;
                        }
                    }
                    let result = load_remote_task_state(&client).map_err(|error| error.to_string());
                    if results.send((key, result)).is_err() {
                        return;
                    }
                    signal_event_pump(&event_wake);
                    client = loop {
                        crossbeam_channel::select! {
                            recv(clients) -> replacement => {
                                let Ok(mut replacement) = replacement else {
                                    return;
                                };
                                while let Ok(newer) = clients.try_recv() {
                                    replacement = newer;
                                }
                                break replacement;
                            }
                            recv(revisions) -> revision => {
                                if revision.is_err() {
                                    // Managed replacement publishes the new
                                    // client after the old socket closes. Wait
                                    // for that publication instead of exiting
                                    // the task-state sync worker permanently.
                                    let Ok(replacement) = clients.recv() else {
                                        return;
                                    };
                                    break replacement;
                                }
                                while revisions.try_recv().is_ok() {}
                                let result = load_remote_task_state(&client)
                                    .map_err(|error| error.to_string());
                                if results.send((key, result)).is_err() {
                                    return;
                                }
                                signal_event_pump(&event_wake);
                            }
                            recv(settings) -> settings => {
                                // A closed settings channel means the socket
                                // dropped; the client replacement above
                                // resubscribes on the next connection.
                                let Ok(settings) = settings else {
                                    let Ok(replacement) = clients.recv() else {
                                        return;
                                    };
                                    break replacement;
                                };
                                if settings_updates.send((key, settings)).is_err() {
                                    return;
                                }
                                signal_event_pump(&event_wake);
                            }
                            recv(friends) -> friends => {
                                let Ok(state) = friends else {
                                    let Ok(replacement) = clients.recv() else {
                                        return;
                                    };
                                    break replacement;
                                };
                                if friends_updates.send(state).is_err() {
                                    return;
                                }
                                signal_event_pump(&event_wake);
                            }
                        }
                    };
                }
            })
            .ok();
    }

    fn drain_task_state_sync_events(&mut self, cx: &mut Context<Self>) -> bool {
        // Snapshots from several daemons interleave on one channel; keep the
        // newest per daemon rather than only the newest overall.
        let mut latest: HashMap<waku_client::DaemonKey, Result<RemoteTaskStateSnapshot, String>> =
            HashMap::new();
        while let Ok((key, result)) = self.task_state_sync_events.try_recv() {
            latest.insert(key, result);
        }
        if latest.is_empty() {
            return false;
        }
        for (key, result) in latest {
            match result {
                Ok(snapshot) => self.apply_remote_task_state(key, snapshot, cx),
                Err(error) => {
                    eprintln!("could not refresh daemon task state: {error}");
                }
            }
        }
        true
    }

    fn drain_daemon_settings_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut latest: HashMap<waku_client::DaemonKey, waku_client::DaemonSettings> =
            HashMap::new();
        while let Ok((key, settings)) = self.daemon_settings_events.try_recv() {
            latest.insert(key, settings);
        }
        if latest.is_empty() {
            return false;
        }
        for (key, settings) in latest {
            self.apply_remote_daemon_settings(key, settings, cx);
        }
        true
    }

    fn drain_friends_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut latest = None;
        while let Ok(state) = self.friends_events.try_recv() {
            latest = Some(state);
        }
        let Some(state) = latest else {
            return false;
        };
        // Transfers that gained a session since the last document are fresh
        // receipts — badge them so the bell offers "go to latest".
        for transfer in &state.transfers {
            let had_session = self
                .friends_state
                .transfers
                .iter()
                .any(|old| old.id == transfer.id && old.session_id.is_some());
            if !had_session
                && let Some(session_id) = transfer.session_id
                && !sidebar::sidebar_session_selected(
                    self.state.selected_session,
                    self.pending_session_activation.map(|p| p.session_id),
                    session_id,
                )
            {
                self.state.unseen_completions.insert(session_id, unix_time());
            }
        }
        self.friends_state = state;
        // Presence is lazy — a fresh document is the cheapest place to
        // refresh probe verdicts for the open page.
        if self.settings_page == Some(SettingsPage::Friends) {
            self.probe_friends(cx);
        }
        cx.notify();
        true
    }

    /// Fire a lazy presence probe at every known friend. The daemon no-ops
    /// probes that are still fresh, so this is safe to call on every
    /// `friendsChanged` delivery while the page is open.
    pub(super) fn probe_friends(&self, cx: &mut Context<Self>) {
        let client = self.daemon.client();
        let node_ids: Vec<String> = self
            .friends_state
            .friends
            .iter()
            .map(|friend| friend.node_id.clone())
            .collect();
        cx.background_executor()
            .spawn(async move {
                for node_id in node_ids {
                    let _ = client.notify(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::ProbeFriend { node_id },
                    );
                }
            })
            .detach();
    }

    /// Send one friends command off the UI thread; results arrive through
    /// the `friendsChanged` broadcast.
    pub(super) fn friends_command(
        &self,
        command: waku_client::Command,
        cx: &mut Context<Self>,
    ) {
        let client = self.daemon.client();
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = client.request(Uuid::nil(), Uuid::nil(), command) {
                    eprintln!("friends command failed: {error}");
                }
            })
            .detach();
    }

    /// Fold a `settingsChanged` broadcast into the local mirrors. The
    /// supervisor's cache is marked persisted so the writer thread does not
    /// echo the document back; the palette and settings pages read the new
    /// command list on the next frame. A command an agent added is the one
    /// settings write the user did not see happen, so it gets a toast.
    fn apply_remote_daemon_settings(
        &mut self,
        key: waku_client::DaemonKey,
        settings: waku_client::DaemonSettings,
        cx: &mut Context<Self>,
    ) {
        // Remote hosts keep their own mirror — provider binary overrides and
        // custom commands are host-local — while `state` stays the merged
        // copy of the local daemon's document.
        if let waku_client::DaemonKey::Remote(host) = key {
            if let Some(supervisor) = self.daemons.supervisor(key) {
                supervisor.note_remote_settings(settings.clone());
            }
            self.remote_daemon_settings.insert(host, settings);
            cx.notify();
            return;
        }
        let known: HashSet<Uuid> = self
            .state
            .custom_commands
            .iter()
            .map(|command| command.id)
            .collect();
        let agent_added: Vec<String> = settings
            .custom_commands
            .iter()
            .filter(|command| command.created_by_task.is_some() && !known.contains(&command.id))
            .map(|command| command.display_name().to_owned())
            .collect();
        self.daemon.note_remote_settings(settings.clone());
        self.state.apply_daemon_settings(settings);
        if let Some(name) = agent_added.first() {
            self.show_toast(tr!("commands.agent_added", name = name));
        }
        cx.notify();
    }

    fn apply_remote_task_state(
        &mut self,
        key: waku_client::DaemonKey,
        snapshot: RemoteTaskStateSnapshot,
        cx: &mut Context<Self>,
    ) {
        let runtime_ids = self.runtimes.keys().copied().collect::<HashSet<_>>();
        // Only this daemon's rows may be removed or replaced — the merged
        // list also carries every other host's sessions and projects.
        let removed = merge_remote_session_catalog(
            &mut self.state.sessions,
            snapshot.sessions.clone(),
            |session_id| self.daemons.session_owner(session_id) == key,
            |session_id| runtime_ids.contains(&session_id),
        );
        for session_id in &removed {
            self.runtime_attach_pending.remove(session_id);
            self.runtime_attach_misses.remove(session_id);
            self.runtimes.remove(session_id);
            self.background_work.remove(session_id);
            self.remove_right_panel_session_state(*session_id, cx);
            self.task_switcher.remove(*session_id);
            self.project_switcher.session_removed(*session_id);
            self.transcript_scroll_positions.remove(session_id);
        }
        self.state
            .projects
            .retain(|project| self.daemons.project_owner(project.id) != key);
        self.state
            .projects
            .extend(snapshot.projects.iter().cloned());
        if let waku_client::DaemonKey::Remote(host) = key {
            self.daemons.replace_remote_catalog(
                host,
                &snapshot
                    .projects
                    .iter()
                    .map(|project| project.id)
                    .collect::<Vec<_>>(),
                &snapshot
                    .sessions
                    .iter()
                    .map(|session| session.id)
                    .collect::<Vec<_>>(),
            );
            self.remote_catalogs.insert(host, snapshot);
            self.save_remote_catalogs();
        }

        let attach = self
            .state
            .sessions
            .iter()
            .filter(|session| {
                session.status.is_busy()
                    || (self.state.selected_session == Some(session.id) && session.has_started())
            })
            .map(|session| session.id)
            .collect::<Vec<_>>();
        for session_id in attach {
            self.start_runtime_attachment(session_id, cx);
        }

        if self.state.selected_session.is_some_and(|selected| {
            !self
                .state
                .sessions
                .iter()
                .any(|session| session.id == selected)
        }) {
            let previous_project = self.state.selected_project;
            self.state.selected_session = None;
            let next = self
                .state
                .sessions
                .iter()
                .filter(|session| {
                    previous_project.is_none_or(|project| session.project_id == project)
                })
                .max_by_key(|session| session.updated_at)
                .map(|session| session.id)
                .or_else(|| {
                    self.state
                        .sessions
                        .iter()
                        .max_by_key(|session| session.updated_at)
                        .map(|session| session.id)
                });
            if let Some(next) = next {
                self.select_session(next, cx);
            } else if let Some(project_id) = self
                .state
                .selected_project
                .filter(|project_id| {
                    self.state
                        .projects
                        .iter()
                        .any(|project| project.id == *project_id)
                })
                .or_else(|| self.state.projects.first().map(|project| project.id))
            {
                self.state.selected_project = Some(project_id);
                self.create_session_for(project_id, self.state.last_provider, cx);
            }
        }
    }

    pub(super) fn start_runtime_attachment(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.runtimes.contains_key(&session_id)
            || !self.runtime_attach_pending.insert(session_id)
        {
            return;
        }
        let Some(daemon) = self.daemons.daemon_for_session(session_id) else {
            // The owning remote host has not connected yet. Leave the pending
            // mark: its first catalog snapshot re-enters here for every busy
            // session it reports.
            return;
        };
        let event_wake = self.event_wake_tx.clone();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { attach_driver(daemon, session_id, event_wake) })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_runtime_attachment(session_id, result, cx);
            });
        })
        .detach();
    }

    /// The supervisor that owns a session — `None` when its remote host is
    /// configured but not connected, or the record was removed.
    pub(super) fn daemon_for_session(
        &self,
        session_id: Uuid,
    ) -> Option<waku_client::DaemonSupervisor> {
        self.daemons.daemon_for_session(session_id)
    }

    pub(super) fn daemon_for_project(
        &self,
        project_id: Uuid,
    ) -> Option<waku_client::DaemonSupervisor> {
        self.daemons.daemon_for_project(project_id)
    }

    /// Resolve a filesystem path to the daemon that owns it: the project — or
    /// session worktree — containing it. Local projects win over remote ones
    /// on an identical path string; a path no catalog row owns is local. The
    /// key is returned even when its remote supervisor is not connected, so
    /// callers can distinguish "offline remote" from "local".
    pub(super) fn daemon_key_for_path(&self, path: &std::path::Path) -> waku_client::DaemonKey {
        let mut remote_match = None;
        let mut remote_len = 0;
        for project in &self.state.projects {
            if path == project.path || path.starts_with(&project.path) {
                match self.daemons.project_owner(project.id) {
                    waku_client::DaemonKey::Local => return waku_client::DaemonKey::Local,
                    waku_client::DaemonKey::Remote(host) => {
                        if project.path.as_os_str().len() > remote_len {
                            remote_len = project.path.as_os_str().len();
                            remote_match = Some(host);
                        }
                    }
                }
            }
        }
        for session in &self.state.sessions {
            let Some(workspace) = session.workspace.path() else {
                continue;
            };
            if path == workspace || path.starts_with(workspace) {
                match self.daemons.session_owner(session.id) {
                    waku_client::DaemonKey::Local => return waku_client::DaemonKey::Local,
                    waku_client::DaemonKey::Remote(host) => {
                        if workspace.as_os_str().len() > remote_len {
                            remote_len = workspace.as_os_str().len();
                            remote_match = Some(host);
                        }
                    }
                }
            }
        }
        remote_match
            .map(waku_client::DaemonKey::Remote)
            .unwrap_or(waku_client::DaemonKey::Local)
    }

    /// The supervisor for `path`'s owner, falling back to the local daemon
    /// only when the owner is local. A disconnected remote resolves to its
    /// missing supervisor, not to the wrong machine.
    pub(super) fn daemon_for_path(
        &self,
        path: &std::path::Path,
    ) -> Option<waku_client::DaemonSupervisor> {
        self.daemons.supervisor(self.daemon_key_for_path(path))
    }

    /// Whether `path` lives on a remote host's filesystem — gates every
    /// local-OS affordance (reveal, open-in-app, desktop terminal).
    pub(super) fn is_remote_path(&self, path: &std::path::Path) -> bool {
        self.daemon.is_remote()
            || matches!(
                self.daemon_key_for_path(path),
                waku_client::DaemonKey::Remote(_)
            )
    }

    pub(super) fn workspace_client_for_project(
        &self,
        project_id: Uuid,
    ) -> Option<waku_client::WorkspaceClient> {
        self.daemon_for_project(project_id)
            .map(|daemon| waku_client::WorkspaceClient::new(daemon.client()))
    }

    pub(super) fn workspace_client_for_session(
        &self,
        session_id: Uuid,
    ) -> Option<waku_client::WorkspaceClient> {
        self.daemon_for_session(session_id)
            .map(|daemon| waku_client::WorkspaceClient::new(daemon.client()))
    }

    /// `None` when the path's remote owner is offline — never the local
    /// daemon, which would run the request against a different filesystem.
    pub(super) fn workspace_client_for_path(
        &self,
        path: &std::path::Path,
    ) -> Option<waku_client::WorkspaceClient> {
        self.daemon_for_path(path)
            .map(|daemon| waku_client::WorkspaceClient::new(daemon.client()))
    }

    /// The daemon that should store and serve a composer draft target — the
    /// session's owner for `Session`, the prospective project's owner for
    /// `NewSession`.
    pub(super) fn daemon_for_draft_key(
        &self,
        key: crate::persistence::ComposerDraftKey,
    ) -> Option<waku_client::DaemonSupervisor> {
        match key {
            crate::persistence::ComposerDraftKey::Session(session_id) => {
                self.daemon_for_session(session_id)
            }
            crate::persistence::ComposerDraftKey::NewSession(project_id) => {
                self.daemon_for_project(project_id)
            }
        }
    }

    /// Whether a catalog row lives on another host — true when the row's
    /// owner is a remote host, or when the app itself is bound to a remote
    /// primary daemon (the env-var connection mode, where every row is).
    pub(super) fn is_remote_session(&self, session_id: Uuid) -> bool {
        self.daemon.is_remote() || self.daemons.is_remote_session(session_id)
    }

    pub(super) fn is_remote_project(&self, project_id: Uuid) -> bool {
        self.daemon.is_remote() || self.daemons.is_remote_project(project_id)
    }

    /// Daemon settings effective for a session's host: the remote mirror when
    /// the session belongs to a remote daemon, the merged local document
    /// otherwise.
    pub(super) fn daemon_settings_for_session(
        &self,
        session_id: Uuid,
    ) -> waku_client::DaemonSettings {
        match self.daemons.session_owner(session_id) {
            waku_client::DaemonKey::Remote(host) => self
                .remote_daemon_settings
                .get(&host)
                .cloned()
                .unwrap_or_else(|| self.daemon.settings()),
            waku_client::DaemonKey::Local => self.state.daemon_settings(),
        }
    }

    /// Persist the per-host catalog cache off the UI thread.
    fn save_remote_catalogs(&self) {
        let catalogs = self.remote_catalogs.clone();
        let path = self.remote_catalogs_path.clone();
        std::thread::Builder::new()
            .name("waku-remote-catalogs".into())
            .spawn(move || {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let Ok(bytes) = serde_json::to_vec_pretty(&catalogs) else {
                    return;
                };
                let _ = std::fs::write(path, bytes);
            })
            .ok();
    }

    /// Connect every saved remote host. Each gets its own retry loop so one
    /// dead host never delays the others; a host whose record was removed
    /// stops being retried.
    pub(super) fn connect_remote_hosts(&mut self, cx: &mut Context<Self>) {
        for host in self.state.remote_hosts.clone() {
            self.connect_remote_host(host, cx);
        }
    }

    /// The saved record still points at what this loop tried — an edited or
    /// removed record makes an in-flight loop stale, and its attempts must
    /// not install a supervisor for coordinates the user replaced.
    fn remote_host_record_matches(&self, host: &waku_client::persistence::RemoteHost) -> bool {
        self.state.remote_hosts.iter().any(|record| {
            record.id == host.id
                && record.address == host.address
                && record.token == host.token
                && record.ssh_destination == host.ssh_destination
        })
    }

    fn connect_remote_host(
        &mut self,
        host: waku_client::persistence::RemoteHost,
        cx: &mut Context<Self>,
    ) {
        let host_id = host.id;
        #[cfg(unix)]
        if host.ssh_destination.is_some() {
            self.ensure_askpass_responder(cx);
        }
        cx.spawn(async move |waku, cx| {
            loop {
                let attempt = cx
                    .background_executor()
                    .spawn({
                        let host = host.clone();
                        async move { connect_remote_supervisor(&host) }
                    })
                    .await;
                match attempt {
                    Ok(outcome) => {
                        let _ = waku.update(cx, |waku, cx| {
                            if waku.remote_host_record_matches(&host) {
                                waku.install_remote_daemon(host_id, outcome, cx);
                            }
                        });
                        return;
                    }
                    Err(error) => {
                        let keep_trying = waku
                            .update(cx, |waku, cx| {
                                let current = waku.remote_host_record_matches(&host);
                                if current {
                                    waku.remote_errors.insert(host_id, error.to_string());
                                    cx.notify();
                                }
                                current
                            })
                            .unwrap_or(false);
                        if !keep_trying {
                            return;
                        }
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(10))
                            .await;
                    }
                }
            }
        })
        .detach();
    }

    /// A remote supervisor answered: register it so routing, task-state sync,
    /// and draft storage see it, and pull that host's composer drafts into the
    /// merged map without touching keys another daemon owns.
    fn install_remote_daemon(
        &mut self,
        host: Uuid,
        link: RemoteDaemonLink,
        cx: &mut Context<Self>,
    ) {
        #[cfg(unix)]
        if let Some(ssh) = link.ssh {
            self.ssh_transports.insert(host, ssh);
            self.watch_ssh_transport(host, cx);
        }
        let supervisor = link.supervisor;
        self.daemons.add_remote(host, supervisor.clone());
        self.remote_errors.remove(&host);
        self.start_task_state_sync(waku_client::DaemonKey::Remote(host), supervisor);
        let drafts = self.composer_draft_store.clone();
        let daemons = self.daemons.clone();
        cx.spawn(async move |waku, cx| {
            let merged = cx
                .background_executor()
                .spawn(async move { drafts.load() })
                .await;
            let _ = waku.update(cx, |waku, _| {
                if let Ok(loaded) = merged {
                    let owns = |id: &Uuid, session: bool| {
                        let owner = if session {
                            daemons.session_owner(*id)
                        } else {
                            daemons.project_owner(*id)
                        };
                        owner == waku_client::DaemonKey::Remote(host)
                    };
                    waku.composer_drafts
                        .sessions
                        .extend(loaded.sessions.into_iter().filter(|(id, _)| owns(id, true)));
                    waku.composer_drafts.new_sessions.extend(
                        loaded
                            .new_sessions
                            .into_iter()
                            .filter(|(id, _)| owns(id, false)),
                    );
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Save a remote host record and start its connect loop.
    pub(super) fn add_remote_host(
        &mut self,
        name: String,
        address: String,
        token: String,
        ssh_destination: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let host = waku_client::persistence::RemoteHost {
            id: Uuid::new_v4(),
            name,
            address,
            token,
            ssh_destination,
        };
        self.state.remote_hosts.push(host.clone());
        self.save();
        self.connect_remote_host(host, cx);
        cx.notify();
    }

    /// Re-point a host record. The id is the catalog's ownership anchor and
    /// never changes, so renames and re-addressed records keep their rows;
    /// the connect loop restarts against the new coordinates and the
    /// matched-record check retires the old loop's in-flight attempts.
    pub(super) fn update_remote_host(
        &mut self,
        host_id: Uuid,
        name: String,
        address: String,
        token: String,
        ssh_destination: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self
            .state
            .remote_hosts
            .iter_mut()
            .find(|record| record.id == host_id)
        else {
            return;
        };
        record.name = name;
        record.address = address;
        record.token = token;
        record.ssh_destination = ssh_destination;
        let record = record.clone();
        self.save();
        #[cfg(unix)]
        if let Some(link) = self.ssh_transports.remove(&host_id) {
            link.transport.shutdown();
        }
        self.daemons.remove_remote(host_id);
        self.remote_errors.remove(&host_id);
        self.connect_remote_host(record, cx);
        cx.notify();
    }

    /// Drop a host record: its supervisor and sync worker stop, its catalog
    /// leaves the merged lists, and its cache entry is forgotten. The remote
    /// daemon's own persisted state is untouched — re-adding the host brings
    /// the rows back.
    pub(super) fn remove_remote_host(&mut self, host: Uuid, cx: &mut Context<Self>) {
        if !self
            .state
            .remote_hosts
            .iter()
            .any(|record| record.id == host)
        {
            return;
        }
        self.state.remote_hosts.retain(|record| record.id != host);
        if self
            .remote_host_editor
            .as_ref()
            .is_some_and(|editor| editor.id == Some(host))
        {
            self.remote_host_editor = None;
        }
        self.save();

        let remote = waku_client::DaemonKey::Remote(host);
        let removed_sessions = self
            .state
            .sessions
            .iter()
            .filter(|session| self.daemons.session_owner(session.id) == remote)
            .map(|session| session.id)
            .collect::<Vec<_>>();
        self.state
            .sessions
            .retain(|session| self.daemons.session_owner(session.id) != remote);
        self.state
            .projects
            .retain(|project| self.daemons.project_owner(project.id) != remote);
        self.daemons.remove_remote(host);
        self.remote_daemon_settings.remove(&host);
        self.remote_errors.remove(&host);
        self.remote_catalogs.remove(&host);
        self.save_remote_catalogs();
        if self.skills_catalogs.remove(&remote).is_some() {
            self.rebuild_skills_catalog();
        }
        if self.usage_history_parts.remove(&remote).is_some() {
            self.rebuild_usage_history();
        }

        for session_id in &removed_sessions {
            self.runtime_attach_pending.remove(session_id);
            self.runtime_attach_misses.remove(session_id);
            self.runtimes.remove(session_id);
            self.background_work.remove(session_id);
            self.remove_right_panel_session_state(*session_id, cx);
            self.task_switcher.remove(*session_id);
            self.project_switcher.session_removed(*session_id);
            self.transcript_scroll_positions.remove(session_id);
        }
        if self
            .state
            .selected_session
            .is_some_and(|selected| removed_sessions.contains(&selected))
        {
            self.state.selected_session = None;
        }
        if self.state.selected_project.is_some_and(|selected| {
            !self
                .state
                .projects
                .iter()
                .any(|project| project.id == selected)
        }) {
            self.state.selected_project = self.state.projects.first().map(|project| project.id);
        }
        cx.notify();
    }

    /// Display label for a host id — the saved name, for sidebar grouping and
    /// settings rows.
    pub(super) fn remote_host_name(&self, host: Uuid) -> Option<String> {
        self.state
            .remote_hosts
            .iter()
            .find(|record| record.id == host)
            .map(|record| record.name.clone())
    }

    /// Whether the host's supervisor is registered and answering — its
    /// catalog may still be showing cached rows when this is false.
    pub(super) fn remote_host_connected(&self, host: Uuid) -> bool {
        self.daemons
            .supervisor(waku_client::DaemonKey::Remote(host))
            .is_some_and(|supervisor| supervisor.status() == waku_client::DaemonStatus::Connected)
    }

    /// Keep the ssh channel under a connected host alive: while the record
    /// and transport exist, verify the ControlMaster and re-run provisioning
    /// plus the forward when it died. The supervisor reconnects its client
    /// on its own once the forward is back.
    #[cfg(unix)]
    fn watch_ssh_transport(&mut self, host_id: Uuid, cx: &mut Context<Self>) {
        cx.spawn(async move |waku, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                let link = waku
                    .update(cx, |waku, _| waku.ssh_transports.get(&host_id).cloned())
                    .ok()
                    .flatten();
                let Some(link) = link else { return };
                let repaired = cx
                    .background_executor()
                    .spawn(async move {
                        if link.transport.master_alive() {
                            return Ok(());
                        }
                        link.transport.restore_forward(link.local_port)
                    })
                    .await;
                if let Err(error) = repaired {
                    let keep = waku
                        .update(cx, |waku, cx| {
                            let current = waku.ssh_transports.contains_key(&host_id);
                            if current {
                                waku.remote_errors.insert(host_id, format!("{error:#}"));
                                cx.notify();
                            }
                            current
                        })
                        .unwrap_or(false);
                    if !keep {
                        return;
                    }
                } else {
                    let _ = waku.update(cx, |waku, cx| {
                        if waku.remote_errors.remove(&host_id).is_some() {
                            cx.notify();
                        }
                    });
                }
            }
        })
        .detach();
    }

    /// Start the askpass responder thread and its UI pump once. ssh invokes
    /// the helper script for passwords and key passphrases; each request
    /// becomes a modal prompt on the entity.
    #[cfg(unix)]
    fn ensure_askpass_responder(&mut self, cx: &mut Context<Self>) {
        if self.ssh_askpass_started {
            return;
        }
        self.ssh_askpass_started = true;
        if let Err(error) = crate::ssh::prepare_askpass() {
            eprintln!("could not prepare ssh askpass helper: {error:#}");
            return;
        }
        let (tx, rx) = smol::channel::unbounded::<crate::ssh::SshAskpassRequest>();
        std::thread::Builder::new()
            .name("waku-ssh-askpass".into())
            .spawn(move || crate::ssh::askpass_responder_loop(tx))
            .ok();
        cx.spawn(async move |waku, cx| {
            while let Ok(request) = rx.recv().await {
                let _ = waku.update(cx, |waku, cx| waku.present_ssh_prompt(request, cx));
            }
        })
        .detach();
    }

    /// Show ssh's prompt text. The input entity is created lazily by the
    /// dialog's render path, which owns a `Window`.
    #[cfg(unix)]
    fn present_ssh_prompt(
        &mut self,
        request: crate::ssh::SshAskpassRequest,
        cx: &mut Context<Self>,
    ) {
        self.pending_ssh_prompt = Some(SshPrompt {
            prompt: request.prompt.clone(),
            request,
            input: None,
        });
        cx.notify();
    }

    /// Deliver the answer — or an empty line when cancelled — to the waiting
    /// askpass helper and close the prompt.
    #[cfg(unix)]
    pub(super) fn answer_ssh_prompt(&mut self, cancel: bool, cx: &mut Context<Self>) {
        let Some(prompt) = self.pending_ssh_prompt.take() else {
            return;
        };
        let answer = if cancel {
            String::new()
        } else {
            prompt
                .input
                .as_ref()
                .map(|input| input.read(cx).content().to_owned())
                .unwrap_or_default()
        };
        cx.background_executor()
            .spawn(async move {
                let _ = prompt.request.answer(&answer);
            })
            .detach();
        cx.notify();
    }

    /// Which remote host owns a session, for badge/label lookups.
    pub(super) fn session_host(&self, session_id: Uuid) -> waku_client::DaemonKey {
        self.daemons.session_owner(session_id)
    }

    /// Which remote host owns a project, for badge/label lookups.
    pub(super) fn project_host(&self, project_id: Uuid) -> waku_client::DaemonKey {
        self.daemons.project_owner(project_id)
    }

    fn finish_runtime_attachment(
        &mut self,
        session_id: Uuid,
        result: anyhow::Result<Option<(AgentSession, PreparedDriver)>>,
        cx: &mut Context<Self>,
    ) {
        if !self.runtime_attach_pending.remove(&session_id) {
            return;
        }
        match result {
            Ok(Some((session, prepared))) => {
                self.runtime_attach_misses.remove(&session_id);
                let Some(index) = self
                    .state
                    .sessions
                    .iter()
                    .position(|candidate| candidate.id == session_id)
                else {
                    return;
                };
                if !self.runtimes.contains_key(&session_id) {
                    self.state.sessions[index] = session;
                    self.install_prepared_driver(session_id, prepared);
                    if self.state.selected_session == Some(session_id) {
                        self.reset_visible_state();
                        self.reset_transcript_rows(self.transcript_row_count());
                        self.reapply_transcript_landing(session_id, cx);
                    }
                    cx.notify();
                }
            }
            Ok(None) => {
                let busy = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .is_some_and(|session| session.status.is_busy());
                if !busy {
                    self.runtime_attach_misses.remove(&session_id);
                    return;
                }
                let misses = self.runtime_attach_misses.entry(session_id).or_default();
                *misses = misses.saturating_add(1);
                if *misses < 4 {
                    cx.spawn(async move |waku, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(250))
                            .await;
                        let _ = waku.update(cx, |waku, cx| {
                            waku.start_runtime_attachment(session_id, cx);
                        });
                    })
                    .detach();
                } else {
                    self.runtime_attach_misses.remove(&session_id);
                    self.interrupt_orphaned_runtime(session_id, cx);
                }
            }
            Err(error) => {
                eprintln!("could not attach desktop to daemon session {session_id}: {error:#}");
            }
        }
    }

    fn interrupt_orphaned_runtime(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let project_paths = self
            .state
            .projects
            .iter()
            .map(|project| (project.id, project.path.clone()))
            .collect::<HashMap<_, _>>();
        let mut checkpoint = None;
        if let Some(session) = self.state.session_mut(session_id) {
            if !session.status.is_busy() {
                return;
            }
            session.status = SessionStatus::Idle;
            let interrupted_turn_count = session
                .turns
                .last_mut()
                .filter(|turn| turn.status == TurnStatus::Running)
                .map(|turn| {
                    turn.status = TurnStatus::Interrupted;
                    turn.completed_at = Some(unix_time());
                    turn.turn_count
                });
            if let Some(turn_count) = interrupted_turn_count {
                let project_path = session
                    .workspace
                    .path()
                    .map(Path::to_path_buf)
                    .or_else(|| project_paths.get(&session.project_id).cloned());
                checkpoint = project_path.map(|project_path| PendingCheckpointCapture {
                    session_id,
                    turn_count,
                    project_path,
                });
            }
            for message in &mut session.messages {
                message.streaming = false;
            }
            for block in &mut session.transcript_blocks {
                block.activities.retain(|activity| {
                    activity
                        .reasoning
                        .as_ref()
                        .is_none_or(|reasoning| !reasoning.content.trim().is_empty())
                });
                for activity in &mut block.activities {
                    activity.complete = true;
                }
            }
            session
                .transcript_blocks
                .retain(|block| !block.activities.is_empty());
        }
        if let Some(checkpoint) = checkpoint {
            self.pending_checkpoint_captures.push(checkpoint);
            self.start_pending_checkpoint_captures(cx);
        }
        if self.state.selected_session == Some(session_id) {
            self.reset_visible_state();
            self.reset_transcript_rows(self.transcript_row_count());
            self.reapply_transcript_landing(session_id, cx);
        }
        self.save();
        cx.notify();
    }

    pub fn composer_focus(&self, cx: &App) -> FocusHandle {
        self.composer.read(cx).focus()
    }

    pub(super) fn selected_project(&self) -> Option<&Project> {
        let id = self.state.selected_project?;
        self.state.projects.iter().find(|project| project.id == id)
    }

    pub(super) fn selected_session(&self) -> Option<&AgentSession> {
        let id = self.state.selected_session?;
        self.state.sessions.iter().find(|session| session.id == id)
    }

    fn active_turn_finished_event(
        &self,
        session_id: Uuid,
        outcome: crate::analytics::TurnOutcome,
    ) -> Option<crate::analytics::Event> {
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;
        let turn = session
            .turns
            .last()
            .filter(|turn| turn.status == TurnStatus::Running)?;
        Some(crate::analytics::Event::TurnFinished {
            provider: session.provider.id(),
            turn_number: turn.turn_count,
            outcome,
            duration_seconds: unix_time().saturating_sub(turn.started_at),
        })
    }

    /// Completes a persisted turn and emits its anonymous outcome exactly
    /// once. All production turn-settlement paths go through this seam.
    pub(super) fn finish_active_turn_with_analytics(
        &mut self,
        session_id: Uuid,
        status: TurnStatus,
        outcome: crate::analytics::TurnOutcome,
    ) -> Option<(Uuid, usize)> {
        let event = self.active_turn_finished_event(session_id, outcome);
        let result = self
            .state
            .session_mut(session_id)?
            .finish_active_turn(status);
        if result.is_some()
            && let Some(event) = event
        {
            self.analytics.track(event);
        }
        if result.is_some() && status != TurnStatus::Interrupted {
            self.mark_unseen_turn_settled(session_id);
        }
        result
    }

    /// A turn that settled off-screen earns an unread dot in the sidebar's
    /// status slot and a GoToNextUnreadCompletion candidate until the task
    /// is activated. Interrupted turns are user-driven stops, not completions,
    /// so the caller's status filter keeps them out.
    fn mark_unseen_turn_settled(&mut self, session_id: Uuid) {
        if sidebar::sidebar_session_selected(
            self.state.selected_session,
            self.pending_session_activation
                .map(|pending| pending.session_id),
            session_id,
        ) {
            return;
        }
        self.state
            .unseen_completions
            .insert(session_id, unix_time());
    }

    /// Records a failed submission that is about to be unwound and therefore
    /// will not remain as a persisted turn.
    fn track_active_turn_outcome(&self, session_id: Uuid, outcome: crate::analytics::TurnOutcome) {
        if let Some(event) = self.active_turn_finished_event(session_id, outcome) {
            self.analytics.track(event);
        }
    }

    /// The directory every filesystem and provider operation for `session`
    /// must use. A not-yet-materialized worktree draft deliberately reads the
    /// local checkout until its first submission creates the isolated copy.
    pub(super) fn workspace_path_for_session<'a>(
        &'a self,
        session: &'a AgentSession,
    ) -> Option<&'a std::path::Path> {
        let project = self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id)?;
        Some(session.workspace.path().unwrap_or(&project.path))
    }

    pub(super) fn selected_workspace_path(&self) -> Option<&std::path::Path> {
        let session = self.selected_session()?;
        self.workspace_path_for_session(session)
    }

    /// Marks the session for the next save; see `PersistedState::session_mut`.
    pub(super) fn selected_session_mut(&mut self) -> Option<&mut AgentSession> {
        let id = self.state.selected_session?;
        self.state.session_mut(id)
    }

    pub(super) fn selected_runtime(&self) -> Option<&SessionRuntime> {
        self.runtimes.get(&self.state.selected_session?)
    }

    pub(super) fn provider_probe(&self, provider: ProviderKind) -> Option<&ProviderProbe> {
        self.probes.iter().find(|probe| probe.provider == provider)
    }

    pub(super) fn request_provider_model_discovery(&mut self, provider: ProviderKind) {
        if !provider.supports_model_discovery()
            || self.provider_model_discoveries.contains(&provider)
        {
            return;
        }
        let Some(probe) = self
            .provider_probe(provider)
            .filter(|probe| probe.installed)
            .cloned()
        else {
            return;
        };
        self.provider_model_discoveries.insert(provider);
        self.provider_model_discoveries_pending.insert(provider);
        let provider_probe_tx = self.provider_probe_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let daemon = self.daemon.client();
        let binary_override = self.state.provider_binary_overrides.get(&provider).cloned();
        if std::thread::Builder::new()
            .name(format!("waku-{}-model-discovery", provider.id()))
            .spawn(move || {
                let discovered = match daemon.request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::ProbeProvider {
                        provider,
                        binary_override,
                        discover_models: true,
                        probe_version: false,
                    },
                ) {
                    Ok(waku_client::ResponsePayload::ProviderProbe { probe, .. }) => probe,
                    _ => probe,
                };
                if provider_probe_tx.send(discovered).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .is_err()
        {
            self.provider_model_discoveries.remove(&provider);
            self.provider_model_discoveries_pending.remove(&provider);
        }
    }

    /// Re-run one provider's model-owned catalog discovery, for selectors whose
    /// contents can change while Goddard stays open — models the user just
    /// authored in a provider's config, or DeepSeek's custom agent presets.
    /// The stale catalog stays on screen until the fresh probe lands, so an
    /// open menu never blanks into a loading state while it refreshes.
    pub(super) fn refresh_provider_model_discovery(&mut self, provider: ProviderKind) {
        if self.provider_model_discoveries_pending.contains(&provider) {
            return;
        }
        self.provider_model_discoveries.remove(&provider);
        self.request_provider_model_discovery(provider);
    }

    /// Ask every installed CLI for its version, one short-lived subprocess per
    /// provider on its own thread. Answers land in `provider_versions` through
    /// the drain loop; render reads only that map.
    pub(super) fn request_provider_version_probes(&mut self) {
        let targets = self
            .probes
            .iter()
            .filter(|probe| probe.installed)
            .map(|probe| probe.provider)
            .collect::<Vec<_>>();
        for provider in targets {
            if !self.provider_version_probes_pending.insert(provider) {
                continue;
            }
            let provider_version_tx = self.provider_version_tx.clone();
            let event_wake = self.event_wake_tx.clone();
            let daemon = self.daemon.client();
            let binary_override = self.state.provider_binary_overrides.get(&provider).cloned();
            if std::thread::Builder::new()
                .name(format!("waku-{}-version-probe", provider.id()))
                .spawn(move || {
                    let version = match daemon.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::ProbeProvider {
                            provider,
                            binary_override,
                            discover_models: false,
                            probe_version: true,
                        },
                    ) {
                        Ok(waku_client::ResponsePayload::ProviderProbe { version, .. }) => version,
                        _ => None,
                    };
                    if provider_version_tx.send((provider, version)).is_ok() {
                        signal_event_pump(&event_wake);
                    }
                })
                .is_err()
            {
                self.provider_version_probes_pending.remove(&provider);
            }
        }
    }

    pub(super) fn drain_provider_version_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok((provider, version)) = self.provider_version_events.try_recv() {
            self.provider_version_probes_pending.remove(&provider);
            self.provider_versions.insert(provider, version);
            changed = true;
        }
        changed
    }

    /// Re-detect provider CLIs off-thread — every provider for the Providers
    /// page's refresh, or one whose binary override just changed. Also re-runs
    /// model discovery and version probes for whatever the detection finds
    /// installed.
    pub(super) fn refresh_provider_detection(&mut self, scope: Option<ProviderKind>) {
        if self.provider_detection_remaining > 0 {
            return;
        }
        let providers = match scope {
            Some(provider) => vec![provider],
            None => ProviderKind::ALL.to_vec(),
        };
        self.provider_detection_remaining = providers.len();
        let overrides = self.state.provider_binary_overrides.clone();
        let provider_detection_tx = self.provider_detection_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let detect_providers = providers.clone();
        let daemon = self.daemon.client();
        if std::thread::Builder::new()
            .name("waku-provider-detection".into())
            .spawn(move || {
                for provider in detect_providers {
                    let response = daemon.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::ProbeProvider {
                            provider,
                            binary_override: overrides.get(&provider).cloned(),
                            discover_models: false,
                            probe_version: false,
                        },
                    );
                    let probe = match response {
                        Ok(waku_client::ResponsePayload::ProviderProbe { probe, .. }) => probe,
                        _ => ProviderProbe {
                            provider,
                            installed: false,
                            path: None,
                            models: crate::model_catalog::fallback_models(provider),
                            agent_presets: crate::model_catalog::fallback_agent_presets(provider),
                        },
                    };
                    if provider_detection_tx.send(probe).is_ok() {
                        signal_event_pump(&event_wake);
                    }
                }
            })
            .is_err()
        {
            self.provider_detection_remaining = 0;
            return;
        }
        // A refresh means "re-check everything about these providers":
        // clearing the per-launch guard lets each one's catalog discovery run
        // again as its detection lands below.
        for provider in providers {
            self.provider_model_discoveries.remove(&provider);
        }
    }

    pub(super) fn drain_provider_detection_events(&mut self) -> bool {
        let mut changed = false;
        let mut installed_providers = Vec::new();
        while let Ok(probe) = self.provider_detection_events.try_recv() {
            let provider = probe.provider;
            let installed = probe.installed;
            self.provider_detection_remaining = self.provider_detection_remaining.saturating_sub(1);
            if self.provider_detection_remaining == 0 {
                self.provider_detection_checked_at = Some(Instant::now());
            }
            if let Some(existing) = self
                .probes
                .iter_mut()
                .find(|existing| existing.provider == provider)
            {
                if self.provider_model_discoveries_pending.contains(&provider) {
                    // A manual refresh may overlap an older live discovery.
                    // Keep that newer catalog while still accepting PATH
                    // detection from this response.
                    existing.installed = probe.installed;
                    existing.path = probe.path;
                } else {
                    *existing = probe;
                }
            } else {
                self.probes.push(probe);
            }
            if installed {
                installed_providers.push(provider);
            } else {
                self.provider_versions.remove(&provider);
            }
            changed = true;
        }
        for provider in installed_providers {
            self.request_provider_model_discovery(provider);
        }
        if changed {
            self.request_provider_version_probes();
        }
        changed
    }

    /// Whether the provider can back a new session: installed and not switched
    /// off in the Providers settings.
    pub(super) fn provider_enabled(&self, provider: ProviderKind) -> bool {
        !self.state.disabled_providers.contains(&provider)
            && self
                .provider_probe(provider)
                .is_some_and(|probe| probe.installed)
    }

    /// Whether the model picker has no provider left to offer — nothing
    /// detected on this machine, or everything switched off — so the
    /// composer's trigger, the picker panel, and the send button all swap to
    /// their unavailable state.
    pub(super) fn model_picker_has_no_providers(&self) -> bool {
        let locked_provider = self
            .selected_session()
            .filter(|session| !session.messages.is_empty())
            .map(|session| session.provider);
        super::composer::picker_has_no_providers(
            &self.probes,
            &self.state.disabled_providers,
            locked_provider,
            self.provider_detection_checked_at.is_some(),
        )
    }

    pub(super) fn model_for_session<'a>(&'a self, session: &'a AgentSession) -> Option<&'a str> {
        session.model.as_deref().or_else(|| {
            self.provider_probe(session.provider)
                .and_then(ProviderProbe::preferred_model)
                .map(|model| model.id.as_str())
        })
    }

    pub(super) fn model_display_name(&self, provider: ProviderKind, model: Option<&str>) -> String {
        let Some(model) = model else {
            return provider.short_name().to_owned();
        };
        self.provider_probe(provider)
            .and_then(|probe| probe.model(model))
            .map(|candidate| candidate.name.clone())
            .unwrap_or_else(|| model.to_owned())
    }

    pub(super) fn model_metadata_for_session(
        &self,
        session: &AgentSession,
    ) -> Option<&ProviderModel> {
        let model = self.model_for_session(session)?;
        self.provider_probe(session.provider)?.model(model)
    }

    pub(super) fn catalog_model_id_for_session<'a>(
        &'a self,
        session: &'a AgentSession,
    ) -> Option<&'a str> {
        self.model_metadata_for_session(session)
            .map(|model| model.id.as_str())
            .or_else(|| self.model_for_session(session))
    }

    pub(super) fn selected_transcript_blocks(&self) -> &[TranscriptBlock] {
        self.selected_session()
            .map(|session| session.transcript_blocks.as_slice())
            .unwrap_or(&[])
    }

    pub(super) fn save(&mut self) {
        self.capture_ui_state();
        self.last_stream_save = Instant::now();
        let daemon_error = self
            .daemon
            .update_settings(self.state.daemon_settings())
            .err()
            .map(|error| error.to_string());
        let app_error = self
            .store
            .save(&mut self.state)
            .err()
            .map(|error| error.to_string());
        if let Some(error) = daemon_error.or(app_error) {
            self.show_toast(tr!("errors.save_local_state", error = error));
        } else {
            self.stream_state_dirty = false;
        }
    }

    fn checkpoint_capture_pending(&self, session_id: Uuid, turn_count: usize) -> bool {
        self.checkpoint_captures_in_flight
            .contains(&(session_id, turn_count))
            || self
                .pending_checkpoint_captures
                .iter()
                .any(|capture| capture.session_id == session_id && capture.turn_count == turn_count)
    }

    pub(super) fn ending_checkpoint_pending(&self, session_id: Uuid) -> bool {
        self.state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| session.turns.last())
            .filter(|turn| turn.status != TurnStatus::Running)
            .is_some_and(|turn| self.checkpoint_capture_pending(session_id, turn.turn_count))
    }

    fn defer_queue_drain(&mut self, session_id: Uuid) {
        if !self.pending_queue_drains.contains(&session_id) {
            self.pending_queue_drains.push(session_id);
        }
    }

    /// Queues the newest finished turn's checkpoint for capture.
    ///
    /// Bookkeeping only. The capture itself is upwards of ten `git`
    /// invocations, one of them a `git add -A` over the whole worktree, and the
    /// hottest caller is the driver-event drain that shares the UI thread with
    /// rendering — so the work belongs to
    /// [`Self::start_pending_checkpoint_captures`], which every caller that
    /// holds a `Context` runs straight after queueing.
    pub(super) fn capture_latest_turn_checkpoint_for(&mut self, session_id: Uuid) {
        let Some((session, turn_count)) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| {
                session
                    .turns
                    .last()
                    .filter(|turn| turn.status != TurnStatus::Running)
                    .map(|turn| (session, turn.turn_count))
            })
        else {
            return;
        };
        if self.checkpoint_capture_pending(session_id, turn_count) {
            return;
        }
        let Some(project_path) = self
            .workspace_path_for_session(session)
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        self.pending_checkpoint_captures
            .push(PendingCheckpointCapture {
                session_id,
                turn_count,
                project_path,
            });
    }

    /// Runs queued turn checkpoints on the background executor.
    ///
    /// A capture lands a frame or many later, and the turn it belongs to may be
    /// gone by then, so the result is matched back by turn count rather than
    /// position. Nothing on screen waits for it: the transcript's rewind
    /// affordance appears when `invalidate_checkpoint_refs` prompts the next
    /// prefetch to notice the new ref.
    pub(super) fn start_pending_checkpoint_captures(&mut self, cx: &mut Context<Self>) {
        for request in std::mem::take(&mut self.pending_checkpoint_captures) {
            let PendingCheckpointCapture {
                session_id,
                turn_count,
                project_path,
            } = request;
            let Some(workspace) = self.workspace_client_for_session(session_id) else {
                continue;
            };
            if !self
                .checkpoint_captures_in_flight
                .insert((session_id, turn_count))
            {
                continue;
            }
            cx.spawn(async move |waku, cx| {
                let captured = cx
                    .background_executor()
                    .spawn({
                        let project_path = project_path.clone();
                        async move {
                            match workspace.request(
                                waku_client::WorkspaceOperation::CaptureTurn {
                                    cwd: project_path,
                                    session_id,
                                    turn_count,
                                },
                            )? {
                                waku_client::WorkspaceResult::Checkpoint { checkpoint } => {
                                    Ok(checkpoint)
                                }
                                _ => anyhow::bail!(
                                    "the daemon returned an invalid checkpoint response"
                                ),
                            }
                        }
                    })
                    .await;
                waku.update(cx, |waku, cx| {
                    waku.checkpoint_captures_in_flight
                        .remove(&(session_id, turn_count));
                    let selected = waku.state.selected_session == Some(session_id);
                    if selected {
                        waku.sync_transcript_rows();
                    }
                    let previous_kinds = if selected {
                        waku.transcript_row_kinds.borrow().clone()
                    } else {
                        Vec::new()
                    };
                    let checkpoint = match captured {
                        Ok(checkpoint) => checkpoint,
                        Err(error) => {
                            waku.show_toast(tr!("errors.capture_turn_checkpoint", error = error));
                            Checkpoint {
                                turn_count,
                                git_ref: checkpoint::checkpoint_ref(session_id, turn_count),
                                status: CheckpointStatus::Error,
                                files: Vec::new(),
                                additions: 0,
                                deletions: 0,
                                created_at: unix_time(),
                            }
                        }
                    };
                    waku.invalidate_checkpoint_refs();
                    let mut attached_turn_id = None;
                    if let Some(session) = waku.state.session_mut(session_id)
                        && let Some(turn) = session
                            .turns
                            .iter_mut()
                            .find(|turn| turn.turn_count == turn_count)
                    {
                        turn.checkpoint = Some(checkpoint);
                        attached_turn_id = Some(turn.id);
                    }
                    if let Some(turn_id) = attached_turn_id
                        && selected
                    {
                        // Reconcile a standalone card by row identity, then
                        // remeasure the terminal response when the card is
                        // hosted inline before its footer.
                        waku.splice_transcript_rows_after_visibility_change(&previous_kinds);
                        waku.remeasure_changed_files(turn_id);
                    }
                    let resume_queue = waku.pending_queue_drains.contains(&session_id);
                    if resume_queue {
                        waku.pending_queue_drains.retain(|id| *id != session_id);
                        waku.drain_queued_message(session_id, cx);
                    }
                    // The landed capture may be the ending checkpoint an
                    // archived worktree cleanup was waiting on.
                    waku.drain_pending_workspace_cleanups(cx);
                    cx.notify();
                    if attached_turn_id.is_some() {
                        // Let the new transcript row paint before SQLite work.
                        // Without this save, a checkpoint that lands after the
                        // turn's final stream save can disappear on relaunch.
                        cx.spawn(async move |waku, cx| {
                            cx.background_executor().timer(STREAM_FRAME_INTERVAL).await;
                            let _ = waku.update(cx, |waku, _| waku.save());
                        })
                        .detach();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    pub(super) fn fork_session_from_response(
        &mut self,
        session_id: Uuid,
        turn_count: usize,
        cx: &mut Context<Self>,
    ) {
        if self.response_fork_preparations.contains_key(&session_id)
            || self.submission_preparations.contains(&session_id)
        {
            self.show_toast(tr!("session.response_cannot_fork"));
            cx.notify();
            return;
        }
        let Some(source) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned()
        else {
            self.show_toast(tr!("session.response_unavailable"));
            cx.notify();
            return;
        };
        if self.state.selected_session != Some(session_id)
            || !matches!(source.status, SessionStatus::Idle | SessionStatus::Failed)
            || !source.provider.supports_conversation_fork()
            || source
                .turns
                .get(turn_count.saturating_sub(1))
                .is_none_or(|turn| turn.turn_count != turn_count || !turn.provider_turn_started)
        {
            self.show_toast(tr!("session.response_cannot_fork"));
            cx.notify();
            return;
        }
        let Some(source_workspace_path) = self
            .workspace_path_for_session(&source)
            .map(std::path::Path::to_path_buf)
        else {
            self.show_toast(tr!("errors.task_project_not_found"));
            cx.notify();
            return;
        };

        let provider = source.provider;
        let project_id = source.project_id;
        let fork_title = next_response_fork_title(
            source.display_title(),
            self.state
                .sessions
                .iter()
                .filter(|session| session.project_id == project_id)
                .map(AgentSession::display_title),
        );
        let provider_turn_count = source
            .turns
            .iter()
            .take(turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let turns_to_remove = source.provider_turns_after(turn_count);
        let driver = self
            .runtimes
            .get(&session_id)
            .map(|runtime| runtime.driver.clone());
        let binary_provider = match provider {
            ProviderKind::Amp => Some("Amp"),
            ProviderKind::Copilot => Some("GitHub Copilot"),
            ProviderKind::OpenCode => Some("OpenCode"),
            ProviderKind::OpenCode2 => Some("OpenCode 2"),
            ProviderKind::Grok => Some("Grok Build"),
            _ => None,
        };
        let binary =
            binary_provider.and_then(|_| self.provider_binary_for_session(session_id, provider));
        if let Some(provider_name) = binary_provider
            && binary.is_none()
        {
            self.show_toast(tr!(
                "errors.provider_not_installed",
                provider = provider_name
            ));
            cx.notify();
            return;
        }
        let driver_start = if matches!(
            provider,
            ProviderKind::Codex | ProviderKind::DeepSeek | ProviderKind::OhMyPi | ProviderKind::Pi
        ) && driver.is_none()
        {
            match self.driver_start_request_for_session(&source, source_workspace_path.clone()) {
                Ok(request) => Some(request),
                Err(error) => {
                    self.show_toast(tr!("errors.fork_task", error = error));
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        let Some(workspace_client) = self.workspace_client_for_session(source.id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        let request = ResponseForkRequest {
            workspace_client,
            source,
            source_workspace_path,
            fork_title,
            turn_count,
            provider_turn_count,
            turns_to_remove,
            binary,
            driver,
            driver_start,
        };

        self.response_fork_preparations
            .insert(session_id, turn_count);
        self.hide_toast();
        cx.notify();

        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { perform_response_fork(request) })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_response_fork(session_id, turn_count, provider, result, cx);
            });
        })
        .detach();
    }

    fn finish_response_fork(
        &mut self,
        session_id: Uuid,
        turn_count: usize,
        provider: ProviderKind,
        result: Result<PreparedResponseFork, String>,
        cx: &mut Context<Self>,
    ) {
        if self.response_fork_preparations.get(&session_id) != Some(&turn_count) {
            return;
        }
        self.response_fork_preparations.remove(&session_id);

        let PreparedResponseFork {
            forked,
            prepared_driver,
            checkpoint_warning,
        } = match result {
            Ok(prepared) => prepared,
            Err(error) => {
                if matches!(provider, ProviderKind::Pi | ProviderKind::OhMyPi) {
                    // A failed restore after one of these creates a fork can
                    // leave the resident RPC process on that fork. Recreate it
                    // lazily from the source cursor on its next prompt.
                    if let Some(runtime) = self.runtimes.remove(&session_id) {
                        runtime.driver.close();
                    }
                }
                self.drain_queued_message(session_id, cx);
                self.show_toast(error);
                cx.notify();
                return;
            }
        };

        if let Some(prepared) = prepared_driver
            && !self.runtimes.contains_key(&session_id)
        {
            self.install_prepared_driver(session_id, prepared);
        }
        self.invalidate_checkpoint_refs();

        let fork_id = forked.id;
        self.daemons
            .claim_session(fork_id, self.daemons.session_owner(session_id));
        self.state.push_session(forked);
        self.analytics
            .track(crate::analytics::Event::ResponseForked {
                provider: provider.id(),
                turn_number: turn_count,
            });
        self.select_session(fork_id, cx);
        self.drain_queued_message(session_id, cx);
        match checkpoint_warning {
            Some(error) => {
                self.show_toast(tr!("session.forked_with_checkpoint_warning", error = error))
            }
            None => self.show_success_toast(tr!("session.forked_from_response")),
        }
        cx.notify();
    }

    /// Composer Enter clears the field after emitting its event. A response
    /// fork temporarily owns the source provider, so restore a keyboard
    /// submission on the next task turn instead of racing it against the fork.
    pub(super) fn defer_restore_composer_after_fork(
        &self,
        session_id: Uuid,
        prompt: String,
        cx: &mut Context<Self>,
    ) {
        let composer = self.composer.clone();
        cx.spawn(async move |waku, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1))
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.state.selected_session == Some(session_id) {
                    composer.update(cx, |input, cx| {
                        if input.content(cx).is_empty() {
                            input.set_content(prompt, cx);
                        }
                    });
                }
            });
        })
        .detach();
    }

    pub(super) fn begin_message_edit(
        &mut self,
        action: UserMessageAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let UserMessageAction {
            session_id,
            message_id,
            turn_count,
        } = action;
        let Some((message_index, initial_message, attachments)) = self
            .state
            .sessions
            .iter()
            .find(|session| {
                session.id == session_id
                    && session.provider.supports_conversation_rollback()
                    && matches!(session.status, SessionStatus::Idle | SessionStatus::Failed)
            })
            .and_then(|session| {
                let turn = session
                    .turns
                    .iter()
                    .find(|turn| turn.turn_count == turn_count)?;
                session
                    .messages
                    .iter()
                    .enumerate()
                    .find_map(|(index, message)| {
                        (message.id == message_id
                            && message.turn_id == Some(turn.id)
                            && message.role == MessageRole::User)
                            .then(|| {
                                (
                                    index,
                                    message.visible_content().to_owned(),
                                    message.attachments.clone(),
                                )
                            })
                    })
            })
        else {
            self.show_toast(tr!("session.message_not_editable"));
            cx.notify();
            return;
        };

        let input = cx.new(|cx| ComposerInput::new(window, cx).padding_x(px(12.0), cx));
        input.update(cx, |input, cx| input.set_content(initial_message, cx));
        cx.subscribe(
            &input,
            |this: &mut Self, _, event: &ComposerEvent, cx| match event {
                ComposerEvent::Submit(prompt) => {
                    this.submit_message_edit_prompt(prompt.clone(), cx)
                }
                // An edited past message resubmits from that point; there is
                // no running turn for it to steer.
                ComposerEvent::SubmitSteer(prompt) => {
                    this.submit_message_edit_prompt(prompt.clone(), cx)
                }
                ComposerEvent::SteerQueued => {}
                ComposerEvent::Edited => cx.notify(),
                ComposerEvent::Focus => {}
                ComposerEvent::BackspaceOnEmpty => {}
            },
        )
        .detach();
        self.message_edit = Some(MessageEdit {
            session_id,
            message_id,
            turn_count,
            input: input.clone(),
            attachments,
        });
        self.hide_toast();
        self.remeasure_transcript_message(message_index);
        let focus_handle = input.read(cx).focus();
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    pub(super) fn cancel_message_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .message_edit
            .as_ref()
            .is_some_and(|edit| self.submission_preparations.contains(&edit.session_id))
        {
            return;
        }
        let Some(edit) = self.message_edit.take() else {
            return;
        };
        let message_index = self.selected_session().and_then(|session| {
            session
                .messages
                .iter()
                .position(|message| message.id == edit.message_id)
        });
        if let Some(message_index) = message_index {
            self.remeasure_transcript_message(message_index);
        }
        let focus_handle = self.composer_focus(cx);
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    pub(super) fn submit_message_edit(&mut self, cx: &mut Context<Self>) {
        let prompt = self
            .message_edit
            .as_ref()
            .map(|edit| edit.input.read(cx).content(cx).to_owned())
            .unwrap_or_default();
        self.submit_message_edit_prompt(prompt, cx);
    }

    fn submit_message_edit_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let Some(edit) = self.message_edit.clone() else {
            return;
        };
        if self.submission_preparations.contains(&edit.session_id) {
            return;
        }
        // Keyboard submission clears ComposerInput after emitting its event.
        // Use the event's captured value rather than rereading the field; the
        // button path enters here with its own pre-clear content as well.
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() && edit.attachments.is_empty() {
            self.show_toast(tr!("session.edited_message_empty"));
            cx.notify();
            return;
        }
        let provider_prompt = composer::merged_submission(&prompt, &edit.attachments)
            .expect("edited text or retained attachments always form a submission");
        let display_content = (!edit.attachments.is_empty()).then_some(prompt);
        self.start_message_rewind(
            edit.clone(),
            ComposerSubmission {
                prompt: provider_prompt,
                display_content,
                human_content: None,
                attachments: edit.attachments,
                pasted_blocks: Vec::new(),
                annotations: Vec::new(),
                hidden: false,
            },
            cx,
        );
    }

    fn start_message_rewind(
        &mut self,
        edit: MessageEdit,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        let session_id = edit.session_id;
        let turn_count = edit.turn_count;
        let retained_turn_count = turn_count.saturating_sub(1);
        let Some(source) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .filter(|session| {
                session
                    .turns
                    .iter()
                    .any(|turn| turn.turn_count == turn_count)
            })
        else {
            self.show_toast(tr!("session.message_unavailable"));
            cx.notify();
            return;
        };
        if self.state.selected_session != Some(session_id) {
            self.show_toast(tr!("session.select_before_rewind"));
            cx.notify();
            return;
        }
        if !matches!(source.status, SessionStatus::Idle | SessionStatus::Failed) {
            self.show_toast(tr!("session.stop_before_rewind"));
            cx.notify();
            return;
        }
        let rollback_turns = source.provider_turns_after(retained_turn_count);
        if !source.provider.supports_conversation_rollback()
            || (rollback_turns > 0 && source.provider_cursor.is_none())
        {
            self.show_toast(tr!(
                "session.provider_cannot_rewind",
                provider = source.provider.display_name()
            ));
            cx.notify();
            return;
        }
        let Some(project_path) = self
            .workspace_path_for_session(&source)
            .map(std::path::Path::to_path_buf)
        else {
            self.show_toast(tr!("errors.task_project_not_found"));
            cx.notify();
            return;
        };
        let provider_turn_count = source
            .turns
            .iter()
            .take(retained_turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let provider_resume_at = retained_turn_count
            .checked_sub(1)
            .and_then(|index| source.turns.get(index))
            .and_then(|turn| turn.provider_resume_at.clone());
        let driver = self
            .runtimes
            .get(&session_id)
            .map(|runtime| runtime.driver.clone());
        let needs_binary = rollback_turns > 0
            && (matches!(source.provider, ProviderKind::Amp)
                || (source.provider == ProviderKind::Copilot && retained_turn_count > 0)
                || (source.provider == ProviderKind::OpenCode && driver.is_none())
                || (source.provider == ProviderKind::OpenCode2 && driver.is_none())
                || (source.provider == ProviderKind::Grok && retained_turn_count > 0));
        let binary = needs_binary
            .then(|| self.provider_binary_for_session(session_id, source.provider))
            .flatten();
        if needs_binary && binary.is_none() {
            self.show_toast(tr!(
                "errors.provider_not_found",
                provider = source.provider.display_name()
            ));
            cx.notify();
            return;
        }
        let driver_start = if rollback_turns > 0
            && matches!(
                source.provider,
                ProviderKind::Codex
                    | ProviderKind::DeepSeek
                    | ProviderKind::OhMyPi
                    | ProviderKind::Pi
            )
            && driver.is_none()
        {
            match self.driver_start_request_for_session(&source, project_path.clone()) {
                Ok(request) => Some(request),
                Err(error) => {
                    self.show_toast(error.to_string());
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        let previous_status = source.status;
        let previous_turn_count = source.turns.len();
        let provider = source.provider;
        let provider_cursor = source.provider_cursor.clone();
        let session_title = source.display_title().to_owned();
        let cursor_source = (provider == ProviderKind::Cursor).then(|| source.clone());
        let edited_message_id = edit.message_id;
        let Some(edited_message_index) = source
            .turns
            .iter()
            .find(|turn| turn.turn_count == turn_count)
            .and_then(|turn| {
                source.messages.iter().position(|message| {
                    message.id == edited_message_id
                        && message.turn_id == Some(turn.id)
                        && message.role == MessageRole::User
                })
            })
        else {
            self.show_toast(tr!("session.message_unavailable"));
            cx.notify();
            return;
        };
        let Some(workspace_client) = self.workspace_client_for_session(session_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        let request = MessageRewindRequest {
            workspace_client,
            session_id,
            provider,
            provider_cursor,
            session_title,
            cursor_source,
            previous_turn_count,
            project_path,
            retained_turn_count,
            rollback_turns,
            provider_turn_count,
            provider_resume_at,
            binary,
            driver,
            driver_start,
        };

        // Optimistically leave edit mode and show the replacement bubble at
        // accept time. The main composer switches to its non-cancellable
        // spinner while every Git, process, native transcript, and provider
        // operation runs off the UI thread. Failure restores both the original
        // bubble and this edit input.
        let original_message = self.state.session_mut(session_id).and_then(|session| {
            let message = session
                .messages
                .iter_mut()
                .find(|message| message.id == edited_message_id)?;
            let original = message.clone();
            message.content = submission.prompt.clone();
            message.display_content = submission.display_content.clone();
            message.attachments = submission.attachments.clone();
            session.status = SessionStatus::Connecting;
            session.updated_at = unix_time();
            Some(original)
        });
        let Some(original_message) = original_message else {
            self.show_toast(tr!("session.message_unavailable"));
            cx.notify();
            return;
        };
        self.message_edit = None;
        self.submission_preparations.insert(session_id);
        self.hide_toast();
        self.remeasure_transcript_message(edited_message_index);
        cx.notify();

        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { perform_message_rewind(request) })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_message_rewind(
                    edit,
                    submission,
                    edited_message_id,
                    original_message,
                    previous_status,
                    result,
                    cx,
                );
            });
        })
        .detach();
    }

    fn finish_message_rewind(
        &mut self,
        edit: MessageEdit,
        submission: ComposerSubmission,
        edited_message_id: Uuid,
        original_message: Message,
        previous_status: SessionStatus,
        result: Result<PreparedMessageRewind, String>,
        cx: &mut Context<Self>,
    ) {
        let session_id = edit.session_id;
        let turn_count = edit.turn_count;
        if !self.submission_preparations.remove(&session_id) {
            return;
        }
        self.drain_pending_workspace_cleanups(cx);
        let selected = self.state.selected_session == Some(session_id);
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(error) => {
                if let Some(session) = self.state.session_mut(session_id) {
                    if let Some(message) = session
                        .messages
                        .iter_mut()
                        .find(|message| message.id == edited_message_id)
                    {
                        *message = original_message;
                    }
                    if session.status == SessionStatus::Connecting {
                        session.status = previous_status;
                    }
                }
                if selected && self.message_edit.is_none() {
                    self.message_edit = Some(edit.clone());
                }
                if selected
                    && let Some(message_index) = self.selected_session().and_then(|session| {
                        session
                            .messages
                            .iter()
                            .position(|message| message.id == edited_message_id)
                    })
                {
                    self.remeasure_transcript_message(message_index);
                }
                self.show_toast(error);
                cx.notify();
                return;
            }
        };
        let PreparedMessageRewind {
            provider_rewind_cursor,
            claude_fork,
            mut prepared_driver,
            reset_native_session,
            cleanup_error,
        } = prepared;
        let retained_turn_count = turn_count.saturating_sub(1);
        let provider_and_removed_turns = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| {
                (
                    session.provider,
                    session.turns.len().saturating_sub(retained_turn_count),
                )
            });
        let Some((provider, removed_turns)) = provider_and_removed_turns else {
            return;
        };
        if selected {
            self.sync_transcript_rows();
        }
        let previous_kinds = if selected {
            self.transcript_row_kinds.borrow().clone()
        } else {
            Vec::new()
        };
        if let Some(session) = self.state.session_mut(session_id) {
            if let Some(fork) = &claude_fork {
                for turn in session.turns.iter_mut().take(retained_turn_count) {
                    if let Some(remapped) = turn
                        .provider_resume_at
                        .as_ref()
                        .and_then(|message_id| fork.message_ids.get(message_id))
                        .cloned()
                    {
                        turn.provider_resume_at = Some(remapped);
                    }
                }
                session.provider_cursor = Some(fork.cursor.clone());
            } else if reset_native_session {
                session.provider_cursor = None;
            } else if let Some(cursor) = provider_rewind_cursor.clone() {
                session.provider_cursor = Some(cursor);
            }
            session.truncate_after_turn(retained_turn_count);
            session.status = SessionStatus::Idle;
        }

        if let Some(prepared) = prepared_driver.as_mut() {
            // Startup announces the source cursor before a cold driver-backed
            // rollback finishes. It is stale now; do not let it overwrite the
            // rewound cursor after this driver is installed.
            while prepared.events.try_recv().is_ok() {}
        }
        if let Some(prepared) = prepared_driver {
            self.install_prepared_driver(session_id, prepared);
        }
        if claude_fork.is_some()
            || reset_native_session
            || (matches!(
                provider,
                ProviderKind::Amp
                    | ProviderKind::Cursor
                    | ProviderKind::DeepSeek
                    | ProviderKind::OpenCode
                    | ProviderKind::OpenCode2
                    | ProviderKind::Grok
            ) && provider_rewind_cursor.is_some())
        {
            // Headless drivers retain their original native session ID. Recreate
            // them lazily so the next prompt resumes the fork instead.
            if let Some(runtime) = self.runtimes.remove(&session_id) {
                runtime.driver.close();
            }
            self.mark_background_work_lost(session_id);
        } else if let Some(runtime) = self.runtimes.get_mut(&session_id) {
            runtime
                .pending_events
                .retain(|event| matches!(event, DriverEvent::BackgroundWork(_)));
            runtime.stream_remeasure_pending = false;
            runtime.stream_phase = None;
            runtime.pending_permission = None;
            runtime.pending_user_input = None;
            runtime.pending_computer_approval = None;
        }
        self.invalidate_checkpoint_refs();
        if self
            .message_edit
            .as_ref()
            .is_some_and(|current| current.session_id == session_id)
        {
            self.message_edit = None;
        }
        if selected {
            self.activities_expanded.clear();
            self.expanded_activity_items.clear();
            self.expanded_turns.clear();
            self.expanded_changed_files.clear();
            self.changed_files_diff_hover = None;
            self.changed_files_diffs.clear();
            self.changed_files_diff_generation = self.changed_files_diff_generation.wrapping_add(1);
            self.transcript_control_focuses.borrow_mut().clear();
            self.splice_transcript_rows_after_visibility_change(&previous_kinds);
            self.show_toast(match cleanup_error {
                None => tr!("session.rewound", turn = turn_count),
                Some(error) => tr!(
                    "session.rewound_with_stale_refs",
                    turn = turn_count,
                    error = error
                ),
            });
        }
        self.analytics
            .track(crate::analytics::Event::ConversationRolledBack {
                provider: provider.id(),
                turns: removed_turns,
            });
        cx.notify();
        self.submit_submission_for_session(session_id, submission, cx);
    }

    /// Resolves the turn options a driver should run with, dropping a reasoning
    /// effort or service tier the resolved model does not offer. Driver start
    /// and in-session option changes both go through this so they cannot
    /// disagree about what the session is currently set to.
    pub(super) fn session_options(&self, session: &AgentSession) -> SessionOptions {
        let model = session.model.clone().or_else(|| {
            self.provider_probe(session.provider)
                .and_then(ProviderProbe::preferred_model)
                .map(|model| model.id.clone())
        });
        let model_metadata = self.model_metadata_for_session(session);
        let mut reasoning_effort = session.reasoning_effort.clone().filter(|effort| {
            model_metadata.is_some_and(|model| {
                model
                    .reasoning_efforts
                    .iter()
                    .any(|option| option.id == *effort)
            })
        });
        let mut service_tier = session.service_tier.clone().filter(|tier| {
            tier == "default"
                || model_metadata.is_some_and(|model| {
                    model.service_tiers.iter().any(|option| option.id == *tier)
                })
        });
        let context_window = session.context_window.clone().filter(|window| {
            model_metadata.is_some_and(|model| {
                model
                    .context_windows
                    .iter()
                    .any(|option| option.id == *window)
            })
        });
        if session.provider == ProviderKind::Cursor
            && let Some(requested) = model.as_deref()
            && let Some(probe) = self.provider_probe(session.provider)
            && let Some(matched) =
                waku_protocol::model_catalog::cursor_catalog_model(&probe.models, requested)
        {
            if reasoning_effort.is_none() {
                reasoning_effort = waku_protocol::model_catalog::cursor_suffix_reasoning_effort(
                    &matched.suffix,
                    &matched.model.reasoning_efforts,
                );
            }
            if service_tier.is_none() {
                service_tier = waku_protocol::model_catalog::cursor_suffix_service_tier(
                    &matched.suffix,
                    &matched.model.service_tiers,
                );
            }
        }
        SessionOptions {
            mode: session.runtime_mode,
            model,
            reasoning_effort,
            service_tier,
            context_window,
        }
    }

    pub(super) fn agent_preset_for_session(&self, session: &AgentSession) -> Option<String> {
        if session.provider != ProviderKind::DeepSeek {
            return None;
        }
        session.agent_preset.clone().or_else(|| {
            self.provider_probe(session.provider)
                .and_then(ProviderProbe::preferred_agent_preset)
                .map(|preset| preset.id.clone())
        })
    }

    pub(super) fn agent_preset_label_for_session(&self, session: &AgentSession) -> Option<String> {
        let id = self.agent_preset_for_session(session)?;
        Some(
            self.provider_probe(session.provider)
                .and_then(|probe| probe.agent_presets.iter().find(|preset| preset.id == id))
                .map(|preset| preset.display_name())
                .unwrap_or(id),
        )
    }

    /// Releases provider processes for sessions nobody has touched in a while.
    ///
    /// Codex, Pi and Oh My Pi keep a process resident between turns, so an abandoned task
    /// otherwise holds an agent — and, with Computer Use on, a whole process
    /// tree — for as long as the app runs. Recreating a runtime is exactly the
    /// work the next prompt already does after Stop, and the resume cursor is
    /// persisted, so the conversation survives.
    pub(super) fn reap_idle_sessions(&mut self) {
        if self.last_idle_session_sweep.elapsed() < IDLE_SESSION_SWEEP_INTERVAL {
            return;
        }
        self.last_idle_session_sweep = Instant::now();
        let idle = self
            .runtimes
            .iter()
            .filter(|(session_id, runtime)| {
                let session = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == **session_id);
                session_is_reapable(
                    session,
                    runtime.last_active_at.elapsed(),
                    self.session_has_live_background_work(**session_id),
                )
            })
            .map(|(session_id, _)| *session_id)
            .collect::<Vec<_>>();
        for session_id in idle {
            // Idle reaping is an explicit daemon-runtime release. Merely
            // dropping a client attachment must not stop work observed by a
            // second desktop or browser client.
            if let Some(runtime) = self.runtimes.remove(&session_id) {
                runtime.driver.close();
            }
        }
    }

    /// Applies a changed model, effort, tier, or mode to a session. Transports
    /// that carry these per turn absorb the change and keep running; the rest
    /// are torn down so the next prompt starts with the new options.
    pub(super) fn apply_session_options(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(options) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| self.session_options(session))
        else {
            return;
        };
        let Some(runtime) = self.runtimes.get_mut(&session_id) else {
            return;
        };
        runtime.options_generation = runtime.options_generation.wrapping_add(1);
        let generation = runtime.options_generation;
        let driver = runtime.driver.clone();
        cx.spawn(async move |waku, cx| {
            let applied = cx
                .background_executor()
                .spawn(async move { driver.apply_options(options) })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let is_current = waku
                    .runtimes
                    .get(&session_id)
                    .is_some_and(|runtime| runtime.options_generation == generation);
                if is_current && !applied {
                    waku.reset_session_runtime(session_id);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The CLI path that launches `provider` on the daemon owning
    /// `session_id`. A remote host resolves its own override or the bare
    /// command name against its own PATH — a locally probed absolute path is
    /// meaningless there. `None` means the local probe found no install.
    fn provider_binary_for_session(
        &self,
        session_id: Uuid,
        provider: ProviderKind,
    ) -> Option<PathBuf> {
        match self.daemons.session_owner(session_id) {
            waku_client::DaemonKey::Remote(_) => Some(
                self.daemon_settings_for_session(session_id)
                    .provider_binary_overrides
                    .get(&provider)
                    .cloned()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(provider.command())),
            ),
            waku_client::DaemonKey::Local => self
                .probes
                .iter()
                .find(|probe| probe.provider == provider)
                .and_then(|probe| probe.path.clone()),
        }
    }

    fn driver_start_request_for_session(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
    ) -> anyhow::Result<DriverStartRequest> {
        let daemon = self
            .daemons
            .daemon_for_session(session.id)
            .ok_or_else(|| anyhow::anyhow!("the task's daemon is not connected"))?;
        let binary = self
            .provider_binary_for_session(session.id, session.provider)
            .ok_or_else(|| {
                anyhow::anyhow!(tr!(
                    "errors.provider_not_found",
                    provider = session.provider.display_name()
                ))
            })?;
        let agent_preset = self.agent_preset_for_session(session);
        let SessionOptions {
            mode,
            model,
            reasoning_effort,
            service_tier,
            context_window,
        } = self.session_options(&session);
        Ok(DriverStartRequest {
            session_id: session.id,
            provider: session.provider,
            options: DriverStartOptions {
                binary,
                cwd,
                mode,
                model,
                reasoning_effort,
                service_tier,
                context_window,
                agent_preset,
                computer_use_enabled: self.state.computer_use_enabled,
                provider_cursor: session.provider_cursor.clone(),
            },
            event_wake: self.event_wake_tx.clone(),
            daemon,
        })
    }

    /// Start the session's provider runtime for a goal operation, without a
    /// prompt or a turn. Goals live on the provider thread itself, so this
    /// mirrors the Codex CLI, whose thread starts at launch: prepare the
    /// workspace, spawn the provider, and let the queued goal operations
    /// drain once the runtime installs. The session stays `Idle` throughout —
    /// no turn begins and nothing lands in the transcript.
    pub(super) fn start_goal_runtime(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.runtimes.contains_key(&session_id)
            || self.goal_runtime_starts.contains(&session_id)
            || self.submission_preparations.contains(&session_id)
        {
            // An installed or installing runtime picks the queue up when the
            // install path drains pending goal operations.
            return;
        }
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            self.pending_goal_operations.remove(&session_id);
            return;
        };
        let project_id = session.project_id;
        let workspace = session.workspace.clone();
        let next_turn_count = session.turns.len() + 1;
        let provisional_cwd = self
            .workspace_path_for_session(session)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let driver_start = self.driver_start_request_for_session(session, provisional_cwd);
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            self.pending_goal_operations.remove(&session_id);
            self.show_toast(tr!("errors.prepare_task_project_not_found"));
            cx.notify();
            return;
        };
        let Some(workspace_client) = self.workspace_client_for_session(session_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        self.goal_runtime_starts.insert(session_id);
        cx.notify();
        cx.spawn(async move |waku, cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    prepare_submission(
                        workspace_client,
                        project,
                        workspace,
                        Some(driver_start),
                        session_id,
                        next_turn_count,
                    )
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_goal_runtime_start(session_id, prepared, cx);
            });
        })
        .detach();
    }

    fn finish_goal_runtime_start(
        &mut self,
        session_id: Uuid,
        prepared: anyhow::Result<PreparedSubmission>,
        cx: &mut Context<Self>,
    ) {
        if !self.goal_runtime_starts.remove(&session_id) {
            return;
        }
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                // The goal is lost but nothing else is: messages queued
                // behind this start resubmit through the ordinary path,
                // which starts its own runtime.
                self.pending_goal_operations.remove(&session_id);
                self.unwind_unconfirmed_pursuit_turn(session_id);
                self.show_toast(error.to_string());
                self.drain_queued_message(session_id, cx);
                cx.notify();
                return;
            }
        };
        let PreparedSubmission {
            workspace,
            checkpoint_warning: _,
            worktree_restored,
            driver,
        } = prepared;
        if !self
            .state
            .sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            // The task was removed while its provider was starting.
            self.pending_goal_operations.remove(&session_id);
            if let Some(Ok(prepared)) = driver {
                prepared.handle.close();
            }
            return;
        }
        let workspace_changed = self.state.session_mut(session_id).is_some_and(|session| {
            let changed = session.workspace != workspace;
            session.workspace = workspace;
            changed
        });
        let selected = self.state.selected_session == Some(session_id);
        if (workspace_changed || worktree_restored) && selected {
            self.invalidate_workspace_queries(cx);
            self.reload_clean_right_panel_file_editors(cx);
            self.ensure_right_panel_terminals(cx);
        }
        if worktree_restored && selected {
            self.show_toast(tr!("session.worktree_recreated"));
        }
        match driver {
            Some(Ok(prepared)) => {
                if self.runtimes.contains_key(&session_id) {
                    // Another path installed a runtime meanwhile; that thread
                    // is the session's, so the goal routes there instead.
                    prepared.handle.close();
                    self.drain_pending_goal_operations(session_id);
                } else {
                    // Install drains the pending operations itself.
                    self.install_prepared_driver(session_id, prepared);
                }
            }
            None => self.drain_pending_goal_operations(session_id),
            Some(Err(error)) => {
                self.pending_goal_operations.remove(&session_id);
                self.unwind_unconfirmed_pursuit_turn(session_id);
                self.show_toast(error.to_string());
                self.drain_queued_message(session_id, cx);
                cx.notify();
                return;
            }
        }
        self.save();
        self.drain_queued_message(session_id, cx);
        cx.notify();
    }

    fn install_prepared_driver(
        &mut self,
        session_id: Uuid,
        prepared: PreparedDriver,
    ) -> DriverHandle {
        let handle = prepared.handle.clone();
        self.runtimes.insert(
            session_id,
            SessionRuntime {
                driver: prepared.handle,
                options_generation: 0,
                events: prepared.events,
                pending_events: VecDeque::new(),
                pending_steers: VecDeque::new(),
                stream_phase: None,
                pending_reasoning_newlines: 0,
                park_announced: false,
                stream_remeasure_pending: false,
                pending_permission: None,
                pending_user_input: None,
                pending_computer_approval: None,
                computer_use_previews: Vec::new(),
                computer_session_grants: HashSet::new(),
                last_driver_error: None,
                last_active_at: Instant::now(),
                last_background_refresh_at: Instant::now()
                    .checked_sub(BACKGROUND_WORK_REFRESH_INTERVAL)
                    .unwrap_or_else(Instant::now),
            },
        );
        // Startup can emit before the background task hands this receiver to
        // the runtime map. Wake once after installation so those buffered
        // events cannot be stranded behind an already-consumed edge.
        signal_event_pump(&self.event_wake_tx);
        // Goal operations accepted while no runtime existed ride the first
        // install, whichever path performed it. The driver applies them once
        // its thread opens, before any queued prompt.
        self.drain_pending_goal_operations(session_id);
        handle
    }

    /// Continue an interrupted turn with no typed prompt. The provider still
    /// gets text — [`CONTINUE_PROMPT`] — but the message is `hidden`, so no
    /// transcript row, title, or restored draft comes of it.
    pub(super) fn continue_interrupted_session(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.composer_session() else {
            return;
        };
        if !composer::session_awaits_continue(session) || self.model_picker_has_no_providers() {
            return;
        }
        let session_id = session.id;
        let submission = ComposerSubmission::hidden_continue();
        if self.big_picture.is_open() {
            self.submit_composer_submission_to(session_id, submission, cx);
        } else {
            self.submit_composer_submission(submission, cx);
        }
    }

    pub(super) fn submit_composer_submission(
        &mut self,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.selected_session() else {
            return;
        };
        if self.response_fork_preparations.contains_key(&session.id) {
            return;
        }
        if session.status == SessionStatus::Background {
            // The turn is parked on detached work and the provider is idle,
            // so the message goes straight in as a steer: queued, it would
            // wait for a settle that only the message itself could hasten.
            self.steer_composer_submission(submission, cx);
            return;
        }
        if session.is_busy() {
            // While the agent is working, Enter queues a follow-up instead of
            // refusing the message. The queue drains once the turn settles.
            self.enqueue_follow_up_submission(session.id, submission, cx);
            return;
        }
        self.submit_submission_for_session(session.id, submission, cx);
    }

    /// `submit_composer_submission` for an explicit session, used by Big
    /// Picture's targeted composer where the destination is a card rather
    /// than whatever the workspace has selected.
    pub(super) fn submit_composer_submission_to(
        &mut self,
        session_id: Uuid,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned()
        else {
            self.restore_composer_submission(submission, cx);
            return;
        };
        if self.response_fork_preparations.contains_key(&session.id) {
            self.restore_composer_submission(submission, cx);
            return;
        }
        if session.status == SessionStatus::Background {
            self.steer_session_submission(session.id, submission, cx);
            return;
        }
        if session.is_busy() {
            self.enqueue_follow_up_submission(session.id, submission, cx);
            return;
        }
        self.submit_submission_for_session(session.id, submission, cx);
    }

    /// Deliver a steering message into the running turn. Providers without a
    /// live-turn transport (or a session that is not actively working) fall
    /// back to queueing a follow-up.
    pub(super) fn steer_composer_submission(
        &mut self,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        self.steer_session_submission(session_id, submission, cx);
    }

    /// [`Self::steer_composer_submission`] for an explicit session.
    pub(super) fn steer_session_submission(
        &mut self,
        session_id: Uuid,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned()
        else {
            self.restore_composer_submission(submission, cx);
            return;
        };
        if !session.is_busy() {
            self.submit_composer_submission_to(session.id, submission, cx);
            return;
        }
        // A turn that has not reached the provider yet cannot be steered; the
        // driver reports the outcome asynchronously via SteerAccepted or
        // SteerRejected once it is handed off.
        if !self.session_can_steer(&session) {
            self.enqueue_follow_up_submission(session.id, submission, cx);
            return;
        }
        let provider_prompt = self.resolve_skill_submission(session.provider, &submission.prompt);
        if let Some(runtime) = self.runtimes.get_mut(&session.id) {
            runtime.driver.steer(provider_prompt);
            runtime.pending_steers.push_back(submission);
        } else {
            self.enqueue_follow_up_submission(session.id, submission, cx);
        }
        cx.notify();
    }

    pub(super) fn session_can_steer(&self, session: &AgentSession) -> bool {
        session_has_active_provider_turn(session)
            && self
                .runtimes
                .get(&session.id)
                .is_some_and(|runtime| runtime.driver.supports_steer())
    }

    /// Resolve presentation-preserving composer syntax immediately before a
    /// prompt crosses into a provider transport.
    fn resolve_provider_submission(&self, provider: ProviderKind, prompt: &str) -> String {
        crate::composer_complete::resolved_submission(provider, prompt, &self.slash_command_index)
            .unwrap_or_else(|| prompt.to_owned())
    }

    /// Resolve only provider-native skill syntax for a live steering message.
    fn resolve_skill_submission(&self, provider: ProviderKind, prompt: &str) -> String {
        crate::composer_complete::resolved_skill_submission(
            provider,
            prompt,
            &self.slash_command_index,
        )
        .unwrap_or_else(|| prompt.to_owned())
    }

    pub(super) fn enqueue_follow_up_submission(
        &mut self,
        session_id: Uuid,
        mut submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        submission.prompt = submission.prompt.trim().to_owned();
        if submission.prompt.is_empty() {
            return;
        }
        // The annotation header already lives inside `prompt`; the structured
        // set parks under the queued id so the follow-up's replies can still
        // resolve its "Annotation N" citations once it sends.
        let annotations = std::mem::take(&mut submission.annotations);
        let message = submission.into_queued_message();
        if !annotations.is_empty() {
            self.queued_annotations.insert(message.id, annotations);
        }
        if let Some(session) = self.state.session_mut(session_id) {
            session.queued_messages.push(message);
            session.updated_at = unix_time();
        }
        self.save();
        cx.notify();
    }

    pub(super) fn remove_queued_message(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.state.session_mut(session_id) {
            session
                .queued_messages
                .retain(|message| message.id != message_id);
        }
        self.queued_annotations.remove(&message_id);
        self.save();
        cx.notify();
    }

    /// Pop a queued message back into the composer so the user can edit and
    /// resubmit it.
    pub(super) fn edit_queued_message(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self.state.session_mut(session_id).and_then(|session| {
            let index = session
                .queued_messages
                .iter()
                .position(|message| message.id == message_id)?;
            Some(session.queued_messages.remove(index))
        }) else {
            return;
        };
        let mut submission = ComposerSubmission::from_queued_message(message);
        submission.annotations = self
            .queued_annotations
            .remove(&message_id)
            .unwrap_or_default();
        self.restore_composer_submission(submission, cx);
        let focus_handle = self.composer_focus(cx);
        window.focus(&focus_handle, cx);
        self.save();
        cx.notify();
    }

    /// Deliver a queued follow-up into the running turn right away instead of
    /// waiting for the turn to settle. Falls through the same paths as a
    /// composer steer: an idle session starts a fresh turn, an unsteerable
    /// one re-queues the message.
    pub(super) fn steer_queued_message(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self.state.session_mut(session_id).and_then(|session| {
            let index = session
                .queued_messages
                .iter()
                .position(|message| message.id == message_id)?;
            Some(session.queued_messages.remove(index))
        }) else {
            return;
        };
        let mut submission = ComposerSubmission::from_queued_message(message);
        submission.annotations = self
            .queued_annotations
            .remove(&message_id)
            .unwrap_or_default();
        self.save();
        self.steer_composer_submission(submission, cx);
    }

    /// Activate the same action as the oldest queued row's Steer control.
    /// When that control is unavailable, leave the queue untouched rather
    /// than removing and re-queueing its first message at the back.
    pub(super) fn steer_oldest_queued_message(&mut self, cx: &mut Context<Self>) {
        let Some((session_id, message_id)) = self.composer_session().and_then(|session| {
            if !self.session_can_steer(session) {
                return None;
            }
            // A hidden queue entry is provider-facing text, not a follow-up
            // the user can steer — its own drain still delivers it.
            let message = session
                .queued_messages
                .iter()
                .find(|message| !message.hidden)?;
            Some((session.id, message.id))
        }) else {
            return;
        };
        self.steer_queued_message(session_id, message_id, cx);
    }

    /// Start the next queued follow-up as a fresh turn. Only called once a
    /// settled turn has been fully closed, so the session is Idle.
    pub(super) fn drain_queued_message(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.response_fork_preparations.contains_key(&session_id) {
            return;
        }
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if session.is_busy()
            || session.queued_messages.is_empty()
            || self.ending_checkpoint_pending(session_id)
            // Messages parked behind a goal-initiated provider start stay
            // queued until that runtime installs.
            || self.goal_runtime_starts.contains(&session_id)
            // Messages submitted while the session moved into a worktree
            // wait for the rebind, then start in the new directory.
            || self.worktree_move_pending.contains(&session_id)
        {
            return;
        }
        let Some(message) = self
            .state
            .session_mut(session_id)
            .map(|session| session.queued_messages.remove(0))
        else {
            return;
        };
        let queued_id = message.id;
        let mut submission = ComposerSubmission::from_queued_message(message);
        submission.annotations = self
            .queued_annotations
            .remove(&queued_id)
            .unwrap_or_default();
        self.submit_submission_for_session(session_id, submission, cx);
    }

    fn submit_submission_for_session(
        &mut self,
        session_id: Uuid,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        if self.response_fork_preparations.contains_key(&session_id) {
            return;
        }
        let selected = self.state.selected_session == Some(session_id);
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        // Quarantined transfer sessions can't start turns — the trust card
        // replaces the composer, so reaching this means a path bypassed the
        // UI. The daemon refuses too; drop the submission here as well.
        if session.quarantined {
            return;
        }
        if self.ending_checkpoint_pending(session_id) {
            self.enqueue_follow_up_submission(session_id, submission, cx);
            self.defer_queue_drain(session_id);
            return;
        }
        // A goal operation is already starting this session's provider.
        // Queue the message so it lands on that thread — after the goal —
        // instead of racing a second provider process into existence.
        if self.goal_runtime_starts.contains(&session_id) {
            self.enqueue_follow_up_submission(session_id, submission, cx);
            self.defer_queue_drain(session_id);
            return;
        }
        // A worktree move is swapping the session's working directory. Queue
        // the message so the turn starts in the worktree rather than racing
        // the rebind — the finish path drains once the workspace settles.
        if self.worktree_move_pending.contains(&session_id) {
            self.enqueue_follow_up_submission(session_id, submission, cx);
            self.defer_queue_drain(session_id);
            return;
        }
        if session.status.is_busy() {
            self.enqueue_follow_up_submission(session_id, submission, cx);
            return;
        }
        let prompt = submission.prompt.clone();
        let hidden = submission.hidden;
        let human_prompt = submission.human_prompt();
        let has_input = !hidden
            && !submission
                .display_content
                .as_deref()
                .unwrap_or(&submission.prompt)
                .trim()
                .is_empty();
        let next_turn_count = session.turns.len() + 1;
        let provider = session.provider.id();
        let model = self
            .session_options(session)
            .model
            .unwrap_or_else(|| "default".into());
        let workspace_kind = if session.workspace.is_worktree() {
            "worktree"
        } else {
            "local"
        };
        let attachment_count = submission.attachments.len();
        let project_id = session.project_id;
        let workspace = session.workspace.clone();
        let driver_start = (!self.runtimes.contains_key(&session_id)).then(|| {
            let provisional_cwd = self
                .workspace_path_for_session(session)
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            self.driver_start_request_for_session(session, provisional_cwd)
        });
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            if selected {
                self.restore_composer_submission(submission, cx);
                self.show_toast(tr!("errors.prepare_task_project_not_found"));
            }
            cx.notify();
            return;
        };
        let projectless = project.is_projectless();
        // A renamed or deleted folder would surface as an opaque spawn or
        // Git failure in `prepare_submission`. A submission is a one-shot
        // user action, so one synchronous stat is the cheapest correct
        // check — and it catches folders that went missing after the last
        // reconciliation pass.
        if !projectless && !self.is_remote_project(project_id) && !project.path.is_dir() {
            self.missing_projects.insert(project_id);
            if selected {
                self.restore_composer_submission(submission, cx);
            }
            self.show_toast_with_tone(
                tr!(
                    "errors.project_folder_missing",
                    name = project.display_name()
                ),
                ToastTone::Alert,
                Some(ToastAction {
                    label: tr!("project.locate").into(),
                    kind: ToastActionKind::RelocateProject(project_id),
                }),
            );
            cx.notify();
            return;
        }
        // Busy is visible before any Git work begins. The separate transient
        // set keeps this non-cancellable phase visually distinct from a
        // connecting provider, whose runtime already has a working Stop path.
        //
        // The turn also begins now, not once preparation settles: the sent
        // message and its working indicator belong in the transcript the
        // moment the submission is accepted — a first prompt otherwise leaves
        // the empty state on screen for as long as a `git add -A` takes.
        // Preparation failure unwinds the turn and restores the prompt.
        if selected {
            self.sync_transcript_rows();
        }
        let previous_kinds = if selected {
            self.transcript_row_kinds.borrow().clone()
        } else {
            Vec::new()
        };
        let (transcript_anchor, sent_message_id) =
            if let Some(session) = self.state.session_mut(session_id) {
                // A hidden prompt is not user input: no title, no anchor, and no
                // transcript row — the turn's work lands on the tail instead.
                if !hidden {
                    session.set_title_from_prompt(&human_prompt);
                }
                let turn_id = if hidden {
                    session.begin_hidden_turn(&prompt)
                } else {
                    session.begin_turn_with_presentation(
                        &prompt,
                        submission.display_content.clone(),
                        submission.attachments.clone(),
                    )
                };
                session.status = SessionStatus::Connecting;
                session.updated_at = unix_time();
                (
                    (selected && !hidden).then_some(TranscriptAnchor {
                        session_id,
                        turn_id,
                    }),
                    session.messages.last().map(|message| message.id),
                )
            } else {
                (None, None)
            };
        if let Some(message_id) = sent_message_id {
            self.record_sent_annotations(session_id, message_id, &submission.annotations);
        }
        self.analytics
            .track(crate::analytics::Event::TurnSubmitted {
                provider,
                model,
                turn_number: next_turn_count,
                workspace: workspace_kind,
                projectless,
                attachment_count,
                has_input,
            });
        self.submission_preparations.insert(session_id);
        if selected {
            self.activities_expanded.clear();
            self.expanded_activity_items.clear();
            self.expanded_turns.clear();
            self.expanded_changed_files.clear();
            self.changed_files_diff_hover = None;
            self.changed_files_diffs.clear();
            self.changed_files_diff_generation = self.changed_files_diff_generation.wrapping_add(1);
            self.transcript_control_focuses.borrow_mut().clear();
            self.message_edit = None;
            self.hide_toast();
            self.transcript_anchor.set(transcript_anchor);
            if hidden {
                // No sent row to anchor on — a hidden prompt renders nothing.
                // Hold the tail so the turn's work streams in where the
                // reader already sits.
                self.transcript_anchor_end_space.set(Pixels::ZERO);
                self.transcript_anchor_following.set(false);
                self.splice_transcript_rows_after_visibility_change(&previous_kinds);
                self.pin_transcript_to_tail();
            } else {
                // Provisional reservation: the anchored list has no measured
                // bounds until its first paint, and a zero end space cannot hold
                // the sent row at the viewport top — without scroll room past the
                // tail, the list clamps to its end and the prompt paints a frame
                // at the bottom before the first measured frame lifts it. Seed a
                // full viewport of end space instead; the overshoot is invisible
                // under the top anchor and the first measured frame trues it up.
                let mut provisional = self.transcript_rows.viewport_bounds().size.height;
                if provisional <= Pixels::ZERO {
                    provisional = self.anchored_transcript_rows.viewport_bounds().size.height;
                }
                self.transcript_anchor_end_space.set(provisional);
                self.transcript_anchor_following.set(true);
                self.splice_transcript_rows_after_visibility_change(&previous_kinds);
                self.scroll_transcript_to_anchor();
            }
        }
        cx.notify();

        let Some(workspace_client) = self.workspace_client_for_session(session_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            return;
        };
        cx.spawn(async move |waku, cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    prepare_submission(
                        workspace_client,
                        project,
                        workspace,
                        driver_start,
                        session_id,
                        next_turn_count,
                    )
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_submission_preparation(session_id, submission, prepared, cx);
            });
        })
        .detach();
    }

    fn finish_submission_preparation(
        &mut self,
        session_id: Uuid,
        submission: ComposerSubmission,
        prepared: anyhow::Result<PreparedSubmission>,
        cx: &mut Context<Self>,
    ) {
        if !self.submission_preparations.contains(&session_id) {
            return;
        }
        let selected = self.state.selected_session == Some(session_id);
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.submission_preparations.remove(&session_id);
                self.drain_pending_workspace_cleanups(cx);
                self.track_active_turn_outcome(
                    session_id,
                    crate::analytics::TurnOutcome::PreparationFailed,
                );
                if selected {
                    self.sync_transcript_rows();
                }
                let previous_kinds = if selected {
                    self.transcript_row_kinds.borrow().clone()
                } else {
                    Vec::new()
                };
                if let Some(session) = self.state.session_mut(session_id)
                    && session.status == SessionStatus::Connecting
                {
                    // The submission never reached a provider and its prompt
                    // returns to the composer, so the eagerly-begun turn and
                    // its message leave the transcript with it.
                    if let Some(turn_id) = session.active_turn_id() {
                        session.unwind_unstarted_turn(turn_id);
                    }
                    session.status = SessionStatus::Idle;
                }
                if selected {
                    if self
                        .transcript_anchor
                        .get()
                        .is_some_and(|anchor| anchor.session_id == session_id)
                    {
                        self.transcript_anchor.set(None);
                        self.transcript_anchor_following.set(false);
                    }
                    self.splice_transcript_rows_after_visibility_change(&previous_kinds);
                    self.restore_composer_submission(submission, cx);
                    self.show_toast(tr!("errors.create_worktree", error = error));
                }
                cx.notify();
                return;
            }
        };
        let PreparedSubmission {
            workspace,
            checkpoint_warning,
            worktree_restored,
            driver: prepared_driver,
        } = prepared;
        // The turn began at accept time; it must still be the untouched one
        // this preparation belongs to. Cancellation is blocked while the
        // preparation set holds the session, so a mismatch means the session
        // was replaced under the preparation rather than a user action.
        let can_start = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .is_some_and(|session| {
                session.status == SessionStatus::Connecting
                    && session.turns.last().is_some_and(|turn| {
                        turn.status == TurnStatus::Running && !turn.provider_turn_started
                    })
            });
        if !can_start {
            self.submission_preparations.remove(&session_id);
            self.drain_pending_workspace_cleanups(cx);
            cx.notify();
            return;
        }

        let workspace_changed = self.state.session_mut(session_id).is_some_and(|session| {
            let changed = session.workspace != workspace;
            session.workspace = workspace;
            changed
        });
        if selected && (workspace_changed || worktree_restored) {
            self.invalidate_workspace_queries(cx);
            self.reload_clean_right_panel_file_editors(cx);
            self.ensure_right_panel_terminals(cx);
        }
        if selected && worktree_restored {
            self.show_toast(tr!("session.worktree_recreated"));
        }
        let driver = match prepared_driver {
            None => self
                .runtimes
                .get(&session_id)
                .map(|runtime| runtime.driver.clone())
                .ok_or_else(|| anyhow::anyhow!(tr!("errors.prepared_runtime_unavailable"))),
            Some(Ok(prepared)) => Ok(self.install_prepared_driver(session_id, prepared)),
            Some(Err(error)) => Err(error),
        };
        self.invalidate_checkpoint_refs();
        if let Some(runtime) = self.runtimes.get_mut(&session_id) {
            runtime
                .pending_events
                .retain(|event| matches!(event, DriverEvent::BackgroundWork(_)));
            runtime.pending_steers.clear();
            runtime.stream_remeasure_pending = false;
            runtime.stream_phase = None;
            runtime.pending_permission = None;
            runtime.pending_user_input = None;
            runtime.pending_computer_approval = None;
            runtime.last_active_at = Instant::now();
        }
        // The transcript already shows the turn — the prompt message, its
        // anchor, and the working indicator all landed at accept time. Only
        // preparation's own output surfaces here.
        if selected && let Some(warning) = checkpoint_warning {
            self.show_toast(warning);
        }
        let provider = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.provider)
            .unwrap_or(self.state.last_provider);
        // Provider syntax resolves here, at the seam between the transcript
        // and the transport. The user message keeps the typed slash form,
        // while templates expand and skills adopt provider-native syntax.
        // Claude's commands pass through untouched; its CLI owns expansion.
        let prompt = submission.prompt;
        let driver_prompt = self.resolve_provider_submission(provider, &prompt);
        // The turn and its user message landed at accept time. Their ids go
        // with the prompt so every other client attached to the runtime
        // mirrors the same rows instead of minting its own.
        let (turn_id, message_id) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(submitted_prompt_identity)
            .unwrap_or((None, None));
        let mut failed_to_start = false;
        match driver {
            Ok(driver) => {
                // The first prompt after a move into a worktree warns the
                // resumed thread that its recorded paths now name a stale
                // checkout. Provider-facing only — the transcript keeps the
                // user's text.
                let driver_prompt = match self
                    .state
                    .session_mut(session_id)
                    .and_then(AgentSession::take_workspace_move_notice)
                {
                    Some(notice) => format!("{notice}\n\n{driver_prompt}"),
                    None => driver_prompt,
                };
                driver.prompt(
                    driver_prompt,
                    turn_id,
                    message_id,
                    submission.hidden,
                    submission.attachments.clone(),
                );
            }
            Err(error) => {
                failed_to_start = true;
                let message = tr!("errors.start_agent", error = error);
                if let Some(session) = self.state.session_mut(session_id) {
                    session.status = SessionStatus::Failed;
                    session.push_message(MessageRole::Assistant, message);
                }
                self.finish_active_turn_with_analytics(
                    session_id,
                    TurnStatus::Failed,
                    crate::analytics::TurnOutcome::StartFailed,
                );
            }
        }
        // From this point onward `cancel_turn` has either a live driver to
        // cancel or a settled startup failure. The next frame must therefore
        // show Stop (or Send after failure), never the preparation spinner.
        self.submission_preparations.remove(&session_id);
        self.drain_pending_workspace_cleanups(cx);
        if failed_to_start {
            self.capture_latest_turn_checkpoint_for(session_id);
            self.start_pending_checkpoint_captures(cx);
        }
        cx.notify();
        // Persist on the next frame boundary. Saving is intentionally after
        // the spinner-to-Stop paint: SQLite or blob externalization must not
        // hold the final preparation frame motionless.
        cx.spawn(async move |waku, cx| {
            cx.background_executor().timer(STREAM_FRAME_INTERVAL).await;
            let _ = waku.update(cx, |waku, _| waku.save());
        })
        .detach();
    }

    pub(super) fn collect_runtime_events(runtime: &mut SessionRuntime) {
        while let Ok(event) = runtime.events.try_recv() {
            runtime.pending_events.push_back(event);
        }
    }

    pub(super) fn drain_event_pump(&mut self, cx: &mut Context<Self>) -> EventPumpSchedule {
        // `|` on purpose: a busy provider must not starve the other result
        // queues just because its own drain reported a change first.
        if self.drain_driver_events(cx)
            | self.drain_provider_probe_events()
            | self.drain_provider_version_events()
            | self.drain_provider_detection_events()
            | self.drain_computer_permission_events()
            | self.drain_plan_usage_events()
            | self.drain_task_state_sync_events(cx)
            | self.drain_daemon_settings_events(cx)
            | self.drain_friends_events(cx)
        {
            cx.notify();
        }
        if std::mem::take(&mut self.workspace_queries_stale) {
            self.invalidate_workspace_queries(cx);
        }
        if std::mem::take(&mut self.composer_sources_stale) {
            self.refresh_composer_sources(cx);
        }
        self.maybe_refresh_background_work(cx);
        // A finished turn asks for a checkpoint from a handler with no
        // `Context`; this is where that `git` work leaves the UI thread.
        self.start_pending_checkpoint_captures(cx);
        // Settling turns and drained detached work are also what archived
        // worktree cleanups wait on; re-check them on the same tick.
        self.drain_pending_workspace_cleanups(cx);

        if self
            .runtimes
            .values()
            .any(|runtime| !runtime.pending_events.is_empty() || runtime.stream_remeasure_pending)
        {
            EventPumpSchedule::StreamFrame
        } else if let Some(delay) = self.background_output_refresh_delay() {
            EventPumpSchedule::BackgroundOutput(delay)
        } else {
            EventPumpSchedule::Idle
        }
    }

    pub(super) fn drain_provider_probe_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(probe) = self.provider_probe_events.try_recv() {
            self.provider_model_discoveries_pending
                .remove(&probe.provider);
            if let Some(existing) = self
                .probes
                .iter_mut()
                .find(|existing| existing.provider == probe.provider)
            {
                *existing = probe;
            } else {
                self.probes.push(probe);
            }
            changed = true;
        }
        changed
    }

    pub(super) fn drain_computer_permission_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.computer_permission_events.try_recv() {
            self.computer_permission_request_pending = false;
            match result {
                Ok(permissions) => self.computer_permissions = permissions,
                Err(error) => self.show_toast(error),
            }
            changed = true;
        }
        changed
    }

    pub(super) fn drain_driver_events(&mut self, cx: &mut Context<Self>) -> bool {
        let session_ids = self.runtimes.keys().copied().collect::<Vec<_>>();
        let mut changed = false;
        let mut persisted_state_changed = false;
        let mut force_save = false;
        let mut selected_changed = false;
        for session_id in session_ids {
            let Some(mut runtime) = self.runtimes.remove(&session_id) else {
                continue;
            };
            let follow_up_remeasure = std::mem::take(&mut runtime.stream_remeasure_pending);
            Self::collect_runtime_events(&mut runtime);
            let mut runtime_changed = false;
            let mut background_changed = false;
            let mut markdown_changed = false;
            let mut keep_runtime = true;
            while let Some(event) = runtime.pending_events.front() {
                let kind = stream_delta_kind(event);
                let event = if let Some(kind) = kind {
                    pop_stream_batch(
                        &mut runtime.pending_events,
                        kind,
                        &mut runtime.pending_reasoning_newlines,
                    )
                } else {
                    runtime.pending_events.pop_front()
                };
                let Some(event) = event else {
                    break;
                };
                let background_event = matches!(event, DriverEvent::BackgroundWork(_));
                let background_output_delta = matches!(
                    event,
                    DriverEvent::BackgroundWork(BackgroundWorkEvent::OutputDelta { .. })
                );
                force_save |= matches!(
                    event,
                    DriverEvent::Connected { .. }
                        | DriverEvent::AgentPresetSelected(_)
                        | DriverEvent::AutoTitleUpdated(_)
                        | DriverEvent::Permission { .. }
                        | DriverEvent::PromptSubmitted { .. }
                        | DriverEvent::SteerAccepted { .. }
                        | DriverEvent::SteerRejected { .. }
                        | DriverEvent::TurnFinished { .. }
                        | DriverEvent::Error(_)
                        | DriverEvent::ProcessExited
                );
                // Reasoning is markdown too (the live peek renders it), and
                // this flag is also what routes the pump onto the coalesced
                // `StreamFrame` cadence: without it a reasoning-only drain
                // reported Idle, so every fast thinking chunk woke the pump
                // for an immediate drain-and-notify — 40+ full re-renders a
                // second, sailing straight past the 120 ms commit floor.
                markdown_changed |= matches!(
                    event,
                    DriverEvent::TextDelta(_) | DriverEvent::ReasoningDelta(_)
                );
                if background_output_delta {
                    // The registry batches log text into SharedString at 10Hz;
                    // repainting and saving for every provider chunk would
                    // turn a noisy command into UI-thread work.
                } else if background_event {
                    background_changed = true;
                } else {
                    runtime_changed = true;
                }
                keep_runtime &= self.handle_driver_event(session_id, &mut runtime, event, true, cx);
                if !keep_runtime {
                    break;
                }
            }
            runtime.stream_remeasure_pending = markdown_changed;
            if keep_runtime {
                self.runtimes.insert(session_id, runtime);
            }
            changed |= runtime_changed || background_changed;
            persisted_state_changed |= runtime_changed;
            if self.state.selected_session == Some(session_id)
                && (runtime_changed || follow_up_remeasure)
            {
                selected_changed = true;
            }
        }

        if !self.pending_queue_drains.is_empty() {
            let drains = std::mem::take(&mut self.pending_queue_drains);
            for session_id in drains {
                if self.ending_checkpoint_pending(session_id) {
                    self.defer_queue_drain(session_id);
                } else {
                    self.drain_queued_message(session_id, cx);
                }
            }
            changed = true;
        }

        if persisted_state_changed {
            self.stream_state_dirty = true;
        }
        if selected_changed {
            self.remeasure_transcript_tail();
        }
        if self.stream_state_dirty
            && (force_save || self.last_stream_save.elapsed() >= STREAM_SAVE_INTERVAL)
        {
            self.save();
        }
        changed || selected_changed
    }
}

#[cfg(test)]
mod response_fork_title_tests {
    use super::next_response_fork_title;

    #[test]
    fn response_fork_titles_advance_one_numbered_sequence() {
        assert_eq!(
            next_response_fork_title("Fix the bug", ["Fix the bug"]),
            "Fix the bug (2)"
        );
        assert_eq!(
            next_response_fork_title(
                "Fix the bug",
                ["Fix the bug", "Fix the bug (2)", "Fix the bug (4)"]
            ),
            "Fix the bug (5)"
        );
        assert_eq!(
            next_response_fork_title("Fix the bug (2)", ["Fix the bug", "Fix the bug (2)"]),
            "Fix the bug (3)"
        );
        assert_eq!(
            next_response_fork_title("Plan (2026)", ["Plan (2026)"]),
            "Plan (2026) (2)"
        );
    }
}

#[cfg(test)]
mod version_tests {
    use crate::model::parse_cli_version;

    #[test]
    fn parses_common_cli_version_banners() {
        assert_eq!(
            parse_cli_version("codex-cli 0.45.0\n"),
            Some("0.45.0".to_owned())
        );
        assert_eq!(
            parse_cli_version("2.1.24 (Claude Code)\n"),
            Some("2.1.24".to_owned())
        );
        assert_eq!(
            parse_cli_version("v1.3.0-beta.2"),
            Some("1.3.0-beta.2".to_owned())
        );
        assert_eq!(
            parse_cli_version("\nAmp CLI version 0.9.12\n"),
            Some("0.9.12".to_owned())
        );
        assert_eq!(parse_cli_version("not a version"), None);
        assert_eq!(parse_cli_version(""), None);
    }

    #[test]
    fn version_requires_a_dotted_number_not_a_bare_digit() {
        // "2024" alone or a hash must not read as a version.
        assert_eq!(parse_cli_version("build 2024 f3a9c1"), None);
        assert_eq!(
            parse_cli_version("cursor-agent 2025.09.12-4f8d8e2"),
            Some("2025.09.12-4f8d8e2".to_owned())
        );
    }
}
