use super::*;
use crate::persistence::TerminalPaneLayout;
use crate::ui::ActivationExt;

/// One entry in the sidebar's Terminals group. `session` scopes the
/// terminal to the task whose right-panel surfaces own it; `None` is a
/// global terminal that exists only in the group.
pub(super) struct TerminalRecord {
    pub session: Option<Uuid>,
    /// Pinned terminals keep a sidebar row while the group is collapsed —
    /// as does the selected standalone terminal.
    pub pinned: bool,
    /// Where the PTY starts — `None` resolves to the owning session's
    /// workspace at spawn time, so the terminal follows the workspace
    /// when it moves; `Some` pins the terminal to a chosen directory.
    pub working_directory: Option<PathBuf>,
    /// The shell's latest prompt-reported cwd, persisted for restart.
    pub last_working_directory: Option<PathBuf>,
    /// Workspace path used to detect a moved workspace. It stays at the
    /// workspace root even when a restored shell starts in a nested cwd.
    pub workspace_directory: Option<PathBuf>,
    /// When the terminal opened (unix seconds) — the row's "…ago" label
    /// until the view reports a command's start.
    pub opened_at: u64,
    /// A sidebar rename — wins over the view's OSC-set title and is
    /// applied to the view when it spawns.
    pub custom_title: Option<String>,
    /// The split group's anchor — the terminal id its surface (a
    /// right-panel tab or the main-area selection) references. `None`
    /// for standalone terminals and for the anchor itself.
    pub pane_of: Option<Uuid>,
    /// Ordered pane ids splitting this terminal beyond the anchor, which
    /// always lays out first. Empty unless this record anchors a group.
    pub panes: Vec<Uuid>,
    /// Explicit split geometry; absent for legacy stack/grid groups.
    pub pane_layout: Option<TerminalPaneLayout>,
}

/// Maximum panes in a right-panel terminal tab.
pub(super) const RIGHT_PANEL_TERMINAL_MAX_PANES: usize = 3;
/// Maximum panes in a standalone or floating terminal group.
pub(super) const STANDALONE_TERMINAL_MAX_PANES: usize = 9;
/// Columns the standalone grid lays out across before wrapping rows.
pub(super) const STANDALONE_TERMINAL_GRID_COLUMNS: usize = 3;

/// How a split group lays its panes out — picked by the surface the
/// group's anchor renders in.
#[derive(Clone, Copy)]
pub(super) enum TerminalGroupLayout {
    /// The right panel's terminal tab: panes stack vertically, each the
    /// strip's full width.
    Stack,
    /// The standalone main-area terminal: panes tile a grid at most
    /// `STANDALONE_TERMINAL_GRID_COLUMNS` wide.
    Grid,
}

/// A Terminals-group row dragged toward a terminal surface. Hovering
/// another row mid-drag previews its floating card; dropping on a
/// surface joins its split group as live panes — the dragged terminal's
/// own session, not a copy.
#[derive(Clone)]
pub(super) struct SidebarTerminalDrag {
    pub(super) terminal_id: Uuid,
    pub(super) title: SharedString,
}

pub(super) struct TerminalDragPreview {
    docked: Option<Uuid>,
    last_visible: Option<Uuid>,
    pub(super) panel_active: Option<RightPanelSurface>,
    pub(super) fullscreen: Option<(RightPanelSurface, Option<String>)>,
    focus: Option<FocusHandle>,
}

impl Render for SidebarTerminalDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div()
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(8.0))
            .bg(theme.composer)
            .border(hairline())
            .border_color(theme.border)
            .text_color(theme.text)
            .child(self.title.clone())
    }
}

fn terminal_group_ids(records: &HashMap<Uuid, TerminalRecord>, anchor_id: Uuid) -> Vec<Uuid> {
    records
        .get(&anchor_id)
        .map(|record| {
            std::iter::once(anchor_id)
                .chain(record.panes.iter().copied())
                .collect()
        })
        .unwrap_or_default()
}

/// Retain surviving leaves, collapsing closed panes, and append panes added by
/// drag and drop. Membership stays authoritative over saved layout metadata.
fn terminal_pane_layout(
    saved: Option<&TerminalPaneLayout>,
    panes: &[Uuid],
    down: bool,
) -> Option<TerminalPaneLayout> {
    fn retain(
        layout: &TerminalPaneLayout,
        remaining: &mut Vec<Uuid>,
    ) -> Option<TerminalPaneLayout> {
        match layout {
            TerminalPaneLayout::Pane(id) => {
                let index = remaining.iter().position(|pane| pane == id)?;
                remaining.remove(index);
                Some(TerminalPaneLayout::Pane(*id))
            }
            TerminalPaneLayout::Split {
                down,
                first,
                second,
            } => match (retain(first, remaining), retain(second, remaining)) {
                (Some(first), Some(second)) => Some(terminal_layout_pair(first, second, *down)),
                (first, second) => first.or(second),
            },
        }
    }
    let mut remaining = panes.to_vec();
    let mut layout = saved.and_then(|saved| retain(saved, &mut remaining));
    for id in remaining {
        let pane = TerminalPaneLayout::Pane(id);
        layout = Some(match layout {
            Some(layout) => terminal_layout_pair(layout, pane, down),
            None => pane,
        });
    }
    layout
}

fn terminal_group_layout(
    records: &HashMap<Uuid, TerminalRecord>,
    anchor_id: Uuid,
    stack: bool,
) -> Option<TerminalPaneLayout> {
    let panes = terminal_group_ids(records, anchor_id);
    let record = records.get(&anchor_id)?;
    if record.pane_layout.is_some() || stack {
        return terminal_pane_layout(record.pane_layout.as_ref(), &panes, stack);
    }
    // Preserve the old balanced grid until the first directional split.
    let rows = panes.len().div_ceil(STANDALONE_TERMINAL_GRID_COLUMNS);
    let columns = panes.len().div_ceil(rows.max(1));
    panes
        .chunks(columns.max(1))
        .filter_map(|row| terminal_pane_layout(None, row, false))
        .reduce(|first, second| terminal_layout_pair(first, second, true))
}

fn terminal_layout_pair(
    first: TerminalPaneLayout,
    second: TerminalPaneLayout,
    down: bool,
) -> TerminalPaneLayout {
    TerminalPaneLayout::Split {
        down,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn split_terminal_layout(layout: &mut TerminalPaneLayout, target: Uuid, pane: Uuid, down: bool) {
    match layout {
        TerminalPaneLayout::Pane(id) if *id == target => {
            *layout = terminal_layout_pair(layout.clone(), TerminalPaneLayout::Pane(pane), down);
        }
        TerminalPaneLayout::Split { first, second, .. } => {
            split_terminal_layout(first, target, pane, down);
            split_terminal_layout(second, target, pane, down);
        }
        _ => {}
    }
}

fn visible_terminal_group_ids(panes: &[Uuid], maximized: Option<Uuid>) -> Vec<Uuid> {
    match maximized.filter(|terminal_id| panes.contains(terminal_id)) {
        Some(terminal_id) => vec![terminal_id],
        None => panes.to_vec(),
    }
}

fn toggle_maximized_terminal_pane(
    current: Option<Uuid>,
    focused: Uuid,
    panes: &[Uuid],
) -> Option<Uuid> {
    if !panes.contains(&focused) {
        return current;
    }
    (current != Some(focused)).then_some(focused)
}

fn terminal_group_can_add(record: &TerminalRecord, incoming: usize, limit: usize) -> bool {
    record.pane_of.is_none() && record.panes.len() + 1 + incoming <= limit
}

/// Which group `secondary-t` grows — a focused floating card's anchor,
/// or the maximized standalone's even when focus sits outside its panes
/// (a fresh card there would float over the maximized surface and bury
/// it). Focus in any other surface's terminal yields `None` so the chord
/// keeps that surface's own meaning.
fn new_terminal_split_target(
    focused_anchor: Option<Uuid>,
    docked_terminal: Option<Uuid>,
    maximized_anchor: Option<Uuid>,
) -> Option<Uuid> {
    match focused_anchor {
        Some(anchor) if docked_terminal == Some(anchor) => Some(anchor),
        Some(anchor) if maximized_anchor == Some(anchor) => Some(anchor),
        Some(_) => None,
        None => maximized_anchor,
    }
}

fn terminal_groups_fit(
    records: &HashMap<Uuid, TerminalRecord>,
    dragged_id: Uuid,
    anchor_id: Uuid,
    limit: usize,
) -> bool {
    let incoming = terminal_group_ids(records, dragged_id);
    let target = terminal_group_ids(records, anchor_id);
    !incoming.is_empty()
        && !incoming.iter().any(|id| target.contains(id))
        && records
            .get(&dragged_id)
            .is_some_and(|record| record.pane_of.is_none())
        && records
            .get(&anchor_id)
            .is_some_and(|record| terminal_group_can_add(record, incoming.len(), limit))
}

fn join_terminal_groups(
    records: &mut HashMap<Uuid, TerminalRecord>,
    dragged_id: Uuid,
    anchor_id: Uuid,
    limit: usize,
) -> Vec<Uuid> {
    if !terminal_groups_fit(records, dragged_id, anchor_id, limit) {
        return Vec::new();
    }
    let incoming = terminal_group_ids(records, dragged_id);
    let directional = [anchor_id, dragged_id].iter().any(|id| {
        records
            .get(id)
            .is_some_and(|record| record.pane_layout.is_some())
    });
    let down = limit == RIGHT_PANEL_TERMINAL_MAX_PANES;
    let layout = directional.then(|| {
        terminal_layout_pair(
            terminal_group_layout(records, anchor_id, down).expect("existing target group"),
            terminal_group_layout(records, dragged_id, down).expect("existing dragged group"),
            down,
        )
    });
    for id in &incoming {
        if let Some(record) = records.get_mut(id) {
            record.pane_of = Some(anchor_id);
            record.panes.clear();
            record.pane_layout = None;
        }
    }
    if let Some(record) = records.get_mut(&anchor_id) {
        record.panes.extend(incoming.iter().copied());
        record.pane_layout = layout;
    }
    incoming
}

/// A terminal row is two lines — title and status over location and time —
/// the same card rhythm a session row uses.
const SIDEBAR_TERMINAL_CARD_HEIGHT: f32 = 51.0;
const SIDEBAR_TERMINAL_ROW_GAP: f32 = 1.0;
pub(super) const SIDEBAR_TERMINAL_ROW_HEIGHT: f32 =
    SIDEBAR_TERMINAL_CARD_HEIGHT + SIDEBAR_TERMINAL_ROW_GAP;

/// The nearest enclosing repository's root — `.git` may be a file in a
/// linked worktree, so existence rather than `is_dir` is the test.
pub(super) fn nearest_repo_root(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

#[derive(Clone, Default)]
pub(super) struct TerminalLocation {
    package: Option<String>,
    /// Linked-worktree name, falling back to the active branch.
    worktree: Option<String>,
}

fn nonempty_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn nearest_package_name(directory: &Path) -> Option<String> {
    for ancestor in directory.ancestors() {
        if let Ok(contents) = std::fs::read_to_string(ancestor.join("package.json"))
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents)
            && let Some(name) = json.get("name").and_then(serde_json::Value::as_str)
            && let Some(name) = nonempty_name(name)
        {
            return Some(name);
        }
        if let Ok(contents) = std::fs::read_to_string(ancestor.join("Cargo.toml"))
            && let Ok(manifest) = contents.parse::<toml::Value>()
            && let Some(name) = manifest
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(toml::Value::as_str)
                .and_then(nonempty_name)
        {
            return Some(name);
        }
        if let Ok(contents) = std::fs::read_to_string(ancestor.join("pubspec.yaml"))
            && let Some(name) = pubspec_package_name(&contents)
        {
            return Some(name);
        }
    }
    None
}

// Dart package names are top-level YAML scalars. Ignore nested dependency
// names; quotes and trailing comments are common in generated manifests.
fn pubspec_package_name(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let value = line.strip_prefix("name:")?.trim();
        if let Some(quote) = value.chars().next().filter(|c| matches!(c, '\"' | '\'')) {
            let value = &value[quote.len_utf8()..];
            nonempty_name(value.split_once(quote)?.0)
        } else {
            nonempty_name(value.split('#').next()?)
        }
    })
}

fn terminal_worktree_name(root: &Path) -> Option<String> {
    let pointer = std::fs::read_to_string(root.join(".git")).ok()?;
    let git_dir = root.join(pointer.trim().strip_prefix("gitdir:")?.trim());
    // Submodules also have a .git file, but only linked worktrees share a
    // common directory through this marker.
    if !git_dir.join("commondir").is_file() {
        return None;
    }
    // Daemon checkouts repeat the repository leaf beneath their named
    // directory: worktrees/<repository>/<name>/<repository>.
    let parent = root.parent()?;
    let namespace = parent.parent()?;
    let managed = namespace.file_name() == root.file_name()
        && namespace
            .parent()?
            .file_name()
            .is_some_and(|name| name == "worktrees");
    let name = if managed {
        parent.file_name()?
    } else {
        root.file_name()?
    };
    name.to_str().and_then(nonempty_name)
}

fn terminal_location_names(cwd: &Path, location: &TerminalLocation) -> Vec<(&'static str, String)> {
    let inferred = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mut names = Vec::new();
    if let Some(package) = location.package.as_deref().filter(|name| *name != inferred) {
        names.push(("icons/package.svg", package.to_owned()));
    }
    if let Some(worktree) = location
        .worktree
        .as_deref()
        .filter(|name| *name != inferred)
    {
        names.push(("icons/git-branch.svg", worktree.to_owned()));
    }
    names
}

/// Character budget for a row's location label before ancestors fold
/// under `…/` — beyond it a layout-level tail clip would saw through a
/// component's middle.
const SIDEBAR_TERMINAL_DETAIL_MAX_CHARS: usize = 32;

/// The docked card's frame: its slot margin, the header strip's height,
/// the inset mat around the view that keeps its square corners off the
/// card's rounded edge, and the band the card height clamps to as the
/// window grows and shrinks.
const DOCKED_TERMINAL_MARGIN: f32 = 8.0;
const DOCKED_TERMINAL_HEADER_HEIGHT: f32 = 34.0;
const DOCKED_TERMINAL_INSET: f32 = 6.0;
const DOCKED_TERMINAL_MIN_HEIGHT: f32 = 220.0;
const DOCKED_TERMINAL_MAX_HEIGHT: f32 = 340.0;

/// Shorten a slash-separated label by popping whole ancestors, never
/// cutting through a component: `~/a/b/c` becomes `…/b/c`, then `…/c`.
/// With `keep_first`, the leading component — a repository name — stays
/// put and the popped middle folds as `repo/…/leaf` instead.
fn truncate_path_ancestors(path: &str, max_chars: usize, keep_first: bool) -> String {
    if path.chars().count() <= max_chars {
        return path.to_owned();
    }
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.len() <= 1 {
        return path.to_owned();
    }
    if keep_first {
        // The leading component anchors; the `…` stands for the middle
        // components between it and the leaf, so it needs both to exist.
        for dropped in 2..components.len() {
            let candidate = format!("{}/…/{}", components[0], components[dropped..].join("/"));
            if candidate.chars().count() <= max_chars {
                return candidate;
            }
        }
        return if components.len() > 2 {
            format!("{}/…/{}", components[0], components[components.len() - 1])
        } else {
            path.to_owned()
        };
    }
    for dropped in 1..components.len() {
        let candidate = format!("…/{}", components[dropped..].join("/"));
        if candidate.chars().count() <= max_chars {
            return candidate;
        }
    }
    format!("…/{}", components[components.len() - 1])
}

/// The group's flat listing in sidebar order: pinned terminals lead,
/// with `terminal_order`'s creation order kept inside each partition.
/// Stays a lazy scan — the sidebar fingerprint reads it every frame.
fn sidebar_terminal_order<'a>(
    terminal_order: &'a [Uuid],
    records: &'a HashMap<Uuid, TerminalRecord>,
) -> impl Iterator<Item = Uuid> + 'a {
    let partition = move |pinned: bool| {
        terminal_order.iter().copied().filter(move |id| {
            records
                .get(id)
                // Split panes render inside their anchor's surface, never as
                // rows of their own.
                .is_some_and(|record| record.pinned == pinned && record.pane_of.is_none())
        })
    };
    partition(true).chain(partition(false))
}

impl Waku {
    #[track_caller]
    pub(super) fn render_terminal_location_for_id(
        &self,
        terminal_id: Uuid,
        cx: &App,
    ) -> AnyElement {
        // The header's folder chip names the directory, never its absolute
        // path: a repository anchor when one is known, the leaf otherwise.
        let label = self
            .terminal_cwd(terminal_id, cx)
            .map(|cwd| {
                self.repo_location_label(&cwd, SIDEBAR_TERMINAL_DETAIL_MAX_CHARS)
                    .or_else(|| {
                        cwd.file_name()
                            .and_then(|name| name.to_str())
                            .map(|name| name.to_owned())
                    })
                    .unwrap_or_else(|| {
                        settings::abbreviate_home_path(&cwd, self.home_directory.as_deref())
                    })
            })
            .unwrap_or_default();
        self.render_terminal_location(terminal_id, "icons/folder.svg", label, false, cx)
    }

    /// The sidebar row's repo-anchored location label — the project's
    /// display name or the repository's directory name at the root,
    /// `name/sub/dir` deeper in. `None` when the cwd sits outside every
    /// known repository (or the scan has not reported it yet).
    fn repo_location_label(&self, cwd: &Path, max_chars: usize) -> Option<String> {
        let root = self
            .sidebar_terminal_repo_roots
            .borrow()
            .get(cwd)
            .cloned()
            .flatten()?;
        let project = self
            .state
            .projects
            .iter()
            .filter(|project| cwd.starts_with(&project.path))
            .max_by_key(|project| project.path.components().count());
        let name = project.map(Project::display_name).unwrap_or_else(|| {
            root.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned()
        });
        let relative_root = project.map_or(root.as_path(), |project| project.path.as_path());
        let label = cwd
            .strip_prefix(relative_root)
            .ok()
            .filter(|rel| !rel.as_os_str().is_empty())
            .map(|rel| format!("{name}/{}", rel.display()))
            .unwrap_or(name);
        // The repo name anchors the label — the folded middle comes from
        // the working directory's ancestors.
        Some(truncate_path_ancestors(&label, max_chars, true))
    }

    #[track_caller]
    fn render_terminal_location(
        &self,
        terminal_id: Uuid,
        cwd_icon: &'static str,
        cwd_label: String,
        folder_first: bool,
        cx: &App,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let mut names = self
            .terminal_cwd(terminal_id, cx)
            .map(|cwd| {
                let locations = self.sidebar_terminal_locations.borrow();
                locations
                    .get(&cwd)
                    .map(|location| terminal_location_names(&cwd, location))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let cwd_index = if folder_first {
            names.reverse();
            0
        } else {
            names.len()
        };
        names.insert(cwd_index, (cwd_icon, cwd_label));
        let mut row = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(5.0))
            .text_size(sp(12.0))
            .text_color(theme.text_tertiary);
        let mut package = None;
        for (index, (glyph, name)) in names.into_iter().enumerate() {
            if folder_first && glyph == "icons/package.svg" {
                package = Some(
                    div()
                        .flex_none()
                        .h(sp(15.0))
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .whitespace_nowrap()
                        .child(icon(glyph, 12.0, theme.text_tertiary))
                        .child(single_line_label(&name)),
                );
                continue;
            }
            let label = if index == cwd_index {
                div().min_w_0().truncate().child(SharedString::from(name))
            } else {
                div()
                    .min_w_0()
                    .max_w(px(140.0))
                    .truncate()
                    .child(single_line_label(&name))
            };
            row = row
                .child(icon(glyph, 12.0, theme.text_tertiary))
                .child(label);
        }
        if folder_first {
            // A non-shrinking package chip wraps as a unit when it cannot
            // fit beside the location. Clip the second line completely.
            div()
                .flex_1()
                .min_w_0()
                .h(sp(15.0))
                .overflow_hidden()
                .flex()
                .flex_wrap()
                .content_start()
                .gap(px(5.0))
                .child(row.flex_shrink_0().max_w_full().h(sp(15.0)))
                .children(package)
                .into_any_element()
        } else {
            row.into_any_element()
        }
    }

    /// Terminal ids in sidebar order — pinned rows lead the group.
    pub(super) fn sidebar_terminal_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        sidebar_terminal_order(&self.terminal_order, &self.terminal_records)
    }

