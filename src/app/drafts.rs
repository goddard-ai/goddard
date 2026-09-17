use super::*;

use crate::md::selection::{FileAnnotation, Span, TextKey};
use crate::persistence::{
    ComposerDraftAnnotation, ComposerDraftAnnotationSpan, ComposerDraftFileAnnotation,
};

const COMPOSER_DRAFT_SAVE_DELAY: Duration = Duration::from_millis(250);

impl From<&Span> for ComposerDraftAnnotationSpan {
    fn from(span: &Span) -> Self {
        Self {
            row: span.key.row.to_string(),
            index: span.key.index,
            start: span.range.start,
            end: span.range.end,
            text: span.text.to_string(),
            block_break: span.block_break,
        }
    }
}

impl From<ComposerDraftAnnotationSpan> for Span {
    fn from(span: ComposerDraftAnnotationSpan) -> Self {
        Self {
            key: TextKey::new(span.row, span.index),
            range: span.start..span.end,
            text: Rc::from(span.text),
            block_break: span.block_break,
        }
    }
}

impl From<&FileAnnotation> for ComposerDraftFileAnnotation {
    fn from(file: &FileAnnotation) -> Self {
        Self {
            path: file.path.clone(),
            start: file.range.start,
            end: file.range.end,
            start_line: file.start_line,
            end_line: file.end_line,
        }
    }
}

impl From<ComposerDraftFileAnnotation> for FileAnnotation {
    fn from(file: ComposerDraftFileAnnotation) -> Self {
        Self {
            path: file.path,
            range: file.start..file.end,
            start_line: file.start_line,
            end_line: file.end_line,
        }
    }
}

impl From<&TranscriptAnnotation> for ComposerDraftAnnotation {
    fn from(annotation: &TranscriptAnnotation) -> Self {
        Self {
            id: annotation.id,
            message_id: annotation.message_id,
            spans: annotation.spans.iter().map(Into::into).collect(),
            comment: annotation.comment.clone(),
            file: annotation.file.as_ref().map(Into::into),
        }
    }
}

impl From<ComposerDraftAnnotation> for TranscriptAnnotation {
    fn from(annotation: ComposerDraftAnnotation) -> Self {
        Self {
            id: annotation.id,
            message_id: annotation.message_id,
            spans: annotation.spans.into_iter().map(Into::into).collect(),
            comment: annotation.comment,
            file: annotation.file.map(Into::into),
        }
    }
}

