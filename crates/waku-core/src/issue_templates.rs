//! Issue template discovery for the new-issue flow.
//!
//! Reads `.github/ISSUE_TEMPLATE` straight from the checkout — a linked
//! worktree's own templates apply — plus the legacy single-file
//! `ISSUE_TEMPLATE.md` locations GitHub still honors. Markdown templates
//! feed the form's prefills; YAML form templates and `config.yml` contact
//! links are picker rows that open on the web, which is the only place
//! GitHub renders them.

use std::fs;
use std::path::Path;

use waku_protocol::workspace::{IssueTemplate, IssueTemplateKind};

/// The legacy single-file template, in GitHub's precedence order.
const LEGACY_TEMPLATE_FILES: [&str; 3] = [
    ".github/ISSUE_TEMPLATE.md",
    "docs/ISSUE_TEMPLATE.md",
    "ISSUE_TEMPLATE.md",
];

/// The checkout's template entries plus `config.yml`'s blank-issue toggle
/// (`true` when the file is absent). An empty list means "no templates —
/// skip the picker step".
pub fn list(cwd: &Path) -> (Vec<IssueTemplate>, bool) {
    let mut blank_issues_enabled = true;
    let mut entries = Vec::new();
    let mut contact_links = Vec::new();

    let directory = cwd.join(".github").join("ISSUE_TEMPLATE");
    let mut names = Vec::new();
    if let Ok(read) = fs::read_dir(&directory) {
        for entry in read.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    names.sort_by_key(|name| name.to_lowercase());

    for name in &names {
        let lower = name.to_lowercase();
        let path = directory.join(name);
        if lower == "config.yml" || lower == "config.yaml" {
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            let (blank, links) = template_config(&contents);
            if let Some(blank) = blank {
                blank_issues_enabled = blank;
            }
            contact_links = links;
        } else if lower.ends_with(".md") {
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            entries.push(markdown_template(name, &contents));
        } else if lower.ends_with(".yml") || lower.ends_with(".yaml") {
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            entries.push(yaml_form_template(name, &contents));
        }
    }

    if entries
        .iter()
        .all(|entry| entry.kind != IssueTemplateKind::Markdown)
    {
        for candidate in LEGACY_TEMPLATE_FILES {
            let Ok(contents) = fs::read_to_string(cwd.join(candidate)) else {
                continue;
            };
            let filename = Path::new(candidate)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| candidate.to_owned());
            entries.push(markdown_template(&filename, &contents));
            break;
        }
    }

    entries.extend(contact_links);
    (entries, blank_issues_enabled)
}

fn markdown_template(filename: &str, contents: &str) -> IssueTemplate {
    let mut name = None;
    let mut about = None;
    let mut title_prefix = None;
    let mut labels = Vec::new();
    let mut assignees = Vec::new();
    let body = crate::frontmatter::parse_frontmatter_fields(contents, |key, value| match key {
        "name" => name = Some(value),
        "about" | "description" => about = Some(value),
        "title" => title_prefix = Some(value),
        "labels" => labels = split_name_list(&value),
        "assignees" => assignees = split_name_list(&value),
        _ => {}
    });
    IssueTemplate {
        name: name.unwrap_or_else(|| filename_stem(filename)),
        about,
        title_prefix,
        labels,
        assignees,
        body: body.trim().to_owned(),
        filename: filename.to_owned(),
        url: None,
        kind: IssueTemplateKind::Markdown,
    }
}

/// A YAML form's picker row. Only its scalar metadata is read — the `body`
/// field list is a github.com form schema, not a Markdown starting text.
fn yaml_form_template(filename: &str, contents: &str) -> IssueTemplate {
    let mut template = IssueTemplate {
        name: filename_stem(filename),
        about: None,
        title_prefix: None,
        labels: Vec::new(),
        assignees: Vec::new(),
        body: String::new(),
        filename: filename.to_owned(),
        url: None,
        kind: IssueTemplateKind::YamlForm,
    };
    let Ok(fields) = serde_saphyr::from_str::<serde_json::Map<String, serde_json::Value>>(contents)
    else {
        return template;
    };
    for (key, value) in fields {
        let Some(value) = crate::frontmatter::frontmatter_value(value) else {
            continue;
        };
        match key.as_str() {
            "name" => template.name = value,
            "description" | "about" => template.about = Some(value),
            "title" => template.title_prefix = Some(value),
            "labels" => template.labels = split_name_list(&value),
            "assignees" => template.assignees = split_name_list(&value),
            _ => {}
        }
    }
    template
}

