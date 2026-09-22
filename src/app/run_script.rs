//! The "Run project script" flow behind ⌘R. The palette drills in two
//! steps — a project, then one of the scripts declared at its root
//! (package.json, makefile, justfile) — and the pick lands here: a terminal
//! running the script. A session whose project matches hosts the run in its
//! right panel; anything else gets a full-width standalone terminal.

use std::collections::HashSet;

use super::*;

/// A file kind the picker reads scripts from, in listing order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScriptSource {
    PackageJson,
    Makefile,
    Justfile,
}

impl ScriptSource {
    /// Row detail prefix — the file the script was found in, untranslated.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::PackageJson => "package.json",
            Self::Makefile => "Makefile",
            Self::Justfile => "just",
        }
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::PackageJson => "icons/package.svg",
            Self::Makefile => "icons/wrench.svg",
            Self::Justfile => "icons/terminal.svg",
        }
    }

    fn command_icon(self) -> CustomCommandIcon {
        match self {
            Self::PackageJson => CustomCommandIcon::Package,
            Self::Makefile => CustomCommandIcon::Wrench,
            Self::Justfile => CustomCommandIcon::Terminal,
        }
    }
}

/// One runnable entry the picker lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProjectScript {
    /// The picker label — the script or target name.
    pub name: String,
    /// The shell line the terminal runs, e.g. `npm run dev`.
    pub command: String,
    /// The script's body where the source declares one (package.json);
    /// makefile and just rows fall back to the command.
    pub detail: String,
    pub source: ScriptSource,
}

impl ProjectScript {
    /// Secondary row text under the script's name.
    pub(super) fn detail_or_command(&self) -> &str {
        if self.detail.is_empty() {
            &self.command
        } else {
            &self.detail
        }
    }
}

/// Quote one shell word only when it carries characters a bare word can't.
fn shell_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%_+=:,./-".contains(c))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// The runner a package.json script gets, guessed from the lockfile the
/// project committed. Anything unmarked is npm's.
fn package_manager_prefix(root: &Path) -> &'static str {
    if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        "bun run"
    } else if root.join("pnpm-lock.yaml").is_file() {
        "pnpm run"
    } else if root.join("yarn.lock").is_file() {
        "yarn run"
    } else {
        "npm run"
    }
}

