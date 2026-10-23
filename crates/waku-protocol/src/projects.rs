//! Canonical registered-project discovery and selection.
//!
//! Boss, planning, and human callers read the daemon-wide registered
//! catalog through `boss projects list` and pin one project to its
//! canonical identity with `boss projects resolve` or the typed
//! [`crate::boss::BossOperation::Projects`] operation. A selector names a
//! project by explicit `id`, exact `name` ignoring ASCII letter case, or
//! absolute registered-root `path`; bare text classifies by shape so a
//! project named like a UUID or a path stays selectable through the
//! kinded fields. Resolution never picks a first match: duplicate names
//! and equivalent roots are ambiguous, unknown selections fail, and
//! temporary registrations accept only an explicit id — preserving the
//! legacy behavior a direct UUID selection always had.
//!
//! Discovery is advisory and read-only. It registers nothing, repairs
//! nothing, and grants no project or memory access; chain acceptance
//! revalidates the resolved project against the live catalog.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// One registered project's canonical identity — the record discovery
/// returns and a chain step binds.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalProject {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    /// Ad-hoc registration picked for one task; it leaves the catalog once
    /// no live session references it, so name and path selectors refuse it.
    pub temporary: bool,
}

impl CanonicalProject {
    pub fn of(project: &crate::model::Project) -> Self {
        Self {
            id: project.id,
            name: project.name.clone(),
            path: project.path.clone(),
            temporary: project.temporary,
        }
    }
}

/// How a caller selects a registered project. Bare text classifies by
/// shape — UUID text selects the id, absolute or separator-bearing text a
/// registered root, anything else an exact name — while the kinded map
/// selects explicitly so a project spelled like a UUID or a path stays
/// selectable. At most one of `id`, `name`, `path` may be set.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(untagged)]
pub enum ProjectSelector {
    Bare(String),
    Kinded(ProjectSelectorFields),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSelectorFields {
    /// Registered project UUID text — parsed during resolution so a
    /// malformed value reports `project_selector_invalid` with context
    /// rather than a bare schema error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    /// Exact registered name, ignoring ASCII letter case only — no
    /// trimming, substring, fuzzy, or Unicode case-fold matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// Absolute registered project root; relative paths and `~` reject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
}

/// The stable machine-readable codes selection failures carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSelectionErrorCode {
    ProjectAmbiguous,
    ProjectUnknown,
    ProjectSelectorInvalid,
    ProjectTemporary,
}

/// A project selection failure: a stable code, the submitted selector,
/// the affected chain step when applicable, a human-readable explanation,
/// the concrete next action, and bounded candidate records.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSelectionError {
    pub code: ProjectSelectionErrorCode,
    /// The submitted selection, echoed as it arrived.
    pub selector: String,
    /// The 1-based chain step the failure came from — `None` for
    /// standalone resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub step: Option<usize>,
    /// Human-readable explanation of the mismatch.
    pub message: String,
    /// The concrete action that resolves the failure.
    pub next_action: String,
    /// Accessible candidate records — the ambiguity set for
    /// `project_ambiguous`, the rejected row for `project_temporary`,
    /// empty otherwise. Bounded; `truncated` marks omission.
    pub candidates: Vec<CanonicalProject>,
    #[serde(default)]
    pub truncated: bool,
}

impl ProjectSelectionError {
    /// Mark the failure as coming from a chain step — `step` is the
    /// definition's 0-based index and renders 1-based in the message.
    pub fn at_step(mut self, step: usize) -> Self {
        self.step = Some(step + 1);
        self.message = format!("Step {}: {}", step + 1, self.message);
        self
    }
}

impl fmt::Display for ProjectSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)?;
        for candidate in &self.candidates {
            write!(
                formatter,
                "\n  {} — {} (id {}){}",
                candidate.name,
                candidate.path.display(),
                candidate.id,
                if candidate.temporary {
                    " — temporary registration"
                } else {
                    ""
                },
            )?;
        }
        if self.truncated {
            formatter.write_str(
                "\n  …additional candidates omitted; `goddard-agent boss projects list` returns the complete catalog",
            )?;
        }
        write!(formatter, " {}", self.next_action)
    }
}

impl std::error::Error for ProjectSelectionError {}