/// `config.yml`'s `blank_issues_enabled` flag and `contact_links` entries.
fn template_config(contents: &str) -> (Option<bool>, Vec<IssueTemplate>) {
    let Ok(fields) = serde_saphyr::from_str::<serde_json::Map<String, serde_json::Value>>(contents)
    else {
        return (None, Vec::new());
    };
    let blank = fields
        .get("blank_issues_enabled")
        .and_then(serde_json::Value::as_bool);
    let links = fields
        .get("contact_links")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|link| {
            let name = link.get("name")?.as_str()?.to_owned();
            let url = link.get("url")?.as_str()?.to_owned();
            let about = link
                .get("about")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            Some(IssueTemplate {
                name,
                about,
                title_prefix: None,
                labels: Vec::new(),
                assignees: Vec::new(),
                body: String::new(),
                filename: String::new(),
                url: Some(url),
                kind: IssueTemplateKind::ContactLink,
            })
        })
        .collect();
    (blank, links)
}

fn filename_stem(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(filename)
        .to_owned()
}

/// `labels: bug, help wanted` and the `[a, b]` bracket form
/// `parse_frontmatter_fields` gives YAML arrays both come back as comma
/// text — split either spelling.
fn split_name_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(trimmed);
    inner
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_frontmatter_feeds_the_prefills() {
        let template = markdown_template(
            "bug-report.md",
            "---\nname: Bug report\nabout: Something broke\ntitle: \"[BUG] \"\n\
             labels: [bug, needs triage]\nassignees: octocat\n---\n\n## What happened?\n\n## Expected\n",
        );
        assert_eq!(template.kind, IssueTemplateKind::Markdown);
        assert_eq!(template.name, "Bug report");
        assert_eq!(template.about.as_deref(), Some("Something broke"));
        assert_eq!(template.title_prefix.as_deref(), Some("[BUG]"));
        assert_eq!(template.labels, ["bug", "needs triage"]);
        assert_eq!(template.assignees, ["octocat"]);
        assert_eq!(template.body, "## What happened?\n\n## Expected");
    }

    #[test]
    fn yaml_form_reads_scalar_metadata_only() {
        let template = yaml_form_template(
            "bug.yml",
            "name: Bug report\ndescription: File a bug\ntitle: \"[Bug]: \"\nlabels: [bug]\n\
             body:\n  - type: textarea\n    id: what\n",
        );
        assert_eq!(template.kind, IssueTemplateKind::YamlForm);
        assert_eq!(template.name, "Bug report");
        assert_eq!(template.about.as_deref(), Some("File a bug"));
        assert_eq!(template.title_prefix.as_deref(), Some("[Bug]:"));
        assert_eq!(template.labels, ["bug"]);
        assert!(template.body.is_empty());
    }

    #[test]
    fn config_reads_blank_toggle_and_contact_links() {
        let (blank, links) = template_config(
            "blank_issues_enabled: false\ncontact_links:\n  - name: Security\n    \
             url: https://example.com/security\n    about: Report privately\n",
        );
        assert_eq!(blank, Some(false));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, IssueTemplateKind::ContactLink);
        assert_eq!(links[0].name, "Security");
        assert_eq!(
            links[0].url.as_deref(),
            Some("https://example.com/security")
        );
        assert_eq!(links[0].about.as_deref(), Some("Report privately"));
    }

    #[test]
    fn name_lists_split_comma_and_bracket_forms() {
        assert_eq!(split_name_list("bug, help wanted"), ["bug", "help wanted"]);
        assert_eq!(
            split_name_list("[bug, needs triage]"),
            ["bug", "needs triage"]
        );
        assert_eq!(split_name_list("bug"), ["bug"]);
        assert!(split_name_list("").is_empty());
    }
}
