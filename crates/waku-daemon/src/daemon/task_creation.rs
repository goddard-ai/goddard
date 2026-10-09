use super::*;

impl WakuBackend {
    /// `agent create`: build a fully configured task and start its first
    /// prompt. Mirrors the app's New Task defaults — the project must be an
    /// absolute path; an existing project resolves by it, and an unknown
    /// path registers only as a primary checkout.
    pub(super) fn agent_create_session(
        &self,
        sender: Option<Uuid>,
        selection: AgentCreateSelection,
        project: PathBuf,
        workspace: AgentWorkspace,
        base_branch: Option<String>,
        prompt: String,
        events: EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        self.require_agent_tools()?;
        if sender.is_some_and(|id| self.boss.is_managed(id)) {
            bail!("Boss roles summon employees through `goddard-agent boss`, not `create`");
        }
        let session_id = self.create_agent_task(
            sender,
            selection,
            project,
            workspace,
            base_branch,
            prompt,
            &events,
        )?;
        Ok(ResponsePayload::AgentSessionCreated { session_id })
    }

    /// The task-creation half of `agent create`, shared with the automation
    /// scheduler: the agent-tools credential gate is the only difference —
    /// the daemon's own scheduler needs no scoped token. A task an agent
    /// spawns also inherits its access posture — `runtime_mode` and run
    /// `environment` — so sandboxed or supervised work stays contained;
    /// senderless automation tasks take the defaults.
    pub(crate) fn create_agent_task(
        &self,
        sender: Option<Uuid>,
        selection: AgentCreateSelection,
        project: PathBuf,
        workspace: AgentWorkspace,
        base_branch: Option<String>,
        prompt: String,
        events: &EventSink,
    ) -> anyhow::Result<Uuid> {
        self.create_agent_task_inner(
            sender,
            selection,
            project,
            workspace,
            base_branch,
            AgentTaskPrompt::Fixed(prompt),
            events,
            None,
            None,
        )
    }

