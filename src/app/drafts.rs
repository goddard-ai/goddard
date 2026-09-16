use super::*;

const COMPOSER_DRAFT_SAVE_DELAY: Duration = Duration::from_millis(250);

impl From<&ComposerAttachment> for crate::persistence::ComposerDraftAttachment {
    fn from(attachment: &ComposerAttachment) -> Self {
        Self {
            path: attachment.path.clone(),
            mention: attachment.mention.clone(),
            name: attachment.name.to_string(),
            is_dir: attachment.is_dir,
            is_image: attachment.is_image,
            blob_reference: attachment.blob_reference.clone(),
        }
    }
}

impl From<crate::persistence::ComposerDraftAttachment> for ComposerAttachment {
    fn from(attachment: crate::persistence::ComposerDraftAttachment) -> Self {
        Self {
            path: attachment.path,
            client_preview_image: None,
            mention: attachment.mention,
            name: SharedString::from(attachment.name),
            is_dir: attachment.is_dir,
            is_image: attachment.is_image,
            blob_reference: attachment.blob_reference,
        }
    }
}

impl From<ComposerAttachment> for MessageAttachment {
    fn from(attachment: ComposerAttachment) -> Self {
        Self {
            path: attachment.path,
            mention: attachment.mention,
            name: attachment.name.to_string(),
            is_dir: attachment.is_dir,
            is_image: attachment.is_image,
            blob_reference: attachment.blob_reference,
        }
    }
}

impl From<MessageAttachment> for ComposerAttachment {
    fn from(attachment: MessageAttachment) -> Self {
        Self {
            path: attachment.path,
            client_preview_image: None,
            mention: attachment.mention,
            name: SharedString::from(attachment.name),
            is_dir: attachment.is_dir,
            is_image: attachment.is_image,
            blob_reference: attachment.blob_reference,
        }
    }
}

