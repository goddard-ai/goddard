//! Static HTML wireframe metadata. Parsing never loads resources or executes code.
//!
//! This is the authoring contract, not a browser sandbox. Consumers must also
//! isolate content, deny resource loading/navigation, and restrict their bridge.

use std::collections::HashSet;

use anyhow::{Result, bail};
use dom_query::{Document, NodeRef};
use sha2::{Digest, Sha256};

/// Maximum UTF-8 document size, checked before HTML parsing.
pub const MAX_HTML_BYTES: usize = 1_048_576;
/// Bound DOM work independently of the source byte limit.
pub const MAX_HTML_NODES: usize = 2_000;
/// Maximum element nesting, including the document root.
pub const MAX_HTML_DEPTH: usize = 64;
/// Maximum named screens in one proposal.
pub const MAX_HTML_SCREENS: usize = 64;
/// Base styles inherit the host's active theme through CSS custom properties.
pub const BASE_STYLES: &str = include_str!("wireframe_html/base.css");

/// A validated HTML wireframe. Fields are immutable outside this module.
#[derive(Clone, Debug, PartialEq)]
pub struct HtmlWireframe {
    id: String,
    name: String,
    screens: Vec<HtmlScreen>,
    html: String,
    revision: String,
}

/// A screen and the stable elements to which discussion can attach.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct HtmlScreen {
    pub id: String,
    pub name: String,
    pub width: f32,
    pub height: f32,
    pub anchors: Vec<HtmlAnchor>,
}

/// Stable anchor ID with a short human-readable fallback if the element disappears.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HtmlAnchor {
    pub id: String,
    pub label: String,
}

impl HtmlWireframe {
    /// Parse ordinary HTML, requiring versioned metadata and unambiguous anchors.
    ///
    /// # Errors
    /// Rejects oversized/deep documents, active content, invalid metadata,
    /// nested screens, duplicate identities, and invalid screen dimensions.
    pub fn parse(source: &str) -> Result<Self> {
        if source.len() > MAX_HTML_BYTES {
            bail!("wireframe exceeds the 1 MiB limit");
        }
        let document = Document::from(source);
        let mut stack = vec![(document.root(), 0usize, None::<usize>)];
        let mut nodes = 0usize;
        let mut screens: Vec<HtmlScreen> = Vec::new();
        let mut screen_ids = HashSet::new();
        let mut anchor_ids = HashSet::new();
        while let Some((node, depth, parent_screen)) = stack.pop() {
            nodes += 1;
            if nodes > MAX_HTML_NODES || depth > MAX_HTML_DEPTH {
                bail!(
                    "wireframe exceeds {MAX_HTML_NODES} DOM nodes or {MAX_HTML_DEPTH} nesting levels"
                );
            }
            reject_active_content(node)?;
            let mut screen = parent_screen;
            if let Some(id) = node.attr("data-wireframe-screen") {
                if parent_screen.is_some() {
                    bail!("wireframe screens cannot be nested");
                }
                validate_id(&id)?;
                if !screen_ids.insert(id.to_string()) {
                    bail!("duplicate wireframe screen ID: {id}");
                }
                if screens.len() >= MAX_HTML_SCREENS {
                    bail!("wireframe exceeds {MAX_HTML_SCREENS} screens");
                }
                screen = Some(screens.len());
                screens.push(HtmlScreen {
                    id: id.to_string(),
                    name: node
                        .attr("aria-label")
                        .map_or_else(|| id.to_string(), |s| s.chars().take(160).collect()),
                    width: dimension(node, "data-wireframe-width", 800.0)?,
                    height: dimension(node, "data-wireframe-height", 600.0)?,
                    anchors: Vec::new(),
                });
            }
            if let Some(id) = node.attr("data-wireframe-id") {
                validate_id(&id)?;
                let Some(index) = screen else {
                    bail!("anchor {id} must be inside a named wireframe screen");
                };
                if !anchor_ids.insert((index, id.to_string())) {
                    bail!("duplicate anchor ID {id} in screen {}", screens[index].id);
                }
                let label = node.attr("aria-label").unwrap_or_else(|| node.text());
                screens[index].anchors.push(HtmlAnchor {
                    id: id.to_string(),
                    label: label
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(160)
                        .collect(),
                });
            }
            stack.extend(
                node.children_it(true)
                    .map(|child| (child, depth + 1, screen)),
            );
        }
        let version = metadata(&document, "goddard-wireframe-version")?;
        if version != "1" {
            bail!("unsupported HTML wireframe version: {version}");
        }
        let id = metadata(&document, "goddard-wireframe-id")?;
        validate_id(&id)?;
        if screens.is_empty() {
            bail!("HTML wireframes require at least one named screen");
        }
        let title = document.select("head > title").text().to_string();
        let name = if title.trim().is_empty() {
            id.clone()
        } else {
            title.trim().chars().take(256).collect()
        };
        Ok(Self {
            id,
            name,
            screens,
            html: source.to_owned(),
            revision: format!("{:x}", Sha256::digest(source.as_bytes())),
        })
    }