fn package_json_scripts(root: &Path) -> Vec<ProjectScript> {
    let Ok(content) = std::fs::read_to_string(root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(scripts) = json.get("scripts").and_then(|scripts| scripts.as_object()) else {
        return Vec::new();
    };
    let prefix = package_manager_prefix(root);
    scripts
        .iter()
        .map(|(name, body)| ProjectScript {
            name: name.clone(),
            command: format!("{prefix} {}", shell_word(name)),
            detail: body.as_str().unwrap_or_default().trim().to_owned(),
            source: ScriptSource::PackageJson,
        })
        .collect()
}

/// Rule names a makefile declares: the left of a top-level `:` line that is
/// not an assignment (`:=`, `::=`, `!=`), a directive, a special `.TARGET`,
/// or a pattern (`%`) rule.
fn makefile_targets(content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for line in content.lines() {
        if line.starts_with(|c: char| c.is_whitespace() || c == '#') {
            continue;
        }
        let Some((names, rest)) = line.split_once(':') else {
            continue;
        };
        if rest.starts_with('=') || rest.starts_with(":=") {
            continue;
        }
        for name in names.split_whitespace() {
            let is_target = name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.contains(['%', '$', '(', ')']);
            if is_target && !targets.iter().any(|seen| seen == name) {
                targets.push(name.to_owned());
            }
        }
    }
    targets
}

fn makefile_scripts(root: &Path) -> Vec<ProjectScript> {
    // GNU make's own search order.
    for name in ["GNUmakefile", "makefile", "Makefile"] {
        if let Ok(content) = std::fs::read_to_string(root.join(name)) {
            return makefile_targets(&content)
                .into_iter()
                .map(|target| ProjectScript {
                    command: format!("make {}", shell_word(&target)),
                    name: target,
                    detail: String::new(),
                    source: ScriptSource::Makefile,
                })
                .collect();
        }
    }
    Vec::new()
}

/// Recipe names a justfile declares: a top-level `name params…:` line that
/// is not a comment, attribute, directive, or `:=` assignment.
fn justfile_recipes(content: &str) -> Vec<String> {
    let mut recipes = Vec::new();
    for line in content.lines() {
        if line.starts_with(|c: char| c.is_whitespace() || c == '#') || line.starts_with('[') {
            continue;
        }
        if ["set ", "unset ", "alias ", "mod ", "import ", "export "]
            .iter()
            .any(|keyword| line.starts_with(keyword))
        {
            continue;
        }
        let line = line.strip_prefix('@').unwrap_or(line);
        let name_end = line
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(line.len());
        let name = &line[..name_end];
        let is_name = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if !is_name {
            continue;
        }
        let rest = &line[name_end..];
        let Some(colon) = rest.find(':') else {
            continue;
        };
        if rest[colon..].starts_with(":=") {
            continue;
        }
        if !recipes.iter().any(|seen| seen == name) {
            recipes.push(name.to_owned());
        }
    }
    recipes
}

fn justfile_scripts(root: &Path) -> Vec<ProjectScript> {
    for name in ["justfile", ".justfile"] {
        if let Ok(content) = std::fs::read_to_string(root.join(name)) {
            return justfile_recipes(&content)
                .into_iter()
                .map(|recipe| ProjectScript {
                    command: format!("just {}", shell_word(&recipe)),
                    name: recipe,
                    detail: String::new(),
                    source: ScriptSource::Justfile,
                })
                .collect();
        }
    }
    Vec::new()
}

/// Every script declared at the project root — the directory the picker
/// scoped to, no deeper. Runs off the UI thread; a few small file reads.
pub(super) fn discover_project_scripts(root: &Path) -> Vec<ProjectScript> {
    let mut scripts = package_json_scripts(root);
    scripts.extend(makefile_scripts(root));
    scripts.extend(justfile_scripts(root));
    scripts
}

/// Pick order for the project step: the selected task's project first, then
/// the MRU list the project switcher records, then the rest by how recently
/// they were added. Unlike the switcher's `ordered_project_ids` this keeps
/// every project — the picker filters rather than capping at ten.
pub(super) fn run_script_project_order(
    current: Option<Uuid>,
    recent: &[Uuid],
    projects: &[&Project],
) -> Vec<Uuid> {
    let mut seen = HashSet::with_capacity(projects.len());
    let mut ordered = Vec::with_capacity(projects.len());
    let mut push = |id: Uuid| {
        if seen.insert(id) {
            ordered.push(id);
        }
    };
    if let Some(current) = current {
        push(current);
    }
    for recent in recent {
        push(*recent);
    }
    let mut recently_added = projects.to_vec();
    recently_added.sort_by_key(|project| std::cmp::Reverse(project.created_at));
    for project in recently_added {
        push(project.id);
    }
    ordered
}

impl Waku {
    /// Run the picked script. When the selected task belongs to the same
    /// project the run lands in its right panel, rooted at that task's
    /// workspace like every other panel terminal; anything else gets a
    /// full-width standalone terminal at the project root. The command's
    /// `close_on_success` stays off, so the shell — and its output — remain.
    pub(super) fn run_project_script(
        &mut self,
        project_id: Uuid,
        script: ProjectScript,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };
        // The run lands in a desktop PTY; a remote project root is not a
        // local path it could open.
        if self.is_remote_project(project.id) {
            return;
        }
        let project_path = project.path.clone();
        let mut command = CustomCommand::new(script.command.clone());
        command.name = Some(script.name.clone());
        command.icon = script.source.command_icon();

        if self
            .selected_session()
            .is_some_and(|session| session.project_id == project_id)
            && self.selected_workspace_path().is_some()
        {
            let surface = RightPanelSurface::new_terminal();
            let terminal_id = surface.terminal_id();
            if let Some(terminal_id) = terminal_id {
                self.right_panel_terminal_commands
                    .insert(terminal_id, command.clone());
            }
            if !cfg!(windows)
                && let Some(terminal_id) = terminal_id
            {
                // The sentinel reports the script's exit code back; the
                // progress toast settles to its result like a custom
                // command's. cmd cannot emit the sentinel, so Windows
                // skips the toast rather than leaving one spinning.
                let toast_id = self.show_progress_toast(
                    tr!("commands.running", name = command.display_name()),
                    PROGRESS_TOAST_DURATION,
                );
                self.custom_command_runs.insert(
                    terminal_id,
                    PendingCommandRun {
                        name: command.display_name().to_owned(),
                        toast_id,
                        tail: Vec::new(),
                        tail_published_at: None,
                        tail_flush_armed: false,
                    },
                );
            }
            self.open_right_panel_surface(surface, cx);
            return;
        }
        if let Some(terminal_id) = self.create_terminal(project_path, None, Some(command), cx) {
            self.select_terminal(terminal_id, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, name: &str, content: &str) {
        std::fs::write(root.join(name), content).unwrap();
    }

    #[test]
    fn package_json_scripts_run_through_the_committed_manager() {
        let root = std::env::temp_dir().join(format!("waku-scripts-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root,
            "package.json",
            r#"{"scripts":{"dev":"vite dev","test":"vitest"},"dependencies":{}}"#,
        );
        write(&root, "bun.lock", "");
        let scripts = package_json_scripts(&root);
        assert_eq!(scripts.len(), 2);
        assert!(
            scripts
                .iter()
                .all(|script| script.command.starts_with("bun run "))
        );
        assert_eq!(
            scripts
                .iter()
                .find(|script| script.name == "dev")
                .unwrap()
                .detail,
            "vite dev"
        );
    }

    #[test]
    fn package_manager_prefix_falls_back_to_npm() {
        let root = std::env::temp_dir().join(format!("waku-scripts-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(package_manager_prefix(&root), "npm run");
        write(&root, "pnpm-lock.yaml", "");
        assert_eq!(package_manager_prefix(&root), "pnpm run");
    }

    #[test]
    fn makefile_targets_skip_assignments_specials_and_recipes() {
        let targets = makefile_targets(
            "SHELL = /bin/sh\n\
             NAME := value\n\
             IMM ::= value\n\
             .PHONY: dev\n\
             dev: build\n\
             \tcargo run\n\
             %.o: %.c\n\
             build test: deps\n\
             release::\n",
        );
        assert_eq!(targets, ["dev", "build", "test", "release"]);
    }

    #[test]
    fn justfile_recipes_skip_directives_assignments_and_bodies() {
        let recipes = justfile_recipes(
            "# comment\n\
             set dotenv-load\n\
             import 'other.just'\n\
             VERSION := \"1.0\"\n\
             [private]\n\
             _hidden: dep\n\
             @quiet arg=\"x\":\n\
             \techo hi\n\
             dev:\n\
             \tcargo run\n",
        );
        assert_eq!(recipes, ["_hidden", "quiet", "dev"]);
    }

    #[test]
    fn project_order_puts_current_then_mru_then_recently_added() {
        let project = |created_at| Project {
            id: Uuid::new_v4(),
            name: String::new(),
            path: PathBuf::new(),
            bookmark: None,
            created_at,
            temporary: false,
            starred: false,
        };
        let current = project(0);
        let recent_a = project(0);
        let recent_b = project(0);
        let added = project(10);
        let stale = project(1);
        let projects = [&added, &stale, &recent_a, &recent_b, &current];
        assert_eq!(
            run_script_project_order(Some(current.id), &[recent_b.id, recent_a.id], &projects),
            [current.id, recent_b.id, recent_a.id, added.id, stale.id]
        );
    }
}