    pub(super) fn create_agent_task_inner(
        &self,
        sender: Option<Uuid>,
        selection: AgentCreateSelection,
        project: PathBuf,
        workspace: AgentWorkspace,
        base_branch: Option<String>,
        prompt: AgentTaskPrompt,
        events: &EventSink,
        employee: Option<waku_protocol::boss::BossEmployee>,
        planning: Option<waku_protocol::boss::BossPlan>,
    ) -> anyhow::Result<Uuid> {
        if prompt.is_blank() {
            bail!("agent sessions require a prompt");
        }
        if selection
            .title
            .as_deref()
            .is_some_and(|title| title.trim().is_empty())
        {
            bail!("agent task titles cannot be empty");
        }
        if !project.is_absolute() {
            bail!("the project path must be absolute");
        }
        if matches!(workspace, AgentWorkspace::Adopt) {
            bail!("adopt workspaces are summon-only — an agent-created task always starts fresh");
        }
        if matches!(workspace, AgentWorkspace::Worktree)
            && base_branch
                .as_deref()
                .is_none_or(|branch| branch.trim().is_empty())
        {
            bail!("worktree sessions require a base branch");
        }
        let project = dunce::canonicalize(&project)
            .with_context(|| format!("project path {} does not exist", project.display()))?;
        let resolved = self.resolve_agent_task_selection(
            sender,
            &selection,
            &project,
            prompt.text(),
            employee.is_some(),
        )?;
        let (project_id, project_path) = self.register_agent_project(&project)?;
        let mut session = AgentSession::new(project_id, resolved.provider);
        if let Some(title) = selection.title.as_deref() {
            session.set_title(title);
        }
        // Posture is stamped verbatim — an environment the resolved
        // provider cannot run (a sandbox guest or cloud it lacks) fails the
        // launch honestly rather than silently running the spawned work
        // somewhere less contained.
        session.runtime_mode = resolved.sender_mode;
        session.environment = resolved.sender_environment;
        session.model = resolved.model.clone();
        if let Some(run) = resolved.routed {
            session.route_decision = Some(run.decision);
        }
        session.reasoning_effort = resolved.reasoning_effort.clone();
        session.service_tier = resolved.service_tier.clone();
        session.context_window = resolved.context_window.clone();
        session.workspace = match workspace {
            AgentWorkspace::Local => SessionWorkspace::Local,
            AgentWorkspace::Worktree => {
                let created = crate::worktree::create(
                    &project_path,
                    None,
                    base_branch.as_deref(),
                    false,
                    &[],
                )?;
                SessionWorkspace::Worktree {
                    path: created.path,
                    name: created.name,
                    branch: None,
                    base_branch,
                    adopted_by: None,
                }
            }
            AgentWorkspace::Adopt => unreachable!("adopt is rejected above"),
        };
        let prompt = prompt.resolve(&session.workspace, &project_path);
        if let Some(employee) = &employee {
            session.id = employee.session_id;
            session.set_title(&employee.identity.name);
            session.agent_rename_allowed = false;
            // Origin rides the session, not the roster: a retired
            // employee's task stays out of the ordinary lists.
            session.boss_managed = true;
        }
        if let Some(plan) = &planning {
            session.id = plan.session_id;
            // Same stamp story as an employee: the planning kind marker
            // rides the session so a list row badges it without the Boss
            // document, and archiving cannot remove the freeze.
            session.boss_managed = true;
            session.planning = Some(plan.session_planning());
        }
        let session_id = session.id;
        let turn_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();
        session.adopt_submitted_prompt(&prompt, turn_id, message_id, sender, false, None);
        {
            let mut state = self.task_state.lock();
            state.push_session(session);
            self.task_store.save(&mut state)?;
        }
        if let Some(employee) = employee {
            // Publish the role only after its task exists, so a revision
            // subscriber can immediately open the new employee's transcript.
            self.boss.add_employee(employee)?;
        }
        if let Some(plan) = planning {
            // Same ordering as the roster push: the plan registers only
            // after its task exists, and a failed launch leaves both the
            // record and the session behind — retrying the prompt revives
            // it like any managed session.
            self.boss.add_plan(plan)?;
        }
        // The adopted prompt above already persisted, so a launch failure
        // still leaves a normal task behind. Delivering it now starts the
        // first turn immediately.
        self.launch_prepared_session(session_id, turn_id, message_id, prompt, sender, events)?;
        Ok(session_id)
    }

    /// The launch tail shared by `agent create` and summon dispatch:
    /// ensure the runtime, publish the adopted prompt into its event
    /// stream, then hand the provider the persona-wrapped text — clean
    /// first, with the session's context blocks following as a hidden
    /// steer or a prefix for providers without steer support.
    pub(super) fn launch_prepared_session(
        &self,
        session_id: Uuid,
        turn_id: Uuid,
        message_id: Uuid,
        prompt: String,
        sender: Option<Uuid>,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        let (runtime_id, driver) = self.ensure_agent_runtime(session_id, events)?;
        let sink = events.for_session(session_id, runtime_id);
        sink.send(event_to_wire(DriverEvent::PromptSubmitted {
            message: prompt.clone(),
            turn_id,
            message_id,
            sent_by_task: sender,
            hidden: false,
            report_trigger: None,
            reference_context: None,
        })?)?;
        if driver.supports_steer() {
            // The prompt reaches the provider exactly as typed — title
            // generation and first-prompt echoes stay clean — and the
            // session's context blocks follow as a hidden steer.
            driver.prompt(self.boss_outbound_prompt(session_id, prompt.clone()));
            self.steer_first_prompt_context(session_id, &prompt, &driver, &sink);
        } else {
            let prompt = self.prepend_agent_surface(session_id, &driver, prompt);
            let prompt = match self.session_memory_block(session_id, &driver) {
                Some(memory) => {
                    self.agent.mark_memory_prepended(session_id);
                    format!("{memory}\n\n{prompt}")
                }
                None => prompt,
            };
            let prompt = if self.boss.is_managed(session_id) {
                self.boss_outbound_prompt(session_id, prompt)
            } else {
                prompt
            };
            driver.prompt(prompt);
        }
        Ok(())
    }

