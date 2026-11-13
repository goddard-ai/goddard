//! Portable chip data alongside the clipboard's ordinary text representation.

use gpui::ClipboardItem;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::model::AtomRefKind;

use crate::input::INLINE_ATOM_MARKER;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ClipboardAtom {
    PastedText {
        text: String,
        category: Option<String>,
    },
    SessionRef {
        session_id: Uuid,
        title: String,
    },
    Reference {
        kind: AtomRefKind,
        name: String,
        target: String,
    },
}

/// Byte offsets refer to marker characters in `text`, never painted labels.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChipContent {
    pub text: String,
    pub atoms: Vec<(usize, ClipboardAtom)>,
}

#[derive(Serialize, Deserialize)]
struct ChipMetadata {
    goddard_chips: u32,
    fallback: String,
    content: ChipContent,
}

impl ChipContent {
    pub fn clipboard_item(self, fallback: String) -> ClipboardItem {
        if self.atoms.is_empty() {
            return ClipboardItem::new_string(fallback);
        }
        ClipboardItem::new_string_with_json_metadata(
            fallback.clone(),
            ChipMetadata {
                goddard_chips: 1,
                fallback,
                content: self,
            },
        )
    }

    /// Reject unknown versions, stale metadata, and invalid byte offsets.
    pub fn from_clipboard(item: &ClipboardItem) -> Option<Self> {
        let metadata: ChipMetadata = serde_json::from_str(item.metadata()?).ok()?;
        if metadata.goddard_chips != 1 || item.text()? != metadata.fallback {
            return None;
        }
        let content = metadata.content;
        let markers = content
            .text
            .match_indices(INLINE_ATOM_MARKER)
            .map(|(offset, _)| offset);
        if content.atoms.is_empty()
            || !markers.eq(content.atoms.iter().map(|(offset, _)| *offset))
            || content.text.contains('\r')
        {
            return None;
        }
        Some(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_clipboard_rejects_stale_or_invalid_metadata() {
        let content = ChipContent {
            text: format!("é {INLINE_ATOM_MARKER}"),
            atoms: vec![(
                3,
                ClipboardAtom::PastedText {
                    text: "hidden\ntext".into(),
                    category: None,
                },
            )],
        };
        let item = content.clone().clipboard_item("fallback".into());
        assert_eq!(
            ChipContent::from_clipboard(&item).unwrap().atoms,
            content.atoms
        );
        let stale = ClipboardItem::new_string_with_metadata(
            "edited".into(),
            item.metadata().unwrap().clone(),
        );
        assert!(ChipContent::from_clipboard(&stale).is_none());
        let mut metadata: ChipMetadata = serde_json::from_str(item.metadata().unwrap()).unwrap();
        metadata.goddard_chips += 1;
        assert!(
            ChipContent::from_clipboard(&ClipboardItem::new_string_with_json_metadata(
                "fallback".into(),
                &metadata
            ))
            .is_none()
        );
        metadata.goddard_chips = 1;
        metadata.content.atoms[0].0 = 1; // Inside é, not at the marker's byte offset.
        assert!(
            ChipContent::from_clipboard(&ClipboardItem::new_string_with_json_metadata(
                "fallback".into(),
                &metadata
            ))
            .is_none()
        );
    }
}
