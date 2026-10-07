//! Display-only decoding of the reference tokens emitted by the composer.

use std::{borrow::Cow, sync::LazyLock};

use regex::Regex;
use waku_protocol::model::{
    AtomRefKind, MESSAGE_ATOM_END, MESSAGE_ATOM_OPEN, MESSAGE_ATOM_REF, encode_atom_session_id,
    escape_atom_label,
};

static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\[([a-z]+) "([^"\n\r\[\]]+)" \(([a-z_]+): ([^\n\r\[\]]+)\)\]"#)
        .expect("valid prompt reference regex")
});

pub(super) fn display(text: &str) -> Cow<'_, str> {
    // Existing display atoms already carry their label and identity. Leave
    // their contents alone, even if a label happens to resemble a token.
    let mut output = String::new();
    let mut copied = 0;
    let mut atom_end = 0;
    for captures in REFERENCE.captures_iter(text) {
        let token = captures.get(0).expect("whole reference match");
        let prefix = &text[atom_end..token.start().max(atom_end)];
        if let Some(open) = prefix.rfind(MESSAGE_ATOM_OPEN) {
            if !prefix[open..].contains(MESSAGE_ATOM_END) {
                atom_end = text[token.start()..]
                    .find(MESSAGE_ATOM_END)
                    .map_or(text.len(), |end| {
                        token.start() + end + MESSAGE_ATOM_END.len_utf8()
                    });
            }
        }
        if token.start() < atom_end {
            continue;
        }
        atom_end = token.end();
        let tag = &captures[1];
        let name = &captures[2];
        let key = &captures[3];
        let operand = &captures[4];
        if name.trim().is_empty()
            || operand.trim().is_empty()
            || name.chars().any(|ch| {
                matches!(ch, MESSAGE_ATOM_OPEN | MESSAGE_ATOM_END | MESSAGE_ATOM_REF)
                    || ('\u{FE00}'..='\u{FE0F}').contains(&ch)
            })
        {
            continue;
        }
        let metadata = if tag == "session" && key == "task_id" {
            let Ok(id) = uuid::Uuid::parse_str(operand) else {
                continue;
            };
            encode_atom_session_id(id)
        } else {
            let kind = match tag {
                "project" => AtomRefKind::Project,
                "persona" => AtomRefKind::Persona,
                "deliverable" => AtomRefKind::Deliverable,
                "bucket" => AtomRefKind::MemoryBucket,
                "automation" => AtomRefKind::Automation,
                "file" => AtomRefKind::MemoryFile,
                _ => continue,
            };
            if key != kind.operand_key() {
                continue;
            }
            format!("{MESSAGE_ATOM_REF}{}", kind.mark())
        };
        output.push_str(&text[copied..token.start()]);
        output.push(MESSAGE_ATOM_OPEN);
        output.push_str(&metadata);
        output.push_str(&escape_atom_label(name));
        output.push(MESSAGE_ATOM_END);
        copied = token.end();
    }
    if copied == 0 {
        Cow::Borrowed(text)
    } else {
        output.push_str(&text[copied..]);
        Cow::Owned(output)
    }
}