    /// The project row an agent task runs in — an existing project resolves
    /// by its canonical path; an unknown path registers only as a primary
    /// checkout (linked worktrees can never be projects).
    pub(super) fn register_agent_project(&self, project: &Path) -> anyhow::Result<(Uuid, PathBuf)> {
        let mut state = self.task_state.lock();
        match state
            .projects
            .iter()
            .find(|existing| dunce::canonicalize(&existing.path).is_ok_and(|path| path == project))
            .map(|existing| (existing.id, existing.path.clone()))
        {
            Some(found) => Ok(found),
            None => {
                if crate::worktree::is_linked_worktree(project) {
                    bail!(
                        "{} is a Git worktree; only primary checkouts can be registered as projects",
                        project.display()
                    );
                }
                let registered = Project::from_path(project.to_path_buf());
                let found = (registered.id, registered.path.clone());
                state.projects.push(registered);
                self.task_store.save(&mut state)?;
                Ok(found)
            }
        }
    }

    /// Resolve one `agent create`/summon selection to a canonical provider
    /// plus the concrete model id the admission queue counts — routing,
    /// inheritance, and catalog defaults run here, never at dispatch.
    /// `strict_effort` rejects an explicit effort the resolved model's
    /// catalog does not list (summons and `setModel` are machine-written
    /// configuration; `agent create` keeps its pass-through).
    pub(super) fn resolve_agent_task_selection(
        &self,
        sender: Option<Uuid>,
        selection: &AgentCreateSelection,
        project: &Path,
        prompt_text: &str,
        strict_effort: bool,
    ) -> anyhow::Result<ResolvedAgentSelection> {
        // Fields the payload omits inherit the sending task's configuration,
        // but only while it runs the resolved provider — a different
        // provider's model and trait vocabularies may not carry over.
        let sender_config = sender.and_then(|id| {
            self.task_state
                .lock()
                .sessions
                .iter()
                .find(|session| session.id == id)
                .map(|session| {
                    (
                        session.provider,
                        session.model.clone(),
                        session.reasoning_effort.clone(),
                        session.service_tier.clone(),
                        session.context_window.clone(),
                        (session.runtime_mode, session.environment()),
                    )
                })
        });
        // `"auto"` hands provider and model selection to the same routing
        // pass `RouteTask` serves an Auto draft; when the eval backend
        // cannot answer it lands on the last-used target instead of
        // failing, so it never blocks a create.
        let routed = match selection.model.as_deref().map(str::trim) {
            Some("auto") => {
                if selection.provider.is_some() {
                    bail!("`model: \"auto\"` routes the provider too; omit `provider`");
                }
                Some(self.route_agent_task(project, prompt_text))
            }
            _ => None,
        };
        let provider = routed
            .as_ref()
            .map(|run| run.decision.target.provider)
            .or(selection.provider)
            .or(sender_config.as_ref().map(|config| config.0))
            .ok_or_else(|| {
                anyhow!("`provider` is required when no sending task is known to inherit from")
            })?;
        // Access posture is not provider vocabulary: a sandboxed or
        // full-access task's spawned work keeps its containment whatever
        // provider it resolves to.
        let (sender_mode, sender_environment) = sender_config
            .as_ref()
            .map(|config| config.5)
            .unwrap_or_default();
        let sender_config = sender_config.filter(|config| config.0 == provider);
        let model = match (&routed, selection.model.as_deref().map(str::trim)) {
            (Some(run), _) => run.decision.target.model.clone(),
            (None, Some("" | "default")) => None,
            (None, Some(model)) => Some(model.to_owned()),
            (None, None) => sender_config.as_ref().and_then(|config| config.1.clone()),
        };
        let (inherited_effort, inherited_tier, inherited_window) = sender_config
            .map(|config| (config.2, config.3, config.4))
            .unwrap_or_default();
        // The resolved model's catalog entry bounds which inherited traits
        // still apply; an empty or missing entry cannot constrain them.
        let catalog = crate::model_catalog::cached_models(provider)
            .unwrap_or_else(|| crate::model_catalog::fallback_models(provider));
        let catalog_model = match model.as_deref() {
            Some(requested) => {
                waku_protocol::model_catalog::packed_catalog_model(&catalog, requested, provider)
                    .map(|matched| matched.model)
            }
            None => catalog
                .iter()
                .find(|entry| entry.is_default)
                .or_else(|| catalog.first()),
        };
        // A summon's explicit effort is machine-written configuration like
        // `setModel`'s: fail the summon when the resolved model's catalog
        // lists efforts and does not include it, rather than silently run
        // the employee at another effort. `agent create` keeps its
        // pass-through for ids a stale catalog may not list yet.
        if strict_effort
            && let Some(effort) = selection.reasoning_effort.as_deref().map(str::trim)
            && !matches!(effort, "" | "default")
            && let Some(model) = catalog_model
            && !model.reasoning_efforts.is_empty()
            && !model
                .reasoning_efforts
                .iter()
                .any(|option| option.id == effort)
        {
            bail!(
                "reasoning effort {effort:?} is not supported by model {:?}",
                model.id
            );
        }
        let reasoning_effort = resolve_agent_trait(
            selection.reasoning_effort.clone().or_else(|| {
                routed
                    .as_ref()
                    .and_then(|run| run.decision.target.effort.clone())
            }),
            inherited_effort
                .map(|value| waku_protocol::model_catalog::normalize_reasoning_effort(&value)),
            catalog_model.map(|model| model.reasoning_efforts.as_slice()),
            catalog_model.and_then(|model| model.default_reasoning_effort.as_deref()),
        );
        let service_tier = resolve_agent_trait(
            selection.service_tier.clone(),
            inherited_tier,
            catalog_model.map(|model| model.service_tiers.as_slice()),
            catalog_model.and_then(|model| model.default_service_tier.as_deref()),
        );
        let context_window = resolve_agent_trait(
            selection.context_window.clone(),
            inherited_window,
            catalog_model.map(|model| model.context_windows.as_slice()),
            catalog_model.and_then(|model| model.default_context_window.as_deref()),
        );
        Ok(ResolvedAgentSelection {
            provider,
            model,
            concrete_model: catalog_model.map(|model| model.id.clone()),
            reasoning_effort,
            service_tier,
            context_window,
            routed,
            sender_mode,
            sender_environment,
        })
    }