/// Read-only catalog operations — `boss projects list` and
/// `boss projects resolve`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProjectOperation {
    /// Every registered project in the daemon-wide catalog, temporary
    /// rows labeled.
    List,
    /// Resolve one selector to its canonical registered record.
    Resolve { selector: ProjectSelector },
}

/// Candidates an ambiguous selection carries before `truncated` marks
/// the remainder — name collisions realistically stay tiny; the cap is a
/// safety rail, not expected behavior.
const CANDIDATE_CAP: usize = 32;

/// Resolve `selector` against the registered catalog. Exactly one
/// project answers a name or path selection — every match counts before
/// any is chosen, and ambiguity is reported before the temporary check.
/// An explicit id preserves the legacy direct-lookup behavior, including
/// temporary registrations.
pub fn resolve_project(
    projects: &[crate::model::Project],
    selector: &ProjectSelector,
) -> Result<CanonicalProject, ProjectSelectionError> {
    match classify(selector)? {
        Kind::Id(id, echoed) => projects
            .iter()
            .find(|project| project.id == id)
            .map(|project| CanonicalProject::of(project))
            .ok_or_else(|| ProjectSelectionError {
                code: ProjectSelectionErrorCode::ProjectUnknown,
                selector: echoed,
                step: None,
                message: format!("unknown registered project ID '{id}'"),
                next_action: "Use project discovery to obtain the canonical \
                              ID — `goddard-agent boss projects list` or \
                              `boss projects resolve`. Memory bucket IDs are \
                              separate identifiers."
                    .into(),
                candidates: Vec::new(),
                truncated: false,
            }),
        Kind::Name(name, echoed) => select(
            projects
                .iter()
                .filter(|project| project.name.eq_ignore_ascii_case(&name))
                .collect(),
            echoed,
            format!("project name '{name}'"),
            format!("no registered project named '{name}'"),
            "Run `goddard-agent boss projects list` and use an exact name, \
             a registered root path, or an ID.",
        ),
        Kind::Path(path, echoed) => {
            if !path.is_absolute() {
                return Err(ProjectSelectionError {
                    code: ProjectSelectionErrorCode::ProjectSelectorInvalid,
                    selector: echoed,
                    step: None,
                    message: format!(
                        "project path '{}' is not an absolute registered root",
                        path.display()
                    ),
                    next_action: "Relative paths and `~` are never expanded \
                                  here — supply the absolute registered \
                                  project root shown by `goddard-agent boss \
                                  projects list`."
                        .into(),
                    candidates: Vec::new(),
                    truncated: false,
                });
            }
            let normalized_input = normalize(&path);
            let canonical_input = dunce::canonicalize(&normalized_input).ok();
            let matches = projects
                .iter()
                .filter(|project| {
                    let root = normalize(&project.path);
                    let canonical_root = dunce::canonicalize(&root).ok();
                    match (&canonical_input, &canonical_root) {
                        (Some(input), Some(root)) if input == root => true,
                        _ => normalized_input == root,
                    }
                })
                .collect();
            select(
                matches,
                echoed,
                format!("registered root '{}'", path.display()),
                format!("no project is registered at '{}'", path.display()),
                "Supply a registered project root from `goddard-agent boss \
                 projects list`, or register it through the normal project \
                 workflow. Subdirectories and unregistered worktrees do not \
                 identify a project.",
            )
        }
    }
}

/// `List` output ordering: name-first, ASCII case-insensitive, id as the
/// tiebreak so duplicate names sort stably.
pub fn catalog(projects: &[crate::model::Project]) -> Vec<CanonicalProject> {
    let mut catalog: Vec<_> = projects.iter().map(CanonicalProject::of).collect();
    catalog.sort_by(|a, b| {
        (a.name.to_ascii_lowercase(), &a.path, a.id).cmp(&(
            b.name.to_ascii_lowercase(),
            &b.path,
            b.id,
        ))
    });
    catalog
}

/// The normalized selection kind plus the submitted text the error echoes.
enum Kind {
    Id(Uuid, String),
    Name(String, String),
    Path(PathBuf, String),
}