impl From<&ComposerAttachment> for crate::persistence::ComposerDraftAttachment {
    fn from(attachment: &ComposerAttachment) -> Self {
        Self {
            path: attachment.path.clone(),
            mention: attachment.mention.clone(),
            name: attachment.name.to_string(),
            is_dir: attachment.is_dir,
            is_image: attachment.is_image,
            blob_reference: attachment.blob_reference.clone(),
            session_id: attachment.session_id,
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
            session_id: attachment.session_id,
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
            session_id: attachment.session_id,
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
            session_id: attachment.session_id,
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

    /// The session's annotations in draft form: the live transcript set,
    /// every file editor's pins, and restored file annotations still waiting
    /// on their editor — merged in creation order like `drain_annotations`.
    fn composer_draft_annotations(&self) -> Vec<ComposerDraftAnnotation> {
        let mut annotations = self.transcript_selection.annotations.borrow().items.clone();
        for editor in self.right_panel_file_editors.values() {
            annotations.extend(editor.annotations.borrow().items.iter().cloned());
        }
        annotations.extend(self.pending_file_annotations.values().flatten().cloned());
        annotations.sort_by_key(|annotation| annotation.id);
        annotations.iter().map(Into::into).collect()
    }

    /// Build the live composer's draft for `key`. Annotations belong to the
    /// session on screen, so a foreign slot — a Big Picture card or the
    /// overlay's new-task draft — keeps whatever its draft already holds
    /// rather than inheriting the live set.
    pub(super) fn current_composer_draft(
        &self,
        key: Option<crate::persistence::ComposerDraftKey>,
        cx: &App,
    ) -> crate::persistence::ComposerDraft {
        let annotations = if key == self.selected_composer_draft_key() {
            self.composer_draft_annotations()
        } else {
            key.and_then(|key| self.composer_drafts.get(key))
                .map(|draft| draft.annotations.clone())
                .unwrap_or_default()
        };
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
            annotations,
        }
    }

    /// Copy the visible composer into its in-memory slot. No I/O happens here;
    /// callers can use this on every real edit and on navigation boundaries.
    pub(super) fn capture_current_composer_draft(&mut self, cx: &App) -> bool {
        let Some(key) = self.composer_draft_key() else {
            return false;
        };
        let draft = self.current_composer_draft(Some(key), cx);
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
    pub(super) fn move_composer_draft_after_project_change(
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
        // Reusing an existing projectless draft resolves synchronously; a
        // freshly provisioned one lands in `create_projectless_session`'s
        // completion, which retargets the overlay there instead.
        if self.big_picture.is_open()
            && let Some(project_id) = self.selected_session().map(|session| session.project_id)
            && self
                .state
                .projects
                .iter()
                .any(|project| project.id == project_id && project.is_projectless())
            && self.big_picture.new_task_project != Some(project_id)
        {
            self.big_picture.new_task_project = Some(project_id);
            self.sync_big_picture_draft(cx);
        }
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
        let key = self.composer_draft_key();
        let draft = key
            .and_then(|key| self.composer_drafts.get(key))
            .cloned()
            .unwrap_or_default();
        self.apply_composer_draft(key, draft, cx);
    }

    /// Push a draft into the live composer — text and attachment chips alike.
    /// Annotations rejoin the live set only when `key` is the selected
    /// session's slot: a foreign draft's highlights keep living in the draft
    /// until its session comes on screen.
    pub(super) fn apply_composer_draft(
        &mut self,
        key: Option<crate::persistence::ComposerDraftKey>,
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
        if key == self.selected_composer_draft_key() {
            self.restore_draft_annotations(draft.annotations);
        }
        self.composer
            .update(cx, |input, cx| input.set_content(draft.text, cx));
        cx.notify();
    }

    /// Load a draft's annotations as the selected session's live set. The
    /// visible transcript items park under their owning session first —
    /// `reset_visible_state` performs the same swap, so a later reset is a
    /// no-op round trip. File annotations whose editor is already on screen
    /// seed its store; the rest wait in `pending_file_annotations` for the
    /// file to open.
    fn restore_draft_annotations(&mut self, draft_annotations: Vec<ComposerDraftAnnotation>) {
        let mut items = Vec::new();
        let mut pending: HashMap<String, Vec<TranscriptAnnotation>> = HashMap::new();
        for annotation in draft_annotations {
            self.annotation_next_id = self.annotation_next_id.max(annotation.id.saturating_add(1));
            let annotation = TranscriptAnnotation::from(annotation);
            if let Some(file) = &annotation.file {
                pending
                    .entry(file.path.clone())
                    .or_default()
                    .push(annotation);
            } else {
                items.push(annotation);
            }
        }
        {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            if let Some(owner) =
                std::mem::replace(&mut self.annotation_session, self.state.selected_session)
            {
                self.transcript_annotations
                    .insert(owner, std::mem::take(&mut annotations.items));
            }
            annotations.items = items;
            annotations.hovered = None;
            annotations.hovered_ref = None;
            annotations.editing = None;
        }
        if let Some(id) = self.state.selected_session {
            self.transcript_annotations.remove(&id);
        }
        for (path, editor) in self.right_panel_file_editors.iter_mut() {
            if let Some(annotations) = pending.remove(path) {
                editor.annotations.borrow_mut().items = annotations;
            }
        }
        self.pending_file_annotations = pending;
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
