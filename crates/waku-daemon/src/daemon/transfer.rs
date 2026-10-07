use super::*;

/// Storage-layout migrations belong to the daemon because both the database
/// rows and the directories name paths on its host. Persist after each move
/// so a later failure cannot leave an earlier project pointing at its old
/// location in SQLite.
pub(super) fn migrate_projectless_state(
    task_store: &StateStore,
    task_state: &mut PersistedState,
) -> anyhow::Result<()> {
    let indices = task_state
        .projects
        .iter()
        .enumerate()
        .filter_map(|(index, project)| {
            crate::projectless::needs_migration(&project.path).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in indices {
        let old_path = task_state.projects[index].path.clone();
        let workspace = crate::projectless::migrate_workspace(&old_path).with_context(|| {
            format!(
                "could not move projectless workspace {} under ~/.goddard/projects",
                old_path.display()
            )
        })?;
        task_state.projects[index].name = crate::model::Project::PROJECTLESS_NAME.to_owned();
        task_state.projects[index].path = workspace.cwd;
        task_store
            .save(task_state)
            .context("could not persist migrated projectless workspace")?;
    }
    Ok(())
}

/// Materialize a transfer's agent session: a task under the sender's
/// friend project whose first messages are the sender's note and the
/// receipt — peer, title, and where the files landed — rendered as
/// assistant messages, not a sent bubble. The session stays quarantined
/// (idle, no provider turn started) until the user chooses to trust it.
pub(super) fn create_transfer_session(
    task_state: &Arc<Mutex<PersistedState>>,
    task_store: &Arc<StateStore>,
    share_dir: &Path,
    transfer: &waku_protocol::friends::TransferInfo,
    peer_name: &str,
) -> anyhow::Result<Uuid> {
    let Some(dest_dir) = &transfer.dest_dir else {
        bail!("incoming transfer completed without a destination");
    };
    let mut state = task_state.lock();
    let project_id = friends_project_id(&mut state, share_dir);
    rename_friend_sessions(&mut state, &transfer.peer_id, peer_name);
    // Match New task: inherit the remembered provider and let new_session
    // carry its model and related provider defaults into the delivery task.
    let provider = state.last_provider;
    let mut session = state.new_session(project_id, provider);
    // The sender owns the title; the row labels them by name.
    session.title = transfer.title.clone();
    session.friend_peer_id = Some(transfer.peer_id.clone());
    session.friend_peer_name = Some(peer_name.to_owned());
    // The receipt is a notification, not a prompt awaiting a reply — a
    // provider turn holds the assistant messages so they render like an
    // agent reply (bot style), not a sent bubble. The sender's note rides
    // above the delivery details.
    session.begin_provider_turn();
    if let Some(note) = transfer.note.as_deref().filter(|note| !note.is_empty()) {
        session.push_message(crate::model::MessageRole::Assistant, note);
    }
    let payload = transfer_payload_path(dest_dir, transfer.payload_name());
    session.push_notice_message(
        crate::model::MessageRole::Assistant,
        format!(
            "{} sent you \"{}\".\n\nFiles are in {}\n\nThe files have not been opened or executed — decide whether you trust them before asking me to work with them.",
            peer_name,
            transfer.payload_name(),
            dest_dir.display()
        ),
        transfer_receipt_notice(transfer, &payload, peer_name),
    );
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;
    session.quarantined = true;
    // Received files stay in the sandbox VM even once trusted — the agent
    // never works on them with this Mac's filesystem in reach.
    session.environment = crate::model::SessionEnvironment::Sandbox;
    // Sandbox isolation is the safety boundary for received files, so the
    // task starts with full autonomy inside that boundary.
    session.runtime_mode = waku_protocol::model::RuntimeMode::FullAccess;
    let session_id = session.id;
    state.push_session(session);
    task_store.save(&mut state)?;
    Ok(session_id)
}

/// Where a received transfer's payload sits: `dest_dir`/`title` when it
/// landed under its own name, the destination folder itself otherwise —
/// the same resolution the transfer's Finder link applies.
pub(super) fn transfer_payload_path(dest_dir: &Path, title: &str) -> PathBuf {
    let payload = dest_dir.join(title);
    if payload.symlink_metadata().is_ok() {
        payload
    } else {
        dest_dir.to_path_buf()
    }
}

/// The structured half of a transfer receipt — what the payload is and, for
/// a folder, its immediate children — resolved once at delivery so
/// transcript renderers never touch the filesystem. The message's `content`
/// still carries the plain-text receipt for the model and clients that
/// predate the variant.
pub(super) fn transfer_receipt_notice(
    transfer: &waku_protocol::friends::TransferInfo,
    payload: &Path,
    peer_name: &str,
) -> crate::model::TranscriptNotice {
    let metadata = std::fs::metadata(payload).ok();
    let is_dir = metadata.as_ref().is_some_and(|meta| meta.is_dir());
    let (entries, entry_count) = if is_dir {
        transfer_manifest_entries(payload)
    } else {
        (Vec::new(), 0)
    };
    crate::model::TranscriptNotice::TransferReceived {
        peer_name: peer_name.to_owned(),
        title: transfer.payload_name().to_owned(),
        path: payload.to_path_buf(),
        is_dir,
        is_image: !is_dir
            && waku_protocol::attachments::is_image_file_name(transfer.payload_name()),
        size_bytes: if is_dir {
            transfer.bytes_total
        } else {
            metadata.map_or(transfer.bytes_total, |meta| meta.len())
        },
        entries,
        entry_count,
    }
}

/// A folder payload's immediate children, directories first — capped at
/// [`crate::model::TRANSFER_MANIFEST_ENTRIES_CAP`] with the true total
/// alongside so a truncated listing still reports what it hides.
pub(super) fn transfer_manifest_entries(
    dir: &Path,
) -> (Vec<crate::model::TransferManifestEntry>, u64) {
    let mut entries: Vec<crate::model::TransferManifestEntry> = std::fs::read_dir(dir)
        .map(|read| {
            read.flatten()
                .map(|entry| {
                    let metadata = entry.metadata().ok();
                    crate::model::TransferManifestEntry {
                        name: entry.file_name().to_string_lossy().into_owned(),
                        is_dir: metadata.as_ref().is_some_and(|meta| meta.is_dir()),
                        size_bytes: metadata.map_or(0, |meta| meta.len()),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let entry_count = entries.len() as u64;
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries.truncate(crate::model::TRANSFER_MANIFEST_ENTRIES_CAP);
    (entries, entry_count)
}

/// The pooled "Friends" project transfer and chat sessions live in —
/// marked so the sidebar shows the friends glyph instead of a folder.
/// Found by the marker, or a legacy project at the share dir gets
/// adopted; registered on first delivery.
pub(super) fn friends_project_id(state: &mut PersistedState, share_dir: &Path) -> Uuid {
    match state
        .projects
        .iter_mut()
        .find(|project| project.is_friends() || project.path == share_dir)
    {
        Some(project) => {
            project.name = "Friends".to_owned();
            project.kind = Some(ProjectKind::Friends);
            project.friend_peer_id = None;
            project.id
        }
        None => {
            let mut project = Project::from_path(share_dir.to_path_buf());
            project.name = "Friends".to_owned();
            project.kind = Some(ProjectKind::Friends);
            let id = project.id;
            state.projects.push(project);
            id
        }
    }
}

/// Resolve a boss `project` reference — a registered project's name or id,
/// or its root path — to its catalog row. Per-project knobs persist on the
/// registered project, so an unregistered path is an error rather than an
/// ad hoc project.
pub(super) fn registered_project_mut<'a>(
    state: &'a mut PersistedState,
    reference: &str,
) -> anyhow::Result<&'a mut Project> {
    if let Ok(id) = Uuid::parse_str(reference)
        && let Some(position) = state.projects.iter().position(|project| project.id == id)
    {
        return Ok(&mut state.projects[position]);
    }
    // An ad-hoc temporary project can share the registered project's
    // basename — a boss reference names the registered row.
    if let Some(position) = state
        .projects
        .iter()
        .position(|project| !project.temporary && project.name.eq_ignore_ascii_case(reference))
        .or_else(|| {
            state
                .projects
                .iter()
                .position(|project| project.name.eq_ignore_ascii_case(reference))
        })
    {
        return Ok(&mut state.projects[position]);
    }
    let reference_path = PathBuf::from(reference);
    anyhow::ensure!(
        reference_path.is_absolute(),
        "project `{reference}` is not a registered project"
    );
    let canonical = dunce::canonicalize(&reference_path)
        .with_context(|| format!("project `{}` does not exist", reference_path.display()))?;
    // A path inside the registered root or a linked worktree of its
    // repository still names the project — the same ownership rule
    // `project_for_path` applies to session working directories.
    let owned = project_for_path(&state.projects, &canonical).map(|project| project.id);
    state
        .projects
        .iter_mut()
        .find(|project| Some(project.id) == owned)
        .ok_or_else(|| {
            anyhow!(
                "project `{}` is not registered with the daemon",
                reference_path.display()
            )
        })
}

/// The registered project owning `cwd` — a checkout beneath the
/// registered root, a repo-subdirectory project's own path, or a linked
/// worktree of the project's repository. `None` for paths the catalog
/// does not own.
pub(super) fn project_for_path<'a>(projects: &'a [Project], cwd: &Path) -> Option<&'a Project> {
    let cwd = dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    if let Some(project) = projects.iter().find(|project| {
        let root = dunce::canonicalize(&project.path).unwrap_or_else(|_| project.path.clone());
        cwd.starts_with(&root)
    }) {
        return Some(project);
    }
    let common = crate::worktree::git_common_dir(&cwd)?;
    projects.iter().find(|project| {
        crate::worktree::git_common_dir(&project.path).is_some_and(|dir| dir == common)
    })
}

/// Fields only the daemon writes: the submissions opt-in and QA-branch
/// override land through boss ops, the special-project markers through
/// friend delivery, and `resolved_name` through name disambiguation — it is
/// never serialized to a client at all. A client's `Project` literal
/// carries defaults for all of them, so a client save must not write them.
pub(super) fn preserve_daemon_project_fields(existing: &Project, incoming: &mut Project) {
    incoming.submissions_enabled = existing.submissions_enabled;
    incoming.qa_branch = existing.qa_branch.clone();
    incoming.kind = existing.kind;
    incoming.friend_peer_id = existing.friend_peer_id.clone();
    incoming.resolved_name = existing.resolved_name.clone();
}

/// Refresh a friend's display name across their delivered sessions —
/// called on each delivery and when a nickname changes so the row label
/// follows the name the user knows them by. Returns whether anything
/// changed.
pub(super) fn rename_friend_sessions(
    state: &mut PersistedState,
    peer_id: &str,
    name: &str,
) -> bool {
    let mut changed = false;
    for session in state.sessions.iter_mut() {
        if session.friend_peer_id.as_deref() == Some(peer_id)
            && session.friend_peer_name.as_deref() != Some(name)
        {
            session.friend_peer_name = Some(name.to_owned());
            changed = true;
        }
    }
    changed
}

/// An incoming chat message materializes like a delivered transfer — a
/// session in the pooled Friends project with the text rendered as an
/// agent reply and the sender's title. There is nothing to trust or hand
/// off, so unlike a transfer it is neither quarantined nor sandboxed —
/// just an idle chat.
pub(super) fn create_chat_session(
    task_state: &Arc<Mutex<PersistedState>>,
    task_store: &Arc<StateStore>,
    share_dir: &Path,
    delivery: &crate::share::ChatDelivery,
) -> anyhow::Result<Uuid> {
    let mut state = task_state.lock();
    let project_id = friends_project_id(&mut state, share_dir);
    rename_friend_sessions(&mut state, &delivery.peer_id, &delivery.peer_name);
    let mut session = state.new_session(project_id, state.last_provider);
    session.title = delivery.title.clone();
    session.friend_peer_id = Some(delivery.peer_id.clone());
    session.friend_peer_name = Some(delivery.peer_name.clone());
    session.begin_provider_turn();
    session.push_message(crate::model::MessageRole::Assistant, delivery.text.clone());
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;
    let session_id = session.id;
    state.push_session(session);
    task_store.save(&mut state)?;
    Ok(session_id)
}