fn classify(selector: &ProjectSelector) -> Result<Kind, ProjectSelectionError> {
    match selector {
        ProjectSelector::Bare(text) => {
            if text.is_empty() {
                return Err(invalid(text.clone(), "an empty selector names no project"));
            }
            if let Ok(id) = Uuid::parse_str(text) {
                return Ok(Kind::Id(id, text.clone()));
            }
            if path_shaped(text) {
                return Ok(Kind::Path(PathBuf::from(text), text.clone()));
            }
            Ok(Kind::Name(text.clone(), text.clone()))
        }
        ProjectSelector::Kinded(fields) => {
            let echoed = serde_json::to_string(fields).unwrap_or_default();
            let set = [&fields.id, &fields.name, &fields.path]
                .into_iter()
                .filter(|field| field.is_some())
                .count();
            if set != 1 {
                return Err(invalid(
                    echoed,
                    "a selector sets exactly one of 'id', 'name', or 'path'",
                ));
            }
            if let Some(id) = &fields.id {
                let parsed = Uuid::parse_str(id).map_err(|_| ProjectSelectionError {
                    code: ProjectSelectionErrorCode::ProjectSelectorInvalid,
                    selector: echoed.clone(),
                    step: None,
                    message: format!("project id '{id}' is not a registered project UUID"),
                    next_action: "Memory bucket identifiers are not \
                                      project IDs. Run `goddard-agent boss \
                                      projects list` to discover the \
                                      canonical ID."
                        .into(),
                    candidates: Vec::new(),
                    truncated: false,
                })?;
                return Ok(Kind::Id(parsed, echoed));
            }
            if let Some(name) = &fields.name {
                if name.is_empty() {
                    return Err(invalid(echoed, "an empty name selects no project"));
                }
                return Ok(Kind::Name(name.clone(), echoed));
            }
            let path = fields.path.as_deref().unwrap_or_default();
            if path.is_empty() {
                return Err(invalid(echoed, "an empty path selects no project"));
            }
            Ok(Kind::Path(PathBuf::from(path), echoed))
        }
    }
}

/// Bare text reads as a path when it is absolute, carries a separator, or
/// leads with `~` — the rejection guidance then points at registered
/// roots instead of names.
fn path_shaped(text: &str) -> bool {
    Path::new(text).is_absolute()
        || text.starts_with('~')
        || text.contains('/')
        || text.contains('\\')
}

/// Lexically normalize an absolute path: redundant separators and `.`
/// segments drop, `..` pops one level and clamps at the root. Symlink
/// resolution is deliberately separate — `dunce::canonicalize` applies
/// platform filesystem case semantics where the entry exists.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Exactly one match selects — ambiguity lists candidates before the
/// temporary check runs, and a single temporary row rejects with its
/// record so the caller can register it or select the id deliberately.
/// `described` names the selection in the ambiguity message, e.g.
/// `project name 'x'` or `registered root '/x'`.
fn select(
    matches: Vec<&crate::model::Project>,
    selector: String,
    described: String,
    miss_message: String,
    miss_action: &str,
) -> Result<CanonicalProject, ProjectSelectionError> {
    match matches.len() {
        0 => Err(ProjectSelectionError {
            code: ProjectSelectionErrorCode::ProjectUnknown,
            selector,
            step: None,
            message: miss_message,
            next_action: miss_action.into(),
            candidates: Vec::new(),
            truncated: false,
        }),
        1 => {
            let project = matches[0];
            if project.temporary {
                return Err(ProjectSelectionError {
                    code: ProjectSelectionErrorCode::ProjectTemporary,
                    selector,
                    step: None,
                    message: format!(
                        "'{}' at '{}' is a temporary project registration",
                        project.name,
                        project.path.display()
                    ),
                    next_action: "Register a persistent project before \
                                  selecting it by name or path; explicit \
                                  project IDs keep their existing behavior."
                        .into(),
                    candidates: vec![CanonicalProject::of(project)],
                    truncated: false,
                });
            }
            Ok(CanonicalProject::of(project))
        }
        _ => {
            let truncated = matches.len() > CANDIDATE_CAP;
            Err(ProjectSelectionError {
                code: ProjectSelectionErrorCode::ProjectAmbiguous,
                selector,
                step: None,
                message: format!("{described} matches {} registered projects", matches.len()),
                next_action: "Choose a listed path or ID — an ambiguous \
                              selection is never resolved to a first match."
                    .into(),
                candidates: matches
                    .into_iter()
                    .take(CANDIDATE_CAP)
                    .map(CanonicalProject::of)
                    .collect(),
                truncated,
            })
        }
    }
}