    /// The session's own project root for memory scoping — `None` for
    /// incognito sessions (they get no project context at all) and for
    /// projectless tasks (there is no shared bucket to point at).
    pub(super) fn session_memory_project(&self, session_id: Uuid) -> Option<PathBuf> {
        let state = self.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;
        if session.incognito {
            return None;
        }
        let path = &state
            .projects
            .iter()
            .find(|project| project.id == session.project_id)?
            .path;
        (!crate::projectless::is_projectless_path(path)).then(|| path.clone())
    }

    /// The once-per-runtime `<project-memory>` block a task agent or
    /// employee is owed: how its `goddard-agent memory` surface works, then
    /// the project bucket's compacted overview when it has one. Sessions
    /// whose launch never carried the memory surface — Boss principals,
    /// incognito, projectless, experiment-off — owe nothing, and sessions
    /// whose provider never put the CLI within reach get neither the
    /// instructions nor the content.
    pub(super) fn session_memory_block(
        &self,
        session_id: Uuid,
        driver: &DriverHandle,
    ) -> Option<String> {
        if !self.agent.memory_owed(session_id)
            || !self.agent.memory_surface(session_id)
            || driver.agent_surface_delivery() == crate::driver::AgentSurfaceDelivery::Absent
        {
            return None;
        }
        let project = self.session_memory_project(session_id)?;
        // The access line reflects the session's live grants: every
        // project task reaches its shared bucket; an employee's extra
        // persona buckets widen it without changing the commands.
        let granted_buckets = self
            .boss
            .employee(session_id)
            .map(|employee| employee.permissions.bucket_ids.len())
            .unwrap_or(0);
        Some(project_memory_block(
            self.boss.project_memory_digest(&project).as_deref(),
            granted_buckets,
        ))
    }

