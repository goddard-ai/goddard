//! The hand-authored Boss Brain wireframe is the format's proving
//! example: it must parse, satisfy the v1 invariants, and carry the
//! section strip every screen draws. Rendering lives in the app — this
//! test asserts the tree the element renderer walks.

use waku_protocol::wireframe::{WireNode, Wireframe};

const BOSS_BRAIN: &str = include_str!("fixtures/boss-brain-redesign.wireframe.json");

/// Every literal the screen shows, from `text` runs and `rect` labels —
/// what the preview would read out, independent of how it paints.
fn collect_labels(node: &WireNode, out: &mut Vec<String>) {
    match node {
        WireNode::Frame(frame) => {
            for child in &frame.children {
                collect_labels(child, out);
            }
        }
        WireNode::Text(text) => out.push(text.text.clone()),
        WireNode::Rect(rect) => {
            if let Some(label) = &rect.label {
                out.push(label.clone());
            }
        }
        WireNode::Divider(_) => {}
    }
}

#[test]
fn boss_brain_example_parses() {
    let wireframe = Wireframe::parse(BOSS_BRAIN).expect("boss brain example parses");
    assert_eq!(wireframe.screens.len(), 8);
    for screen in &wireframe.screens {
        assert!(
            screen.width.is_finite() && screen.width > 0.0,
            "{}: invalid width",
            screen.name
        );
        // Every screen carries the shared Brain chrome: the section strip
        // names all five sections (the task-page screen intentionally
        // lacks them).
        if screen.name != "Employee — task page" {
            let mut labels = Vec::new();
            collect_labels(&screen.root, &mut labels);
            for section in ["Memory", "Personas", "Employees", "Plans", "Deliverables"] {
                assert!(
                    labels.iter().any(|label| label == section),
                    "{}: section strip is missing {section}",
                    screen.name
                );
            }
        }
    }
}