    /// The directory a terminal spawns into — the record's own directory,
    /// or the owning session's workspace when the record tracks it.
    pub(super) fn terminal_spawn_directory(&self, record: &TerminalRecord) -> Option<PathBuf> {
        record.working_directory.clone().or_else(|| {
            record
                .session
                .and_then(|session_id| {
                    self.state
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                })
                .and_then(|session| self.workspace_path_for_session(session))
                .map(Path::to_path_buf)
        })
    }

    /// The directory a row reports — the live PTY cwd once the view is up,
    /// the recorded spawn directory otherwise.
    pub(super) fn terminal_cwd(&self, terminal_id: Uuid, cx: &App) -> Option<PathBuf> {
        self.right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).working_directory().to_path_buf())
            .or_else(|| {
                self.terminal_records
                    .get(&terminal_id)
                    .and_then(|record| record.last_working_directory.clone())
            })
            .or_else(|| {
                self.terminal_records
                    .get(&terminal_id)
                    .and_then(|record| self.terminal_spawn_directory(record))
            })
    }

    /// Resolve every terminal's nearest repository root in one background
    /// pass. Rows read only the store — the ancestor walk is a filesystem
    /// probe, and a miss just means "not known yet".
    pub(super) fn ensure_sidebar_terminal_repo_roots(&self, cx: &mut Context<Self>) {
        let mut fingerprint = 0x7e0e_5a1d_1e0c_a710;
        let mut directories = HashSet::new();
        for terminal_id in self.sidebar_terminal_ids() {
            fingerprint = mix_uuid(fingerprint, terminal_id);
            if let Some(cwd) = self.terminal_cwd(terminal_id, cx) {
                fingerprint = mix_str(fingerprint, &cwd.to_string_lossy());
                directories.insert(cwd);
            }
        }
        if self.sidebar_terminal_repo_scan_fingerprint.get() == Some(fingerprint) {
            return;
        }
        self.sidebar_terminal_repo_scan_fingerprint
            .set(Some(fingerprint));
        let generation = self
            .sidebar_terminal_repo_scan_generation
            .get()
            .wrapping_add(1);
        self.sidebar_terminal_repo_scan_generation.set(generation);

        if directories.is_empty() {
            self.sidebar_terminal_repo_roots.borrow_mut().clear();
            self.sidebar_terminal_locations.borrow_mut().clear();
            return;
        }
        let workspace = waku_client::WorkspaceClient::new(self.daemons.local().client());
        cx.spawn(async move |waku, cx| {
            let roots = cx
                .background_executor()
                .spawn(async move {
                    directories
                        .into_iter()
                        .map(|cwd| {
                            let root = nearest_repo_root(&cwd);
                            let location = TerminalLocation {
                                package: nearest_package_name(&cwd),
                                worktree: root.as_deref().and_then(terminal_worktree_name).or_else(
                                    || match workspace.request(
                                        waku_client::WorkspaceOperation::InspectBranchMetadata {
                                            cwd: cwd.clone(),
                                        },
                                    ) {
                                        Ok(waku_client::WorkspaceResult::BranchMetadata {
                                            snapshot: Some(snapshot),
                                        }) => {
                                            snapshot.current.and_then(|name| nonempty_name(&name))
                                        }
                                        _ => None,
                                    },
                                ),
                            };
                            (cwd, (root, location))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.sidebar_terminal_repo_scan_generation.get() != generation {
                    return;
                }
                *waku.sidebar_terminal_repo_roots.borrow_mut() = roots
                    .iter()
                    .map(|(cwd, (root, _))| (cwd.clone(), root.clone()))
                    .collect();
                *waku.sidebar_terminal_locations.borrow_mut() = roots
                    .into_iter()
                    .map(|(cwd, (_, location))| (cwd, location))
                    .collect();
                waku.sync_standalone_terminal_git_panel(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-target an open standalone terminal's Git panel after a cwd change
    /// or repository scan. The window creates the input only once the root
    /// is ready, without taking keyboard focus from the terminal.
    fn sync_standalone_terminal_git_panel(&mut self, cx: &mut Context<Self>) {
        if !self.git_panel_visible || self.standalone_git_panel_terminal().is_none() {
            return;
        }
        self.sync_git_panel_workspace(cx);
        if self.git_panel.is_some() {
            return;
        }
        let waku = cx.entity();
        let window_handle = self.window_handle;
        cx.defer(move |cx| {
            let _ = window_handle.update(cx, move |_, window, cx| {
                let _ = waku.update(cx, |this, cx| {
                    if this.git_panel_visible
                        && this.git_panel.is_none()
                        && this.standalone_git_panel_terminal().is_some()
                    {
                        this.open_git_panel(window, cx, false);
                    }
                });
            });
        });
    }

    /// The next "…ago" boundary any terminal row crosses, in seconds from
    /// `now` — the same cadence contract `next_time_label_change` keeps
    /// for session rows, so the shared wake re-renders when a label flips.
    pub(super) fn next_terminal_time_label_change(&self, now: u64, cx: &App) -> Option<u64> {
        let mut next: Option<u64> = None;
        for terminal_id in self.sidebar_terminal_ids() {
            let Some(record) = self.terminal_records.get(&terminal_id) else {
                continue;
            };
            let started_at = self
                .right_panel_terminals
                .get(&terminal_id)
                .and_then(|view| view.read(cx).last_command_started_at())
                .unwrap_or(record.opened_at);
            let elapsed = now.saturating_sub(started_at);
            let step = match elapsed {
                0..=3_599 => 60,
                3_600..=86_399 => 3_600,
                _ => 86_400,
            };
            let remaining = (step - elapsed % step).max(1);
            next = Some(next.map_or(remaining, |next| next.min(remaining)));
        }
        next
    }

    /// Register a terminal surface with the group. Called wherever a
    /// terminal surface is created; records carry what the surface lists
    /// cannot — creation order, pin state, and the spawn directory.
    pub(super) fn register_terminal(
        &mut self,
        terminal_id: Uuid,
        session: Option<Uuid>,
        working_directory: Option<PathBuf>,
    ) {
        if self.terminal_records.contains_key(&terminal_id) {
            return;
        }
        let workspace_directory = working_directory
            .is_none()
            .then(|| {
                session
                    .and_then(|session_id| {
                        self.state
                            .sessions
                            .iter()
                            .find(|session| session.id == session_id)
                    })
                    .and_then(|session| self.workspace_path_for_session(session))
                    .map(Path::to_path_buf)
            })
            .flatten();
        self.terminal_records.insert(
            terminal_id,
            TerminalRecord {
                session,
                pinned: false,
                last_working_directory: working_directory
                    .clone()
                    .or_else(|| workspace_directory.clone()),
                workspace_directory,
                working_directory,
                opened_at: unix_time(),
                custom_title: None,
                pane_of: None,
                panes: Vec::new(),
                pane_layout: None,
            },
        );
        self.terminal_order.push(terminal_id);
        self.sidebar_rows_fingerprint.set(None);
    }

    /// Drop every trace of a terminal: the view entity, its launch state,
    /// the group record, and any selection pointing at it.
    pub(super) fn drop_terminal(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        if self.maximized_terminal_pane == Some(terminal_id) {
            self.maximized_terminal_pane = None;
        }
        // Group bookkeeping before anything else: a dropped pane unlinks
        // from its anchor — a group down to its anchor stops being a
        // group — and a dropped anchor takes every pane down with it.
        if let Some(record) = self.terminal_records.get(&terminal_id) {
            let (pane_of, panes) = (record.pane_of, record.panes.clone());
            if let Some(anchor_id) = pane_of
                && let Some(anchor) = self.terminal_records.get_mut(&anchor_id)
            {
                anchor.panes.retain(|pane| *pane != terminal_id);
                if anchor.panes.is_empty()
                    && let Some(view) = self.right_panel_terminals.get(&anchor_id)
                {
                    view.update(cx, |view, _| view.set_pane(false));
                }
            }
            for pane in panes {
                self.drop_terminal(pane, cx);
            }
        }
        // A terminal filling the main area hands the view to a neighbor —
        // the row listed before it, else the one after — found before the
        // order entry disappears. No neighbor means the new task page.
        let listed = self.sidebar_terminal_ids().collect::<Vec<_>>();
        let successor = (self.selected_terminal == Some(terminal_id))
            .then(|| {
                let index = listed.iter().position(|id| *id == terminal_id)?;
                listed[..index]
                    .iter()
                    .rev()
                    .chain(listed[index + 1..].iter())
                    .copied()
                    .next()
            })
            .flatten();
        self.right_panel_terminals.remove(&terminal_id);
        self.right_panel_terminal_commands.remove(&terminal_id);
        self.right_panel_terminal_programs.remove(&terminal_id);
        self.custom_command_runs.remove(&terminal_id);
        self.terminal_records.remove(&terminal_id);
        self.terminal_order.retain(|id| *id != terminal_id);
        self.unseen_terminal_completions.remove(&terminal_id);
        self.session_navigation.remove_terminal(terminal_id);
        self.terminal_restore_directories.remove(&terminal_id);
        let scrollback_path = self.store.terminal_scrollback_path(terminal_id);
        cx.background_executor()
            .spawn(async move {
                let _ = std::fs::remove_file(scrollback_path);
            })
            .detach();
        if self.terminal_rename == Some(terminal_id) {
            self.terminal_rename = None;
        }
        if self
            .terminal_close_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.terminal_id == terminal_id)
        {
            self.terminal_close_dialog = None;
        }
        if self.docked_terminal == Some(terminal_id) {
            self.docked_terminal = None;
        }
        if self.selected_terminal == Some(terminal_id) {
            self.selected_terminal = None;
            // The dead terminal's strip transfers to Bare rather than
            // parking under a key nothing can select again — its panel
            // terminals and browsers are live utilities, not the shell's.
            self.right_panel_live_owner = RightPanelOwner::Bare;
            self.right_panel_states
                .remove(&RightPanelOwner::Terminal(terminal_id));
            self.sync_right_panel_owner(cx);
        }
        if self.last_visible_terminal == Some(terminal_id) {
            self.last_visible_terminal = None;
        }
        if self.terminal_order.is_empty() {
            // Closing the last terminal folds the group — expanding an
            // empty group is how a new global terminal is made, so a row of
            // nothing adds nothing.
            self.set_sidebar_group_collapsed(SidebarGroup::Terminals, true, cx);
        }
        self.sidebar_rows_fingerprint.set(None);
        if let Some(successor) = successor
            && let Some(focus) = self.activate_terminal_state(successor, true, cx)
        {
            // Close paths reach here without a `Window`; focus goes
            // through the stored handle like `commit_go_to_line`.
            let window_handle = self.window_handle;
            let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
        }
        self.save();
    }

    /// The record half of a detach: drop the session binding and pin the
    /// live directory so the shell keeps running where it is.
    fn detach_terminal_record(&mut self, terminal_id: Uuid, cx: &App) {
        let Some(record) = self.terminal_records.get_mut(&terminal_id) else {
            return;
        };
        record.session = None;
        if record.working_directory.is_none() {
            record.working_directory = self
                .right_panel_terminals
                .get(&terminal_id)
                .map(|terminal| terminal.read(cx).working_directory().to_path_buf());
        }
        record.last_working_directory = record
            .working_directory
            .clone()
            .or_else(|| record.last_working_directory.clone());
        record.workspace_directory = None;
    }

    /// Move a session-scoped terminal out of every session strip and into
    /// the Terminals group, keeping its PTY. Called when the workspace it
    /// tracked moved: the running shell can't follow, and respawning would
    /// kill whatever the user ran — or re-run a launch line — so the record
    /// drops its session, pins the live directory, and the surface leaves
    /// every strip that borrowed it.
    pub(super) fn detach_terminal_to_group(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        // A pane has no surface to strip and no row to reveal — its
        // record just sheds the session binding where it sits.
        if self
            .terminal_records
            .get(&terminal_id)
            .is_some_and(|record| record.pane_of.is_some())
        {
            self.detach_terminal_record(terminal_id, cx);
            return;
        }
        let Some(record) = self.terminal_records.get(&terminal_id) else {
            return;
        };
        // The group travels as one — the anchor's detach sheds every
        // member's session binding before the anchor's own strip work.
        let panes = record.panes.clone();
        for pane in panes {
            self.detach_terminal_record(pane, cx);
        }
        let Some(record) = self.terminal_records.get_mut(&terminal_id) else {
            return;
        };
        record.session = None;
        if record.working_directory.is_none() {
            record.working_directory = self
                .right_panel_terminals
                .get(&terminal_id)
                .map(|terminal| terminal.read(cx).working_directory().to_path_buf());
        }
        record.last_working_directory = record
            .working_directory
            .clone()
            .or_else(|| record.last_working_directory.clone());
        record.workspace_directory = None;
        self.remove_terminal_surfaces(terminal_id, cx);
        self.sidebar_rows_fingerprint.set(None);
        // Reveal the terminal's new home — under a folded group the row
        // hides and the vanished tab reads as a kill after all.
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
        self.reveal_sidebar_terminal(terminal_id);
        self.show_toast(tr!("terminal.moved_to_group"));
        self.save();
        cx.notify();
    }

    /// Pull `terminal_id`'s tab out of every strip that borrowed it —
    /// the live strip, then each parked owner's. The terminal's own
    /// strip keeps its tab: it belongs there while the terminal fills
    /// the main area. The record and PTY survive; only surface slots
    /// leave.
    fn remove_terminal_surfaces(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        // The live strip is the terminal's own when it fills the main area —
        // its tab belongs there; anywhere else it sat on a session's loan.
        if self.right_panel_live_owner != RightPanelOwner::Terminal(terminal_id)
            && let Some(index) = self
                .right_panel_surfaces
                .iter()
                .position(|surface| surface.terminal_id() == Some(terminal_id))
        {
            self.right_panel_surfaces.remove(index);
            self.right_panel_active_surface = if self.right_panel_surfaces.is_empty() {
                None
            } else {
                Some(match self.right_panel_active_surface {
                    Some(active) if active > index => active - 1,
                    Some(active) if active == index => index.saturating_sub(1),
                    Some(active) => active.min(self.right_panel_surfaces.len() - 1),
                    None => 0,
                })
            };
            if let Some(active) = self.right_panel_active_surface {
                self.reveal_right_panel_tab(active);
                self.request_active_terminal_focus();
                self.request_active_browser_focus();
            } else {
                self.right_panel_pending_tab_reveal = None;
                self.right_panel_pending_terminal_focus = None;
                self.right_panel_pending_browser_focus = None;
                self.set_right_panel_visible(false, cx);
            }
        }
        if self.right_panel_last_focused_terminal == Some(terminal_id) {
            self.right_panel_last_focused_terminal = None;
        }
        for (owner, state) in self.right_panel_states.iter_mut() {
            if matches!(owner, RightPanelOwner::Terminal(id) if *id == terminal_id) {
                continue;
            }
            let Some(index) = state
                .surfaces
                .iter()
                .position(|surface| surface.terminal_id() == Some(terminal_id))
            else {
                continue;
            };
            state.surfaces.remove(index);
            state.active_surface = if state.surfaces.is_empty() {
                None
            } else {
                Some(match state.active_surface {
                    Some(active) if active > index => active - 1,
                    Some(active) if active == index => index.saturating_sub(1),
                    Some(active) => active.min(state.surfaces.len() - 1),
                    None => 0,
                })
            };
            state.drop_dead_fullscreen();
            if state.last_focused_terminal == Some(terminal_id) {
                state.last_focused_terminal = None;
            }
        }
    }

    /// Spawn the PTY-backed view for a terminal id. Shared by the right
    /// panel's lazy ensure and the Terminals group's direct creation.
    pub(super) fn spawn_terminal_entity(
        &mut self,
        terminal_id: Uuid,
        working_directory: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let command = self.right_panel_terminal_commands.get(&terminal_id);
        let launch = self
            .right_panel_terminal_programs
            .get(&terminal_id)
            .map(|(program, args)| TerminalLaunch::Program {
                program: program.clone(),
                args: args.clone(),
            })
            .or_else(|| command.cloned().map(TerminalLaunch::CustomCommand))
            .unwrap_or(TerminalLaunch::Shell);
        let close_on_exit = command.is_some_and(|command| command.close_on_success);
        let restored_working_directory = self
            .terminal_restore_directories
            .remove(&terminal_id)
            .filter(|path| self.is_remote_path(path) || path.is_dir());
        let launch_directory = restored_working_directory.unwrap_or(working_directory.clone());
        let workspace_directory = self
            .terminal_records
            .get(&terminal_id)
            .filter(|record| record.working_directory.is_none())
            .map(|_| working_directory.clone());
        if let Some(record) = self.terminal_records.get_mut(&terminal_id) {
            record.workspace_directory = workspace_directory;
            if record.last_working_directory.is_none() {
                record.last_working_directory = Some(launch_directory.clone());
            }
        }
        let scrollback_path = Some(self.store.terminal_scrollback_path(terminal_id));
        let view = if self.is_remote_path(&launch_directory) {
            let daemon = self.daemon_for_path(&launch_directory);
            let owner = self
                .terminal_records
                .get(&terminal_id)
                .and_then(|record| record.session);
            cx.new(|cx| {
                TerminalView::remote_with_scrollback(
                    daemon,
                    terminal_id,
                    launch_directory.clone(),
                    owner,
                    scrollback_path,
                    cx,
                )
            })
        } else {
            cx.new(|cx| {
                TerminalView::with_launch_and_scrollback(
                    launch_directory.clone(),
                    launch,
                    scrollback_path,
                    cx,
                )
            })
        };
        cx.subscribe(&view, move |this, view, event: &TerminalViewEvent, cx| {
            match event {
                // The PTY exiting retires whatever surface held it — a
                // right-panel tab whose shell or program ended (ctrl+d,
                // `exit`, a signal), a terminal filling the main area,
                // or a close-on-success command whose launch line's
                // `&& exit` already reported through CommandFinished.
                TerminalViewEvent::Exited => {
                    // A sign-in tab's exit resolves the auth gate: probe the
                    // provider home and resubmit whatever it was holding.
                    this.sandbox_sign_in_terminal_exited(terminal_id, cx);
                    this.close_terminal_view_surface(&view, cx);
                }
                TerminalViewEvent::CommandFinished(code) => {
                    // A finished command may have changed the
                    // checkout, so drop the cached snapshot; the
                    // next read refetches.
                    this.refresh_selected_branch_snapshot(cx);
                    // Branch switches keep the same cwd, so explicitly
                    // refresh terminal labels after commands complete.
                    this.sidebar_terminal_repo_scan_fingerprint.set(None);
                    this.ensure_sidebar_terminal_repo_roots(cx);
                    // Hand-typed commands journal here; custom-command
                    // terminals already journaled at launch.
                    if !this
                        .right_panel_terminal_commands
                        .contains_key(&terminal_id)
                    {
                        let session = this
                            .terminal_records
                            .get(&terminal_id)
                            .and_then(|record| record.session)
                            .or(this.state.selected_session);
                        this.record_action(session, action_predictions::JournalAction::TerminalRun);
                    }
                    this.custom_command_finished(terminal_id, *code, cx);
                    // A clean exit off-screen earns the row an unread dot;
                    // the terminal the user is watching needs none. A
                    // pane's completion marks the anchor — the row the
                    // dot renders on.
                    if *code == Some(0)
                        && !this.terminal_is_active_surface(terminal_id)
                        && this
                            .unseen_terminal_completions
                            .insert(this.terminal_group_anchor(terminal_id))
                    {
                        cx.notify();
                    }
                    // A command launched as plain typed input leaves its
                    // shell running after a clean exit, so the report
                    // itself is the close signal the sourced line's
                    // `&& exit` provides on the fallback path.
                    if close_on_exit && *code == Some(0) {
                        this.close_terminal_view_surface(&view, cx);
                    }
                }
                TerminalViewEvent::LocalhostUrl(url) => {
                    this.on_localhost_url_detected(&view, url.clone(), cx);
                }
                // Command status or cwd changed — the sidebar row reads
                // both straight off the view; the cwd fingerprint makes
                // the repo scan re-run on its own. A `cd` in the terminal
                // on screen also re-roots the panel's files.
                TerminalViewEvent::ActivityChanged => {
                    let cwd = view.read(cx).working_directory().to_path_buf();
                    let cwd_changed =
                        this.terminal_records
                            .get_mut(&terminal_id)
                            .is_some_and(|record| {
                                if record.last_working_directory.as_ref() == Some(&cwd) {
                                    false
                                } else {
                                    record.last_working_directory = Some(cwd);
                                    true
                                }
                            });
                    if this.selected_terminal == Some(terminal_id)
                        && this.sync_right_panel_files_root(cx)
                    {
                        this.refresh_right_panel_working_tree(cx);
                    }
                    if cwd_changed {
                        this.ensure_sidebar_terminal_repo_roots(cx);
                        this.sync_standalone_terminal_git_panel(cx);
                        this.save();
                    }
                    cx.notify()
                }
                TerminalViewEvent::GenerateCommand {
                    generation,
                    request,
                    scrollback,
                    cwd,
                    shell,
                } => this.generate_terminal_command(
                    terminal_id,
                    &view,
                    *generation,
                    request.clone(),
                    scrollback.clone(),
                    cwd.clone(),
                    shell.clone(),
                    cx,
                ),
            }
        })
        .detach();
        if command.is_some() {
            // The view's 24ms poll notifies on every PTY dirty flag;
            // each one is a chance to refresh the toast's output tail.
            cx.observe(&view, move |this, _, cx| {
                this.refresh_command_run_tail(terminal_id, cx);
            })
            .detach();
        }
        // A rename that landed before the view existed applies on spawn.
        if let Some(title) = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.custom_title.clone())
        {
            view.update(cx, |view, cx| view.set_custom_title(Some(title), cx));
        }
        // A group member's view sizes to its painted cell and carries the
        // pane toolbar; a respawned view (workspace move) re-derives it here.
        if self
            .terminal_records
            .get(&terminal_id)
            .is_some_and(|record| record.pane_of.is_some() || !record.panes.is_empty())
        {
            view.update(cx, |view, _| view.set_pane(true));
        }
        self.right_panel_terminals.insert(terminal_id, view);
    }

    /// The command bar's daemon round-trip: resolve the owning session's
    /// provider — a global terminal falls back to the selected one — and
    /// run the same one-shot agent invocation the commit dialog uses. The
    /// answer lands back on the view's bar, guarded by its generation.
    fn generate_terminal_command(
        &mut self,
        terminal_id: Uuid,
        view: &Entity<TerminalView>,
        generation: u64,
        request: String,
        scrollback: String,
        cwd: PathBuf,
        shell: String,
        cx: &mut Context<Self>,
    ) {
        let session = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.session)
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
            .or_else(|| self.selected_session());
        let invocation = session.and_then(|session| {
            Some(crate::git_commit::AgentInvocation {
                provider: session.provider,
                binary: self.provider_binary_for_session(session.id, session.provider)?,
                model: self.model_for_session(session).map(str::to_owned),
                reasoning_effort: session.reasoning_effort.clone(),
            })
        });
        let Some(invocation) = invocation else {
            view.update(cx, |view, cx| {
                view.apply_command_generation(
                    generation,
                    SharedString::default(),
                    Err(tr!("terminal_command.agent_unavailable")),
                    cx,
                );
            });
            return;
        };
        let provider = SharedString::new_static(invocation.provider.display_name());
        let workspace_client = self.workspace_client_for_path(&cwd);
        let view = view.downgrade();
        cx.spawn(async move |_, cx| {
            let Some(workspace_client) = workspace_client else {
                let _ = view.update(cx, |view, cx| {
                    view.apply_command_generation(
                        generation,
                        provider,
                        Err(tr!("errors.daemon_disconnected")),
                        cx,
                    );
                });
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace_client.request(
                        waku_client::WorkspaceOperation::GenerateTerminalCommand {
                            cwd,
                            request,
                            scrollback: Some(scrollback).filter(|text| !text.trim().is_empty()),
                            shell: Some(shell).filter(|name| !name.is_empty()),
                            invocation,
                        },
                    ) {
                        Ok(waku_client::WorkspaceResult::TerminalCommand { command }) => {
                            Ok(command)
                        }
                        Ok(_) => {
                            Err("the daemon returned an invalid terminal command response"
                                .to_owned())
                        }
                        Err(error) => Err(error.to_string()),
                    }
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.apply_command_generation(generation, provider, result, cx)
            });
        })
        .detach();
    }

    /// Create a terminal rooted at `working_directory`. With `session` set
    /// it joins that task's right-panel surfaces; `None` makes it global —
    /// listed in the Terminals group but in no tab strip. A `command`
    /// replaces the plain shell launch: the PTY sources its script and the
    /// tab wears its icon.
    pub(super) fn create_terminal(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
        command: Option<CustomCommand>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        self.create_terminal_with_launch(
            working_directory,
            session,
            command.map(TerminalLaunch::CustomCommand),
            None,
            cx,
        )
    }

    /// A terminal tab running a bare program rather than a shell or a
    /// custom-command script — the sandbox sign-in `shuru run` argv lands
    /// here.
    pub(super) fn create_program_terminal(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
        program: PathBuf,
        args: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        self.create_terminal_with_launch(
            working_directory,
            session,
            Some(TerminalLaunch::Program { program, args }),
            None,
            cx,
        )
    }

    /// Fulfill a boss terminal request on the app that received its runtime
    /// intent. Terminal views are client-owned, so the daemon only records
    /// and delivers the request.
    pub(super) fn fulfill_boss_terminal_intent(
        &mut self,
        title: String,
        cwd: String,
        command: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let requested = PathBuf::from(cwd.trim());
        let remote = self.is_remote_path(&requested);
        let requested_ok = remote || requested.is_dir();
        let working_directory = if requested_ok {
            Some(requested.clone())
        } else {
            let project_root = self
                .selected_workspace_path()
                .map(Path::to_path_buf)
                .filter(|path| path.is_dir());
            project_root
                .or_else(dirs::home_dir)
                .filter(|path| path.is_dir())
        };
        if !requested_ok {
            let Some(working_directory) = working_directory.as_ref() else {
                self.show_toast(tr!("terminal.boss_directory_unavailable"));
                return;
            };
            self.show_toast(tr!(
                "terminal.boss_directory_fallback",
                requested = requested.display(),
                fallback = working_directory.display()
            ));
        }
        let Some(working_directory) = working_directory else {
            return;
        };
        let command = command.map(|script| {
            let mut command = waku_protocol::custom_commands::CustomCommand::new(script);
            command.name = Some(title.clone());
            command.icon = waku_protocol::custom_commands::CustomCommandIcon::Terminal;
            command
        });
        let Some(terminal_id) = self.create_terminal(working_directory, None, command, cx) else {
            self.show_toast(tr!("terminal.boss_create_failed"));
            return;
        };
        if let Some(record) = self.terminal_records.get_mut(&terminal_id) {
            record.custom_title = Some(title.clone());
            record.pinned = true;
        }
        if let Some(terminal) = self.right_panel_terminals.get(&terminal_id) {
            terminal.update(cx, |terminal, cx| {
                terminal.set_custom_title(Some(title), cx)
            });
        }
        self.save();
        cx.notify();
    }

    fn create_terminal_with_launch(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
        launch: Option<TerminalLaunch>,
        pane_of: Option<Uuid>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        // A remote working directory hosts a plain shell on that host's
        // daemon; commands and program launches carry local paths and
        // scripts, so they stay local-only.
        if self.is_remote_path(&working_directory)
            && !matches!(launch, None | Some(TerminalLaunch::Shell))
        {
            return None;
        }
        let session = session.filter(|session_id| {
            self.state
                .sessions
                .iter()
                .any(|session| session.id == *session_id)
                // Managed sessions keep no terminal tabs — the terminal
                // spawns unbound rather than claiming a slot their strip
                // cannot host.
                && (!self.state.boss_experiment_enabled
                    || !self.boss_ui.managed.contains(session_id))
        });
        // A terminal created at its owning session's workspace tracks the
        // workspace (`None` resolves to it at spawn); any other directory
        // is the caller's deliberate choice and the record keeps it.
        let workspace_bound = session
            .and_then(|session_id| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
            })
            .and_then(|session| self.workspace_path_for_session(session))
            .is_some_and(|workspace| workspace == working_directory.as_path());
        let terminal_id = Uuid::new_v4();
        self.register_terminal(
            terminal_id,
            session,
            (!workspace_bound).then(|| working_directory.clone()),
        );
        if pane_of.is_some()
            && let Some(record) = self.terminal_records.get_mut(&terminal_id)
        {
            record.pane_of = pane_of;
        }
        let kind = if pane_of.is_some() {
            "pane"
        } else {
            match &launch {
                Some(TerminalLaunch::CustomCommand(command)) => {
                    self.right_panel_terminal_commands
                        .insert(terminal_id, command.clone());
                    "command"
                }
                Some(TerminalLaunch::Program { program, args }) => {
                    self.right_panel_terminal_programs
                        .insert(terminal_id, (program.clone(), args.clone()));
                    "command"
                }
                Some(TerminalLaunch::Shell) | None if session.is_some() => "session",
                _ => "global",
            }
        };
        // A pane renders inside its anchor's surface — it never claims a
        // tab of its own.
        if pane_of.is_none()
            && let Some(session_id) = session
        {
            let surface = RightPanelSurface::Terminal(terminal_id);
            if self.right_panel_live_owner == RightPanelOwner::Session(session_id) {
                // Push directly rather than add_right_panel_surface: the tab
                // joins the strip without activating or stealing focus.
                self.right_panel_surfaces.push(surface);
            } else {
                self.right_panel_states
                    .entry(RightPanelOwner::Session(session_id))
                    .or_insert_with(|| RightPanelSessionState::empty(false))
                    .surfaces
                    .push(surface);
            }
        }
        self.spawn_terminal_entity(terminal_id, working_directory, cx);
        self.analytics
            .track(crate::analytics::Event::TerminalOpened { kind });
        // Every terminal lists a row in the Terminals group, but creating
        // one leaves the fold alone — a selected terminal keeps a row under
        // the collapsed header anyway. When the group is open the fresh row
        // still scrolls on screen so a creation lands nowhere the user
        // cannot see. Panes have no row to reveal.
        if pane_of.is_none() {
            self.reveal_sidebar_terminal(terminal_id);
        }
        self.save();
        cx.notify();
        Some(terminal_id)
    }

    /// Run a transcript code block in a terminal: the fence tag picks the
    /// interpreter, the block's own session roots it at its workspace — a
    /// side chat's strip is its parent's, the surface the user is looking
    /// at. A boss chat owns no project workspace, so its blocks resolve the
    /// project from the message's turn instead — see
    /// [`Self::code_run_working_directory`]. The run rides the
    /// custom-command launch, so the shell stays open with the output when
    /// the script finishes.
    pub(super) fn run_markdown_code_block(
        &mut self,
        session_id: Option<Uuid>,
        message_id: Option<Uuid>,
        language: &str,
        code: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = md::render::code_run_script(language, code) else {
            return;
        };
        let session = session_id.or(self.state.selected_session).and_then(|id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == id)
                .or_else(|| self.boss_history_session(id).map(|entry| &entry.session))
        });
        // The strip the tab joins: the session itself, or a side chat's
        // parent — a side-chat surface only renders inside it. Any other
        // attach point falls through to the full-width terminal.
        let attach = session.map(|session| session.side_chat_of.unwrap_or(session.id));
        let mut command = CustomCommand::new(run.script);
        command.name = Some(language.to_owned());
        command.icon = CustomCommandIcon::Terminal;
        command.shell = run.shell.map(str::to_owned);
        let working_directory = match session {
            Some(session) if self.session_uses_boss_workspace(session) => {
                match self.code_run_working_directory(session, message_id, code) {
                    CodeRunDirectory::Resolved(directory) => Some(directory),
                    CodeRunDirectory::Pick(candidates) => {
                        self.prompt_code_block_project(
                            PendingCodeBlockRun {
                                command,
                                attach,
                                journal_session: session_id,
                            },
                            candidates,
                            window,
                            cx,
                        );
                        return;
                    }
                }
            }
            _ => session
                .and_then(|session| self.workspace_path_for_session(session))
                .map(std::path::Path::to_path_buf),
        };
        let Some(working_directory) =
            working_directory.filter(|directory| !self.is_remote_path(directory))
        else {
            self.show_toast(tr!("code_block.no_local_workspace"));
            return;
        };
        let Some(terminal_id) = self.create_terminal(working_directory, attach, Some(command), cx)
        else {
            return;
        };
        self.finish_code_block_terminal(terminal_id, session_id, window, cx);
    }

    /// The run the code-block project picker parked, executed at the picked
    /// project's root — same terminal treatment as an inferred run.
    pub(super) fn run_code_block_in_project(
        &mut self,
        project_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.command_palette.code_block_run.take() else {
            return;
        };
        let Some(working_directory) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .filter(|project| !self.is_remote_project(project.id))
            .map(|project| project.path.clone())
        else {
            return;
        };
        let Some(terminal_id) =
            self.create_terminal(working_directory, run.attach, Some(run.command), cx)
        else {
            return;
        };
        self.finish_code_block_terminal(terminal_id, run.journal_session, window, cx);
    }

    /// The terminal a transcript code-block run spawned, landed: the tab
    /// joins the live right-panel strip when it owns one, else the
    /// full-width terminal, focused like ⌘T's fresh shell.
    fn finish_code_block_terminal(
        &mut self,
        terminal_id: Uuid,
        session_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.record_action(session_id, action_predictions::JournalAction::TerminalRun);
        if let Some(index) = self
            .right_panel_surfaces
            .iter()
            .rposition(|surface| surface.terminal_id() == Some(terminal_id))
        {
            // The tab landed in the live strip — select it, scroll it
            // into view, and give it the pending focus slot like ⌘T's
            // fresh terminal.
            self.right_panel_active_surface = Some(index);
            self.reveal_right_panel_tab(index);
            self.request_active_terminal_focus();
            self.set_right_panel_visible(true, cx);
        } else {
            self.select_terminal(terminal_id, window, cx);
        }
        cx.notify();
    }

    /// Whether `session`'s project is a boss's own workspace — the boss
    /// chat and its planning sessions. Their blocks need the message's
    /// provenance to land in a real project; every other session already
    /// names one.
    fn session_uses_boss_workspace(&self, session: &AgentSession) -> bool {
        self.boss_ui.projects.contains_key(&session.project_id)
    }

    /// Where a boss-surface code block belongs. The workspace of a task the
    /// block's turn names — a report's employee, a task-sent prompt's
    /// author — wins outright; next, the one project whose paths the
    /// message references; last, the boss's own workspace. `Pick` carries
    /// the candidates inference could not choose between — the palette
    /// asks with them first.
    fn code_run_working_directory(
        &self,
        session: &AgentSession,
        message_id: Option<Uuid>,
        code: &str,
    ) -> CodeRunDirectory {
        let message =
            message_id.and_then(|id| session.messages.iter().find(|message| message.id == id));
        let mut task_dir = None;
        let mut task_projects = Vec::new();
        for task_id in code_run_turn_tasks(session, message) {
            let Some(task) = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == task_id)
            else {
                continue;
            };
            // An expired employee's recorded worktree may already be
            // reclaimed — only a directory that exists can host the run.
            let Some(dir) = self
                .workspace_path_for_session(task)
                .filter(|dir| dir.is_dir() && !self.is_remote_path(dir))
            else {
                continue;
            };
            if task_dir.is_none() {
                task_dir = Some(dir.to_path_buf());
            }
            if !task_projects.contains(&task.project_id) {
                task_projects.push(task.project_id);
            }
        }
        match task_projects.len() {
            1 => return CodeRunDirectory::Resolved(task_dir.expect("a project implies a dir")),
            2.. => return CodeRunDirectory::Pick(task_projects),
            _ => {}
        }
        let roots = self.code_run_project_roots();
        let referenced = code_run_content_projects(
            &roots,
            message.map(Message::visible_content).unwrap_or(code),
            code,
            self.home_directory.as_deref(),
        );
        match referenced.as_slice() {
            [project] => {
                if let Some((_, root)) = roots.iter().find(|(id, _)| id == project) {
                    return CodeRunDirectory::Resolved(root.to_path_buf());
                }
            }
            [_, _, ..] => return CodeRunDirectory::Pick(referenced),
            _ => {}
        }
        match self
            .workspace_path_for_session(session)
            .filter(|dir| !self.is_remote_path(dir))
        {
            Some(dir) => CodeRunDirectory::Resolved(dir.to_path_buf()),
            // Nothing named a project — the picker offers every one it has.
            None => CodeRunDirectory::Pick(Vec::new()),
        }
    }

    /// The local projects a transcript code block can run in — the daemon's
    /// real projects plus each boss's own workspace. Remote rows and the
    /// projectless destination have no local directory a desktop PTY opens.
    fn code_run_project_roots(&self) -> Vec<(Uuid, &Path)> {
        let mut roots: Vec<(Uuid, &Path)> = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless() && !self.is_remote_project(project.id))
            .map(|project| (project.id, project.path.as_path()))
            .collect();
        for project in self.boss_ui.projects.values() {
            if !self.is_remote_project(project.id) && roots.iter().all(|(id, _)| *id != project.id)
            {
                roots.push((project.id, project.path.as_path()));
            }
        }
        roots
    }

    /// The terminal id the surface references — `terminal_id` itself
    /// unless it's a pane, whose surface is its anchor's.
    pub(super) fn terminal_group_anchor(&self, terminal_id: Uuid) -> Uuid {
        self.terminal_records
            .get(&terminal_id)
            .and_then(|record| record.pane_of)
            .unwrap_or(terminal_id)
    }

    /// A group's members in layout order — the anchor first, then its
    /// panes in the order they were split. Ungrouped terminals yield
    /// just themselves; a missing record yields nothing.
    pub(super) fn terminal_group_panes(&self, anchor_id: Uuid) -> Vec<Uuid> {
        terminal_group_ids(&self.terminal_records, anchor_id)
    }

    /// Whether the terminal is the surface on screen — the full-width
    /// selection, or the visible right panel's active tab. An inactive tab
    /// or a background session's surface counts as unseen even when its
    /// stored strip still points at it.
    pub(super) fn terminal_is_active_surface(&self, terminal_id: Uuid) -> bool {
        // A group stands or falls with its anchor's visibility — a pane
        // on screen is seen wherever its anchor renders.
        let anchor_id = self.terminal_group_anchor(terminal_id);
        self.selected_terminal == Some(anchor_id)
            || self.docked_terminal == Some(anchor_id)
            || (self.right_panel_visible
                && self
                    .active_right_panel_surface()
                    .and_then(RightPanelSurface::terminal_id)
                    == Some(anchor_id))
    }

    /// Split `anchor_id`'s surface, adding a fresh shell rooted in the
    /// anchor's current directory. The group's size is hard-capped at
    /// `max_panes` — the right-panel stack or the standalone grid picks
    /// the limit — and `None` returns at the cap.
    pub(super) fn split_terminal(
        &mut self,
        anchor_id: Uuid,
        max_panes: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        self.split_terminal_in_direction(anchor_id, max_panes, false, window, cx)
    }

    fn split_terminal_in_direction(
        &mut self,
        anchor_id: Uuid,
        max_panes: usize,
        down: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let record = self.terminal_records.get(&anchor_id)?;
        if !terminal_group_can_add(record, 1, max_panes) {
            return None;
        }
        let target = self
            .focused_terminal_id(window, cx)
            .filter(|id| self.terminal_group_anchor(*id) == anchor_id)
            .unwrap_or(anchor_id);
        let mut layout = terminal_group_layout(
            &self.terminal_records,
            anchor_id,
            max_panes == RIGHT_PANEL_TERMINAL_MAX_PANES,
        )?;
        let session = record.session;
        let growing_beyond_stack = record.panes.len() + 2 > RIGHT_PANEL_TERMINAL_MAX_PANES;
        let working_directory = self
            .terminal_cwd(anchor_id, cx)
            .or_else(|| self.terminal_spawn_directory(record))
            .or_else(|| self.selected_workspace_path().map(Path::to_path_buf))
            .or_else(dirs::home_dir)?;
        let pane_id = self.create_terminal_with_launch(
            working_directory,
            session,
            Some(TerminalLaunch::Shell),
            Some(anchor_id),
            cx,
        )?;
        if growing_beyond_stack {
            self.remove_terminal_surfaces(anchor_id, cx);
        }
        if let Some(record) = self.terminal_records.get_mut(&anchor_id) {
            record.panes.push(pane_id);
            split_terminal_layout(&mut layout, target, pane_id, down);
            record.pane_layout = Some(layout);
        }
        if self.selected_terminal == Some(anchor_id) {
            self.maximized_terminal_pane = None;
        }
        if let Some(anchor) = self.right_panel_terminals.get(&anchor_id) {
            anchor.update(cx, |view, _| view.set_pane(true));
        }
        if let Some(view) = self.right_panel_terminals.get(&pane_id) {
            let focus = view.read(cx).focus_handle(cx);
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        }
        self.save();
        cx.notify();
        Some(pane_id)
    }

    /// `secondary-alt-d` — split whichever terminal surface the user is
    /// looking at. A focused member's group decides the surface and its
    /// limit (3 panes in a right-panel tab, 9 in the main area).
    /// With no terminal focused the
    /// main-area selection splits first, then the active panel tab.
    pub(super) fn split_terminal_action(
        &mut self,
        _: &SplitTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_terminal_surface(false, window, cx);
    }

    pub(super) fn split_terminal_down_action(
        &mut self,
        _: &SplitTerminalDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_terminal_surface(true, window, cx);
    }

    fn split_terminal_surface(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = None;
        let focused_anchor = self
            .focused_terminal_id(window, cx)
            .map(|terminal_id| self.terminal_group_anchor(terminal_id));
        if let Some(anchor_id) = focused_anchor {
            let max_panes = if self.selected_terminal == Some(anchor_id)
                || self.docked_terminal == Some(anchor_id)
            {
                STANDALONE_TERMINAL_MAX_PANES
            } else {
                RIGHT_PANEL_TERMINAL_MAX_PANES
            };
            self.split_terminal_in_direction(anchor_id, max_panes, down, window, cx);
            return;
        }
        if let Some(anchor_id) = self.selected_terminal {
            self.split_terminal_in_direction(
                anchor_id,
                STANDALONE_TERMINAL_MAX_PANES,
                down,
                window,
                cx,
            );
            return;
        }
        if let Some(anchor_id) = self
            .active_right_panel_surface()
            .and_then(RightPanelSurface::terminal_id)
        {
            self.split_terminal_in_direction(
                anchor_id,
                RIGHT_PANEL_TERMINAL_MAX_PANES,
                down,
                window,
                cx,
            );
        }
    }

    /// `secondary-t`'s in-place split for a focused floating card or a
    /// maximized standalone — a fresh pane joins the group rooted in the
    /// anchor's directory and takes focus. At the group's cap nothing is
    /// created, hidden, or replaced: the chord reports the limit the same
    /// way the drop affordance does and returns `false`.
    fn new_terminal_split(
        &mut self,
        anchor_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(record) = self.terminal_records.get(&anchor_id) else {
            return false;
        };
        if !terminal_group_can_add(record, 1, STANDALONE_TERMINAL_MAX_PANES) {
            self.show_toast(tr!(
                "terminal.split_at_max",
                max = STANDALONE_TERMINAL_MAX_PANES
            ));
            return false;
        }
        self.split_terminal(anchor_id, STANDALONE_TERMINAL_MAX_PANES, window, cx)
            .is_some()
    }

    pub(super) fn toggle_terminal_maximize_action(
        &mut self,
        _: &ToggleTerminalMaximize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focused_terminal = self.focused_terminal_id(window, cx);
        if let Some(terminal_id) = self.selected_terminal {
            if self.terminal_group_panes(terminal_id).len() == 1
                && focused_terminal
                    .is_some_and(|focused| self.terminal_group_anchor(focused) == terminal_id)
            {
                self.minimize_terminal_to_card(terminal_id, window, cx);
                return;
            }
        }
        if let Some(terminal_id) = self.docked_terminal
            && focused_terminal
                .is_some_and(|focused| self.terminal_group_anchor(focused) == terminal_id)
        {
            self.activate_terminal(terminal_id, true, window, cx);
            return;
        }
        let Some(anchor_id) = self.selected_terminal else {
            return;
        };
        let target_id = self
            .focused_terminal_id(window, cx)
            .filter(|terminal_id| self.terminal_group_anchor(*terminal_id) == anchor_id)
            .or_else(|| {
                self.maximized_terminal_pane.filter(|terminal_id| {
                    self.terminal_group_panes(anchor_id).contains(terminal_id)
                })
            })
            .unwrap_or(anchor_id);
        let panes = self.terminal_group_panes(anchor_id);
        if !panes.contains(&target_id) {
            return;
        }
        self.maximized_terminal_pane =
            toggle_maximized_terminal_pane(self.maximized_terminal_pane, target_id, &panes);
        if let Some(focus) = self
            .right_panel_terminals
            .get(&target_id)
            .map(|terminal| terminal.read(cx).focus_handle(cx))
        {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Any terminal view holding keyboard focus — a pane or an anchor in
    /// either surface. The scan only walks the two groups that can be on
    /// screen: the main-area selection and the active panel tab.
    fn focused_terminal_id(&self, window: &Window, cx: &App) -> Option<Uuid> {
        let candidates = self
            .selected_terminal
            .into_iter()
            .chain(self.docked_terminal)
            .flat_map(|anchor_id| self.terminal_group_panes(anchor_id))
            .chain(
                self.active_right_panel_surface()
                    .and_then(RightPanelSurface::terminal_id)
                    .into_iter()
                    .flat_map(|anchor_id| self.terminal_group_panes(anchor_id)),
            );
        for terminal_id in candidates {
            if self
                .right_panel_terminals
                .get(&terminal_id)
                .is_some_and(|terminal| terminal.read(cx).focus_handle(cx).is_focused(window))
            {
                return Some(terminal_id);
            }
        }
        None
    }

    /// The group anchor of the terminal surface the keyboard is on — a
    /// focused member's anchor, or the floating card's while focus sits
    /// on its chrome (the split/expand/close and pane-close buttons are
    /// siblings of the views, so `focused_terminal_id` alone misses
    /// them). ⌘T reads the card's chrome focus as "a floating terminal
    /// is focused" so the chord still splits instead of replacing it.
    fn focused_terminal_anchor(&self, window: &Window, cx: &App) -> Option<Uuid> {
        if let Some(terminal_id) = self.focused_terminal_id(window, cx) {
            return Some(self.terminal_group_anchor(terminal_id));
        }
        let anchor_id = self
            .docked_terminal
            .filter(|id| self.docked_terminal_visible(*id))?;
        let chrome = [
            "docked-terminal-split".to_owned(),
            format!("docked-terminal-expand-{anchor_id}"),
            format!("docked-terminal-close-{anchor_id}"),
        ]
        .into_iter()
        .chain(
            self.terminal_group_panes(anchor_id)
                .into_iter()
                .map(|pane_id| format!("terminal-pane-close-{pane_id}")),
        );
        chrome
            .filter_map(|key| self.transcript_control_focuses.borrow().get(&key).cloned())
            .any(|handle| handle.is_focused(window))
            .then_some(anchor_id)
    }

    /// Close one member of a split group — a pane leaves its anchor, and
    /// an anchor promotes its next pane so the surface stays. Ungrouped
    /// terminals close outright.
    pub(super) fn close_terminal_pane(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        let Some(record) = self.terminal_records.get(&terminal_id) else {
            return;
        };
        if record.pane_of.is_none() && record.panes.is_empty() {
            self.close_terminal(terminal_id, cx);
            return;
        }
        let closing_focus = self
            .right_panel_terminals
            .get(&terminal_id)
            .map(|view| view.read(cx).focus_handle(cx));
        let close_focus =
            self.transcript_control_focus(format!("terminal-pane-close-{terminal_id}"), cx);
        let anchor_id = self.terminal_group_anchor(terminal_id);
        let survivor = self
            .terminal_group_panes(anchor_id)
            .into_iter()
            .find(|id| *id != terminal_id);
        if record.pane_of.is_none() {
            self.promote_terminal_pane(terminal_id, cx);
        }
        self.drop_terminal(terminal_id, cx);
        if let Some(focus) = survivor
            .and_then(|id| self.right_panel_terminals.get(&id))
            .map(|view| view.read(cx).focus_handle(cx))
        {
            let handle = self.window_handle;
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    if window.focused(cx).is_none_or(|current| {
                        closing_focus.as_ref() == Some(&current) || current == close_focus
                    }) {
                        window.focus(&focus, cx);
                    }
                });
            });
        }
    }

    /// Move a group's surface identity from a closing anchor to its next
    /// pane — every reference (tabs, selection, panel owner, history)
    /// rekeys to the promoted terminal. The old anchor's record empties
    /// its pane list so `drop_terminal` treats it as ungrouped.
    fn promote_terminal_pane(&mut self, anchor_id: Uuid, cx: &mut Context<Self>) {
        let Some(mut panes) = self
            .terminal_records
            .get(&anchor_id)
            .map(|record| record.panes.clone())
            .filter(|panes| !panes.is_empty())
        else {
            return;
        };
        let new_anchor = panes.remove(0);
        if !self.terminal_records.contains_key(&new_anchor) {
            return;
        }
        let pinned = self
            .terminal_records
            .get(&anchor_id)
            .is_some_and(|record| record.pinned);
        self.terminal_order.retain(|id| *id != new_anchor);
        if let Some(index) = self.terminal_order.iter().position(|id| *id == anchor_id) {
            self.terminal_order.insert(index, new_anchor);
        }
        let pane_layout = self
            .terminal_records
            .get_mut(&anchor_id)
            .and_then(|record| {
                record.panes.clear();
                record.pane_layout.take()
            });
        if let Some(record) = self.terminal_records.get_mut(&new_anchor) {
            record.pinned |= pinned;
            record.pane_of = None;
            record.panes = panes.clone();
            record.pane_layout = pane_layout;
        }
        for pane in &panes {
            if let Some(record) = self.terminal_records.get_mut(pane) {
                record.pane_of = Some(new_anchor);
            }
        }
        // A single survivor stops being a group — its view loses the
        // pane chrome along with the membership.
        if panes.is_empty()
            && let Some(view) = self.right_panel_terminals.get(&new_anchor)
        {
            view.update(cx, |view, _| view.set_pane(false));
        }
        let retarget = |surface: &mut RightPanelSurface| {
            if *surface == RightPanelSurface::Terminal(anchor_id) {
                *surface = RightPanelSurface::Terminal(new_anchor);
            }
        };
        for surface in self.right_panel_surfaces.iter_mut() {
            retarget(surface);
        }
        for state in self.right_panel_states.values_mut() {
            for surface in state.surfaces.iter_mut() {
                retarget(surface);
            }
            if let Some((surface, _)) = state.fullscreen.as_mut() {
                retarget(surface);
            }
        }
        if let Some((surface, _)) = self.fullscreen_surface.as_mut() {
            retarget(surface);
        }
        if let Some(state) = self
            .right_panel_states
            .remove(&RightPanelOwner::Terminal(anchor_id))
        {
            self.right_panel_states
                .insert(RightPanelOwner::Terminal(new_anchor), state);
        }
        if self.right_panel_live_owner == RightPanelOwner::Terminal(anchor_id) {
            self.right_panel_live_owner = RightPanelOwner::Terminal(new_anchor);
        }
        for entry in self
            .session_navigation
            .back
            .iter_mut()
            .chain(self.session_navigation.forward.iter_mut())
        {
            if *entry == NavigationLocation::Terminal(anchor_id) {
                *entry = NavigationLocation::Terminal(new_anchor);
            }
        }
        if self.selected_terminal == Some(anchor_id) {
            self.selected_terminal = Some(new_anchor);
        }
        if self.docked_terminal == Some(anchor_id) {
            self.docked_terminal = Some(new_anchor);
        }
        if self.last_visible_terminal == Some(anchor_id) {
            self.last_visible_terminal = Some(new_anchor);
        }
        if self.right_panel_last_focused_terminal == Some(anchor_id) {
            self.right_panel_last_focused_terminal = Some(new_anchor);
        }
        if self.right_panel_pending_terminal_focus == Some(anchor_id) {
            self.right_panel_pending_terminal_focus = Some(new_anchor);
        }
        if self.terminal_rename == Some(anchor_id) {
            self.terminal_rename = None;
        }
        self.unseen_terminal_completions.remove(&anchor_id);
        if let Some(dialog) = self.terminal_close_dialog.as_mut()
            && dialog.terminal_id == anchor_id
        {
            dialog.terminal_id = new_anchor;
        }
    }

    /// Hover uses the same floating-card selection as a normal sidebar open.
    /// Cancellation restores the prior card and panel without a history visit.
    pub(super) fn preview_terminal_drag(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.docked_terminal == Some(terminal_id) || self.selected_terminal == Some(terminal_id)
        {
            return;
        }
        if self.terminal_drag_preview.is_none() {
            self.terminal_drag_preview = Some(TerminalDragPreview {
                docked: self.docked_terminal,
                last_visible: self.last_visible_terminal,
                panel_active: self.active_right_panel_surface().cloned(),
                fullscreen: self.fullscreen_surface.clone(),
                focus: window.focused(cx),
            });
        }
        self.open_docked_terminal(terminal_id, window, cx);
    }

    fn restore_terminal_drag_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.terminal_drag_preview.take() else {
            return;
        };
        self.docked_terminal = preview
            .docked
            .filter(|id| self.terminal_records.contains_key(id))
            .map(|id| self.terminal_group_anchor(id));
        self.last_visible_terminal = preview
            .last_visible
            .filter(|id| self.terminal_records.contains_key(id))
            .map(|id| self.terminal_group_anchor(id));
        self.right_panel_active_surface = preview.panel_active.and_then(|surface| {
            self.right_panel_surfaces
                .iter()
                .position(|candidate| *candidate == surface)
        });
        self.fullscreen_surface = preview.fullscreen.filter(|(surface, _)| {
            surface.terminal_id().is_none_or(|id| {
                self.terminal_group_anchor(id) == id && self.terminal_records.contains_key(&id)
            })
        });
        if let Some(focus) = preview.focus {
            window.focus(&focus, cx);
        }
        self.save();
        cx.notify();
    }

    fn watch_terminal_drag(weak: WeakEntity<Self>, window: &mut Window, cx: &mut App) {
        if !cx.has_active_drag() {
            let _ = weak.update(cx, |this, cx| {
                this.terminal_drag_rejected_target = None;
                this.restore_terminal_drag_preview(window, cx)
            });
            return;
        }
        window.on_next_frame(move |window, cx| Self::watch_terminal_drag(weak, window, cx));
        window.refresh();
    }

    /// Whether `anchor_id`'s group can absorb `dragged_id`'s whole group
    /// — itself included — and stay under `max_panes`. `can_drop` and
    /// the hover affordance share the test so a drop that would
    /// overflow is rejected before it can land.
    fn terminal_drop_fits(&self, dragged_id: Uuid, anchor_id: Uuid, max_panes: usize) -> bool {
        terminal_groups_fit(&self.terminal_records, dragged_id, anchor_id, max_panes)
    }

    /// The drop side of a terminal drag: `dragged_id`'s group members
    /// leave every strip and join `anchor_id`'s panes as live
    /// terminals. Returns the moved ids — empty when the drop can't
    /// fit, so the caller can no-op.
    fn attach_terminal_pane_group(
        &mut self,
        dragged_id: Uuid,
        anchor_id: Uuid,
        max_panes: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Uuid> {
        if !self.terminal_drop_fits(dragged_id, anchor_id, max_panes) {
            return Vec::new();
        }
        let incoming = self.terminal_group_panes(dragged_id);
        for terminal_id in &incoming {
            if !self.right_panel_terminals.contains_key(terminal_id)
                && let Some(directory) = self
                    .terminal_records
                    .get(terminal_id)
                    .and_then(|record| self.terminal_spawn_directory(record))
            {
                self.spawn_terminal_entity(*terminal_id, directory, cx);
            }
            self.remove_terminal_surfaces(*terminal_id, cx);
        }
        if self
            .fullscreen_surface
            .as_ref()
            .is_some_and(|(surface, _)| {
                surface
                    .terminal_id()
                    .is_some_and(|id| incoming.contains(&id))
            })
        {
            self.fullscreen_surface = None;
        }
        if self.terminal_group_panes(anchor_id).len() + incoming.len()
            > RIGHT_PANEL_TERMINAL_MAX_PANES
        {
            self.remove_terminal_surfaces(anchor_id, cx);
        }
        let incoming =
            join_terminal_groups(&mut self.terminal_records, dragged_id, anchor_id, max_panes);
        for terminal_id in std::iter::once(&anchor_id).chain(&incoming) {
            if let Some(view) = self.right_panel_terminals.get(terminal_id) {
                view.update(cx, |view, _| view.set_pane(true));
            }
        }
        // References pointing at the dragged terminal follow it into
        // the group — a selected terminal's main-area slot hands over
        // to its new anchor.
        if self.docked_terminal == Some(dragged_id) {
            self.docked_terminal = Some(anchor_id);
        }
        if self.selected_terminal == Some(dragged_id) {
            self.selected_terminal = Some(anchor_id);
        }
        if self.last_visible_terminal == Some(dragged_id) {
            self.last_visible_terminal = Some(anchor_id);
        }
        if self.right_panel_last_focused_terminal == Some(dragged_id) {
            self.right_panel_last_focused_terminal = Some(anchor_id);
        }
        if self.terminal_rename == Some(dragged_id) {
            self.terminal_rename = None;
        }
        self.sidebar_rows_fingerprint.set(None);
        if let Some(view) = self.right_panel_terminals.get(&dragged_id) {
            let focus = view.read(cx).focus_handle(cx);
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        }
        self.save();
        cx.notify();
        incoming
    }

    /// A terminal drag dropped on a terminal surface: clear the hover
    /// preview, attach the dragged group, and commit the anchor's
    /// selection — a previewed anchor's temporary selection becomes
    /// real now that the group genuinely owns the main area.
    pub(super) fn accept_terminal_pane_drop(
        &mut self,
        dragged_id: Uuid,
        anchor_id: Uuid,
        max_panes: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preview = self.terminal_drag_preview.take();
        if self
            .attach_terminal_pane_group(dragged_id, anchor_id, max_panes, window, cx)
            .is_empty()
        {
            self.terminal_drag_preview = preview;
            self.restore_terminal_drag_preview(window, cx);
            return;
        }
        if max_panes == STANDALONE_TERMINAL_MAX_PANES {
            if self.selected_terminal == Some(anchor_id) {
                self.activate_terminal_state(anchor_id, false, cx);
            } else {
                self.open_docked_terminal(anchor_id, window, cx);
            }
        } else {
            self.terminal_drag_preview = preview;
            self.restore_terminal_drag_preview(window, cx);
        }
        self.save();
    }

    /// The drop affordance over a terminal surface: a sidebar terminal
    /// dropped here joins the surface's split group. `can_drop` turns
    /// down what wouldn't fit — at the cap, or a group into itself — so
    /// the highlight never promises a drop that can't land.
    pub(super) fn terminal_drop_target(
        &self,
        element: Stateful<Div>,
        anchor_id: Uuid,
        max_panes: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        element
            .on_drag_move::<SidebarTerminalDrag>(cx.listener(
                move |this, event: &gpui::DragMoveEvent<SidebarTerminalDrag>, window, cx| {
                    if event.bounds.contains(&window.mouse_position()) {
                        let dragged = event.drag(cx).terminal_id;
                        if dragged != anchor_id
                            && !this.terminal_drop_fits(dragged, anchor_id, max_panes)
                            && this.terminal_drag_rejected_target != Some(anchor_id)
                        {
                            this.terminal_drag_rejected_target = Some(anchor_id);
                            this.show_toast(tr!("terminal.split_at_max", max = max_panes));
                            cx.notify();
                        }
                    }
                },
            ))
            .can_drop({
                let weak = weak.clone();
                move |dragged, _, app| {
                    let Some(drag) = dragged.downcast_ref::<SidebarTerminalDrag>() else {
                        return false;
                    };
                    weak.read_with(app, |this, _| {
                        this.terminal_drop_fits(drag.terminal_id, anchor_id, max_panes)
                    })
                    .unwrap_or(false)
                }
            })
            .drag_over::<SidebarTerminalDrag>(move |style, drag, _, app| {
                let fits = weak
                    .read_with(app, |this, _| {
                        this.terminal_drop_fits(drag.terminal_id, anchor_id, max_panes)
                    })
                    .unwrap_or(false);
                if fits {
                    style
                        .border(px(2.0))
                        .border_color(theme.accent)
                        .bg(theme.accent.opacity(0.08))
                } else {
                    style
                }
            })
            .on_drop(
                cx.listener(move |this, drag: &SidebarTerminalDrag, window, cx| {
                    this.accept_terminal_pane_drop(
                        drag.terminal_id,
                        anchor_id,
                        max_panes,
                        window,
                        cx,
                    );
                }),
            )
    }

    /// Swap a sidebar terminal row's title for the shared rename field,
    /// seeded with the terminal's current title.
    fn begin_terminal_rename(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.terminal_records.contains_key(&terminal_id) {
            return;
        }
        let title = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.custom_title.clone())
            .or_else(|| {
                self.right_panel_terminals
                    .get(&terminal_id)
                    .map(|terminal| terminal.read(cx).title().to_owned())
                    .filter(|title| !title.is_empty())
            })
            .unwrap_or_else(|| tr!("right_panel.terminal"));

        self.session_rename = None;
        self.terminal_rename = Some(terminal_id);
        self.session_rename_input.update(cx, |input, cx| {
            input.set_content(title, cx);
            input.select_all_text(cx);
        });
        let focus = self.session_rename_input.read(cx).focus();
        window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        cx.notify();
    }

    pub(super) fn commit_terminal_rename(&mut self, cx: &mut Context<Self>) {
        let Some(terminal_id) = self.terminal_rename.take() else {
            return;
        };
        let title = self
            .session_rename_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        if !title.is_empty() {
            if let Some(record) = self.terminal_records.get_mut(&terminal_id) {
                record.custom_title = Some(title.clone());
            }
            if let Some(terminal) = self.right_panel_terminals.get(&terminal_id) {
                terminal.update(cx, |terminal, cx| {
                    terminal.set_custom_title(Some(title), cx);
                });
            }
            self.save();
        }
        cx.notify();
    }

    fn cancel_terminal_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_rename.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Show a terminal full-width in the main area. Entering terminal mode
    /// parks the session selection — composer draft, transcript scroll, and
    /// right-panel state are stored exactly as a session switch stores them.
    pub(super) fn select_terminal(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_terminal(terminal_id, true, window, cx);
    }

    /// `record_visit` is false only for back/forward history, which already
    /// moved the stacks before landing here.
    pub(super) fn activate_terminal(
        &mut self,
        terminal_id: Uuid,
        record_visit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focus) = self.activate_terminal_state(terminal_id, record_visit, cx) {
            window.focus(&focus, cx);
        }
    }

    /// The state half of activation — spawn the view if needed, record the
    /// visit, move selection — returning the terminal's focus handle so a
    /// caller with or without a `Window` can aim it.
    pub(super) fn activate_terminal_state(
        &mut self,
        terminal_id: Uuid,
        record_visit: bool,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        // A pane's surface is its anchor's — selecting a member lands
        // on the group.
        let terminal_id = self.terminal_group_anchor(terminal_id);
        let record = self.terminal_records.get(&terminal_id)?;
        if !self.right_panel_terminals.contains_key(&terminal_id) {
            // Remote directories spawn through their host's daemon; a
            // disconnected host surfaces as the view's spawn error.
            let working_directory = self.terminal_spawn_directory(record)?;
            self.spawn_terminal_entity(terminal_id, working_directory, cx);
        }
        for pane_id in self.terminal_group_panes(terminal_id).into_iter().skip(1) {
            if !self.right_panel_terminals.contains_key(&pane_id)
                && let Some(directory) = self
                    .terminal_records
                    .get(&pane_id)
                    .and_then(|record| self.terminal_spawn_directory(record))
            {
                self.spawn_terminal_entity(pane_id, directory, cx);
            }
        }
        if record_visit {
            self.session_navigation.visit(
                self.navigation_location(),
                NavigationLocation::Terminal(terminal_id),
            );
        }
        if self.state.selected_session.is_some() {
            self.capture_and_save_current_composer_draft(cx);
            self.store_transcript_scroll_position();
            self.state.selected_session = None;
            self.sync_voice_scratchpad_capture(cx);
        }
        self.pending_session_activation = None;
        // A terminal claims the main area too: open pages fold, keeping
        // their state for the next visit.
        self.projects_page = None;
        self.drafts_page = false;
        self.automations_page = false;
        self.boss_ui.page = None;
        self.automations_detail = None;
        self.notifications.open = false;
        self.selected_terminal = Some(terminal_id);
        if self.docked_terminal == Some(terminal_id) {
            // The standalone page and the card are two homes for one
            // view — taking the main area releases the dock.
            self.docked_terminal = None;
        }
        self.last_visible_terminal = Some(terminal_id);
        self.unseen_terminal_completions.remove(&terminal_id);
        // Whatever owned the strip — a session, a page, or another
        // terminal — parks; this terminal's own strip comes back.
        self.sync_right_panel_owner(cx);
        // The terminal may have `cd`'d while it was off screen, or its
        // strip may have come back rooted somewhere else.
        if self.sync_right_panel_files_root(cx) {
            self.refresh_right_panel_working_tree(cx);
        }
        self.sync_standalone_terminal_git_panel(cx);
        let focus = self
            .right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).focus_handle(cx));
        self.save();
        cx.notify();
        focus
    }

    /// Fold the group open and land on the last terminal that was on
    /// screen — the newest one if none has been shown yet, or a fresh
    /// global terminal in ~ when the group is empty. Deliberate opens hold
    /// the group against the fold that selections and pages apply.
    pub(super) fn expand_terminals_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminals_group_held_open = true;
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
        let target = self
            .last_visible_terminal
            .filter(|id| self.terminal_records.contains_key(id))
            .or_else(|| {
                self.terminal_order
                    .iter()
                    .rev()
                    .copied()
                    .find(|id| self.terminal_records.contains_key(id))
            });
        let terminal_id = match target {
            Some(terminal_id) => Some(terminal_id),
            None => dirs::home_dir().and_then(|home| self.create_terminal(home, None, None, cx)),
        };
        if let Some(terminal_id) = terminal_id {
            self.select_terminal(terminal_id, window, cx);
        }
    }

    /// Folding the group only hides its rows; the active page stays selected.
    pub(super) fn collapse_terminals_group(&mut self, cx: &mut Context<Self>) {
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, true, cx);
    }

    /// Picking a chat or opening a page folds the Terminals group — the
    /// terminal keeps its last-visible memory for the next expand — unless
    /// the user opened the group deliberately, in which case the rows stay
    /// on screen.
    pub(super) fn fold_terminals_group_for_navigation(&mut self) {
        if !self.terminals_group_held_open
            && self
                .sidebar_collapsed_groups
                .insert(SidebarGroup::Terminals)
        {
            self.sidebar_rows_fingerprint.set(None);
        }
    }

    /// Mount a terminal as the card floating at the top of the main
    /// column — the selected surface stays put underneath; the card is a
    /// layer over it, not a page. The card is how ⌘T, the "New terminal
    /// in…" pick, and the standalone page's minimize open a terminal; the
    /// row of the terminal already filling the main area just re-focuses
    /// its standalone view.
    pub(super) fn open_docked_terminal(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_terminal == Some(terminal_id) {
            self.select_terminal(terminal_id, window, cx);
            return;
        }
        let Some(record) = self.terminal_records.get(&terminal_id) else {
            return;
        };
        if !self.right_panel_terminals.contains_key(&terminal_id) {
            // Remote directories spawn through their host's daemon; a
            // disconnected host surfaces as the view's spawn error.
            let Some(working_directory) = self.terminal_spawn_directory(record) else {
                return;
            };
            self.spawn_terminal_entity(terminal_id, working_directory, cx);
        }
        // One view, one mount: while the terminal is the right panel's
        // active tab — docked or maximized — the strip hands its slot to
        // a neighbor so the card can take the view. A maximized strip
        // would hide the card under its layer, so it restores first.
        if self
            .fullscreen_surface
            .as_ref()
            .and_then(|(surface, _)| surface.terminal_id())
            == Some(terminal_id)
        {
            self.toggle_panel_fullscreen(cx);
        }
        if self.terminal_mounts_in_right_panel(terminal_id) {
            self.right_panel_active_surface = (0..self.right_panel_surfaces.len())
                .find(|index| Some(*index) != self.right_panel_active_surface);
            if let Some(index) = self.right_panel_active_surface {
                self.reveal_right_panel_tab(index);
            }
        }
        for pane_id in self.terminal_group_panes(terminal_id).into_iter().skip(1) {
            if !self.right_panel_terminals.contains_key(&pane_id)
                && let Some(directory) = self
                    .terminal_records
                    .get(&pane_id)
                    .and_then(|record| self.terminal_spawn_directory(record))
            {
                self.spawn_terminal_entity(pane_id, directory, cx);
            }
        }
        self.docked_terminal = Some(terminal_id);
        self.last_visible_terminal = Some(terminal_id);
        self.unseen_terminal_completions.remove(&terminal_id);
        let focus = self
            .right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).focus_handle(cx));
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Whether the right panel — docked or maximized — currently mounts
    /// the terminal's view. Only the active tab's view is painted, so a
    /// parked or background strip holding the same tab is no conflict.
    fn terminal_mounts_in_right_panel(&self, terminal_id: Uuid) -> bool {
        (self.panel_fullscreen_active() || (self.right_panel_visible && !self.git_panel_visible))
            && self
                .active_right_panel_surface()
                .and_then(RightPanelSurface::terminal_id)
                == Some(terminal_id)
    }

    /// Whether the docked card can paint — set, and no other surface
    /// holds the view's mount this frame. A strip tab or the standalone
    /// page that claimed it suppresses the card until it lets go; the
    /// dock itself stays.
    pub(super) fn docked_terminal_visible(&self, terminal_id: Uuid) -> bool {
        self.right_panel_terminals.contains_key(&terminal_id)
            && self.selected_terminal != Some(terminal_id)
            && !self.terminal_mounts_in_right_panel(terminal_id)
    }

    /// Close the card without killing the terminal — the shell keeps
    /// running under its sidebar row, and focus returns to the composer.
    pub(super) fn dismiss_docked_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.docked_terminal.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The standalone page's minimize: the terminal drops to the floating
    /// card and the main area returns to the page history came from — or,
    /// with no back entry recorded, the empty state below the card. The
    /// terminal keeps running; its standalone page lands on forward so the
    /// hop back replays.
    pub(super) fn minimize_terminal_to_card(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_terminal != Some(terminal_id) {
            return;
        }
        Self::prune_navigation_stack(
            &self.state.projects,
            self.state.projects_page_enabled,
            self.state.automations_enabled,
            self.state.github.inbox_enabled,
            &mut self.session_navigation.back,
        );
        self.docked_terminal = Some(terminal_id);
        self.last_visible_terminal = Some(terminal_id);
        if self.session_navigation.back_target().is_some() {
            // The terminal is still selected, so back resolves it as the
            // current location and parks it on forward; the target's
            // activation clears the standalone page underneath the card —
            // deferred while a cold session hydrates, so the terminal
            // stays full screen until its landing is ready.
            self.navigate_back_action(&NavigateBack, window, cx);
        } else {
            // Nothing to navigate back to — the column falls to its
            // empty state while the card floats over it.
            self.selected_terminal = None;
            self.sync_right_panel_owner(cx);
        }
        // The floating terminal opens focused — the same contract as ⌘T
        // and the "New terminal in…" pick.
        if let Some(focus) = self
            .right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).focus_handle(cx))
        {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// `secondary-t` — always a fresh terminal, rooted where the user is.
    /// The chord never strands the terminal it was pressed on: a focused
    /// floating card splits its own group in place instead of being
    /// replaced, and a maximized standalone grows its underlying grid and
    /// restores it rather than taking a floating overlay that buries the
    /// maximized pane. At the group's cap nothing is created — the chord
    /// only reports the limit. A focused right-panel terminal keeps the
    /// chord in the panel: the new terminal joins its strip in the same
    /// directory. Over the boss chat the chord first asks which project
    /// the terminal belongs to. Everywhere else the terminal lands in the
    /// Terminals group — rooted in the selected terminal's directory, the
    /// selected session's workspace, or ~ when the main area shows neither
    /// — and floats over the current surface.
    pub(super) fn new_terminal_action(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        let focused_anchor = self.focused_terminal_anchor(window, cx);
        let maximized_anchor = self.selected_terminal.filter(|anchor_id| {
            self.maximized_terminal_pane
                .is_some_and(|pane| self.terminal_group_panes(*anchor_id).contains(&pane))
        });
        if let Some(anchor_id) =
            new_terminal_split_target(focused_anchor, self.docked_terminal, maximized_anchor)
        {
            if self.new_terminal_split(anchor_id, window, cx) && maximized_anchor == Some(anchor_id)
            {
                // The new pane joins the grid — keeping the old pane
                // maximized would hide it, so the maximize restores.
                self.maximized_terminal_pane = None;
            }
            cx.notify();
            return;
        }
        if let Some(terminal_id) = self.focused_right_panel_terminal(window, cx)
            && let Some(session_id) = self.state.selected_session
            && let Some(working_directory) = self.terminal_cwd(terminal_id, cx)
            && let Some(new_terminal_id) =
                self.create_terminal(working_directory, Some(session_id), None, cx)
            && let Some(index) = self
                .right_panel_surfaces
                .iter()
                .rposition(|surface| surface.terminal_id() == Some(new_terminal_id))
        {
            // create_terminal appended the tab without activating it —
            // select it in the strip and hand it the pending focus slot.
            self.right_panel_active_surface = Some(index);
            self.reveal_right_panel_tab(index);
            self.request_active_terminal_focus();
            cx.notify();
            return;
        }
        // Over a boss-owned surface the terminal first earns a project:
        // the switcher lists the registered projects, and the pick becomes
        // the terminal's root. The question reaches past the chat itself —
        // the boss's page, a planning session, or a deliverable's preview
        // page all live in the boss-minted project and answer it the same
        // way. No registered projects — the chord falls back to the usual
        // guess below.
        let boss_owned = self.boss_ui.page.is_some()
            || (self.session_surface_active()
                && (self.boss_chat_key().is_some() || self.selected_surface_boss_key().is_some()));
        if boss_owned && self.open_terminal_project_switcher(window, cx) {
            cx.notify();
            return;
        }
        // The terminal on screen seeds the spawn directory: the full-width
        // selection in terminal mode, or the visible right panel's active
        // tab while its session is selected.
        let source_terminal = self.selected_terminal.or_else(|| {
            if self.right_panel_visible {
                self.active_right_panel_surface()
                    .and_then(RightPanelSurface::terminal_id)
            } else {
                None
            }
        });
        let (working_directory, session) = if let Some(terminal_id) = source_terminal {
            (
                self.terminal_cwd(terminal_id, cx),
                self.terminal_records
                    .get(&terminal_id)
                    .and_then(|record| record.session),
            )
        } else if let Some(session) = self.selected_session() {
            (
                self.workspace_path_for_session(session)
                    .map(std::path::Path::to_path_buf),
                None,
            )
        } else {
            (dirs::home_dir(), None)
        };
        if let Some(working_directory) = working_directory
            && let Some(terminal_id) = self.create_terminal(working_directory, session, None, cx)
        {
            self.open_docked_terminal(terminal_id, window, cx);
        }
        cx.notify();
    }

    /// `secondary-shift-t` — the Terminals chord. It opens the group on
    /// the last-shown terminal — spawning a global one in ~ when the
    /// group is empty — or, while a terminal holds the main area, cycles
    /// to the next one in sidebar order, wrapping past the end. A lone
    /// terminal has nowhere to cycle, so the chord folds back to where
    /// it took over instead.
    pub(super) fn toggle_terminals_action(
        &mut self,
        _: &ToggleTerminals,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        if let Some(terminal_id) = self.selected_terminal {
            // The next row after the selected one, wrapping — the
            // selected id itself sits out of the chained slices, so a
            // lone terminal finds nothing.
            let listed = self.sidebar_terminal_ids().collect::<Vec<_>>();
            let next = listed
                .iter()
                .position(|id| *id == terminal_id)
                .and_then(|index| {
                    listed[index + 1..]
                        .iter()
                        .chain(listed[..index].iter())
                        .copied()
                        .next()
                });
            if let Some(next) = next {
                self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
                self.select_terminal(next, window, cx);
            } else {
                self.collapse_terminals_group(cx);
            }
        } else {
            self.expand_terminals_group(window, cx);
        }
        cx.notify();
    }

    /// ⌘⌥P on a terminal row: pinned terminals keep a sidebar row while the
    /// group is collapsed. Returns the new pinned state for the chord's
    /// toast — `None` when the terminal no longer exists.
    pub(super) fn toggle_terminal_pin(
        &mut self,
        terminal_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let Some(record) = self.terminal_records.get_mut(&terminal_id) else {
            return None;
        };
        record.pinned = !record.pinned;
        let pinned = record.pinned;
        self.sidebar_rows_fingerprint.set(None);
        self.save();
        cx.notify();
        Some(pinned)
    }

    /// Close a terminal wherever it lives — the active session's tab
    /// strip, a background session's stored surfaces, or the global list.
    /// An anchor takes its whole group down; a pane's own close is a
    /// group edit that leaves the surface standing.
    pub(super) fn close_terminal(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        if self
            .terminal_records
            .get(&terminal_id)
            .is_some_and(|record| record.pane_of.is_some())
        {
            self.close_terminal_pane(terminal_id, cx);
            return;
        }
        let filled_main_area = self.selected_terminal == Some(terminal_id);
        // A main-area terminal opened from another surface is a temporary
        // visit. Consume its back entry before dropping the terminal, so
        // closing the tab or exiting the shell returns to where it came from.
        let history_target = if filled_main_area {
            Self::prune_navigation_stack(
                &self.state.projects,
                self.state.projects_page_enabled,
                self.state.automations_enabled,
                self.state.github.inbox_enabled,
                &mut self.session_navigation.back,
            );
            let current = NavigationLocation::Terminal(terminal_id);
            self.session_navigation
                .back_target()
                .and_then(|target| self.session_navigation.go_back(current).map(|_| target))
        } else {
            None
        };
        let terminal_focus = filled_main_area
            .then(|| {
                self.right_panel_terminals
                    .get(&terminal_id)
                    .map(|terminal| terminal.read(cx).focus_handle(cx))
            })
            .flatten();
        if let Some(index) = self
            .right_panel_surfaces
            .iter()
            .position(|surface| surface.terminal_id() == Some(terminal_id))
        {
            self.close_right_panel_surface(index, cx);
        } else {
            for state in self.right_panel_states.values_mut() {
                let Some(index) = state
                    .surfaces
                    .iter()
                    .position(|surface| surface.terminal_id() == Some(terminal_id))
                else {
                    continue;
                };
                state.surfaces.remove(index);
                state.active_surface = state.active_surface.and_then(|active| {
                    (!state.surfaces.is_empty()).then(|| match active.cmp(&index) {
                        std::cmp::Ordering::Greater => active - 1,
                        std::cmp::Ordering::Equal => index.saturating_sub(1),
                        std::cmp::Ordering::Less => active.min(state.surfaces.len() - 1),
                    })
                });
                state.drop_dead_fullscreen();
                break;
            }
            self.drop_terminal(terminal_id, cx);
        }
        // A terminal that filled the main area parked the session selection
        // on its way in, so with no successor taking over the main area
        // falls to a session-less new task page: the composer mounts but
        // its pickers stay disabled, and window focus stays on the dead
        // view. Land on the new-task draft the way ⌘N does. This runs
        // after the strip settles — selecting a session re-syncs the panel
        // owner, which swaps `right_panel_surfaces` out from under any
        // caller still holding an index into it.
        if filled_main_area
            && history_target.is_none()
            && self.selected_terminal.is_none()
            && self.state.selected_session.is_none()
        {
            let current_project = self
                .state
                .selected_project
                .and_then(|id| self.state.projects.iter().find(|project| project.id == id))
                .map(|project| (project.id, project.is_projectless()));
            match current_project {
                Some((_, true)) | None => self.create_projectless_session(cx),
                Some((project_id, false)) => {
                    if let Some(session_id) = self.session_navigation.remembered_new_task(
                        &self.state.sessions,
                        project_id,
                        |session| self.session_is_boss_managed(session),
                    ) {
                        self.select_session(session_id, cx);
                    } else {
                        self.create_session_for(project_id, self.state.last_provider, cx);
                    }
                }
            }
            // Settings covers the workspace and owns the keyboard for its
            // visit; the composer field is not in the dispatch tree there.
            if self.settings_page.is_none() {
                let focus = self.composer_focus(cx);
                let window_handle = self.window_handle;
                let _ = window_handle.update(cx, move |_, window, cx| {
                    // Take the keyboard back only from the dead view — a
                    // field or overlay focused in the meantime keeps it.
                    if window
                        .focused(cx)
                        .is_none_or(|focused| terminal_focus.as_ref() == Some(&focused))
                    {
                        window.focus(&focus, cx);
                    }
                });
            }
        }
        if let Some(target) = history_target {
            let window_handle = self.window_handle;
            let waku = cx.entity();
            cx.defer(move |cx| {
                let _ = window_handle.update(cx, move |_, window, cx| {
                    let _ = waku.update(cx, |this, cx| match target {
                        NavigationLocation::Task(session_id) => this.request_session_activation(
                            session_id,
                            SessionActivationTransition::Visit,
                            cx,
                        ),
                        NavigationLocation::Terminal(terminal_id) => {
                            this.activate_terminal(terminal_id, false, window, cx);
                        }
                        NavigationLocation::ProjectsPage(project_id) => {
                            this.show_projects_page(project_id, window, cx);
                        }
                        NavigationLocation::DraftsPage => this.show_drafts_page(window, cx),
                        NavigationLocation::AutomationsPage => {
                            this.show_automations_page(window, cx);
                        }
                        NavigationLocation::BossPage(key, tab) => {
                            this.show_boss_page(key, tab, window, cx)
                        }
                        NavigationLocation::Deliverable(key, deliverable_id) => {
                            this.show_deliverable_page(key, deliverable_id, cx)
                        }
                        NavigationLocation::Inbox => this.show_inbox(window, cx),
                        NavigationLocation::Settings => {}
                    });
                });
            });
        }
        cx.notify();
    }

    /// ⌘W with a terminal filling the main area: an idle shell dies
    /// outright, a shell mid-command earns a confirmation first — the
    /// same `command_running` signal the sidebar spinner reads.
    pub(super) fn close_main_terminal(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let busy = self
            .terminal_group_panes(terminal_id)
            .iter()
            .any(|pane_id| {
                self.right_panel_terminals
                    .get(pane_id)
                    .is_some_and(|terminal| terminal.read(cx).command_running())
            });
        if busy {
            let focus = self.open_terminal_close_dialog(terminal_id, cx);
            window.focus(&focus, cx);
        } else {
            self.close_terminal(terminal_id, cx);
        }
    }

    /// The selected terminal rendered full-width in the main area,
    /// standing in for the transcript while terminal mode is active.
    pub(super) fn render_main_terminal(
        &self,
        terminal_id: Uuid,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_sidebar_terminal_repo_roots(cx);
        let theme = Theme::current(cx);
        let Some(terminal) = self.right_panel_terminals.get(&terminal_id).cloned() else {
            return div().into_any_element();
        };
        let panes = self.terminal_group_panes(terminal_id);
        let maximized = self.maximized_terminal_pane == Some(terminal_id)
            || self
                .maximized_terminal_pane
                .is_some_and(|pane_id| panes.contains(&pane_id));
        let size_drifted =
            terminal.read_with(cx, |view, _| (view.panel_width() - width).abs() > 0.5);
        if size_drifted {
            terminal.update(cx, |terminal, _| terminal.set_panel_width(width));
        }
        let minimize_focus =
            self.transcript_control_focus(format!("terminal-minimize-{terminal_id}"), cx);
        let minimize_button = div()
            .id(SharedString::from(format!(
                "terminal-minimize-{terminal_id}"
            )))
            .track_focus(&minimize_focus)
            .tab_index(0)
            .size(px(26.0))
            .rounded(px(8.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon("icons/minimize.svg", 13.0, theme.text_tertiary))
            .tooltip(|window, cx| Tooltip::new(tr!("terminal.minimize")).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.minimize_terminal_to_card(terminal_id, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.minimize_terminal_to_card(terminal_id, window, cx);
                    cx.stop_propagation();
                }
            }));
        let maximize_button = (panes.len() > 1).then(|| {
            let label = if maximized {
                tr!("terminal.restore")
            } else {
                tr!("terminal.maximize")
            };
            div()
                .id(SharedString::from(format!(
                    "terminal-maximize-{terminal_id}"
                )))
                .track_focus(
                    &self.transcript_control_focus(format!("terminal-maximize-{terminal_id}"), cx),
                )
                .tab_index(0)
                .size(px(26.0))
                .rounded(px(8.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .hover(|element| element.bg(theme.overlay))
                .active(|element| element.bg(theme.overlay_strong))
                .child(icon(
                    if maximized {
                        "icons/minimize.svg"
                    } else {
                        "icons/maximize.svg"
                    },
                    13.0,
                    theme.text_tertiary,
                ))
                .tooltip(move |window, cx| {
                    Tooltip::new(label.clone())
                        .shortcut(crate::ui::shortcut::ShortcutHint::action(
                            &ToggleTerminalMaximize,
                        ))
                        .build(window, cx)
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.toggle_terminal_maximize_action(&ToggleTerminalMaximize, window, cx);
                }))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.toggle_terminal_maximize_action(&ToggleTerminalMaximize, window, cx);
                        cx.stop_propagation();
                    }
                }))
        });
        div()
            .key_context("StandaloneTerminal")
            .on_action(cx.listener(Self::toggle_terminal_maximize_action))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .pl(px(8.0))
                    .pr(px(4.0))
                    .py(px(4.0))
                    .flex()
                    .items_center()
                    .child(self.render_terminal_location_for_id(terminal_id, cx))
                    .child(div().flex_1())
                    .children(maximize_button)
                    .child(minimize_button),
            )
            .child(
                if self
                    .terminal_records
                    .get(&terminal_id)
                    .is_some_and(|record| !record.panes.is_empty())
                {
                    self.render_terminal_group(terminal_id, TerminalGroupLayout::Grid, window, cx)
                } else {
                    self.terminal_drop_target(
                        div()
                            .id(SharedString::from(format!(
                                "terminal-body-drop-{terminal_id}"
                            )))
                            .flex_1()
                            .min_h_0()
                            .size_full()
                            .child(terminal),
                        terminal_id,
                        STANDALONE_TERMINAL_MAX_PANES,
                        cx,
                    )
                    .into_any_element()
                },
            )
            .into_any_element()
    }

    /// The docked terminal card pinned to the top of the main column —
    /// in flow, so the surface underneath shifts down rather than being
    /// covered. The header carries the title and directory, the expand
    /// affordance that promotes the card to the standalone page, and the
    /// close that only dismisses the card — the shell keeps running.
    pub(super) fn render_docked_terminal_card(
        &self,
        terminal_id: Uuid,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_sidebar_terminal_repo_roots(cx);
        let theme = Theme::current(cx);
        let Some(terminal) = self.right_panel_terminals.get(&terminal_id).cloned() else {
            return div().into_any_element();
        };
        // The view's column math needs its painted width — the card's
        // margin, border, and inset mat shave the column's.
        let inner_width =
            (width - DOCKED_TERMINAL_MARGIN * 2.0 - 2.0 - DOCKED_TERMINAL_INSET * 2.0).max(0.0);
        if (terminal.read(cx).panel_width() - inner_width).abs() > 0.5 {
            terminal.update(cx, |terminal, _| terminal.set_panel_width(inner_width));
        }
        let card_height = (f32::from(window.viewport_size().height) * 0.4)
            .clamp(DOCKED_TERMINAL_MIN_HEIGHT, DOCKED_TERMINAL_MAX_HEIGHT);
        let record = self.terminal_records.get(&terminal_id);
        let title = record
            .and_then(|record| record.custom_title.clone())
            .or_else(|| Some(single_line_label(terminal.read(cx).title())))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| tr!("right_panel.terminal"));
        let location = self.render_terminal_location_for_id(terminal_id, cx);
        let expand_focus =
            self.transcript_control_focus(format!("docked-terminal-expand-{terminal_id}"), cx);
        let close_focus =
            self.transcript_control_focus(format!("docked-terminal-close-{terminal_id}"), cx);
        let expand_button = div()
            .id(SharedString::from(format!(
                "docked-terminal-expand-{terminal_id}"
            )))
            .track_focus(&expand_focus)
            .tab_index(0)
            .size(px(26.0))
            .rounded(px(8.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon("icons/maximize.svg", 13.0, theme.text_tertiary))
            .tooltip(|window, cx| Tooltip::new(tr!("terminal.expand")).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_terminal(terminal_id, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.select_terminal(terminal_id, window, cx);
                    cx.stop_propagation();
                }
            }));
        let close_button = div()
            .id(SharedString::from(format!(
                "docked-terminal-close-{terminal_id}"
            )))
            .track_focus(&close_focus)
            .tab_index(0)
            .size(px(26.0))
            .rounded(px(8.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon("icons/x.svg", 13.0, theme.text_tertiary))
            .tooltip(|window, cx| Tooltip::new(tr!("common.close")).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.dismiss_docked_terminal(window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.dismiss_docked_terminal(window, cx);
                    cx.stop_propagation();
                }
            }));
        div()
            .flex_none()
            .w_full()
            .px(px(DOCKED_TERMINAL_MARGIN))
            .pt(px(DOCKED_TERMINAL_MARGIN))
            .pb(px(DOCKED_TERMINAL_MARGIN / 2.0))
            .key_context("StandaloneTerminal")
            .on_action(cx.listener(Self::toggle_terminal_maximize_action))
            .child(
                div()
                    .id(SharedString::from(format!("docked-terminal-{terminal_id}")))
                    .key_context("DockedTerminal")
                    .on_action(
                        cx.listener(move |this, _: &CloseDockedTerminal, window, cx| {
                            this.close_main_terminal(terminal_id, window, cx);
                        }),
                    )
                    .on_action(cx.listener(|this, _: &DismissDockedTerminal, window, cx| {
                        this.dismiss_docked_terminal(window, cx);
                    }))
                    .w_full()
                    .h(px(card_height))
                    .flex()
                    .flex_col()
                    .rounded(px(12.0))
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.raised)
                    .shadow_md()
                    .child(
                        div()
                            .h(px(DOCKED_TERMINAL_HEADER_HEIGHT))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(10.0))
                            .pr(px(4.0))
                            .border_b(hairline())
                            .border_color(theme.border_subtle)
                            .child(icon("icons/terminal-square.svg", 13.0, theme.text_tertiary))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(SharedString::from(title)),
                            )
                            .child(location)
                            .child(div().flex_1())
                            .child(self.render_terminal_split_button(
                                "docked-terminal-split",
                                terminal_id,
                                STANDALONE_TERMINAL_MAX_PANES,
                                cx,
                            ))
                            .child(expand_button)
                            .child(close_button),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .px(px(DOCKED_TERMINAL_INSET))
                            .pb(px(DOCKED_TERMINAL_INSET))
                            .child(
                                if self
                                    .terminal_records
                                    .get(&terminal_id)
                                    .is_some_and(|record| !record.panes.is_empty())
                                {
                                    self.render_terminal_group(
                                        terminal_id,
                                        TerminalGroupLayout::Grid,
                                        window,
                                        cx,
                                    )
                                } else {
                                    self.terminal_drop_target(
                                        div()
                                            .id(SharedString::from(format!(
                                                "terminal-body-drop-{terminal_id}"
                                            )))
                                            .flex_1()
                                            .min_h_0()
                                            .size_full()
                                            .child(terminal),
                                        terminal_id,
                                        STANDALONE_TERMINAL_MAX_PANES,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_terminal_pane_layout(
        &self,
        layout: &TerminalPaneLayout,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match layout {
            TerminalPaneLayout::Pane(id) => self.render_terminal_pane(*id, window, cx),
            TerminalPaneLayout::Split {
                down,
                first,
                second,
            } => div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .when(*down, |element| element.flex_col())
                .child(self.render_terminal_pane_layout(first, window, cx))
                .child(self.render_terminal_pane_layout(second, window, cx))
                .into_any_element(),
        }
    }

    /// Render saved directional splits, or the surface's legacy stack/grid
    /// for groups created before directional splitting was available.
    pub(super) fn render_terminal_group(
        &self,
        anchor_id: Uuid,
        layout: TerminalGroupLayout,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panes = self.terminal_group_panes(anchor_id);
        let standalone_maximized = (matches!(layout, TerminalGroupLayout::Grid)
            && self.selected_terminal == Some(anchor_id))
        .then_some(self.maximized_terminal_pane)
        .flatten();
        let panes = visible_terminal_group_ids(&panes, standalone_maximized);
        let directional = self
            .terminal_records
            .get(&anchor_id)
            .and_then(|record| record.pane_layout.as_ref());
        let body =
            if directional.is_some() {
                terminal_pane_layout(
                    directional,
                    &panes,
                    matches!(layout, TerminalGroupLayout::Stack),
                )
                .map(|layout| self.render_terminal_pane_layout(&layout, window, cx))
                .unwrap_or_else(|| div().into_any_element())
            } else {
                match layout {
                    TerminalGroupLayout::Stack => div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .flex()
                        .flex_col()
                        .children(
                            panes
                                .into_iter()
                                .map(|pane_id| self.render_terminal_pane(pane_id, window, cx)),
                        )
                        .into_any_element(),
                    TerminalGroupLayout::Grid => {
                        // Balance the cells: rows cap the grid at three tall, so
                        // the column count floats with the pane count — 2 panes
                        // sit side by side, 4 make a 2x2, 9 fill the 3x3.
                        let rows = panes.len().div_ceil(STANDALONE_TERMINAL_GRID_COLUMNS);
                        let columns = panes.len().div_ceil(rows.max(1));
                        let mut grid = div().flex_1().min_h_0().w_full().flex().flex_col();
                        for row in panes.chunks(columns.max(1)) {
                            grid =
                                grid.child(div().flex_1().min_h_0().w_full().flex().children(
                                    row.iter().map(|pane_id| {
                                        self.render_terminal_pane(*pane_id, window, cx)
                                    }),
                                ));
                        }
                        grid.into_any_element()
                    }
                }
            };
        let limit = match layout {
            TerminalGroupLayout::Stack => RIGHT_PANEL_TERMINAL_MAX_PANES,
            TerminalGroupLayout::Grid => STANDALONE_TERMINAL_MAX_PANES,
        };
        let standalone = self.selected_terminal == Some(anchor_id)
            && matches!(layout, TerminalGroupLayout::Grid);
        let mut group = div()
            .id(SharedString::from(format!(
                "terminal-group-drop-{anchor_id}"
            )))
            .flex_1()
            .min_h_0()
            .flex()
            .child(body);
        if standalone {
            group = group.key_context("StandaloneTerminal");
        }
        self.terminal_drop_target(group, anchor_id, limit, cx)
            .into_any_element()
    }

    /// One cell of a split group — the terminal's view plus the kill
    /// button overlaid at its toolbar's right edge, the convention the
    /// embedded settings terminal established. The hairline frame
    /// doubles as the cell separator and, tinted, as the focus marker.
    fn render_terminal_pane(
        &self,
        terminal_id: Uuid,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(view) = self.right_panel_terminals.get(&terminal_id).cloned() else {
            return div().into_any_element();
        };
        let focused = view.read(cx).focus_handle(cx).is_focused(window);
        let busy = view.read(cx).command_running();
        let close_focus =
            self.transcript_control_focus(format!("terminal-pane-close-{terminal_id}"), cx);
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .border(hairline())
            .border_color(if focused { theme.accent } else { theme.border })
            .child(view)
            .child(
                div().absolute().top(px(6.0)).right(px(6.0)).child(
                    icon_button(
                        SharedString::from(format!("terminal-pane-close-{terminal_id}")),
                        "icons/x.svg",
                        theme,
                    )
                    .track_focus(&close_focus)
                    .tab_index(0)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .tooltip(Tooltip::text(tr!("common.close")))
                    .on_activation(cx, move |this, window, cx| {
                        // The same running-command guard a surface close
                        // gets: a busy pane's shell dies with the cell,
                        // so it asks first.
                        if busy {
                            let focus = this.open_terminal_pane_close_dialog(terminal_id, cx);
                            window.focus(&focus, cx);
                        } else {
                            this.close_terminal_pane(terminal_id, cx);
                        }
                    }),
                ),
            )
            .into_any_element()
    }

    /// The split control shared by the right-panel header's terminal tab
    /// and the window header in terminal mode. At the surface's cap it
    /// dims and stops answering — the history buttons' disabled
    /// convention.
    pub(super) fn render_terminal_split_button(
        &self,
        id: &'static str,
        anchor_id: Uuid,
        max_panes: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let down = self.sidebar_alt_modifier_down;
        let at_cap = self.terminal_group_panes(anchor_id).len() >= max_panes;
        let label = if at_cap {
            tr!("terminal.split_at_max", max = max_panes)
        } else {
            tr!(if down {
                "terminal.split_down"
            } else {
                "terminal.split_right"
            })
        };
        let focus = self.transcript_control_focus(id, cx);
        div()
            .id(id)
            .track_focus(&focus)
            .when(!at_cap, |element| element.tab_index(0))
            .size(px(26.0))
            .rounded(px(8.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .tooltip(move |window, cx| {
                if down {
                    Tooltip::new(label.clone())
                        .action(&SplitTerminalDown)
                        .build(window, cx)
                } else {
                    Tooltip::new(label.clone())
                        .action(&SplitTerminal)
                        .build(window, cx)
                }
            })
            .when(at_cap, |element| element.opacity(0.35))
            .when(!at_cap, |element| {
                element
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.split_terminal_in_direction(
                            anchor_id,
                            max_panes,
                            window.modifiers().alt,
                            window,
                            cx,
                        );
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.split_terminal_in_direction(
                                anchor_id,
                                max_panes,
                                window.modifiers().alt,
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(icon(
                if down {
                    "icons/split-down.svg"
                } else {
                    "icons/split.svg"
                },
                13.0,
                theme.text_tertiary,
            ))
    }

    /// One terminal row in the Terminals group — also what a pinned
    /// terminal contributes to the collapsed group.
    pub(super) fn render_sidebar_terminal_item(
        &self,
        terminal_id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(record) = self.terminal_records.get(&terminal_id) else {
            return div().into_any_element();
        };
        let pinned = record.pinned;
        let terminal = self.right_panel_terminals.get(&terminal_id);
        let title = record
            .custom_title
            .clone()
            .or_else(|| {
                terminal
                    .map(|terminal| single_line_label(terminal.read(cx).title()))
                    .filter(|title| !title.is_empty())
            })
            .unwrap_or_else(|| tr!("right_panel.terminal"));
        let cwd = self.terminal_cwd(terminal_id, cx);
        // The title line's trailing slot is the command's status: spinning
        // while one runs, a failure mark or — for a clean exit the user
        // has not seen — an unread dot after, empty when the shell reports
        // nothing. A split row reports across the whole group — any pane
        // busy spins, any pane's failure marks.
        let group_views = std::iter::once(terminal_id)
            .chain(record.panes.iter().copied())
            .filter_map(|id| self.right_panel_terminals.get(&id))
            .collect::<Vec<_>>();
        let status_icon = (!group_views.is_empty())
            .then(|| {
                if group_views
                    .iter()
                    .any(|view| view.read(cx).command_running())
                {
                    return Some(
                        motion::spin_slow(icon(
                            "icons/loader-circle.svg",
                            12.0,
                            theme.text_tertiary,
                        ))
                        .into_any_element(),
                    );
                }
                if group_views.iter().any(
                    |view| matches!(view.read(cx).last_command_exit(), Some(code) if code != 0),
                ) {
                    return Some(icon("icons/x-bold.svg", 12.0, theme.danger).into_any_element());
                }
                (self.unseen_terminal_completions.contains(&terminal_id)
                    && !self.terminal_is_active_surface(terminal_id)
                    && group_views
                        .iter()
                        .any(|view| matches!(view.read(cx).last_command_exit(), Some(0))))
                .then(|| {
                    div()
                        .size(px(7.0))
                        .rounded_full()
                        .bg(theme.info)
                        .into_any_element()
                })
            })
            .flatten();
        let shell_name = terminal
            .map(|terminal| terminal.read(cx).shell_name().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                crate::command_env::default_terminal_shell()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned()
            });
        // The leading label is the terminal's location, named by its
        // nearest repository the way a session row names its project —
        // "waku" at the root, "waku/src/app" deeper in. Outside any
        // repository the shell's name stands in under a shell glyph
        // rather than a folder, with the cwd trailing it.
        let repo_label = cwd
            .as_deref()
            .and_then(|cwd| self.repo_location_label(cwd, SIDEBAR_TERMINAL_DETAIL_MAX_CHARS));
        let (detail_icon, detail) = match repo_label {
            Some(label) => ("icons/folder.svg", label),
            None => (
                "icons/terminal-square.svg",
                cwd.as_deref()
                    .map(|cwd| {
                        let cwd =
                            settings::abbreviate_home_path(cwd, self.home_directory.as_deref());
                        let budget = SIDEBAR_TERMINAL_DETAIL_MAX_CHARS
                            .saturating_sub(shell_name.chars().count() + 1);
                        let cwd = truncate_path_ancestors(&cwd, budget, false);
                        format!("{shell_name} {cwd}")
                    })
                    .unwrap_or_else(|| shell_name.clone()),
            ),
        };
        // "…ago" marks the latest command's start while one runs; before
        // the first command it is the terminal's own open time.
        let started_at = terminal
            .and_then(|terminal| terminal.read(cx).last_command_started_at())
            .unwrap_or(record.opened_at);
        let time_label = format_time_ago(unix_time().saturating_sub(started_at));
        // Only a standalone terminal owns selection in the Terminals group;
        // session terminals stay associated with their task even when active.
        let selected = record.session.is_none() && self.terminal_is_active_surface(terminal_id);
        let renaming = self.terminal_rename == Some(terminal_id);
        let menu = self.menu_handle(format!("terminal-{terminal_id}"), cx);
        let row_focus = menu.trigger_focus_handle().clone();
        let keyboard_menu = menu.clone();
        let waku = cx.entity().downgrade();
        let group_name = SharedString::from(format!("terminal-row-{terminal_id}"));
        let close_focus = self
            .sidebar_terminal_close_focuses
            .borrow_mut()
            .entry(terminal_id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let pin_focus = self
            .sidebar_terminal_pin_focuses
            .borrow_mut()
            .entry(terminal_id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        // The pin control shares the close control's reveal: zero-width
        // until the row is hovered or the button takes keyboard focus.
        let pin_button = div()
            .id(SharedString::from(format!("terminal-pin-{terminal_id}")))
            .track_focus(&pin_focus)
            .tab_index(0)
            .flex_none()
            .w_0()
            .h(px(18.0))
            .overflow_hidden()
            .rounded(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(0.0)
            .group_hover(group_name.clone(), |style| style.w(px(20.0)).opacity(1.0))
            .focus_visible(|style| style.w(px(20.0)).opacity(1.0).bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text_with_action(
                if pinned {
                    tr!("session.unpin")
                } else {
                    tr!("session.pin")
                },
                &ToggleSessionPin,
            ))
            .child(icon(
                if pinned {
                    "icons/pin-filled.svg"
                } else {
                    "icons/pin.svg"
                },
                12.0,
                theme.text_secondary,
            ))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                let _ = this.toggle_terminal_pin(terminal_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    let _ = this.toggle_terminal_pin(terminal_id, cx);
                    cx.stop_propagation();
                }
            }));
        // The close control borrows the status slot: it stays zero-width
        // until the row is hovered or the button takes keyboard focus.
        let close_button = div()
            .id(SharedString::from(format!("terminal-close-{terminal_id}")))
            .track_focus(&close_focus)
            .tab_index(0)
            .flex_none()
            .w_0()
            .h(px(18.0))
            .overflow_hidden()
            .rounded(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(0.0)
            .group_hover(group_name.clone(), |style| style.w(px(20.0)).opacity(1.0))
            .focus_visible(|style| style.w(px(20.0)).opacity(1.0).bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text(tr!("common.close")))
            .child(icon("icons/trash.svg", 12.0, theme.text_secondary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.close_terminal(terminal_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.close_terminal(terminal_id, cx);
                    cx.stop_propagation();
                }
            }));
        let row = div()
            .id(SharedString::from(format!(
                "sidebar-terminal-{terminal_id}"
            )))
            .group(group_name.clone())
            .w_full()
            .min_w_0()
            .h(px(SIDEBAR_TERMINAL_CARD_HEIGHT))
            .pl(px(8.0))
            .pr(px(8.0))
            .py(px(7.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .rounded(px(9.0))
            .cursor_default()
            .when(selected, |element| {
                element.bg(theme.sidebar_item_background)
            })
            .hover(|element| element.bg(theme.sidebar_item_background))
            .active(|element| element.bg(theme.sidebar_item_background))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .overflow_hidden()
                    .line_height(sp(18.0))
                    .child(if renaming {
                        div()
                            .id(SharedString::from(format!(
                                "terminal-rename-field-{terminal_id}"
                            )))
                            .key_context(sidebar::SESSION_RENAME_PARENT_CONTEXT)
                            .on_action(cx.listener(|this, _: &CancelSessionRename, window, cx| {
                                this.cancel_terminal_rename(window, cx);
                            }))
                            .h(px(18.0))
                            .flex_1()
                            .min_w_0()
                            .px(px(4.0))
                            .rounded(px(4.0))
                            .border(hairline())
                            .border_color(theme.accent)
                            .bg(theme.inset)
                            .flex()
                            .items_center()
                            .text_size(sp(13.5))
                            .text_color(theme.text)
                            .child(self.session_rename_input.clone())
                    } else {
                        div()
                            .id(SharedString::from(format!("terminal-title-{terminal_id}")))
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(13.5))
                            .text_color(theme.text)
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    if event.click_count() == 2 {
                                        this.begin_terminal_rename(terminal_id, window, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(SharedString::from(title.clone()))
                    })
                    .when_some(status_icon, |element, status_icon| {
                        element.child(
                            div()
                                .flex_none()
                                .size(px(12.0))
                                // The zero-width pin/close pair still
                                // claims its two flex gaps; pulling the slot
                                // right by that amount keeps the indicator's
                                // right edge flush with the timestamp below.
                                .mr(px(-12.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .group_hover(group_name.clone(), |style| style.invisible())
                                .child(status_icon),
                        )
                    })
                    .child(pin_button)
                    .child(close_button),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .text_size(sp(13.0))
                    .line_height(sp(15.0))
                    .child(self.render_terminal_location(
                        terminal_id,
                        detail_icon,
                        detail,
                        true,
                        cx,
                    ))
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(time_label)),
                    ),
            )
            .when(!renaming, |element| {
                element
                    .track_focus(&row_focus)
                    .tab_index(0)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let key = event.keystroke.key.as_str();
                        if matches!(key, "enter" | "space") {
                            this.select_terminal(terminal_id, window, cx);
                            cx.stop_propagation();
                        } else if key == "f10" && event.keystroke.modifiers.shift {
                            keyboard_menu.open_context_menu(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_drag(
                        SidebarTerminalDrag {
                            terminal_id,
                            title: SharedString::from(title.clone()),
                        },
                        {
                            let weak = cx.entity().downgrade();
                            move |drag, _, window, cx| {
                                let weak = weak.clone();
                                window.on_next_frame(move |window, cx| {
                                    Self::watch_terminal_drag(weak, window, cx)
                                });
                                cx.new(|_| drag.clone())
                            }
                        },
                    )
                    .on_drag_move::<SidebarTerminalDrag>(cx.listener(
                        move |this,
                              event: &gpui::DragMoveEvent<SidebarTerminalDrag>,
                              window,
                              cx| {
                            if event.bounds.contains(&window.mouse_position())
                                && event.drag(cx).terminal_id != terminal_id
                            {
                                this.preview_terminal_drag(terminal_id, window, cx);
                            }
                        },
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_terminal(terminal_id, window, cx);
                    }))
            });

        let row = if renaming {
            div()
                .w_full()
                .child(row)
                .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                    if this.terminal_rename == Some(terminal_id) {
                        this.commit_terminal_rename(cx);
                    }
                }))
                .into_any_element()
        } else {
            context_menu(
                div().w_full().child(row),
                SharedString::from(format!("terminal-menu-{terminal_id}")),
                &menu,
                move |cx| {
                    let pinned = waku
                        .update(cx, |waku, _| {
                            waku.terminal_records
                                .get(&terminal_id)
                                .is_some_and(|record| record.pinned)
                        })
                        .unwrap_or(false);
                    let rename_waku = waku.clone();
                    let pin_waku = waku.clone();
                    let close_waku = waku.clone();
                    vec![
                        MenuItem::new(tr!("common.rename"), move |window, cx| {
                            let _ = rename_waku.update(cx, |waku, cx| {
                                waku.begin_terminal_rename(terminal_id, window, cx);
                            });
                        })
                        .icon("icons/pencil.svg"),
                        MenuItem::new(
                            if pinned {
                                tr!("session.unpin")
                            } else {
                                tr!("session.pin")
                            },
                            move |_, cx| {
                                let _ = pin_waku.update(cx, |waku, cx| {
                                    let _ = waku.toggle_terminal_pin(terminal_id, cx);
                                });
                            },
                        )
                        .shortcut_action(&ToggleSessionPin)
                        .icon(if pinned {
                            "icons/pin-off.svg"
                        } else {
                            "icons/pin.svg"
                        }),
                        MenuItem::Separator,
                        MenuItem::new(tr!("common.close"), move |_, cx| {
                            let _ = close_waku.update(cx, |waku, cx| {
                                waku.close_terminal(terminal_id, cx);
                            });
                        })
                        .icon("icons/x.svg"),
                    ]
                },
            )
        };

        div()
            .w_full()
            .pb(px(SIDEBAR_TERMINAL_ROW_GAP))
            .child(row)
            .into_any_element()
    }
}

/// Where a boss-surface code block's run lands — `Resolved` carries the
/// settled directory, `Pick` the project candidates inference could not
/// choose between, ordered best-first for the palette's picker.
enum CodeRunDirectory {
    Resolved(PathBuf),
    Pick(Vec<Uuid>),
}

/// A transcript code-block run parked on an ambiguous project: the
/// block's prepared launch, the right-panel strip its terminal joins, and
/// the session the action journal records against.
pub(super) struct PendingCodeBlockRun {
    pub command: CustomCommand,
    pub attach: Option<Uuid>,
    pub journal_session: Option<Uuid>,
}

/// The tasks a code block's turn names, deduped in transcript order: the
/// block's own message's author first, then each prompt in the turn — a
/// report's employee and a task-sent prompt's author.
fn code_run_turn_tasks(session: &AgentSession, message: Option<&Message>) -> Vec<Uuid> {
    let mut tasks = Vec::new();
    let push = |tasks: &mut Vec<Uuid>, task: Option<Uuid>| {
        if let Some(task) = task.filter(|task| !tasks.contains(task)) {
            tasks.push(task);
        }
    };
    let Some(message) = message else {
        return tasks;
    };
    push(&mut tasks, message.sent_by_task);
    let Some(turn) = message.turn_id else {
        return tasks;
    };
    for prompt in session
        .messages
        .iter()
        .filter(|candidate| candidate.turn_id == Some(turn))
    {
        push(
            &mut tasks,
            prompt
                .report_trigger
                .as_ref()
                .map(|trigger| trigger.employee),
        );
        push(&mut tasks, prompt.sent_by_task);
    }
    tasks
}

/// Bounds the path-token scan — a wall of log text cannot multiply the
/// relative-path probes' stat calls.
const CODE_RUN_TOKEN_LIMIT: usize = 32;

/// Path-shaped words `text` mentions, in order — whitespace-, quote-,
/// and bracket-delimited tokens with prose punctuation and `:line`/`[:col]`
/// suffixes stripped.
fn code_run_path_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '(' | ')' | '<' | '>' | '[' | ']' | '{' | '}' | '|'
            )
    })
    .filter_map(|token| {
        let mut token = token.trim_matches(|c: char| matches!(c, ',' | ';'));
        // A `path:line[:col]` reference names the file, not the position.
        while let Some((head, tail)) = token.rsplit_once(':') {
            if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) {
                break;
            }
            token = head;
        }
        (!token.is_empty()).then(|| token.to_owned())
    })
    .take(CODE_RUN_TOKEN_LIMIT)
    .collect()
}

/// The projects a single path token names. An absolute or `~/` path picks
/// the deepest root it sits under — a nested project beats its parent;
/// a relative path picks every root it exists under, so `../`, flags,
/// URLs, and words without a separator match nothing.
fn code_run_token_projects(roots: &[(Uuid, &Path)], token: &str, home: Option<&Path>) -> Vec<Uuid> {
    let expanded;
    let path = if let Some(rest) = token.strip_prefix("~/") {
        let Some(home) = home else {
            return Vec::new();
        };
        expanded = home.join(rest);
        expanded.as_path()
    } else {
        Path::new(token)
    };
    if path.is_absolute() {
        let depth = roots
            .iter()
            .filter(|(_, root)| path.starts_with(root))
            .map(|(_, root)| root.components().count())
            .max();
        return roots
            .iter()
            .filter(|(_, root)| Some(root.components().count()) == depth && path.starts_with(root))
            .map(|(id, _)| *id)
            .collect();
    }
    let relative = token.strip_prefix("./").unwrap_or(token);
    if relative.starts_with(['/', '.', '-', '~'])
        || relative.contains("://")
        || !relative.contains('/')
    {
        return Vec::new();
    }
    roots
        .iter()
        .filter(|(_, root)| root.join(relative).exists())
        .map(|(id, _)| *id)
        .collect()
}

/// The projects `message` and the block's own source reference, in
/// first-mention order — absolute and `~` paths by prefix, relative paths
/// by existence under a project root.
fn code_run_content_projects(
    roots: &[(Uuid, &Path)],
    message: &str,
    code: &str,
    home: Option<&Path>,
) -> Vec<Uuid> {
    let mut projects: Vec<Uuid> = Vec::new();
    for token in code_run_path_tokens(message)
        .into_iter()
        .chain(code_run_path_tokens(code))
    {
        for id in code_run_token_projects(roots, &token, home) {
            if !projects.contains(&id) {
                projects.push(id);
            }
        }
    }
    projects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximizing_a_standalone_split_keeps_the_group_available_for_restore() {
        let anchor = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let third = Uuid::from_u128(3);
        let panes = [anchor, second, third];

        assert_eq!(visible_terminal_group_ids(&panes, Some(second)), [second]);
        assert_eq!(visible_terminal_group_ids(&panes, None), panes);
        assert_eq!(visible_terminal_group_ids(&panes, Some(Uuid::nil())), panes);
        assert_eq!(
            toggle_maximized_terminal_pane(None, second, &panes),
            Some(second)
        );
        assert_eq!(
            toggle_maximized_terminal_pane(Some(second), second, &panes),
            None
        );
        assert_eq!(
            toggle_maximized_terminal_pane(Some(second), Uuid::nil(), &panes),
            Some(second)
        );
    }

    #[test]
    fn new_terminal_splits_the_focused_floating_group_and_the_maximized_standalone() {
        let floating = Uuid::from_u128(1);
        let standalone = Uuid::from_u128(2);
        let panel = Uuid::from_u128(3);
        // ⌘T on a focused floating card splits its own group in place —
        // even while a maximized standalone sits underneath.
        assert_eq!(
            new_terminal_split_target(Some(floating), Some(floating), None),
            Some(floating)
        );
        assert_eq!(
            new_terminal_split_target(Some(floating), Some(floating), Some(standalone)),
            Some(floating)
        );
        // A focused member of the maximized standalone grows that grid.
        assert_eq!(
            new_terminal_split_target(Some(standalone), None, Some(standalone)),
            Some(standalone)
        );
        // Focus in any other surface's terminal — a panel tab, or an
        // unmaximized standalone — keeps the chord's usual placement.
        assert_eq!(
            new_terminal_split_target(Some(panel), Some(floating), Some(standalone)),
            None
        );
        assert_eq!(
            new_terminal_split_target(Some(standalone), Some(floating), None),
            None
        );
        // With no terminal focused a maximized standalone still takes the
        // split; anything else falls through to the floating card.
        assert_eq!(
            new_terminal_split_target(None, None, Some(standalone)),
            Some(standalone)
        );
        assert_eq!(new_terminal_split_target(None, Some(floating), None), None);
    }

    fn split_record(id: Uuid) -> TerminalRecord {
        TerminalRecord {
            session: Some(id),
            pinned: false,
            working_directory: Some(PathBuf::from("/tmp")),
            last_working_directory: None,
            workspace_directory: None,
            opened_at: 42,
            custom_title: Some(id.to_string()),
            pane_of: None,
            panes: Vec::new(),
            pane_layout: None,
        }
    }

    #[test]
    fn terminal_split_directions_survive_closing_and_promoting_panes() {
        let [anchor, right, down, dropped] = [1, 2, 3, 4].map(Uuid::from_u128);
        let mut layout = TerminalPaneLayout::Pane(anchor);
        split_terminal_layout(&mut layout, anchor, right, false);
        split_terminal_layout(&mut layout, right, down, true);
        assert_eq!(
            layout,
            terminal_layout_pair(
                TerminalPaneLayout::Pane(anchor),
                terminal_layout_pair(
                    TerminalPaneLayout::Pane(right),
                    TerminalPaneLayout::Pane(down),
                    true
                ),
                false,
            )
        );
        // Closing the original anchor promotes its surviving subtree intact.
        let promoted = terminal_pane_layout(Some(&layout), &[right, down], false).unwrap();
        assert_eq!(
            promoted,
            terminal_layout_pair(
                TerminalPaneLayout::Pane(right),
                TerminalPaneLayout::Pane(down),
                true,
            )
        );
        // Closing a nested pane collapses only that split; new dropped panes
        // still render once alongside the surviving directional layout.
        assert_eq!(
            terminal_pane_layout(Some(&layout), &[anchor, down, dropped], false),
            Some(terminal_layout_pair(
                terminal_layout_pair(
                    TerminalPaneLayout::Pane(anchor),
                    TerminalPaneLayout::Pane(down),
                    false
                ),
                TerminalPaneLayout::Pane(dropped),
                false,
            ))
        );
        assert_eq!(
            terminal_pane_layout(Some(&layout), &[down], false),
            Some(TerminalPaneLayout::Pane(down))
        );
    }

    #[test]
    fn terminal_split_limits_count_anchor_and_reject_without_mutation() {
        for limit in [
            RIGHT_PANEL_TERMINAL_MAX_PANES,
            STANDALONE_TERMINAL_MAX_PANES,
        ] {
            let anchor = Uuid::from_u128(1);
            let mut records = HashMap::from([(anchor, split_record(anchor))]);
            for n in 2..=limit {
                let id = Uuid::from_u128(n as u128);
                records.insert(id, split_record(id));
                assert!(terminal_group_can_add(&records[&anchor], 1, limit));
                assert_eq!(
                    join_terminal_groups(&mut records, id, anchor, limit),
                    vec![id]
                );
            }
            let overflow = Uuid::from_u128(100);
            records.insert(overflow, split_record(overflow));
            assert!(!terminal_group_can_add(&records[&anchor], 1, limit));
            assert!(join_terminal_groups(&mut records, overflow, anchor, limit).is_empty());
            assert_eq!(terminal_group_ids(&records, anchor).len(), limit);
            assert_eq!(records[&overflow].pane_of, None);
            assert!(join_terminal_groups(&mut records, anchor, anchor, limit).is_empty());
        }
    }

    #[test]
    fn terminal_split_drop_moves_whole_group_preserving_identity_and_launch_metadata() {
        let ids = (1..=4).map(Uuid::from_u128).collect::<Vec<_>>();
        let mut records = ids
            .iter()
            .map(|id| (*id, split_record(*id)))
            .collect::<HashMap<_, _>>();
        join_terminal_groups(&mut records, ids[1], ids[0], 9);
        join_terminal_groups(&mut records, ids[3], ids[2], 9);
        assert!(!terminal_groups_fit(&records, ids[2], ids[0], 3));
        assert!(!terminal_groups_fit(&records, ids[1], ids[2], 9));
        let dragged_layout = terminal_layout_pair(
            TerminalPaneLayout::Pane(ids[2]),
            TerminalPaneLayout::Pane(ids[3]),
            true,
        );
        records.get_mut(&ids[2]).unwrap().pane_layout = Some(dragged_layout.clone());
        assert_eq!(
            join_terminal_groups(&mut records, ids[2], ids[0], 9),
            ids[2..]
        );
        assert_eq!(
            records[&ids[0]].pane_layout,
            Some(terminal_layout_pair(
                terminal_layout_pair(
                    TerminalPaneLayout::Pane(ids[0]),
                    TerminalPaneLayout::Pane(ids[1]),
                    false
                ),
                dragged_layout,
                false,
            ))
        );
        assert_eq!(terminal_group_ids(&records, ids[0]), ids);
        assert_eq!(records.len(), 4);
        for id in &ids[1..] {
            assert_eq!(records[id].pane_of, Some(ids[0]));
            assert!(records[id].panes.is_empty());
            assert_eq!(records[id].session, Some(*id));
            assert_eq!(records[id].custom_title, Some(id.to_string()));
            assert_eq!(records[id].working_directory, Some(PathBuf::from("/tmp")));
        }
    }

    #[test]
    fn terminal_split_persistence_accepts_legacy_records_and_round_trips_membership() {
        let id = Uuid::from_u128(1);
        let pane = Uuid::from_u128(2);
        let mut record: PersistedTerminal =
            serde_json::from_value(serde_json::json!({ "id": id, "opened_at": 42 }))
                .expect("legacy terminal record");
        assert_eq!(record.pane_of, None);
        assert!(record.panes.is_empty());
        assert!(record.pane_layout.is_none());
        record.panes.push(pane);
        record.pane_layout = Some(terminal_layout_pair(
            TerminalPaneLayout::Pane(id),
            TerminalPaneLayout::Pane(pane),
            true,
        ));
        let saved = serde_json::to_value(&record).unwrap();
        let restored: PersistedTerminal = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.id, id);
        assert_eq!(restored.panes, vec![pane]);
        assert_eq!(restored.pane_layout, record.pane_layout);
        record.id = pane;
        record.panes.clear();
        record.pane_of = Some(id);
        let restored: PersistedTerminal =
            serde_json::from_value(serde_json::to_value(&record).unwrap()).unwrap();
        assert_eq!(restored.id, pane);
        assert_eq!(restored.pane_of, Some(id));
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("goddard-code-run-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn terminal_location_selects_nearest_named_manifest() {
        let root = temp_root("package-labels");
        let nested = root.join("crates/child/src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("package.json"), r#"{"name":"@scope/parent"}"#).unwrap();
        assert_eq!(
            nearest_package_name(&nested).as_deref(),
            Some("@scope/parent")
        );
        std::fs::write(
            root.join("crates/child/Cargo.toml"),
            "[package]\nname = 'child' # comment\n",
        )
        .unwrap();
        assert_eq!(nearest_package_name(&nested).as_deref(), Some("child"));
        std::fs::write(
            nested.join("pubspec.yaml"),
            "dependencies:\n  nested:\n    name: wrong\nname: 'dart_child' # comment\n",
        )
        .unwrap();
        assert_eq!(nearest_package_name(&nested).as_deref(), Some("dart_child"));
        // Empty and malformed names do not hide a valid enclosing package.
        std::fs::write(nested.join("pubspec.yaml"), "name: ''\n").unwrap();
        std::fs::write(nested.join("package.json"), "not json").unwrap();
        assert_eq!(nearest_package_name(&nested).as_deref(), Some("child"));
        std::fs::write(
            root.join("crates/child/Cargo.toml"),
            "[workspace]\nmembers = []\n",
        )
        .unwrap();
        assert_eq!(
            nearest_package_name(&nested).as_deref(),
            Some("@scope/parent")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_location_names_distinguish_package_worktree_and_cwd() {
        let cwd = Path::new("/repo/src");
        for (package, worktree, expected) in [
            (None, None, vec![]),
            (Some("src"), None, vec![]),
            (Some("app"), None, vec![("icons/package.svg", "app")]),
            (
                None,
                Some("feature"),
                vec![("icons/git-branch.svg", "feature")],
            ),
            (
                Some("src"),
                Some("feature"),
                vec![("icons/git-branch.svg", "feature")],
            ),
            (Some("app"), Some("src"), vec![("icons/package.svg", "app")]),
            (
                Some("app"),
                Some("feature"),
                vec![
                    ("icons/package.svg", "app"),
                    ("icons/git-branch.svg", "feature"),
                ],
            ),
        ] {
            let location = TerminalLocation {
                package: package.map(str::to_owned),
                worktree: worktree.map(str::to_owned),
            };
            assert_eq!(
                terminal_location_names(cwd, &location),
                expected
                    .into_iter()
                    .map(|(icon, name)| (icon, name.to_owned()))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn terminal_location_names_managed_and_ordinary_linked_worktrees() {
        let root = temp_root("worktree-labels");
        let managed = root.join("worktrees/repo/feature/repo");
        let ordinary = root.join("ordinary-feature");
        let main = root.join("main");
        for checkout in [&managed, &ordinary, &main] {
            std::fs::create_dir_all(checkout).unwrap();
        }
        let metadata = root.join("git-metadata");
        std::fs::create_dir_all(&metadata).unwrap();
        std::fs::write(metadata.join("commondir"), "../..").unwrap();
        for checkout in [&managed, &ordinary] {
            std::fs::write(
                checkout.join(".git"),
                format!("gitdir: {}", metadata.display()),
            )
            .unwrap();
        }
        std::fs::create_dir(main.join(".git")).unwrap();
        assert_eq!(terminal_worktree_name(&managed).as_deref(), Some("feature"));
        assert_eq!(
            terminal_worktree_name(&ordinary).as_deref(),
            Some("ordinary-feature")
        );
        assert_eq!(terminal_worktree_name(&main), None);
        let submodule = root.join("submodule");
        std::fs::create_dir(&submodule).unwrap();
        std::fs::write(submodule.join(".git"), "gitdir: ../non-worktree-metadata").unwrap();
        assert_eq!(terminal_worktree_name(&submodule), None);
        assert_eq!(nearest_repo_root(&managed.join("src")), Some(managed));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn code_run_path_tokens_split_prose_and_strip_positions() {
        let tokens = code_run_path_tokens(
            "run `node scripts/db/feed.ts:42` then (bun ./tools/x.ts:7:3), see \"~/dev/app\"",
        );
        assert_eq!(
            tokens,
            [
                "run",
                "node",
                "scripts/db/feed.ts",
                "then",
                "bun",
                "./tools/x.ts",
                "see",
                "~/dev/app"
            ]
        );
    }

    #[test]
    fn code_run_token_projects_match_absolute_home_and_relative_paths() {
        let coral = temp_root("coral");
        let other = temp_root("other");
        let nested = coral.join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(coral.join("scripts/db")).unwrap();
        std::fs::write(coral.join("scripts/db/feed.ts"), "").unwrap();
        let coral_id = Uuid::from_u128(1);
        let other_id = Uuid::from_u128(2);
        let nested_id = Uuid::from_u128(3);
        let roots: Vec<(Uuid, &Path)> = vec![
            (coral_id, coral.as_path()),
            (other_id, other.as_path()),
            (nested_id, nested.as_path()),
        ];

        // An absolute path picks its project; a nested root wins.
        assert_eq!(
            code_run_token_projects(&roots, &other.join("src/main.rs").to_string_lossy(), None),
            [other_id]
        );
        assert_eq!(
            code_run_token_projects(&roots, &nested.join("a").to_string_lossy(), None),
            [nested_id]
        );
        // `~` expands through the given home before the prefix match.
        let home = temp_root("home");
        let home_coral = home.join("coral");
        std::fs::create_dir_all(&home_coral).unwrap();
        let home_roots: Vec<(Uuid, &Path)> = vec![(coral_id, home_coral.as_path())];
        assert_eq!(
            code_run_token_projects(&home_roots, "~/coral/f.ts", Some(home.as_path())),
            [coral_id]
        );
        assert!(
            code_run_token_projects(&home_roots, "~/elsewhere/f.ts", Some(home.as_path()))
                .is_empty()
        );
        // A relative path matches the root it exists under — and only that.
        assert_eq!(
            code_run_token_projects(&roots, "scripts/db/feed.ts", None),
            [coral_id]
        );
        assert_eq!(
            code_run_token_projects(&roots, "./scripts/db/feed.ts", None),
            [coral_id]
        );
        // Flags, URLs, escapes, and slash-free words never name a project.
        for token in ["../up/x", "--out/a", "https://a/b", "Makefile", ".env/x"] {
            assert!(
                code_run_token_projects(&roots, token, None).is_empty(),
                "{token} matched"
            );
        }
        std::fs::remove_dir_all(&coral).ok();
        std::fs::remove_dir_all(&other).ok();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn code_run_content_projects_leads_with_mentions_and_dedupes() {
        let coral = temp_root("coral");
        let other = temp_root("other");
        let coral_id = Uuid::from_u128(1);
        let other_id = Uuid::from_u128(2);
        let roots: Vec<(Uuid, &Path)> =
            vec![(coral_id, coral.as_path()), (other_id, other.as_path())];
        let coral_file = format!("{}/scripts/feed.ts", coral.display());
        let other_file = format!("{}/src/lib.rs", other.display());
        let projects = code_run_content_projects(
            &roots,
            &format!("The answer mentions {other_file} and again {coral_file}."),
            &format!("cat {coral_file}"),
            None,
        );
        assert_eq!(projects, [other_id, coral_id]);
        std::fs::remove_dir_all(&coral).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    #[test]
    fn code_run_turn_tasks_lead_with_the_message_then_each_trigger() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let turn = Uuid::new_v4();
        let employee_a = Uuid::new_v4();
        let employee_b = Uuid::new_v4();
        let mut report = Message::new_for_turn(MessageRole::User, "report", turn);
        report.report_trigger = Some(crate::model::ReportTrigger {
            event_id: Uuid::new_v4(),
            employee: employee_a,
            employee_name: "Faye".into(),
            job_title: "Runnable scripts".into(),
            kind: crate::model::ReportTriggerKind::Finished,
            boundary: crate::model::ReportTriggerBoundary::Opening,
            reference_context: None,
        });
        report.hidden = true;
        session.messages.push(report);
        let mut steer = Message::new_for_turn(MessageRole::User, "steer", turn);
        steer.report_trigger = Some(crate::model::ReportTrigger {
            event_id: Uuid::new_v4(),
            employee: employee_b,
            employee_name: "Ivo".into(),
            job_title: "Steer".into(),
            kind: crate::model::ReportTriggerKind::Finished,
            boundary: crate::model::ReportTriggerBoundary::Steer,
            reference_context: None,
        });
        steer.hidden = true;
        session.messages.push(steer);
        let answer = Message::new_for_turn(MessageRole::Assistant, "answer", turn);
        let answer_id = answer.id;
        session.messages.push(answer);

        let message = session.messages.iter().find(|m| m.id == answer_id);
        assert_eq!(
            code_run_turn_tasks(&session, message),
            [employee_a, employee_b]
        );
        // No message, no tasks — the boss project fallback decides.
        assert!(code_run_turn_tasks(&session, None).is_empty());
        // A task-sent prompt leads with its own author.
        let sender = Uuid::new_v4();
        let mut sent = Message::new_for_turn(MessageRole::User, "sent", Uuid::new_v4());
        sent.sent_by_task = Some(sender);
        let sent_id = sent.id;
        session.messages.push(sent);
        let message = session.messages.iter().find(|m| m.id == sent_id);
        assert_eq!(code_run_turn_tasks(&session, message), [sender]);
    }

    #[test]
    fn nearest_repo_root_handles_nested_repos_worktrees_and_non_repo_cwds() {
        let root = std::env::temp_dir().join(format!("goddard-terminal-repo-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let nested = repo.join("nested");
        let cwd = nested.join("src");
        let outside = root.join("outside");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir(repo.join(".git")).unwrap();

        assert_eq!(nearest_repo_root(&repo), Some(repo.clone()));
        assert_eq!(nearest_repo_root(&cwd), Some(repo.clone()));
        // A linked worktree has a .git file, and the nearest marker wins.
        std::fs::write(nested.join(".git"), "gitdir: /unused/worktree\n").unwrap();
        assert_eq!(nearest_repo_root(&cwd), Some(nested));
        assert_eq!(nearest_repo_root(&outside), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sidebar_terminal_order_leads_with_pinned() {
        let ids = (1..=4u128).map(Uuid::from_u128).collect::<Vec<_>>();
        let record = |pinned| TerminalRecord {
            session: None,
            pinned,
            working_directory: None,
            last_working_directory: None,
            workspace_directory: None,
            opened_at: 0,
            custom_title: None,
            pane_of: None,
            panes: Vec::new(),
            pane_layout: None,
        };
        let mut records = HashMap::new();
        records.insert(ids[0], record(false));
        records.insert(ids[1], record(true));
        records.insert(ids[2], record(false));
        records.insert(ids[3], record(true));

        // Each partition keeps the flat list's creation order.
        let listed = sidebar_terminal_order(&ids, &records).collect::<Vec<_>>();
        assert_eq!(listed, vec![ids[1], ids[3], ids[0], ids[2]]);

        // Order entries without records drop out of the listing.
        records.remove(&ids[3]);
        let listed = sidebar_terminal_order(&ids, &records).collect::<Vec<_>>();
        assert_eq!(listed, vec![ids[1], ids[0], ids[2]]);
    }

    #[test]
    fn sidebar_terminal_order_hides_panes() {
        // A split group's panes live inside their anchor's surface — even
        // pinned, they never get a row of their own.
        let ids = (1..=3u128).map(Uuid::from_u128).collect::<Vec<_>>();
        let record = |pinned, pane_of| TerminalRecord {
            session: None,
            pinned,
            working_directory: None,
            last_working_directory: None,
            workspace_directory: None,
            opened_at: 0,
            custom_title: None,
            pane_of,
            panes: Vec::new(),
            pane_layout: None,
        };
        let mut records = HashMap::new();
        records.insert(ids[0], record(false, None));
        records.insert(ids[1], record(true, Some(ids[0])));
        records.insert(ids[2], record(false, Some(ids[0])));

        let listed = sidebar_terminal_order(&ids, &records).collect::<Vec<_>>();
        assert_eq!(listed, vec![ids[0]]);
    }

    #[test]
    fn ancestor_truncation_leaves_short_paths_alone() {
        assert_eq!(truncate_path_ancestors("~/src", 32, false), "~/src");
        assert_eq!(truncate_path_ancestors("waku", 4, true), "waku");
    }

    #[test]
    fn ancestor_truncation_pops_whole_components() {
        assert_eq!(
            truncate_path_ancestors("~/very/long/ancestor/path", 12, false),
            "…/path"
        );
        assert_eq!(
            truncate_path_ancestors("~/very/long/ancestor/path", 16, false),
            "…/ancestor/path"
        );
    }

    #[test]
    fn ancestor_truncation_keeps_the_repo_name_anchored() {
        assert_eq!(
            truncate_path_ancestors("repo/alpha/beta/gamma", 14, true),
            "repo/…/gamma"
        );
        assert_eq!(
            truncate_path_ancestors("repo/alpha/beta/gamma", 18, true),
            "repo/…/beta/gamma"
        );
        // Two components have no ancestors to pop — nothing is faked.
        assert_eq!(truncate_path_ancestors("repo/leaf", 4, true), "repo/leaf");
    }

    #[test]
    fn ancestor_truncation_never_cuts_through_a_component() {
        let truncated = truncate_path_ancestors("~/a/averyverylongleafname", 8, false);
        assert_eq!(truncated, "…/averyverylongleafname");
        assert!(
            truncated.ends_with("averyverylongleafname"),
            "the leaf stays whole even past the budget: {truncated}"
        );
        // A lone component has no ancestors to pop.
        assert_eq!(
            truncate_path_ancestors("averyverylongname", 8, false),
            "averyverylongname"
        );
    }
}