    /// The session's first prompt already went out clean; its context
    /// blocks — the project map, project memory, and enabled tool guidance —
    /// follow as a hidden steer so provider title generation never sees them.
    /// Delivery
    /// confirms on the `steerAccepted` echo: memory's injected flag is set
    /// there, and a rejected steer leaves the session eligible so the next
    /// prompt retries.
    pub(super) fn steer_first_prompt_context(
        &self,
        session_id: Uuid,
        _task: &str,
        driver: &DriverHandle,
        _sink: &EventSink,
    ) {
        if self.agent.context_steer_pending(session_id) {
            return;
        }
        let parent_index = self.side_chat_parent_block(session_id);
        let memory = self.session_memory_block(session_id, driver);
        let computer_use_available = self
            .sessions
            .lock()
            .get(&session_id)
            .is_some_and(|entry| entry.computer_use_available);
        let computer_use = computer_use_available
            .then(|| {
                crate::computer_use::skill_root_path().ok().map(|root| {
                    driver::computer_use_hint(&root.join("goddard-computer-use/SKILL.md"))
                })
            })
            .flatten();
        let block = [
            parent_index.clone(),
            memory.clone(),
            computer_use,
            self.agent_surface_block(session_id, driver),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");
        if block.is_empty() {
            return;
        }
        // The index rides this steer — an accept settles it, a rejection
        // leaves the flag so the next prompt retries.
        if parent_index.is_some() {
            self.agent.note_index_steer(session_id);
        }
        // Same for the project-memory block.
        if memory.is_some() {
            self.agent.note_memory_steer(session_id);
        }
        // The steer lands as a user message mid-turn — frame the blocks as
        // context so the provider does not read them as a new instruction.
        let steer = format!(
            "Session context — background information only, not a new \
             instruction. Continue the task you are already working on.\n\n{block}"
        );
        self.agent.record_pending_steer(
            session_id,
            crate::agent::AgentPrompt {
                prompt: steer.clone(),
                transport: None,
                sender: None,
                queued_id: None,
                context: Some(if memory.is_some() {
                    crate::agent::ContextSteer::Memory
                } else {
                    crate::agent::ContextSteer::Blocks
                }),
                hidden: false,
                report_trigger: None,
            },
        );
        driver.steer(steer);
    }

    /// The daemon-owned memory store: session transcript indexes and
    /// project memory sit beside the boss scope under one file-canonical
    /// root.
    /// A side chat's context block: the parent task's user messages verbatim
    /// plus a per-turn cue index — extractive pointers, never a summary —
    /// snapshot at the side chat's first prompt. The snapshot inserts into
    /// the parent's session scope and the block renders through the
    /// engine's handoff under the side chat's own grant; a store that
    /// cannot take it falls back to the identical inline render. When the
    /// session's launch carried the `goddard-agent` surface the block names
    /// the `read` invocations that reach the parent's current text; without
    /// it the index degrades to plain context. `None` for ordinary sessions
    /// and parents with nothing to show.
    pub(super) fn side_chat_parent_block(&self, session_id: Uuid) -> Option<String> {
        if !self.agent.parent_index_owed(session_id) {
            return None;
        }
        let parent = {
            let mut state = self.task_state.lock();
            let parent_id = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)?
                .side_chat_of?;
            let index = state
                .sessions
                .iter()
                .position(|session| session.id == parent_id)?;
            self.task_store.hydrate(&mut state.sessions[index]).ok()?;
            state.sessions[index].clone()
        };
        let groups = parent.transcript_index();
        if groups.is_empty() {
            return None;
        }
        let (body, range) = crate::model::render_transcript_index(&groups);
        let body = body.trim_end().to_owned();
        let read_note = if self.agent.has_surface(session_id) {
            format!(
                " — a snapshot taken now; `goddard-agent read \
                 '{{\"task_id\":\"{}\"}}'` always returns its current text, and \
                 `goddard-agent read '{{\"task_id\":\"{}\",\"turn\":N}}'` returns \
                 one turn's full messages and tool output.",
                parent.id, parent.id
            )
        } else {
            String::from(".")
        };
        Some(format!(
            "This session is a side chat of the task \"{}\"; its transcript is \
             indexed below{read_note}\n\n<goddard-session-context source=\"{}\" \
             kind=\"index\"{range}>\n{body}</goddard-session-context>",
            parent.display_title(),
            parent.provider.id(),
        ))
    }