impl Waku {
    pub(super) fn selected_composer_draft_key(
        &self,
    ) -> Option<crate::persistence::ComposerDraftKey> {
        self.state
            .selected_session
            .and_then(|selected| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == selected)
            })
            .map(crate::persistence::ComposerDraftKey::for_session)
    }

    /// The draft slot the live composer is editing right now. While Big
    /// Picture is open the composer belongs to the overlay's armed target —
    /// a card's session or the standing new-task project — and the overlay
    /// tracks which key it loaded the current text from, because a capture
    /// must file under the outgoing key, not wherever state has moved to.
    pub(super) fn composer_draft_key(&self) -> Option<crate::persistence::ComposerDraftKey> {
        if self.big_picture.is_open() {
            return self.big_picture.draft_key;
        }
        self.selected_composer_draft_key()
    }

    pub(super) fn current_composer_draft(
        &self,
        cx: &App,
    ) -> crate::persistence::ComposerDraft {
        crate::persistence::ComposerDraft {
            // Collapsed paste blocks have no draft slot of their own — the
            // shared schema is just text — so they fold in here and come back
            // as ordinary inline text on restore.
            text: super::composer::prompt_with_pasted_blocks(
                self.composer.read(cx).content(cx),
                &self.composer_pasted_blocks,
            ),
            attachments: self
                .composer_attachments
                .iter()
                .map(crate::persistence::ComposerDraftAttachment::from)
                .collect(),
        }
    }

    /// Copy the visible composer into its in-memory slot. No I/O happens here;
    /// callers can use this on every real edit and on navigation boundaries.
    pub(super) fn capture_current_composer_draft(&mut self, cx: &App) -> bool {
        let Some(key) = self.composer_draft_key() else {
            return false;
        };
        let draft = self.current_composer_draft(cx);
        self.composer_drafts.set(key, draft)
    }

    pub(super) fn capture_and_save_current_composer_draft(&mut self, cx: &mut Context<Self>) {
        if self.capture_current_composer_draft(cx) {
            self.schedule_composer_draft_save(cx);
        }
    }

    /// A project choice in the composer changes where the current unsent task
    /// will run; it is not ordinary task navigation. Carry its draft into a
    /// blank destination instead of letting session activation clear it.
    fn move_composer_draft_after_project_change(
        &mut self,
        source: Option<crate::persistence::ComposerDraftKey>,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = source else {
            return;
        };
        let Some(destination) = self.composer_draft_key() else {
            return;
        };
        if self.composer_drafts.move_to_empty(source, destination) {
            self.restore_selected_composer_draft(cx);
            self.schedule_composer_draft_save(cx);
        }
    }

    pub(super) fn select_project_from_composer(
        &mut self,
        project_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let source = self.composer_draft_key();
        self.select_project(project_id, cx);
        if self.big_picture.is_open() {
            // The composer card's project picker is Big Picture's destination
            // control: the untargeted draft follows the choice.
            self.big_picture.new_task_project = Some(project_id);
            self.sync_big_picture_draft(cx);
        }
        self.move_composer_draft_after_project_change(source, cx);
    }

    pub(super) fn create_projectless_session_from_composer(&mut self, cx: &mut Context<Self>) {
        let source = self.composer_draft_key();
        self.create_projectless_session(cx);
        self.move_composer_draft_after_project_change(source, cx);
    }

    /// Submission consumes the active draft before a blank session gains a
    /// durable session identity, so its project-scoped text cannot reappear
    /// the next time the user opens New Task.
    pub(super) fn discard_current_composer_draft(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.composer_draft_key() else {
            return;
        };
        if self.composer_drafts.remove(key) {
            self.schedule_composer_draft_save(cx);
        }
    }

    pub(super) fn remove_composer_draft(
        &mut self,
        key: crate::persistence::ComposerDraftKey,
        cx: &mut Context<Self>,
    ) {
        if self.composer_drafts.remove(key) {
            self.schedule_composer_draft_save(cx);
        }
    }

    /// Replace the global input entity with the newly selected task's draft.
    /// The lookup is entirely in memory; attachments carry their cached file
    /// metadata so a session switch never stats their paths.
    pub(super) fn restore_selected_composer_draft(&mut self, cx: &mut Context<Self>) {
        let draft = self
            .composer_draft_key()
            .and_then(|key| self.composer_drafts.get(key))
            .cloned()
            .unwrap_or_default();
        self.apply_composer_draft(draft, cx);
    }

    /// Push a draft into the live composer — text and attachment chips alike.
    pub(super) fn apply_composer_draft(
        &mut self,
        draft: crate::persistence::ComposerDraft,
        cx: &mut Context<Self>,
    ) {
        self.composer_attachments = draft
            .attachments
            .into_iter()
            .map(ComposerAttachment::from)
            .collect();
        // The previous target's blocks already folded into its draft text;
        // a restored draft carries them inline, not as cards.
        self.composer_pasted_blocks.clear();
        self.composer
            .update(cx, |input, cx| input.set_content(draft.text, cx));
        cx.notify();
    }

    /// Debounce disk traffic while keeping serialization and filesystem I/O on
    /// the background executor. Generation checks also make detached timers
    /// and out-of-order writes harmless.
    pub(super) fn schedule_composer_draft_save(&mut self, cx: &mut Context<Self>) {
        self.composer_draft_save_generation = self.composer_draft_save_generation.saturating_add(1);
        let generation = self.composer_draft_save_generation;
        cx.spawn(async move |waku, cx| {
            cx.background_executor()
                .timer(COMPOSER_DRAFT_SAVE_DELAY)
                .await;
            let Some((store, drafts)) = waku
                .update(cx, |waku, cx| {
                    if waku.composer_draft_save_generation != generation {
                        return None;
                    }
                    waku.capture_current_composer_draft(cx);
                    Some((
                        waku.composer_draft_store.clone(),
                        waku.composer_drafts.clone(),
                    ))
                })
                .ok()
                .flatten()
            else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { store.save(drafts, generation) })
                .await;
            if let Err(error) = result {
                let _ = waku.update(cx, |waku, cx| {
                    waku.show_toast(tr!("errors.save_local_state", error = error));
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
