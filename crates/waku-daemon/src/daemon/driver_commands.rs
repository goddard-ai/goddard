use super::*;

pub(super) fn handle_driver_command(
    driver: &DriverHandle,
    command: Command,
) -> anyhow::Result<ResponsePayload> {
    match command {
        Command::Prompt {
            prompt,
            attachments,
            ..
        } => driver.prompt_with_attachments(prompt, attachments),
        Command::Steer { prompt, .. } => driver.steer(prompt),
        Command::ClarifyUserInput {
            request_id,
            content,
        } => driver.clarify_user_input(request_id, content),
        Command::CancelUserInput { request_id } => driver.cancel_user_input(request_id),
        Command::Cancel => driver.cancel(),
        Command::CancelComputerUse => driver.cancel_computer_use(),
        Command::RefreshBackgroundWork => driver.refresh_background_work(),
        Command::StopBackgroundWork { key, control_id } => {
            driver.stop_background_work(
                serde_json::from_value(key).context("invalid background-work key")?,
                control_id,
            );
        }
        Command::Respond {
            request_id,
            option_id,
        } => driver.respond(request_id, option_id),
        Command::RespondUserInput {
            request_id,
            answers,
        } => driver.respond_user_input(request_id, answers),
        Command::Goal { operation } => driver.goal(operation),
        // Fire-and-forget like Goal: admission and the outcome arrive as
        // driver events, so the caller never waits on a response.
        Command::Compact => driver.compact(),
        Command::RunComputerTool { request } => {
            driver.run_computer_tool(crate::computer_use::ComputerToolRequest {
                call_id: request.call_id,
                tool: request.tool,
                arguments: request.arguments,
            });
        }
        Command::RejectComputerTool { request, reason } => {
            driver.reject_computer_tool(
                crate::computer_use::ComputerToolRequest {
                    call_id: request.call_id,
                    tool: request.tool,
                    arguments: request.arguments,
                },
                reason,
            );
        }
        Command::ApplyOptions { options } => {
            return Ok(ResponsePayload::OptionsApplied {
                applied: driver.apply_options(SessionOptions {
                    mode: decode_enum(&options.mode)?,
                    model: options.model,
                    reasoning_effort: options.reasoning_effort,
                    service_tier: options.service_tier,
                    context_window: options.context_window,
                }),
            });
        }
        Command::Rollback { turns } => {
            let cursor = driver
                .rollback(turns)?
                .map(serde_json::to_value)
                .transpose()?;
            return Ok(ResponsePayload::Cursor { cursor });
        }
        Command::Fork { turns_to_remove } => {
            let cursor = Some(serde_json::to_value(driver.fork(turns_to_remove)?)?);
            return Ok(ResponsePayload::Cursor { cursor });
        }
        Command::AttachSession
        | Command::ClaimManagedGoalTurn { .. }
        | Command::Start { .. }
        | Command::GetSettings
        | Command::UpdateSettings { .. }
        | Command::SetDaemonExposure { .. }
        | Command::ProbeProvider { .. }
        | Command::SandboxSignIn { .. }
        | Command::SandboxAuthStatus { .. }
        | Command::FetchPlanUsage { .. }
        | Command::ConsumeCodexResetCredit { .. }
        | Command::ProbeComputerPermissions { .. }
        | Command::Evaluate { .. }
        | Command::Transcribe { .. }
        | Command::GetWhistleStatus
        | Command::DownloadWhistleModel
        | Command::TestEvalConnection { .. }
        | Command::GetInferenceCredential { .. }
        | Command::RouteTask { .. }
        | Command::RecordRouteOverride { .. }
        | Command::RecordRouteClass { .. }
        | Command::LoadEvalUsage
        | Command::LoadUsageHistory { .. }
        | Command::LoadSkills { .. }
        | Command::SetSkillsEnabled { .. }
        | Command::TrashSkills { .. }
        | Command::LoadTaskState
        | Command::SaveTaskState { .. }
        | Command::RemoveSession
        | Command::RemoveProject { .. }
        | Command::HydrateSession { .. }
        | Command::IndexSession
        | Command::SearchSessionMessages { .. }
        | Command::ListProviderSessions { .. }
        | Command::LoadProviderSession { .. }
        | Command::LoadComposerDrafts
        | Command::SaveComposerDrafts { .. }
        | Command::ApplyComposerDraftChanges { .. }
        | Command::StoreBlob { .. }
        | Command::ImportAttachment { .. }
        | Command::ImportPathAttachment { .. }
        | Command::ReadBlob { .. }
        | Command::ReadAttachment { .. }
        | Command::SweepBlobs
        | Command::ForkSessionFromResponse { .. }
        | Command::RewindSessionToMessage { .. }
        | Command::ForkProviderSession { .. }
        | Command::Workspace { .. }
        | Command::OpenTerminal { .. }
        | Command::WriteTerminal { .. }
        | Command::ResizeTerminal { .. }
        | Command::CloseTerminal
        | Command::CloseSession
        | Command::AgentCreateSession { .. }
        | Command::AgentPrompt { .. }
        | Command::AgentRenameSelf { .. }
        | Command::AgentProposeArchive { .. }
        | Command::AgentMergeSubmit
        | Command::AgentReadSession { .. }
        | Command::AgentSearchSessions { .. }
        | Command::AgentProjectMap { .. }
        | Command::AgentAsk { .. }
        | Command::AgentResources { .. }
        | Command::AgentListModels
        | Command::AgentComputerUse { .. }
        | Command::AgentComputerUseReset
        | Command::CancelQueuedPrompt { .. }
        | Command::UpsertCustomCommand { .. }
        | Command::RemoveCustomCommand { .. }
        | Command::ListCustomCommands
        | Command::ListIntegrations
        | Command::ConnectIntegration { .. }
        | Command::SetIntegrationProviders { .. }
        | Command::DisconnectIntegration { .. }
        | Command::StartIntegrationAuth { .. }
        | Command::GetFriends
        | Command::GetPairing
        | Command::RespondPairRequest { .. }
        | Command::RevokePairedClient { .. }
        | Command::SendFriendRequest { .. }
        | Command::RespondFriendRequest { .. }
        | Command::WithdrawFriendRequest { .. }
        | Command::RemoveFriend { .. }
        | Command::SendFileToFriend { .. }
        | Command::SendMessageToFriend { .. }
        | Command::CancelTransfer { .. }
        | Command::ProbeFriend { .. }
        | Command::SetFriendDisplayName { .. }
        | Command::SetFriendNickname { .. }
        | Command::GetAutomations
        | Command::Boss { .. }
        | Command::GetDaemonStats
        | Command::UpsertAutomation { .. }
        | Command::RemoveAutomation { .. }
        | Command::RunAutomationNow { .. }
        | Command::ShareProjectWithFriend { .. }
        | Command::UnshareProjectWithFriend { .. }
        | Command::EnableFriendSync { .. }
        | Command::DisableFriendSync { .. }
        | Command::SetFriendSyncConfig { .. }
        | Command::FriendSyncNow { .. }
        | Command::FriendSyncAlertAction { .. }
        | Command::GetFriendSyncBranches { .. }
        | Command::SetFriendSessionSharing { .. }
        | Command::GetFriendSessions { .. }
        | Command::WatchFriendSession { .. }
        | Command::UnwatchFriendSession { .. } => {
            bail!("daemon received a command in the wrong dispatch path")
        }
    }
    Ok(ResponsePayload::Ack)
}