    /// The launch-scoped `goddard-agent` instruction for a session whose
    /// provider delivered the surface without telling the model — `None`
    /// when the driver announced it natively or the launch env never
    /// arrived.
    pub(super) fn agent_surface_block(
        &self,
        session_id: Uuid,
        driver: &DriverHandle,
    ) -> Option<String> {
        if driver.agent_surface_delivery() != crate::driver::AgentSurfaceDelivery::Silent {
            return None;
        }
        self.agent.surface_block(session_id)
    }

    /// Fold the owed agent-surface instruction into a first prompt — the
    /// non-steer path's single shot at telling the session about
    /// `goddard-agent`, so delivery is marked as the prompt goes out.
    pub(super) fn prepend_agent_surface(
        &self,
        session_id: Uuid,
        driver: &DriverHandle,
        prompt: String,
    ) -> String {
        let Some(surface) = self.agent_surface_block(session_id, driver) else {
            return prompt;
        };
        self.agent.mark_surface_announced(session_id);
        format!("{surface}\n\n{prompt}")
    }
}

/// The `<project-memory>` block: what the bucket is for and how the
/// session's `goddard-agent memory` surface reaches it, then the bucket's
/// compacted overview — never a raw note dump — when notes exist.
fn project_memory_block(digest: Option<&str>, granted_buckets: usize) -> String {
    let access = if granted_buckets == 0 {
        "Your access: this project's shared bucket. `buckets` is \
         authoritative for your current grants — each listed bucket \
         supports reading and recording."
            .to_owned()
    } else {
        format!(
            "Your access: this project's shared bucket plus {granted_buckets} \
             additional granted bucket(s). `buckets` is authoritative for \
             your current grants — each listed bucket supports reading and \
             recording."
        )
    };
    let mut block = format!(
        "<project-memory>\nThis project has a shared memory bucket — durable \
         notes recorded by the boss, employees, and tasks working here \
         survive across sessions. Reach it through `goddard-agent memory`: \
         `overview` returns a bucket's compacted view, `scan QUERY` matches \
         note text, `zoom START END` expands a note range, `record \
         --json|--json-file` appends a note, `summary` answers a pending \
         compression request, and `buckets` lists every bucket you can use. \
         Memory is append-only — corrections are new notes and a repeated \
         retry key never duplicates one; bucket creation and legacy import \
         are Boss-only. {access} Record durable facts, decisions, and \
         gotchas a future session would need — not progress on the current \
         task. A rejected operation reports its reason and persists \
         nothing; a failed `record` is not saved anywhere else.",
    );
    match digest {
        Some(digest) => {
            block.push_str("\n\nOverview so far — the project bucket only:\n");
            block.push_str(digest);
        }
        None => block.push_str("\n\nNo notes are recorded yet in the project bucket."),
    }
    block.push_str("\n</project-memory>");
    block
}