fn invalid(selector: String, message: &str) -> ProjectSelectionError {
    ProjectSelectionError {
        code: ProjectSelectionErrorCode::ProjectSelectorInvalid,
        selector,
        step: None,
        message: message.into(),
        next_action: "Run `goddard-agent boss projects list` for the \
                      registered catalog and select with --id, --name, or \
                      --path."
            .into(),
        candidates: Vec::new(),
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;

    fn project(name: &str, path: PathBuf, temporary: bool) -> Project {
        Project {
            id: Uuid::new_v4(),
            name: name.into(),
            path,
            bookmark: None,
            created_at: 0,
            temporary,
            starred: false,
            submissions_enabled: false,
            qa_branch: None,
            friend_peer_id: None,
            kind: None,
            resolved_name: None,
        }
    }

    fn persistent(name: &str, path: &str) -> Project {
        project(name, PathBuf::from(path), false)
    }

    /// An absolute fixture root — bare `/work` is not absolute on Windows,
    /// so path selectors need a drive-prefixed root there.
    fn work(name: &str) -> PathBuf {
        Path::new(if cfg!(windows) { "C:/work" } else { "/work" }).join(name)
    }

    fn bare(text: &str) -> ProjectSelector {
        ProjectSelector::Bare(text.into())
    }

    fn kinded(fields: ProjectSelectorFields) -> ProjectSelector {
        ProjectSelector::Kinded(fields)
    }

    fn name(name: &str) -> ProjectSelector {
        kinded(ProjectSelectorFields {
            name: Some(name.into()),
            ..Default::default()
        })
    }

    fn path(path: &str) -> ProjectSelector {
        kinded(ProjectSelectorFields {
            path: Some(path.into()),
            ..Default::default()
        })
    }

    fn id(id: &str) -> ProjectSelector {
        kinded(ProjectSelectorFields {
            id: Some(id.into()),
            ..Default::default()
        })
    }

    #[test]
    fn an_id_selects_directly_including_temporary_rows() {
        let catalog = vec![
            persistent("alpha", "/work/alpha"),
            project("adhoc", PathBuf::from("/work/adhoc"), true),
        ];
        for selector in [
            bare(&catalog[0].id.to_string()),
            id(&catalog[0].id.to_string()),
        ] {
            let resolved = resolve_project(&catalog, &selector).unwrap();
            assert_eq!(resolved.id, catalog[0].id);
        }
        // Legacy explicit-UUID behavior: temporary rows still resolve by id.
        let resolved = resolve_project(&catalog, &id(&catalog[1].id.to_string())).unwrap();
        assert!(resolved.temporary);
    }

    #[test]
    fn an_unknown_id_is_project_unknown_with_a_memory_bucket_hint() {
        let catalog = vec![persistent("alpha", "/work/alpha")];
        let missing = Uuid::new_v4().to_string();
        for selector in [bare(&missing), id(&missing)] {
            let error = resolve_project(&catalog, &selector).unwrap_err();
            assert_eq!(error.code, ProjectSelectionErrorCode::ProjectUnknown);
            assert!(error.message.contains("unknown registered project ID"));
            assert!(error.next_action.contains("Memory bucket IDs"));
        }
        // A well-formed unknown id never falls back to a name lookup.
        let named_like_uuid = project(&missing, PathBuf::from("/work/uuidish"), false);
        let catalog = vec![named_like_uuid];
        let error = resolve_project(&catalog, &bare(&missing)).unwrap_err();
        assert_eq!(error.code, ProjectSelectionErrorCode::ProjectUnknown);
    }

    #[test]
    fn a_malformed_id_is_selector_invalid_with_context() {
        let catalog = vec![persistent("alpha", "/work/alpha")];
        let error = resolve_project(&catalog, &id("project-abc123")).unwrap_err();
        assert_eq!(
            error.code,
            ProjectSelectionErrorCode::ProjectSelectorInvalid
        );
        assert!(error.message.contains("not a registered project UUID"));
        assert!(error.next_action.contains("Memory bucket"));
    }

    #[test]
    fn names_match_exactly_ignoring_ascii_case_only() {
        let catalog = vec![persistent("Goddard", "/work/goddard")];
        assert_eq!(
            resolve_project(&catalog, &name("GODDARD")).unwrap().id,
            catalog[0].id
        );
        assert_eq!(
            resolve_project(&catalog, &bare("goddard")).unwrap().id,
            catalog[0].id
        );
        for misspelling in ["goddar", "goddard ", " goddard", "god"] {
            let error = resolve_project(&catalog, &name(misspelling)).unwrap_err();
            assert_eq!(
                error.code,
                ProjectSelectionErrorCode::ProjectUnknown,
                "{misspelling:?} must not match"
            );
        }
    }

    #[test]
    fn duplicate_names_are_ambiguous_with_candidates() {
        let catalog = vec![
            persistent("Octane", "/work/octane-a"),
            persistent("octane", "/elsewhere/octane-b"),
        ];
        let error = resolve_project(&catalog, &name("OCTANE")).unwrap_err();
        assert_eq!(error.code, ProjectSelectionErrorCode::ProjectAmbiguous);
        assert_eq!(error.candidates.len(), 2);
        assert!(error.next_action.contains("path or ID"));
        // Each candidate still resolves individually by id.
        for project in &catalog {
            assert_eq!(
                resolve_project(&catalog, &id(&project.id.to_string()))
                    .unwrap()
                    .id,
                project.id
            );
        }
    }

    #[test]
    fn a_temporary_row_rejects_name_and_path_but_reports_its_record() {
        let temporary = project("adhoc", work("adhoc"), true);
        let catalog = vec![temporary.clone()];
        let adhoc = work("adhoc").display().to_string();
        for selector in [name("adhoc"), path(&adhoc)] {
            let error = resolve_project(&catalog, &selector).unwrap_err();
            assert_eq!(error.code, ProjectSelectionErrorCode::ProjectTemporary);
            assert_eq!(error.candidates, vec![CanonicalProject::of(&temporary)]);
            assert!(error.next_action.contains("persistent"));
        }
        // Ambiguity wins over the temporary check: a duplicate pair with a
        // temporary member is ambiguous, not temporary.
        let catalog = vec![temporary, persistent("adhoc", "/work/adhoc-persistent")];
        let error = resolve_project(&catalog, &name("ADHOC")).unwrap_err();
        assert_eq!(error.code, ProjectSelectionErrorCode::ProjectAmbiguous);
        assert_eq!(error.candidates.len(), 2);
    }

    #[test]
    fn relative_and_tilde_paths_are_selector_invalid() {
        let catalog = vec![persistent("alpha", "/work/alpha")];
        for selector in [
            path("work/alpha"),
            path("~/work/alpha"),
            path(""),
            bare("work/alpha"),
            bare("~/work/alpha"),
            bare("./alpha"),
        ] {
            let error = resolve_project(&catalog, &selector).unwrap_err();
            assert_eq!(
                error.code,
                ProjectSelectionErrorCode::ProjectSelectorInvalid,
                "{selector:?} must reject as invalid"
            );
        }
    }

    #[test]
    fn kinded_selectors_require_exactly_one_field() {
        let catalog = vec![persistent("alpha", "/work/alpha")];
        for fields in [
            ProjectSelectorFields::default(),
            ProjectSelectorFields {
                name: Some("alpha".into()),
                path: Some("/work/alpha".into()),
                ..Default::default()
            },
        ] {
            let error = resolve_project(&catalog, &kinded(fields)).unwrap_err();
            assert_eq!(
                error.code,
                ProjectSelectionErrorCode::ProjectSelectorInvalid
            );
        }
    }

    #[test]
    fn a_registered_root_path_resolves_and_a_subdirectory_does_not() {
        let root = std::env::temp_dir().join(format!("waku-proj-{}", Uuid::new_v4()));
        let registered = root.join("alpha");
        std::fs::create_dir_all(registered.join("nested")).unwrap();
        let catalog = vec![project(
            "alpha",
            dunce::canonicalize(&registered).unwrap(),
            false,
        )];

        // Dot segments, a trailing separator, and a symlinked spelling all
        // name the same canonical root.
        let dotted = format!("{}/nested/../", registered.display());
        let resolved = resolve_project(&catalog, &path(&dotted)).unwrap();
        assert_eq!(resolved.id, catalog[0].id);
        #[cfg(unix)]
        {
            let alias = root.join("alias");
            std::os::unix::fs::symlink(&registered, &alias).unwrap();
            let resolved = resolve_project(&catalog, &path(&alias.to_string_lossy())).unwrap();
            assert_eq!(resolved.id, catalog[0].id);
        }

        // A subdirectory is never a parent registration's match, and an
        // unregistered sibling is unknown — no containment or repository
        // inference.
        for miss in [
            registered.join("nested").to_string_lossy().into_owned(),
            root.join("sibling").to_string_lossy().into_owned(),
        ] {
            let error = resolve_project(&catalog, &path(&miss)).unwrap_err();
            assert_eq!(error.code, ProjectSelectionErrorCode::ProjectUnknown);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_location_still_matches_the_stored_spelling() {
        let gone = std::env::temp_dir().join(format!("waku-gone-{}", Uuid::new_v4()));
        let catalog = vec![persistent("ghost", &gone.to_string_lossy())];
        let resolved = resolve_project(&catalog, &path(&gone.to_string_lossy())).unwrap();
        assert_eq!(resolved.id, catalog[0].id);
    }

    #[test]
    fn equivalent_roots_registered_twice_are_ambiguous() {
        let root = std::env::temp_dir().join(format!("waku-dupe-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let alias = root.join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let catalog = vec![
            persistent("one", &root.to_string_lossy()),
            #[cfg(unix)]
            persistent("two", &alias.to_string_lossy()),
            #[cfg(not(unix))]
            persistent("two", &root.to_string_lossy()),
        ];
        let error = resolve_project(&catalog, &path(&root.to_string_lossy())).unwrap_err();
        assert_eq!(error.code, ProjectSelectionErrorCode::ProjectAmbiguous);
        assert_eq!(error.candidates.len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_uuid_or_path_shaped_name_stays_selectable_through_the_kinded_fields() {
        let uuid_named = project(
            "11111111-1111-4111-8111-111111111111",
            PathBuf::from("/work/uuidish"),
            false,
        );
        let slash_named = project("a/b", PathBuf::from("/work/slashy"), false);
        let catalog = vec![uuid_named.clone(), slash_named.clone()];
        assert_eq!(
            resolve_project(&catalog, &name("a/b")).unwrap().id,
            slash_named.id
        );
        // The bare spelling reaches the id lookup, not the name.
        let error = resolve_project(&catalog, &bare("a/b")).unwrap_err();
        assert_eq!(
            error.code,
            ProjectSelectionErrorCode::ProjectSelectorInvalid
        );
        assert_eq!(
            resolve_project(&catalog, &name(&uuid_named.name))
                .unwrap()
                .id,
            uuid_named.id
        );
    }

    #[test]
    fn step_context_marks_the_failing_step() {
        let error = resolve_project(&[], &name("nothing"))
            .unwrap_err()
            .at_step(1);
        assert_eq!(error.step, Some(2));
        assert!(error.message.starts_with("Step 2: "));
    }

    #[test]
    fn the_catalog_sorts_by_name_and_labels_temporary_rows() {
        let catalog = catalog(&[
            persistent("beta", "/work/beta"),
            project("adhoc", PathBuf::from("/work/adhoc"), true),
            persistent("Alpha", "/work/alpha"),
        ]);
        let names: Vec<_> = catalog.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["adhoc", "Alpha", "beta"]);
        assert!(catalog[0].temporary && !catalog[1].temporary);
    }

    #[test]
    fn ambiguity_candidates_are_bounded() {
        let catalog: Vec<_> = (0..40)
            .map(|index| persistent("same", &format!("/work/same-{index}")))
            .collect();
        let error = resolve_project(&catalog, &name("same")).unwrap_err();
        assert_eq!(error.code, ProjectSelectionErrorCode::ProjectAmbiguous);
        assert_eq!(error.candidates.len(), 32);
        assert!(error.truncated);
    }

    #[test]
    fn selectors_decode_bare_text_and_kinded_maps() {
        assert_eq!(
            serde_json::from_str::<ProjectSelector>("\"plain\"").unwrap(),
            bare("plain")
        );
        assert_eq!(
            serde_json::from_str::<ProjectSelector>("{\"name\":\"alpha\"}").unwrap(),
            name("alpha")
        );
        // Unknown fields and non-selector values fail input parsing.
        assert!(serde_json::from_str::<ProjectSelector>("{\"bogus\":1}").is_err());
        assert!(serde_json::from_str::<ProjectSelector>("42").is_err());
    }
}