    /// Stable document identity within its workspace.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Human-readable proposal title.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Screens in authored order.
    pub fn screens(&self) -> &[HtmlScreen] {
        &self.screens
    }
    /// Authored HTML to load only inside the restricted preview.
    pub fn html(&self) -> &str {
        &self.html
    }
    /// SHA-256 of the authored bytes, identifying the displayed revision.
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Look up an element without interpreting IDs as CSS selectors.
    pub fn anchor(&self, screen: &str, anchor: &str) -> Option<&HtmlAnchor> {
        self.screens
            .iter()
            .find(|s| s.id == screen)?
            .anchors
            .iter()
            .find(|a| a.id == anchor)
    }
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        bail!(
            "wireframe IDs must contain 1–128 ASCII letters, digits, dots, underscores, or hyphens"
        );
    }
    Ok(())
}

fn metadata(document: &Document, name: &str) -> Result<String> {
    let matching = document.select(&format!("head > meta[name='{name}']"));
    if matching.length() != 1 {
        bail!("expected exactly one {name} meta element in head");
    }
    matching
        .attr("content")
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing content for {name}"))
}

fn dimension(node: NodeRef<'_>, attr: &str, default: f32) -> Result<f32> {
    let Some(raw) = node.attr(attr) else {
        return Ok(default);
    };
    let value: f32 = raw
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid {attr}: {raw}"))?;
    if !value.is_finite() || value <= 0.0 || value > 16_384.0 {
        bail!("{attr} must be finite and between 0 and 16384 pixels");
    }
    Ok(value)
}

fn reject_active_content(node: NodeRef<'_>) -> Result<()> {
    if let Some(name) = node.node_name() {
        // Template contents have a separate fragment tree. Reject them rather
        // than letting inert fragments bypass the bounded traversal.
        if matches!(
            name.as_ref(),
            "script"
                | "iframe"
                | "frame"
                | "frameset"
                | "object"
                | "embed"
                | "base"
                | "link"
                | "template"
        ) {
            bail!("HTML wireframes do not support <{name}> elements");
        }
    }
    for attribute in node.attrs() {
        let name = attribute.name.local.as_ref();
        if name.starts_with("on") || matches!(name, "srcdoc" | "http-equiv") {
            bail!("HTML wireframes do not support {name} attributes");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPOSAL: &str = include_str!("../tests/fixtures/tasks.wireframe.html");

    #[test]
    fn screens_and_anchors_survive_normal_html_parsing() {
        let document = HtmlWireframe::parse(PROPOSAL).unwrap();
        assert_eq!(document.id(), "task-review");
        assert_eq!(document.screens().len(), 2);
        assert_eq!(document.screens()[0].width, 960.0);
        assert_eq!(document.screens()[1].width, 800.0);
        assert_eq!(
            document.anchor("tasks", "task-sidebar").unwrap().label,
            "Task navigation"
        );
        assert!(document.anchor("empty", "task-sidebar").is_some());
        assert!(document.anchor("tasks", "missing").is_none());
    }

    #[test]
    fn ambiguous_or_missing_identity_is_rejected() {
        for invalid in [
            PROPOSAL.replace("content=\"1\"", "content=\"2\""),
            PROPOSAL.replace("name=\"goddard-wireframe-id\"", "name=\"other\""),
            PROPOSAL.replace(
                "data-wireframe-screen=\"empty\"",
                "data-wireframe-screen=\"tasks\"",
            ),
            PROPOSAL.replace(
                "</aside>",
                "<span data-wireframe-id=\"task-sidebar\"></span></aside>",
            ),
            PROPOSAL.replace(
                "data-wireframe-id=\"task-sidebar\"",
                "data-wireframe-id=\"bad id\"",
            ),
            PROPOSAL.replace(
                "data-wireframe-width=\"960\"",
                "data-wireframe-width=\"NaN\"",
            ),
        ] {
            assert!(HtmlWireframe::parse(&invalid).is_err());
        }
    }

    #[test]
    fn active_content_and_unbounded_documents_are_rejected() {
        for payload in [
            "<script>alert(1)</script>",
            "<svg onload='alert(1)'></svg>",
            "<iframe srcdoc='x'></iframe>",
            "<template><script>x</script></template>",
            "<meta http-equiv='refresh' content='0;url=https://example.com'>",
        ] {
            assert!(
                HtmlWireframe::parse(&PROPOSAL.replace("</body>", &format!("{payload}</body>")))
                    .is_err()
            );
        }
        assert!(HtmlWireframe::parse(&" ".repeat(MAX_HTML_BYTES + 1)).is_err());
        let deep = format!(
            "{}{}",
            "<div>".repeat(MAX_HTML_DEPTH + 1),
            "</div>".repeat(MAX_HTML_DEPTH + 1)
        );
        assert!(
            HtmlWireframe::parse(&PROPOSAL.replace("</body>", &format!("{deep}</body>"))).is_err()
        );
        let many = "<br>".repeat(MAX_HTML_NODES + 1);
        assert!(
            HtmlWireframe::parse(&PROPOSAL.replace("</body>", &format!("{many}</body>"))).is_err()
        );
    }
}
