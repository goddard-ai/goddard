//! The hand-authored Boss Brain wireframe is the format's proving
//! example: it must parse, satisfy the v1 invariants, and render every
//! screen to a well-formed SVG document.

use waku_protocol::wireframe::Wireframe;

const BOSS_BRAIN: &str = include_str!("fixtures/boss-brain-redesign.wireframe.json");

#[test]
fn boss_brain_example_parses_and_renders() {
    let wireframe = Wireframe::parse(BOSS_BRAIN).expect("boss brain example parses");
    assert_eq!(wireframe.screens.len(), 8);
    for screen in &wireframe.screens {
        let svg = screen.render_svg();
        assert!(
            svg.starts_with("<svg") && svg.ends_with("</svg>"),
            "{}: rendered document is not an SVG",
            screen.name
        );
        // Every screen carries the shared Brain chrome: the section strip
        // names all five sections (the task-page screen intentionally
        // lacks them).
        if screen.name != "Employee — task page" {
            for section in ["Memory", "Personas", "Employees", "Plans", "Deliverables"] {
                assert!(
                    svg.contains(&format!(">{section}</text>")),
                    "{}: section strip is missing {section}",
                    screen.name
                );
            }
        }
    }
}