pub(super) fn ensure_shell_environment() {
    static REFRESHED: OnceLock<()> = OnceLock::new();
    REFRESHED.get_or_init(|| {
        crate::command_env::refresh_from_default_shell();
    });
}

/// Kick a build or refresh of one workspace's code index on a
/// background thread. Already-building roots are skipped; the index steps
/// out of the map while it parses so readers never wait on a refresh — a
/// missing entry just means "send unmapped" that turn.
pub(super) fn spawn_repo_map_refresh(repo_maps: &Arc<(Mutex<RepoMaps>, Condvar)>, root: PathBuf) {
    {
        let mut maps = repo_maps.0.lock();
        if !maps.building.insert(root.clone()) {
            return;
        }
    }
    let repo_maps = repo_maps.clone();
    let _ = std::thread::Builder::new()
        .name("goddard-repo-map".to_owned())
        .spawn(move || {
            let mut index = repo_maps.0.lock().indexes.remove(&root);
            let result = match index.as_mut() {
                Some(existing) => existing.refresh().map(|_| ()),
                None => crate::repo_map::RepoMapIndex::scan(&root).map(|built| index = Some(built)),
            };
            let mut maps = repo_maps.0.lock();
            match result {
                Ok(()) => {
                    if let Some(index) = index {
                        maps.indexes.insert(root.clone(), index);
                    }
                }
                Err(error) => {
                    eprintln!(
                        "goddard-daemon: project map scan failed for {}: {error:#}",
                        root.display()
                    );
                    if let Some(prior) = index {
                        maps.indexes.insert(root.clone(), prior);
                    }
                }
            }
            maps.building.remove(&root);
            repo_maps.1.notify_all();
        });
}
