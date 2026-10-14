//! Recently submitted composer prompts and the command palette picker that
//! restores them.
//!
//! Every send path — the session column's composer, a side chat's lane, the
//! Big Picture overlay's — files its payload here just before the composer
//! clears, capped at the newest few. The "Restore recent prompt" chip in
//! the composer's chip row lists them; a pick stages the payload into the
//! live composer the same way a parked draft's "Use" does, with the same
//! workspace-level ⌘Z handing back whatever the composer held. The list
//! persists through `AppState`, so a restart keeps it.

use super::saved_drafts::DraftUseUndo;
use super::*;
use crate::persistence::ComposerDraft;

/// How many submitted prompts the restore menu keeps.
const RECENT_PROMPT_CAP: usize = 5;
/// The menu row's single-line label budget, in characters.
const RECENT_PROMPT_LABEL_CHARS: usize = 72;

/// Push a just-submitted payload onto `list`, newest first. An empty or
/// whitespace-only payload files nothing, and resubmitting the identical
/// payload keeps a single entry — the list is a recall aid, not a log.
fn push_recent_prompt(list: &mut Vec<ComposerDraft>, draft: ComposerDraft) -> bool {
    let mut draft = draft;
    // Annotations pin quotes and comments to the session they were sent to —
    // they cannot restore meaningfully anywhere else, so the history keeps
    // the typed prompt, not the comment header it carried.
    draft.annotations.clear();
    if draft.text.trim().is_empty() && draft.attachments.is_empty() && draft.inline_atoms.is_empty()
    {
        return false;
    }
    if list.first() == Some(&draft) {
        return false;
    }
    list.insert(0, draft);
    list.truncate(RECENT_PROMPT_CAP);
    true
}

/// The single-line label a menu row gets for a stored prompt: whitespace
/// runs collapsed, then an ellipsis cut at the label budget. A prompt that
/// was only attachments labels by their file names.
pub(super) fn recent_prompt_label(draft: &ComposerDraft) -> String {
    let text = if draft.text.trim().is_empty() {
        draft
            .attachments
            .iter()
            .map(|attachment| attachment.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        draft.text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let mut chars = text.chars();
    let label: String = chars.by_ref().take(RECENT_PROMPT_LABEL_CHARS).collect();
    if chars.next().is_some() {
        format!("{label}…")
    } else {
        label
    }
}

pub(super) fn recent_prompt_matches(draft: &ComposerDraft, query: &str) -> bool {
    query.is_empty()
        || recent_prompt_label(draft)
            .to_lowercase()
            .contains(&query.to_lowercase())
}

impl Waku {
    /// File a submission's payload into the rolling history. Call sites hand
    /// the draft captured just before the composer cleared — text as the
    /// user saw it, attachment chips, and live inline atoms.
    pub(super) fn record_recent_prompt(&mut self, draft: ComposerDraft) {
        if push_recent_prompt(&mut self.state.recent_prompts, draft) {
            self.save();
        }
    }

    /// Stage a picked prompt into the live composer — the same wholesale
    /// apply a parked draft's "Use" performs, with the slot updated
    /// immediately so the debounced autosave and the workspace ⌘Z undo see
    /// consistent state. The prompt stays in the history.
    pub(super) fn restore_recent_prompt(
        &mut self,
        draft: ComposerDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = self.composer_draft_key();
        if let Some(key) = key {
            // What the live composer holds now is the undo's hand-back —
            // captured before the apply overwrites it.
            let previous = self.current_composer_draft(Some(key), cx);
            self.push_draft_use_undo(DraftUseUndo::prompt_restore(
                key,
                draft.clone(),
                (!previous.is_empty()).then_some(previous),
            ));
            self.composer_drafts.set(key, draft.clone());
        }
        self.apply_composer_draft(key, draft, cx);
        self.schedule_composer_draft_save(cx);
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{ComposerDraftAnnotation, ComposerDraftAttachment};

    fn prompt(text: &str) -> ComposerDraft {
        ComposerDraft {
            text: text.to_owned(),
            ..ComposerDraft::default()
        }
    }

    fn attachment(name: &str) -> ComposerDraftAttachment {
        ComposerDraftAttachment {
            path: std::path::PathBuf::from(format!("/tmp/{name}")),
            mention: String::new(),
            name: name.to_owned(),
            is_dir: false,
            is_image: false,
            blob_reference: None,
            pasted_text_preview: None,
            session_id: None,
        }
    }

    #[test]
    fn push_keeps_newest_first_and_caps_the_list() {
        let mut list = Vec::new();
        for index in 0..RECENT_PROMPT_CAP + 3 {
            assert!(push_recent_prompt(
                &mut list,
                prompt(&format!("prompt {index}"))
            ));
        }
        let texts: Vec<&str> = list.iter().map(|draft| draft.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["prompt 7", "prompt 6", "prompt 5", "prompt 4", "prompt 3"]
        );
    }

    #[test]
    fn push_skips_empty_and_consecutive_duplicates() {
        let mut list = Vec::new();
        assert!(!push_recent_prompt(&mut list, prompt("   \n\t  ")));
        assert!(!push_recent_prompt(&mut list, ComposerDraft::default()));
        assert!(push_recent_prompt(&mut list, prompt("hello")));
        assert!(!push_recent_prompt(&mut list, prompt("hello")));
        // A repeat after a different submission is a fresh entry.
        assert!(push_recent_prompt(&mut list, prompt("other")));
        assert!(push_recent_prompt(&mut list, prompt("hello")));
        let texts: Vec<&str> = list.iter().map(|draft| draft.text.as_str()).collect();
        assert_eq!(texts, vec!["hello", "other", "hello"]);
    }

    #[test]
    fn push_strips_annotations_but_keeps_attachments() {
        let mut list = Vec::new();
        let mut draft = prompt("with a comment");
        draft.annotations = vec![ComposerDraftAnnotation {
            id: 1,
            message_id: Uuid::new_v4(),
            spans: Vec::new(),
            comment: "a comment".to_owned(),
            file: None,
            history: None,
        }];
        assert!(push_recent_prompt(&mut list, draft));
        assert!(list[0].annotations.is_empty());
        // Attachment-only submissions still record — the chips restore.
        let mut attachment_only = ComposerDraft::default();
        attachment_only.attachments = vec![attachment("a.rs")];
        assert!(push_recent_prompt(&mut list, attachment_only));
        assert_eq!(list[0].attachments.len(), 1);
    }

    #[test]
    fn label_collapses_whitespace_and_truncates() {
        let draft = prompt("  line one\nline\t two   ");
        assert_eq!(recent_prompt_label(&draft), "line one line two");

        let long = prompt(&"word ".repeat(40));
        let label = recent_prompt_label(&long);
        assert!(label.ends_with('…'));
        assert_eq!(label.chars().count(), RECENT_PROMPT_LABEL_CHARS + 1);

        let mut attachment_only = ComposerDraft::default();
        attachment_only.attachments = vec![attachment("a.rs"), attachment("b.rs")];
        assert_eq!(recent_prompt_label(&attachment_only), "a.rs, b.rs");
    }

    #[test]
    fn palette_query_matches_prompt_text_and_attachment_names() {
        let text = prompt("Explain the workspace");
        let mut attachment_only = ComposerDraft::default();
        attachment_only.attachments = vec![attachment("report.pdf")];

        assert!(recent_prompt_matches(&text, "WORKSPACE"));
        assert!(recent_prompt_matches(&attachment_only, "report"));
        assert!(!recent_prompt_matches(&text, "missing"));
    }
}
