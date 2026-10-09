use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

use super::*;
use crate::ui::ActivationExt;

const TAB_SCROLL_FADE_WIDTH: f32 = 24.0;
const REVIEW_DIFF_FILE_HEADER_HEIGHT: f32 = 36.0;

/// Image-preview zoom is image-pixels → screen-pixels. The bounds mirror
/// Zed's image viewer, widened downward so tall thumbnails can shrink to
/// fit narrow panes.
const FILE_IMAGE_MIN_ZOOM: f32 = 0.05;
const FILE_IMAGE_MAX_ZOOM: f32 = 32.0;
/// Wheel deltas arrive in pixels on macOS; line-based wheels (Windows,
/// Linux) use the same 20px-per-line convention as Zed.
const FILE_IMAGE_SCROLL_LINE_PX: f32 = 20.0;
/// ⌘+scroll zoom sensitivity: a 100px wheel tick scales by ~2.7×.
const FILE_IMAGE_ZOOM_PER_PIXEL: f32 = 0.01;

/// Extra scrollable room past the end of a file, in text lines — the file
/// editor and the markdown preview each pad their scroll extent by this so
/// the last lines can scroll up off the pane's bottom edge.
const FILE_SCROLL_PAD_LINES: f32 = 10.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WorkingTreeEntry {
    relative_path: String,
    absolute_path: PathBuf,
    name: String,
    is_dir: bool,
    file_icon: Option<&'static str>,
    expanded: bool,
    depth: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TranscriptLinkRoute {
    ProjectFile(String, Option<String>),
    Finder(PathBuf),
    /// A `goddard://task/<id>` reference — `None` when the id is malformed.
    /// A `?message=<id>` query deep-links to that message's transcript row.
    Task(Option<Uuid>, Option<Uuid>),
    External,
}

pub(super) fn positive_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<usize>().is_ok_and(|value| value > 0)
}

/// The host whose Boss document lists `session_id`'s plan owns its reads —
/// the caller's key when it lists the plan, else whichever connected host
/// does, else the caller. `daemons.session_owner` resolves an unclaimed
/// remote session to `Local`, where `plans/<name>.md` does not
/// exist, so callers resolving keys independently would otherwise
/// re-arm — and blank — a healthy read under the wrong host.
pub(super) fn plan_doc_host(
    states: &HashMap<waku_client::DaemonKey, waku_client::boss::BossState>,
    caller: waku_client::DaemonKey,
    session_id: Uuid,
) -> waku_client::DaemonKey {
    let lists_plan = |key: waku_client::DaemonKey| {
        states.get(&key).is_some_and(|state| {
            state
                .planning
                .iter()
                .any(|plan| plan.session_id == session_id)
        })
    };
    if lists_plan(caller) {
        return caller;
    }
    states
        .keys()
        .find(|key| lists_plan(**key))
        .copied()
        .unwrap_or(caller)
}

fn line_fragment(fragment: &str) -> bool {
    let Some(location) = fragment.strip_prefix('L') else {
        return false;
    };
    match location.split_once('C') {
        Some((line, column)) => positive_number(line) && positive_number(column),
        None => positive_number(location),
    }
}

/// Removes the `:line`, `:line:column`, or `#LlineCcolumn` suffixes Codex uses
/// in clickable local-file references. The location is not yet consumed by
/// Goddard's compact editor, but it must not become part of the filesystem path.
fn strip_file_location(target: &str) -> &str {
    if let Some((path, fragment)) = target.rsplit_once('#')
        && line_fragment(fragment)
    {
        return path;
    }

    let Some((before_last, last)) = target.rsplit_once(':') else {
        return target;
    };
    if !positive_number(last) {
        return target;
    }
    if let Some((path, line)) = before_last.rsplit_once(':')
        && positive_number(line)
    {
        path
    } else {
        before_last
    }
}

/// The line target on a transcript file link — the `:line`, `:line:column`,
/// or `#LlineCcolumn` suffix `strip_file_location` removes from the path.
/// Mirrors that stripping exactly, so a location that survives as part of the
/// path is never also reported here.
fn file_link_location(target: &str) -> Option<(usize, Option<usize>)> {
    let target = target.trim();
    if let Some((_, fragment)) = target.rsplit_once('#')
        && line_fragment(fragment)
    {
        let location = fragment.strip_prefix('L')?;
        let (line, column) = match location.split_once('C') {
            Some((line, column)) => (line, Some(column)),
            None => (location, None),
        };
        let column = match column {
            Some(column) => Some(column.parse().ok()?),
            None => None,
        };
        return Some((line.parse().ok()?, column));
    }

    let (before_last, last) = target.rsplit_once(':')?;
    if !positive_number(last) {
        return None;
    }
    let last = last.parse().ok()?;
    if let Some((_, line)) = before_last.rsplit_once(':')
        && positive_number(line)
    {
        return Some((line.parse().ok()?, Some(last)));
    }
    Some((last, None))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_file_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(index + 1).copied().and_then(hex_value),
                bytes.get(index + 2).copied().and_then(hex_value),
            )
        {
            decoded.push(high << 4 | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| path.to_owned())
}

fn markdown_file_link_path(target: &str) -> Option<PathBuf> {
    let path = markdown_file_link_path_inner(target)?;
    path.is_absolute().then_some(path)
}

/// Provider references may name a file relative to their captured workspace.
/// Keep this permissive parser scoped to those references; ordinary Markdown
/// links still require an absolute file path before they enter the file route.
fn reference_file_link_path(target: &str) -> Option<PathBuf> {
    markdown_file_link_path_inner(target)
}

fn markdown_file_link_path_inner(target: &str) -> Option<PathBuf> {
    let target = strip_file_location(target.trim());
    let target = target
        .rsplit_once('#')
        .filter(|(path, fragment)| {
            !line_fragment(fragment) && path.to_ascii_lowercase().ends_with(".md")
        })
        .map_or(target, |(path, _)| path);
    if target
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
    {
        return url::Url::parse(target).ok()?.to_file_path().ok();
    }

    Some(PathBuf::from(percent_decode_file_path(target)))
}

fn markdown_file_link_heading(target: &str) -> Option<String> {
    let (path_target, fragment) = target.trim().rsplit_once('#')?;
    if fragment.is_empty() || line_fragment(fragment) {
        return None;
    }
    let path = markdown_file_link_path(path_target)?;
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        .then(|| percent_decode_file_path(fragment))
}

fn markdown_heading_slug(text: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else if character.is_whitespace() || character == '-' {
            separator = true;
        }
    }
    slug
}

fn markdown_heading_line(content: &str, fragment: &str) -> Option<usize> {
    let target = markdown_heading_slug(fragment);
    if target.is_empty() {
        return None;
    }
    let mut prior = Vec::<String>::new();
    for (index, line) in content.lines().enumerate() {
        let heading = line.trim_start();
        let level = heading.bytes().take_while(|byte| *byte == b'#').count();
        if !(1..=6).contains(&level) {
            continue;
        }
        let Some(title) = heading
            .get(level..)
            .filter(|rest| rest.chars().next().is_some_and(char::is_whitespace))
        else {
            continue;
        };
        let title = title.trim().trim_end_matches('#').trim();
        let base = markdown_heading_slug(title);
        if base.is_empty() {
            continue;
        }
        let duplicate = prior.iter().filter(|slug| **slug == base).count();
        let slug = if duplicate == 0 {
            base.clone()
        } else {
            format!("{base}-{duplicate}")
        };
        if slug == target {
            return Some(index + 1);
        }
        prior.push(base);
    }
    None
}

fn normalized_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn workspace_relative_file_path(workspace: &Path, target: &Path) -> Option<String> {
    fn relative(workspace: &Path, target: &Path) -> Option<String> {
        let relative = target.strip_prefix(workspace).ok()?;
        if relative.as_os_str().is_empty() {
            return None;
        }
        Some(relative.to_string_lossy().into_owned())
    }

    let workspace = normalized_path(workspace);
    let target = normalized_path(target);
    // These are daemon-host paths. Routing is intentionally lexical: probing
    // the desktop filesystem would reinterpret a remote workspace locally.
    relative(&workspace, &target)
}

fn transcript_link_route(target: &str, workspace: Option<&Path>) -> TranscriptLinkRoute {
    // A task reference never reaches the file or browser paths — a malformed
    // id is reported as a bad task link rather than opened externally.
    if let Some(rest) = target.strip_prefix(waku_protocol::TASK_LINK_PREFIX) {
        let (id, query) = rest.split_once('?').unwrap_or((rest, ""));
        let message = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("message="))
            .and_then(|value| Uuid::parse_str(value).ok());
        return TranscriptLinkRoute::Task(Uuid::parse_str(id.trim_end_matches('/')).ok(), message);
    }
    let Some(path) = markdown_file_link_path(target) else {
        return TranscriptLinkRoute::External;
    };
    let heading = markdown_file_link_heading(target);
    let path = normalized_path(&path);
    if let Some(relative_path) =
        workspace.and_then(|workspace| workspace_relative_file_path(workspace, &path))
    {
        TranscriptLinkRoute::ProjectFile(relative_path, heading)
    } else {
        TranscriptLinkRoute::Finder(path)
    }
}

/// What a right-clicked transcript link's copy item puts on the clipboard,
/// and its label key: file targets copy their decoded path — the `:line` /
/// `#heading` suffixes and `file:` scheme are link addressing, not part of
/// the path — while everything else copies the link target itself.
fn transcript_link_copy(url: &str) -> (String, &'static str) {
    match markdown_file_link_path(url) {
        Some(path) => (path.to_string_lossy().into_owned(), "common.copy_file_path"),
        None => (url.to_owned(), "common.copy_url"),
    }
}

/// Byte offset of a 1-based `line:column` in `content`, clamped into the
/// file: a line past the end lands at the end, a column past its line's end
/// lands on the line break. `column` counts characters, the way editors
/// spell it.
fn cursor_offset_for_line_column(content: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (index, text) in content.split('\n').enumerate() {
        if index + 1 == line.max(1) {
            return offset
                + text
                    .chars()
                    .take(column.saturating_sub(1))
                    .map(char::len_utf8)
                    .sum::<usize>();
        }
        offset += text.len() + 1;
    }
    content.len()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ReviewDiffTreeRow {
    Directory {
        path: String,
        name: String,
        depth: usize,
        expanded: bool,
    },
    File {
        file_index: usize,
        depth: usize,
    },
}

/// Select from a compact, embedded subset of Material Icon Theme rather than
/// shipping its entire icon catalog. The SVG path is resolved once per entry
/// during the directory scan, not on every row paint.
pub(super) fn file_icon_for_path(path: &str) -> &'static str {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    file_icon_for_name(name)
}

fn review_diff_gap_icon_path(direction: crate::review_diff::ExpansionDirection) -> &'static str {
    match direction {
        // Pierre's direction attributes and rendered chevrons are inverted by
        // CSS. Goddard names the data operation directly, so encode the resulting
        // visual here: reveal-from-start points down; reveal-from-end points up.
        crate::review_diff::ExpansionDirection::Start => "icons/chevron-down.svg",
        crate::review_diff::ExpansionDirection::End => "icons/chevron-up.svg",
        crate::review_diff::ExpansionDirection::Both
        | crate::review_diff::ExpansionDirection::All => "icons/chevrons-up-down.svg",
    }
}

fn review_diff_gap_tooltip(direction: crate::review_diff::ExpansionDirection) -> String {
    match direction {
        crate::review_diff::ExpansionDirection::Start => tr!("diff.expand_context_below"),
        crate::review_diff::ExpansionDirection::End => tr!("diff.expand_context_above"),
        crate::review_diff::ExpansionDirection::Both => tr!("diff.expand_context"),
        crate::review_diff::ExpansionDirection::All => tr!("diff.expand_all_context"),
    }
}

fn review_diff_gap_directions(
    position: crate::review_diff::GapPosition,
    chunked: bool,
) -> &'static [crate::review_diff::ExpansionDirection] {
    use crate::review_diff::{ExpansionDirection, GapPosition};

    match (position, chunked) {
        (GapPosition::Leading, _) => &[ExpansionDirection::End],
        (GapPosition::Trailing, _) => &[ExpansionDirection::Start],
        (GapPosition::Between, false) => &[ExpansionDirection::Both],
        (GapPosition::Between, true) => &[ExpansionDirection::Start, ExpansionDirection::End],
    }
}

fn review_diff_directory_paths(files: &[crate::review_diff::File]) -> HashSet<String> {
    let mut paths = HashSet::new();
    for file in files {
        let parts = file.path.split('/').collect::<Vec<_>>();
        let mut path = String::new();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            paths.insert(path.clone());
        }
    }
    paths
}

pub(super) fn review_diff_tree_rows(
    files: &[crate::review_diff::File],
    expanded_paths: &HashSet<String>,
    filter: &str,
) -> Vec<ReviewDiffTreeRow> {
    let filter = filter.trim().to_ascii_lowercase();
    let filtering = !filter.is_empty();
    let mut indexes = files
        .iter()
        .enumerate()
        .filter(|(_, file)| {
            filtering
                .then(|| file.path.to_ascii_lowercase().contains(&filter))
                .unwrap_or(true)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    indexes.sort_by_key(|index| files[*index].path.to_ascii_lowercase());

    let mut rows = Vec::new();
    let mut emitted_directories = HashSet::new();
    for file_index in indexes {
        let parts = files[file_index].path.split('/').collect::<Vec<_>>();
        let mut directory = String::new();
        let mut visible = true;
        for (depth, part) in parts.iter().take(parts.len().saturating_sub(1)).enumerate() {
            if !directory.is_empty() {
                directory.push('/');
            }
            directory.push_str(part);
            let expanded = filtering || expanded_paths.contains(&directory);
            if emitted_directories.insert(directory.clone()) && visible {
                rows.push(ReviewDiffTreeRow::Directory {
                    path: directory.clone(),
                    name: (*part).to_owned(),
                    depth,
                    expanded,
                });
            }
            if !expanded {
                visible = false;
                break;
            }
        }
        if visible {
            rows.push(ReviewDiffTreeRow::File {
                file_index,
                depth: parts.len().saturating_sub(1),
            });
        }
    }
    rows
}

/// How wide and tall a diff row is drawn. The Review panel is a reading
/// surface; the copy embedded in a transcript activity is a summary and gives
/// its space back to the code.
#[derive(Clone)]
pub(super) struct DiffRowStyle {
    gutter_width: f32,
    row_height: f32,
    text_size: f32,
    /// The code face rows shape against. Carried here rather than re-read per
    /// row so a font change mid-frame cannot split one diff across two faces.
    code_family: SharedString,
    /// What to put in the gutter of a row that has no line number. Git always
    /// reports positions, so this only comes up on a diff synthesized from a
    /// provider's before/after text: there the `+`/`-` marker stands in, which
    /// keeps the gutter from going blank and the meaning off color alone.
    marker_fallback: bool,
}

impl DiffRowStyle {
    /// Review-tab rows at the user's code font size. The gutter holds a
    /// right-aligned line number: ~0.6em per mono digit, five digits, plus
    /// its padding and border.
    pub(super) fn review(text_size: f32, code_family: SharedString) -> Self {
        Self {
            gutter_width: (text_size * 3.0 + 14.0).round(),
            row_height: (text_size * 1.5).round(),
            text_size,
            code_family,
            marker_fallback: false,
        }
    }

    /// The same rows the Review tab draws, so an edit reads the same wherever
    /// it is opened.
    pub(super) fn activity(text_size: f32, code_family: SharedString) -> Self {
        Self {
            marker_fallback: true,
            ..Self::review(text_size, code_family)
        }
    }

    pub(super) fn gutter_width(&self) -> f32 {
        self.gutter_width
    }
}

/// Selection identity for one diff code row. Selection resolves a drag by
/// looking rows up by key, so every row must have its own.
///
/// Rows with line numbers key on them: they survive Review's gap expansion,
/// where a revealed gap shifts every later row's index. Rows without them — a
/// diff synthesized from a provider's before/after text — key on the row index
/// instead, which is stable there because an activity diff is only ever
/// rebuilt whole. Keying those on their (absent) numbers gave every added row
/// the same key, and a drag resolved against whichever duplicate registered
/// first: selections jumped rows, skipped wrapped lines, and collapsed when
/// the head crossed into context.
fn diff_row_selection_key(
    key_prefix: &str,
    line: &crate::review_diff::Line,
    index: usize,
) -> String {
    let kind = match &line.kind {
        crate::review_diff::LineKind::Context => "context",
        crate::review_diff::LineKind::Addition => "addition",
        crate::review_diff::LineKind::Deletion => "deletion",
        _ => "other",
    };
    match (line.old_line, line.new_line) {
        (None, None) => format!("{key_prefix}-line-{}-{kind}-i{index}", line.file_index),
        (old, new) => format!(
            "{key_prefix}-line-{}-{kind}-{}-{}",
            line.file_index,
            old.unwrap_or(0),
            new.unwrap_or(0),
        ),
    }
}

/// One context, addition, or deletion row, shared by the Review panel and the
/// diff inside an expanded file-change activity so the two never drift.
pub(super) fn render_diff_code_row(
    line: &crate::review_diff::Line,
    index: usize,
    key_prefix: &str,
    selection: &TranscriptSelection,
    style: DiffRowStyle,
    theme: &Theme,
) -> AnyElement {
    let semantic_body_opacity = if theme.is_dark { 0.20 } else { 0.12 };
    let semantic_gutter_opacity = if theme.is_dark { 0.15 } else { 0.09 };
    let (marker, body_background, gutter_background, edge, number_color) = match &line.kind {
        crate::review_diff::LineKind::Addition => (
            "+",
            Some(theme.success.opacity(semantic_body_opacity)),
            Some(theme.success.opacity(semantic_gutter_opacity)),
            Some(theme.success),
            theme.success,
        ),
        crate::review_diff::LineKind::Deletion => (
            "-",
            Some(theme.danger.opacity(semantic_body_opacity)),
            Some(theme.danger.opacity(semantic_gutter_opacity)),
            Some(theme.danger),
            theme.danger,
        ),
        _ => (" ", None, None, None, theme.text_tertiary),
    };
    let shown_line = line.new_line.or(line.old_line);
    let flat = review_diff_flat_text(line, theme, &style.code_family);
    let selectable = md::render::selectable_flat_text(
        &flat,
        crate::md::selection::TextKey::new(diff_row_selection_key(key_prefix, line, index), 0),
        selection.clone(),
        theme.code_wash,
        theme.selection,
        false,
    );
    let gutter = div()
        .w(px(style.gutter_width))
        .min_h(px(style.row_height))
        .self_stretch()
        .flex_none()
        .pr(px(9.0))
        .flex()
        .items_start()
        .justify_end()
        .border_r(hairline())
        .border_color(theme.separator)
        .text_color(number_color)
        .when_some(gutter_background, |gutter, background| {
            gutter.bg(background)
        })
        .child(
            shown_line
                .map(|line| line.to_string())
                .or_else(|| style.marker_fallback.then(|| marker.to_owned()))
                .unwrap_or_default(),
        );
    let body = div()
        .min_h(px(style.row_height))
        .self_stretch()
        .min_w_0()
        .flex_1()
        .pl(px(12.0))
        .flex()
        .items_start()
        .when_some(body_background, |body, background| body.bg(background))
        .child(
            div()
                .id(SharedString::from(format!(
                    "{key_prefix}-line-content-{index}"
                )))
                .min_h(px(style.row_height))
                .min_w_0()
                .flex_1()
                .pr(px(10.0))
                .flex()
                .items_start()
                .overflow_hidden()
                .whitespace_normal()
                .child(selectable),
        );
    div()
        .id(SharedString::from(format!("{key_prefix}-row-{index}")))
        .w_full()
        .min_w_0()
        .min_h(px(style.row_height))
        // A wrapped line makes the row taller than one line. Stacked in a
        // scrolling column, a shrinkable row would be squeezed back to one
        // and paint its overflow over the row beneath it.
        .flex_none()
        .flex()
        .items_stretch()
        .font_family(style.code_family.clone())
        .text_size(px(style.text_size))
        .line_height(px(style.row_height))
        .when_some(edge, |row, edge| row.border_l_2().border_color(edge))
        .child(gutter)
        .child(body)
        .into_any_element()
}

fn review_diff_flat_text(
    line: &crate::review_diff::Line,
    theme: &Theme,
    code_family: &SharedString,
) -> md::render::FlatText {
    let text = line.content.clone();
    let palette = MarkdownPalette::from_theme(theme);
    let code_font = font(code_family.clone());
    let mut runs = Vec::with_capacity(line.tokens.len() * 2 + 1);
    let mut offset = 0;
    let mut push = |len: usize, color: Hsla| {
        if len > 0 {
            runs.push(TextRun {
                len,
                font: code_font.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
    };
    for token in &line.tokens {
        if token.range.start > offset {
            push(token.range.start - offset, theme.text_secondary);
        }
        push(token.range.len(), palette.token(token.class));
        offset = token.range.end;
    }
    if offset < text.len() {
        push(text.len() - offset, theme.text_secondary);
    }
    md::render::FlatText {
        text: text.into(),
        runs,
        links: Vec::new(),
        code_ranges: Vec::new(),
        atom_ranges: Vec::new(),
        annotation_refs: Vec::new(),
        commit_refs: Vec::new(),
        file_refs: Vec::new(),
        math: None,
        copy: Rc::default(),
    }
}

fn file_icon_for_name(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    let named_icon = if name.starts_with("readme") {
        Some("icons/file-types/readme.svg")
    } else if name.starts_with("license")
        || name.starts_with("licence")
        || name.starts_with("copying")
    {
        Some("icons/file-types/certificate.svg")
    } else if name.starts_with("dockerfile") || name.starts_with("compose.") {
        Some("icons/file-types/docker.svg")
    } else if name == "cmakelists.txt" || name.starts_with("cmake.") {
        Some("icons/file-types/cmake.svg")
    } else if name == "makefile" || name.starts_with("makefile.") || name == "justfile" {
        Some("icons/file-types/makefile.svg")
    } else if matches!(
        name.as_str(),
        "cargo.toml" | "cargo.lock" | "rust-toolchain.toml"
    ) {
        Some("icons/file-types/rust.svg")
    } else if matches!(name.as_str(), "go.mod" | "go.sum" | "go.work") {
        Some("icons/file-types/go.svg")
    } else if name == "pyproject.toml" || name == "pipfile" || name.starts_with("requirements") {
        Some("icons/file-types/python.svg")
    } else if matches!(name.as_str(), "bun.lock" | "bun.lockb" | "bunfig.toml") {
        Some("icons/file-types/bun.svg")
    } else if name.starts_with("pnpm-") || name == ".pnpmfile.cjs" {
        Some("icons/file-types/pnpm.svg")
    } else if name == "yarn.lock" || name.starts_with(".yarnrc") {
        Some("icons/file-types/yarn.svg")
    } else if name == "package.json" {
        Some("icons/file-types/nodejs.svg")
    } else if name == "package-lock.json" {
        Some("icons/file-types/npm.svg")
    } else if name.starts_with("tsconfig.") || name == "tsconfig.json" {
        Some("icons/file-types/typescript.svg")
    } else if name.starts_with("jsconfig.") || name == "jsconfig.json" {
        Some("icons/file-types/javascript.svg")
    } else if name == ".gitignore"
        || name == ".gitattributes"
        || name == ".gitmodules"
        || name == ".gitconfig"
    {
        Some("icons/file-types/git.svg")
    } else if name == ".editorconfig" {
        Some("icons/file-types/editorconfig.svg")
    } else if name.starts_with(".env") {
        Some("icons/file-types/settings.svg")
    } else if name.starts_with(".prettier") || name.starts_with("prettier.config.") {
        Some("icons/file-types/prettier.svg")
    } else if name.starts_with(".eslint") || name.starts_with("eslint.config.") {
        Some("icons/file-types/eslint.svg")
    } else if name.starts_with("biome.json") {
        Some("icons/file-types/biome.svg")
    } else if name.starts_with(".babel") || name.starts_with("babel.config.") {
        Some("icons/file-types/babel.svg")
    } else if name.starts_with(".stylelint") || name.starts_with("stylelint.config.") {
        Some("icons/file-types/stylelint.svg")
    } else if name.starts_with("vite.config.") {
        Some("icons/file-types/vite.svg")
    } else if name.starts_with("vitest.config.") || name.starts_with("vitest.workspace.") {
        Some("icons/file-types/vitest.svg")
    } else if name.starts_with("webpack.") {
        Some("icons/file-types/webpack.svg")
    } else if name.starts_with("rollup.config.") {
        Some("icons/file-types/rollup.svg")
    } else if name.starts_with("next.config.") {
        Some("icons/file-types/next.svg")
    } else if name == "next-env.d.ts" {
        Some("icons/file-types/next.svg")
    } else if name.starts_with("nuxt.config.") || name == ".nuxtrc" {
        Some("icons/file-types/nuxt.svg")
    } else if name.starts_with("astro.config.") {
        Some("icons/file-types/astro.svg")
    } else if name == "angular.json" || name.ends_with(".component.ts") {
        Some("icons/file-types/angular.svg")
    } else if name == "nest-cli.json" {
        Some("icons/file-types/nest.svg")
    } else if name.starts_with("tailwind.config.") {
        Some("icons/file-types/tailwindcss.svg")
    } else if name.starts_with("svelte.config.") {
        Some("icons/file-types/svelte.svg")
    } else if name.starts_with("vue.config.") {
        Some("icons/file-types/vue.svg")
    } else if name == "firebase.json" || name == ".firebaserc" {
        Some("icons/file-types/firebase.svg")
    } else if name == "supabase.toml" {
        Some("icons/file-types/supabase.svg")
    } else if name.starts_with("prisma.config.") {
        Some("icons/file-types/prisma.svg")
    } else if name == "turbo.json" {
        Some("icons/file-types/turborepo.svg")
    } else if name.starts_with("deno.json") || name == "deno.lock" {
        Some("icons/file-types/deno.svg")
    } else if name == ".gitlab-ci.yml" || name == ".gitlab-ci.yaml" {
        Some("icons/file-types/gitlab.svg")
    } else if name == "kustomization.yaml" || name == "kustomization.yml" {
        Some("icons/file-types/kubernetes.svg")
    } else if name == "chart.yaml" || name == "values.yaml" {
        Some("icons/file-types/helm.svg")
    } else if name == "nginx.conf" {
        Some("icons/file-types/nginx.svg")
    } else if name == ".nvmrc" || name == ".node-version" {
        Some("icons/file-types/nodejs.svg")
    } else if name == "build.gradle"
        || name == "settings.gradle"
        || name == "gradlew"
        || name == "gradlew.bat"
    {
        Some("icons/file-types/gradle.svg")
    } else if name.contains(".stories.") || name.contains(".story.") {
        Some("icons/file-types/storybook.svg")
    } else if name == "gemfile" || name == "gemfile.lock" {
        Some("icons/file-types/ruby.svg")
    } else if name == "pom.xml" {
        Some("icons/file-types/java.svg")
    } else {
        None
    };
    if let Some(icon) = named_icon {
        return icon;
    }

    let extension = Path::new(&name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    match extension {
        "rs" => "icons/file-types/rust.svg",
        "js" | "mjs" | "cjs" => "icons/file-types/javascript.svg",
        "ts" | "mts" | "cts" => "icons/file-types/typescript.svg",
        "jsx" | "tsx" => "icons/file-types/react.svg",
        "py" | "pyi" | "pyw" => "icons/file-types/python.svg",
        "go" => "icons/file-types/go.svg",
        "c" | "h" | "m" => "icons/file-types/c.svg",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" | "mm" => "icons/file-types/cpp.svg",
        "cs" => "icons/file-types/csharp.svg",
        "swift" => "icons/file-types/swift.svg",
        "kt" | "kts" => "icons/file-types/kotlin.svg",
        "java" | "class" => "icons/file-types/java.svg",
        "rb" => "icons/file-types/ruby.svg",
        "php" => "icons/file-types/php.svg",
        "html" | "htm" => "icons/file-types/html.svg",
        "css" | "less" => "icons/file-types/css.svg",
        "scss" | "sass" => "icons/file-types/sass.svg",
        "json" | "jsonc" | "jsonl" => "icons/file-types/json.svg",
        "yaml" | "yml" => "icons/file-types/yaml.svg",
        "toml" | "ini" | "cfg" | "conf" | "config" => "icons/file-types/settings.svg",
        "xml" | "xsl" | "plist" => "icons/file-types/xml.svg",
        "md" | "mdx" | "markdown" => "icons/file-types/markdown.svg",
        "sh" | "bash" | "zsh" | "fish" => "icons/file-types/console.svg",
        "ps1" | "psm1" => "icons/file-types/powershell.svg",
        "sql" | "db" | "sqlite" | "sqlite3" | "csv" | "xls" | "xlsx" => {
            "icons/file-types/database.svg"
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "ico" | "tiff" => {
            "icons/file-types/image.svg"
        }
        "svg" => "icons/file-types/svg.svg",
        "pdf" => "icons/file-types/pdf.svg",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => "icons/file-types/audio.svg",
        "mp4" | "mov" | "avi" | "webm" | "mkv" => "icons/file-types/video.svg",
        "zip" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "tar" | "jar" => {
            "icons/file-types/zip.svg"
        }
        "wasm" | "wat" => "icons/file-types/webassembly.svg",
        "svelte" => "icons/file-types/svelte.svg",
        "vue" => "icons/file-types/vue.svg",
        "tf" | "tfvars" => "icons/file-types/terraform.svg",
        "graphql" | "gql" => "icons/file-types/graphql.svg",
        "lua" => "icons/file-types/lua.svg",
        "dart" => "icons/file-types/dart.svg",
        "astro" => "icons/file-types/astro.svg",
        "coffee" | "cson" => "icons/file-types/coffee.svg",
        "cr" => "icons/file-types/crystal.svg",
        "ex" | "exs" => "icons/file-types/elixir.svg",
        "elm" => "icons/file-types/elm.svg",
        "erl" | "hrl" => "icons/file-types/erlang.svg",
        "clj" | "cljs" | "cljc" | "edn" => "icons/file-types/clojure.svg",
        "hs" | "lhs" => "icons/file-types/haskell.svg",
        "hx" | "hxml" => "icons/file-types/haxe.svg",
        "jinja" | "jinja2" | "j2" => "icons/file-types/jinja.svg",
        "jl" => "icons/file-types/julia.svg",
        "ml" | "mli" => "icons/file-types/ocaml.svg",
        "pl" | "pm" => "icons/file-types/perl.svg",
        "prisma" => "icons/file-types/prisma.svg",
        "pug" | "jade" => "icons/file-types/pug.svg",
        "scala" | "sbt" | "sc" => "icons/file-types/scala.svg",
        "sol" => "icons/file-types/solidity.svg",
        "tex" | "sty" | "cls" => "icons/file-types/tex.svg",
        "xaml" => "icons/file-types/xaml.svg",
        "zig" => "icons/file-types/zig.svg",
        "nix" => "icons/file-types/nix.svg",
        "proto" => "icons/file-types/proto.svg",
        "diff" | "patch" => "icons/file-types/diff.svg",
        "exe" | "dll" | "so" | "dylib" => "icons/file-types/exe.svg",
        "lock" => "icons/file-types/lock.svg",
        _ => "icons/file-types/file.svg",
    }
}

#[cfg(test)]
fn visible_working_tree_entries(
    root: &Path,
    expanded_paths: &HashSet<PathBuf>,
) -> Vec<WorkingTreeEntry> {
    fn visit(
        directory: &Path,
        relative_directory: &Path,
        depth: usize,
        expanded_paths: &HashSet<PathBuf>,
        entries: &mut Vec<WorkingTreeEntry>,
    ) {
        let Ok(read_dir) = std::fs::read_dir(directory) else {
            return;
        };
        let mut children = read_dir
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    return None;
                }
                let is_dir = entry.file_type().ok()?.is_dir();
                Some((entry.path(), name, is_dir))
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|(_, name, is_dir)| (!*is_dir, name.to_lowercase()));

        for (absolute_path, name, is_dir) in children {
            let relative_path = relative_directory.join(&name);
            let expanded = is_dir && expanded_paths.contains(&absolute_path);
            let file_icon = (!is_dir).then(|| file_icon_for_name(&name));
            entries.push(WorkingTreeEntry {
                relative_path: relative_path.to_string_lossy().into_owned(),
                absolute_path: absolute_path.clone(),
                name,
                is_dir,
                file_icon,
                expanded,
                depth,
            });
            if expanded {
                visit(
                    &absolute_path,
                    &relative_path,
                    depth + 1,
                    expanded_paths,
                    entries,
                );
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, Path::new(""), 0, expanded_paths, &mut entries);
    entries
}

/// The language name for a file, as understood by [`crate::md::highlight`].
/// Names the lexer does not know simply render unhighlighted.
pub(super) fn file_highlighter_language(relative_path: &str) -> &'static str {
    let path = Path::new(relative_path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let normalized_file_name = file_name.to_ascii_lowercase();

    // Lockfiles often have a generic `.lock` suffix (or no useful extension),
    // so resolve their actual serialization format before extension fallback.
    let lockfile_language = match normalized_file_name.as_str() {
        "bun.lock"
        | "composer.lock"
        | "conan.lock"
        | "deno.lock"
        | "flake.lock"
        | "npm-shrinkwrap.json"
        | "package-lock.json"
        | "package.resolved"
        | "packages.lock.json"
        | "pipfile.lock" => Some("json"),
        "cargo.lock" | "pdm.lock" | "poetry.lock" | "uv.lock" => Some("toml"),
        "chart.lock" | "gemfile.lock" | "pnpm-lock.yaml" | "podfile.lock" | "pubspec.lock"
        | "yarn.lock" => Some("yaml"),
        "mix.lock" => Some("elixir"),
        _ => None,
    };
    if let Some(language) = lockfile_language {
        return language;
    }

    if file_name == "Makefile" || file_name.starts_with("Makefile.") {
        return "make";
    }
    if normalized_file_name == "dockerfile" || normalized_file_name.starts_with("dockerfile.") {
        return "dockerfile";
    }

    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("rs") => "rust",
        Some("ts" | "mts" | "cts") => "typescript",
        Some("tsx" | "tsrx") => "tsx",
        Some("js" | "jsx" | "mjs" | "cjs") => "javascript",
        Some("py" | "pyi") => "python",
        Some("go") => "go",
        Some("c") => "c",
        Some("h" | "hpp" | "hh" | "hxx" | "cc" | "cpp" | "cxx") => "cpp",
        Some("m" | "mm") => "objc",
        Some("java" | "kt" | "kts") => "java",
        Some("cs") => "csharp",
        Some("scala" | "sc") => "scala",
        Some("rb" | "rake" | "gemspec") => "ruby",
        Some("ex" | "exs" | "heex" | "eex" | "leex") => "elixir",
        Some("lua") => "lua",
        Some("php" | "php3" | "php4" | "php5" | "phtml") => "php",
        Some("swift") => "swift",
        Some("dart") => "dart",
        Some("zig" | "zon") => "zig",
        Some("json" | "jsonc" | "json5") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("ini" | "cfg" | "conf") => "ini",
        Some("sh" | "bash" | "zsh" | "fish") => "bash",
        Some("css" | "scss" | "sass" | "less") => "css",
        Some("html" | "htm" | "xml" | "svg" | "vue" | "svelte" | "astro") => "html",
        Some("sql") => "sql",
        Some("diff" | "patch") => "diff",
        Some("md" | "markdown" | "mdx") => "markdown",
        _ => "text",
    }
}

pub(super) fn transfer_file_is_previewable(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if image_preview::image_format_for_name(file_name).is_some()
        || file_highlighter_language(file_name) != "text"
    {
        return true;
    }

    let normalized_name = file_name.to_ascii_lowercase();
    if matches!(
        normalized_name.as_str(),
        "readme"
            | "license"
            | "licence"
            | "notice"
            | "authors"
            | "contributors"
            | "copying"
            | "changelog"
            | "changes"
            | "todo"
            | ".gitignore"
            | ".dockerignore"
            | ".gitattributes"
            | ".editorconfig"
            | ".env.example"
    ) {
        return true;
    }

    matches!(
        Path::new(file_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "txt"
                | "text"
                | "log"
                | "csv"
                | "tsv"
                | "rst"
                | "adoc"
                | "properties"
                | "nfo"
                | "out"
                | "err"
                | "jsonl"
        )
    )
}

/// Reads a file for the editor, returning its text and whether it can be saved.
///
/// One unbounded `read_to_string`, so callers keep it off the UI thread; the
/// only caller is [`Waku::read_right_panel_file_into_editor`].
fn read_right_panel_file(
    workspace: &waku_client::WorkspaceClient,
    project_path: &Path,
    relative_path: &str,
) -> (String, bool) {
    match workspace.request(waku_client::WorkspaceOperation::ReadTextFile {
        root: project_path.to_path_buf(),
        relative_path: PathBuf::from(relative_path),
    }) {
        Ok(waku_client::WorkspaceResult::TextFile { content }) => (content, true),
        Ok(_) => (
            tr!(
                "files.unable_to_edit",
                error = "the daemon returned an invalid file response"
            ),
            false,
        ),
        Err(error) => (
            tr!("files.unable_to_edit", error = error.to_string()),
            false,
        ),
    }
}

/// Reads a file's raw bytes for the image preview. Same daemon round-trip
/// as `read_right_panel_file`; the caller keeps it off the UI thread.
fn read_right_panel_binary_file(
    workspace: &waku_client::WorkspaceClient,
    project_path: &Path,
    relative_path: &str,
) -> Result<Vec<u8>, String> {
    match workspace.request(waku_client::WorkspaceOperation::ReadBinaryFile {
        root: project_path.to_path_buf(),
        relative_path: PathBuf::from(relative_path),
    }) {
        Ok(waku_client::WorkspaceResult::File { data }) => Ok(data),
        Ok(_) => Err("the daemon returned an invalid file response".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// One background read's payload: text for the editor, or decoded bytes
/// wrapped as a GPUI image for the preview pane.
enum RightPanelFileRead {
    Text(String, bool),
    Image(Result<Arc<gpui::Image>, String>),
}

/// Whether the pane renders this file as an image rather than text: every
/// extension `image_format_for_name` knows, except an SVG the user has
/// flipped into source editing.
fn file_shows_image(editor: &RightPanelFileEditor, relative_path: &str) -> bool {
    !editor.show_source && image_preview::image_format_for_name(relative_path).is_some()
}

/// Deliverables start in reading mode independently of the source editor preference.
fn markdown_preview_mode(
    is_markdown: bool,
    deliverable_page: bool,
    show_source: bool,
    global_preview: bool,
) -> bool {
    is_markdown
        && if deliverable_page {
            !show_source
        } else {
            global_preview
        }
}

/// Whether this file names a planning-session wireframe document.
fn is_wireframe_document(relative_path: &str) -> bool {
    Path::new(relative_path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".wireframe.json"))
}

/// Whether the pane draws this `.wireframe.json` as themed elements
/// rather than text — the experiment flag and the per-file source toggle
/// both decide.
fn file_shows_wireframe(editor: &RightPanelFileEditor, relative_path: &str, enabled: bool) -> bool {
    enabled && !editor.show_source && is_wireframe_document(relative_path)
}

impl RightPanelSurface {
    fn new_browser() -> Self {
        Self::Browser(Uuid::new_v4())
    }

    pub(super) fn new_terminal() -> Self {
        Self::Terminal(Uuid::new_v4())
    }

    pub(super) fn terminal_id(&self) -> Option<Uuid> {
        match self {
            Self::Terminal(id) => Some(*id),
            _ => None,
        }
    }

    fn browser_id(&self) -> Option<Uuid> {
        match self {
            Self::Browser(id) => Some(*id),
            _ => None,
        }
    }

    /// Analytics surface name — unlike `label` this is stable English, not
    /// the localized tab text.
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Browser(_) => "browser",
            Self::Terminal(_) => "terminal",
            Self::BackgroundWork { .. } => "background_work",
            Self::PullRequest { .. } => "pull_request",
            Self::Files => "files",
            Self::Diff => "diff",
            Self::File(_) | Self::FileAtRef { .. } => "file",
            Self::GitHub(_) => "github",
            Self::SideChat(_) => "side_chat",
            Self::Plan { .. } => "plan",
            Self::Goals => "goals",
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Browser(_) => tr!("right_panel.browser"),
            Self::Terminal(_) => tr!("right_panel.terminal"),
            Self::BackgroundWork { key, title } => {
                if title.is_empty() {
                    match key.kind {
                        BackgroundWorkKind::Process => tr!("background.process"),
                        BackgroundWorkKind::Monitor => tr!("background.monitor"),
                        BackgroundWorkKind::Subagent => tr!("background.subagent"),
                    }
                } else {
                    title.clone()
                }
            }
            Self::Files => tr!("right_panel.files"),
            Self::Diff => tr!("right_panel.diff"),
            Self::PullRequest { number } => format!("#{number}"),
            Self::File(path) => path.rsplit('/').next().unwrap_or(path).to_owned(),
            Self::FileAtRef { path, git_ref } => {
                format!("{} ({git_ref})", path.rsplit('/').next().unwrap_or(path))
            }
            Self::GitHub(_) => tr!("right_panel.github"),
            Self::SideChat(_) => tr!("right_panel.side_chat"),
            Self::Plan { plan_file, .. } => {
                plan_file.rsplit('/').next().unwrap_or(plan_file).to_owned()
            }
            Self::Goals => tr!("right_panel.goals"),
        }
    }

    fn icon_path(&self) -> &'static str {
        match self {
            Self::Browser(_) => "icons/globe.svg",
            Self::Terminal(_) => "icons/terminal.svg",
            Self::BackgroundWork { key, .. } => work_kind_icon(key.kind),
            Self::PullRequest { .. } => "icons/git-pull-request.svg",
            Self::Files => "icons/folder.svg",
            Self::Diff => "icons/file-diff.svg",
            Self::File(path) | Self::FileAtRef { path, .. } => file_icon_for_path(path),
            Self::GitHub(_) => "icons/github.svg",
            Self::SideChat(_) => "icons/chat.svg",
            Self::Plan { plan_file, .. } => file_icon_for_path(plan_file),
            Self::Goals => "icons/target.svg",
        }
    }
}

/// Add the runtime-only Tasks tab to a Boss chat's strip. When it is the
/// first tab, select it; otherwise preserve the active user tab.
fn ensure_boss_tasks_tab(
    surfaces: &mut Vec<RightPanelSurface>,
    active_surface: &mut Option<usize>,
) -> Option<bool> {
    if surfaces.contains(&RightPanelSurface::Goals) {
        return None;
    }
    let first_visit = surfaces.is_empty();
    surfaces.insert(0, RightPanelSurface::Goals);
    *active_surface = if first_visit {
        Some(0)
    } else {
        active_surface.map(|active| active + 1).or(Some(1))
    };
    Some(first_visit)
}

fn right_panel_surface_is_closable(surface: &RightPanelSurface) -> bool {
    !matches!(surface, RightPanelSurface::Plan { .. } | RightPanelSurface::Goals)
}

fn right_panel_tab_label(surface: &RightPanelSurface, files_selected_path: Option<&str>) -> String {
    let label = match surface {
        RightPanelSurface::Files => files_selected_path
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| tr!("right_panel.files")),
        _ => surface.label(),
    };
    single_line_label(&label)
}

fn right_panel_tab_icon(
    surface: &RightPanelSurface,
    files_selected_path: Option<&str>,
) -> &'static str {
    match surface {
        RightPanelSurface::Files => files_selected_path
            .map(file_icon_for_path)
            .unwrap_or_else(|| surface.icon_path()),
        _ => surface.icon_path(),
    }
}

/// What a boss-managed session's strip may host — file previews opened
/// from the transcript, webview tabs, and side chats. Terminals, the file
/// tree, and code review stay out.
fn managed_panel_surface(surface: &RightPanelSurface) -> bool {
    matches!(
        surface,
        RightPanelSurface::Browser(_)
            | RightPanelSurface::File(_)
            | RightPanelSurface::FileAtRef { .. }
            | RightPanelSurface::SideChat(_)
            | RightPanelSurface::Plan { .. }
    )
}

fn session_panel_surface(surface: &RightPanelSurface) -> bool {
    !matches!(surface, RightPanelSurface::Goals)
}

pub(super) fn reusable_surface_index(
    surfaces: &[RightPanelSurface],
    requested: &RightPanelSurface,
) -> Option<usize> {
    match requested {
        RightPanelSurface::Browser(_) | RightPanelSurface::Terminal(_) => None,
        RightPanelSurface::BackgroundWork { key, .. } => surfaces.iter().position(|surface| {
            matches!(surface, RightPanelSurface::BackgroundWork { key: candidate, .. } if candidate == key)
        }),
        RightPanelSurface::GitHub(project_id) => surfaces.iter().position(|surface| {
            matches!(surface, RightPanelSurface::GitHub(candidate) if candidate == project_id)
        }),
        RightPanelSurface::SideChat(session_id) => surfaces.iter().position(|surface| {
            matches!(surface, RightPanelSurface::SideChat(candidate) if candidate == session_id)
        }),
        RightPanelSurface::Goals => surfaces
            .iter()
            .position(|surface| matches!(surface, RightPanelSurface::Goals)),
        RightPanelSurface::Files
        | RightPanelSurface::Diff
        | RightPanelSurface::File(_)
        | RightPanelSurface::FileAtRef { .. }
        | RightPanelSurface::PullRequest { .. }
        | RightPanelSurface::Plan { .. } => {
            surfaces.iter().position(|surface| surface == requested)
        }
    }
}

#[derive(Clone, Copy)]
enum TabScrollFadeSide {
    Left,
    Right,
}

fn tab_scroll_fade_visibility(offset_x: Pixels, max_offset: Pixels) -> (bool, bool) {
    let scrolled = -offset_x;
    let threshold = px(0.5);
    (scrolled > threshold, max_offset - scrolled > threshold)
}

fn fade_safe_tab_offset(
    current_offset: Pixels,
    max_offset: Pixels,
    item_left: Pixels,
    item_right: Pixels,
    viewport_left: Pixels,
    viewport_right: Pixels,
) -> Pixels {
    let inset = px(TAB_SCROLL_FADE_WIDTH);
    let mut offset = current_offset;
    let visible_left = item_left + offset;
    let visible_right = item_right + offset;
    if visible_left < viewport_left + inset {
        offset += viewport_left + inset - visible_left;
    } else if visible_right > viewport_right - inset {
        offset -= visible_right - (viewport_right - inset);
    }
    offset.clamp(-max_offset, px(0.0))
}

fn tab_scroll_reveal_guard(
    scroll_handle: ScrollHandle,
    tab_index: usize,
    waku: WeakEntity<Waku>,
) -> impl IntoElement {
    canvas(
        move |_, window, _| {
            if let Some(item) = scroll_handle.bounds_for_item(tab_index) {
                let viewport = scroll_handle.bounds();
                let offset = scroll_handle.offset();
                let safe_offset = fade_safe_tab_offset(
                    offset.x,
                    scroll_handle.max_offset().x,
                    item.left(),
                    item.right(),
                    viewport.left(),
                    viewport.right(),
                );
                if safe_offset != offset.x {
                    scroll_handle.set_offset(point(safe_offset, offset.y));
                }
            }

            window.on_next_frame(move |_, cx| {
                let _ = waku.update(cx, |this, cx| {
                    if this.right_panel_pending_tab_reveal == Some(tab_index) {
                        this.right_panel_pending_tab_reveal = None;
                        cx.notify();
                    }
                });
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

fn tab_scroll_fade(
    scroll_handle: ScrollHandle,
    side: TabScrollFadeSide,
    surface: Hsla,
) -> impl IntoElement {
    canvas(
        move |bounds, _, _| {
            let (show_left, show_right) =
                tab_scroll_fade_visibility(scroll_handle.offset().x, scroll_handle.max_offset().x);
            let visible = match side {
                TabScrollFadeSide::Left => show_left,
                TabScrollFadeSide::Right => show_right,
            };
            visible.then(|| {
                let transparent = surface.opacity(0.0);
                let background = match side {
                    TabScrollFadeSide::Left => linear_gradient(
                        90.0,
                        linear_color_stop(surface, 0.0),
                        linear_color_stop(transparent, 1.0),
                    ),
                    TabScrollFadeSide::Right => linear_gradient(
                        90.0,
                        linear_color_stop(transparent, 0.0),
                        linear_color_stop(surface, 1.0),
                    ),
                };
                fill(bounds, background)
            })
        },
        |_, fade, window, _| {
            if let Some(fade) = fade {
                window.paint_quad(fade);
            }
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .when(matches!(side, TabScrollFadeSide::Left), |element| {
        element.left_0()
    })
    .when(matches!(side, TabScrollFadeSide::Right), |element| {
        element.right_0()
    })
    .w(px(TAB_SCROLL_FADE_WIDTH))
}

/// The Goals panel's compact row geometry: uniform two-line rows and a
/// finished-history viewport bounded to five complete rows while ongoing
/// work exists. Show more expands history into the panel's flexible share.
const GOALS_PANEL_ROW_HEIGHT: f32 = 50.0;
const GOALS_PANEL_RECENT_LIMIT: usize = 5;
const GOALS_PANEL_AVATAR: f32 = 16.0;
const GOALS_PANEL_SECTION_GAP: f32 = 12.0;

fn boss_goal_row_height(font_size: f32) -> f32 {
    // Whole-pixel heights keep fractional measurement from accumulating
    // at the preview's cutoff.
    (GOALS_PANEL_ROW_HEIGHT * waku_client::persistence::sanitized_ui_font_size(font_size)
        / waku_client::persistence::DEFAULT_UI_FONT_SIZE)
        .ceil()
        .max(GOALS_PANEL_ROW_HEIGHT)
}

/// The coarse execution bucket that decides which Goals section a row
/// renders under — the specific [`BossGoalStatus`] stays on the row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BossGoalBucket {
    Finished,
    Running,
    Pending,
}

/// The specific status a goal row reports. Glyphs and tones mirror the
/// sidebar's session status language; finished goals swap the
/// unread/completion indicator for a checkmark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BossGoalStatus {
    Queued,
    Starting,
    Working,
    BackgroundWork,
    NeedsInput,
    Paused,
    UsageLimited,
    Blocked,
    Attention,
    Finishing,
    Active,
    Unavailable,
    Failed,
    BudgetReached,
    Complete,
    Finished,
    /// A recorded wait — a dependency note or a defer-until the boss set.
    Waiting,
    /// A handoff still owes the boss a decision.
    FollowUp,
    /// Open with no assignments yet.
    NotStarted,
    /// Open, its assignments all settled, and nothing else outstanding —
    /// the boss simply has not moved it yet.
    Open,
    /// The outcome closed without success — never read as completed.
    Cancelled,
    /// A replaced admission or a history row nothing can describe.
    Superseded,
    UnavailableHistory,
}

impl BossGoalStatus {
    fn label(self) -> String {
        match self {
            Self::Queued => tr!("boss.goals_status_queued"),
            Self::Starting => tr!("boss.goals_status_starting"),
            Self::Working => tr!("boss.goals_status_working"),
            Self::BackgroundWork => tr!("boss.goals_status_background"),
            Self::NeedsInput => tr!("boss.goals_status_needs_input"),
            Self::Paused => tr!("boss.goals_status_paused"),
            Self::UsageLimited => tr!("boss.goals_status_usage_limited"),
            Self::Blocked => tr!("boss.goals_status_blocked"),
            Self::Attention => tr!("boss.goals_status_attention"),
            Self::Finishing => tr!("boss.goals_status_finishing"),
            Self::Active => tr!("boss.goals_status_active"),
            Self::Unavailable => tr!("boss.goals_status_unavailable"),
            Self::Failed => tr!("boss.goals_status_failed"),
            Self::BudgetReached => tr!("boss.goals_status_budget_limited"),
            Self::Complete => tr!("boss.goals_status_complete"),
            Self::Finished => tr!("boss.goals_status_finished"),
            Self::Waiting => tr!("boss.goals_status_waiting"),
            Self::FollowUp => tr!("boss.goals_status_follow_up"),
            Self::NotStarted => tr!("boss.goals_status_not_started"),
            Self::Open => tr!("boss.goals_status_open"),
            Self::Cancelled => tr!("boss.goals_status_cancelled"),
            Self::Superseded => tr!("boss.goals_status_superseded"),
            Self::UnavailableHistory => tr!("boss.goals_history_unavailable"),
        }
    }

    /// Icon, tone, and whether it spins — the sidebar task rows' status
    /// markers, plus the panel's queued/paused/finished additions.
    fn marker(self) -> (&'static str, BossGoalTone, bool) {
        match self {
            Self::Starting | Self::Working => {
                ("icons/loader-circle.svg", BossGoalTone::Accent, true)
            }
            Self::Finishing => ("icons/loader-circle.svg", BossGoalTone::Secondary, true),
            Self::Queued | Self::Active | Self::BackgroundWork | Self::Waiting => {
                ("icons/hourglass.svg", BossGoalTone::Secondary, false)
            }
            Self::Unavailable | Self::UnavailableHistory => {
                ("icons/hourglass.svg", BossGoalTone::Tertiary, false)
            }
            Self::NeedsInput
            | Self::UsageLimited
            | Self::Blocked
            | Self::Attention
            | Self::FollowUp
            | Self::BudgetReached => ("icons/alert.svg", BossGoalTone::Warning, false),
            Self::Paused => ("icons/pause.svg", BossGoalTone::Secondary, false),
            Self::Failed => ("icons/x-bold.svg", BossGoalTone::Danger, false),
            Self::Cancelled => ("icons/ban.svg", BossGoalTone::Secondary, false),
            Self::Superseded => ("icons/rotate-cw.svg", BossGoalTone::Tertiary, false),
            Self::NotStarted | Self::Open => {
                ("icons/circle-dot.svg", BossGoalTone::Tertiary, false)
            }
            Self::Complete => ("icons/check.svg", BossGoalTone::Success, false),
            Self::Finished => ("icons/check.svg", BossGoalTone::Secondary, false),
        }
    }

    /// Whether the status counts toward a section's "needs attention" tally.
    fn attention(self) -> bool {
        matches!(
            self,
            Self::Failed
                | Self::Blocked
                | Self::Attention
                | Self::NeedsInput
                | Self::Paused
                | Self::UsageLimited
                | Self::BudgetReached
                | Self::FollowUp
        )
    }
}

#[derive(Clone, Copy)]
enum BossGoalTone {
    Accent,
    Secondary,
    Tertiary,
    Warning,
    Danger,
    Success,
}

impl BossGoalTone {
    fn color(self, theme: &Theme) -> Hsla {
        match self {
            Self::Accent => theme.accent,
            Self::Secondary => theme.text_secondary,
            Self::Tertiary => theme.text_tertiary,
            Self::Warning => theme.warning,
            Self::Danger => theme.danger,
            Self::Success => theme.success,
        }
    }
}

/// One member's lifecycle plus cached session evidence → its execution
/// bucket and status, in the design's precedence order: queue admission,
/// explicit terminal results, then live session signals. The same
/// derivation backs the outcome row's running status and each expanded
/// in-flight entry.
fn boss_member_status(
    member: &waku_protocol::boss::BossEmployee,
    session: Option<&AgentSession>,
) -> (BossGoalBucket, BossGoalStatus) {
    use waku_protocol::boss::EmployeeLifecycle;
    let goal_status = session
        .and_then(|session| session.thread_goal.as_ref())
        .map(|goal| goal.status);
    if member.cancelled {
        return (BossGoalBucket::Finished, BossGoalStatus::Cancelled);
    }
    match member.lifecycle() {
        EmployeeLifecycle::Queued => (BossGoalBucket::Pending, BossGoalStatus::Queued),
        EmployeeLifecycle::Dispatching => (BossGoalBucket::Running, BossGoalStatus::Starting),
        EmployeeLifecycle::Expired => {
            let status = if session.is_some_and(|session| session.status == SessionStatus::Failed) {
                BossGoalStatus::Failed
            } else if member.blocker.is_some() {
                BossGoalStatus::Attention
            } else if goal_status == Some(crate::model::ThreadGoalStatus::Complete) {
                BossGoalStatus::Complete
            } else if goal_status == Some(crate::model::ThreadGoalStatus::BudgetLimited) {
                BossGoalStatus::BudgetReached
            } else {
                BossGoalStatus::Finished
            };
            (BossGoalBucket::Finished, status)
        }
        EmployeeLifecycle::Working | EmployeeLifecycle::Finishing => {
            if session.is_some_and(|session| session.status == SessionStatus::Failed) {
                return (BossGoalBucket::Running, BossGoalStatus::Failed);
            }
            match goal_status {
                Some(crate::model::ThreadGoalStatus::Complete) => {
                    return (BossGoalBucket::Finished, BossGoalStatus::Complete);
                }
                Some(crate::model::ThreadGoalStatus::BudgetLimited) => {
                    return (BossGoalBucket::Finished, BossGoalStatus::BudgetReached);
                }
                _ => {}
            }
            if member.lifecycle() == EmployeeLifecycle::Finishing {
                return (BossGoalBucket::Running, BossGoalStatus::Finishing);
            }
            if member.blocker.is_some() {
                return (BossGoalBucket::Running, BossGoalStatus::Attention);
            }
            let status = match goal_status {
                Some(crate::model::ThreadGoalStatus::Blocked) => BossGoalStatus::Blocked,
                Some(crate::model::ThreadGoalStatus::Paused) => BossGoalStatus::Paused,
                Some(crate::model::ThreadGoalStatus::UsageLimited) => BossGoalStatus::UsageLimited,
                _ => match session.map(|session| session.status) {
                    Some(SessionStatus::Waiting) => BossGoalStatus::NeedsInput,
                    Some(SessionStatus::Connecting) => BossGoalStatus::Starting,
                    Some(SessionStatus::Working) => BossGoalStatus::Working,
                    Some(SessionStatus::Background) => BossGoalStatus::BackgroundWork,
                    // An unfinished pursuit with no live turn — including an
                    // admitted employee idle between prompts — reads Active.
                    Some(_) => BossGoalStatus::Active,
                    None => BossGoalStatus::Unavailable,
                },
            };
            (BossGoalBucket::Running, status)
        }
    }
}

/// The member an outcome's collapsed row speaks for — the live assignee
/// with attention first, then the newest live admission. `None` when no
/// member holds running or starting work.
fn boss_outcome_live_member(
    row: &boss::BossOutcomeRow,
) -> Option<&boss::BossOutcomeMember> {
    use waku_protocol::boss::EmployeeLifecycle;
    let live = |member: &&boss::BossOutcomeMember| {
        !member.employee.cancelled
            && matches!(
                member.employee.lifecycle(),
                EmployeeLifecycle::Dispatching
                    | EmployeeLifecycle::Working
                    | EmployeeLifecycle::Finishing
            )
    };
    row.members
        .iter()
        .filter(live)
        .find(|member| member.employee.blocker.is_some())
        .or_else(|| row.members.iter().filter(live).next_back())
}

/// A member still waiting on admission — queued and not cancelled.
fn boss_outcome_queued(member: &boss::BossOutcomeMember) -> bool {
    !member.employee.cancelled
        && member.employee.lifecycle() == waku_protocol::boss::EmployeeLifecycle::Queued
}

/// A defer-until's local timestamp — "Oct 9, 14:32" — for the recorded
/// wait's readable reason.
fn boss_wait_time_label(at: u64) -> String {
    chrono::DateTime::from_timestamp(at as i64, 0)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%b %-d, %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| at.to_string())
}

/// The recorded pause's readable reason — a tracked wait first, then a
/// snooze's own expiry, which is the outcome's other deliberate pause.
/// `None` once a defer-until or snooze has elapsed: an expired timestamp
/// no longer explains the pause.
fn boss_outcome_wait_label(
    outcome: &waku_protocol::boss::BossOutcome,
    now: u64,
) -> Option<String> {
    let until = |at: u64| tr!("boss.goals_wait_until", time = boss_wait_time_label(at));
    match &outcome.waiting {
        Some(waku_protocol::boss::OutcomeWait::Dependency { note }) => {
            Some(tr!("boss.goals_wait_note", note = note.clone()))
        }
        Some(waku_protocol::boss::OutcomeWait::Until { at }) if *at > now => Some(until(*at)),
        _ => outcome.snoozed_until.filter(|at| *at > now).map(until),
    }
}

/// The finer-grained settle cause an attempt's verdict already implies is
/// left out — a failed or cancelled verdict reads the same with or
/// without its matching cause. Interruption and leftover causes name
/// something the verdict alone cannot: the provider died, a restart cut
/// the turn off, or work was left behind.
fn boss_expiry_cause_label(cause: waku_protocol::boss::ExpiryCause) -> Option<String> {
    use waku_protocol::boss::ExpiryCause;
    match cause {
        ExpiryCause::Finished | ExpiryCause::Failed | ExpiryCause::Stopped => None,
        ExpiryCause::ExitedMidTurn => Some(tr!("boss.goals_cause_exited_mid_turn")),
        ExpiryCause::ExitedIdle => Some(tr!("boss.goals_cause_exited_idle")),
        ExpiryCause::ParkedWork => Some(tr!("boss.goals_cause_parked_work")),
        ExpiryCause::UnansweredAsk => Some(tr!("boss.goals_cause_unanswered_ask")),
        ExpiryCause::Restarted => Some(tr!("boss.goals_cause_restarted")),
    }
}

/// The outcome's section and headline status — the plan's section rules.
/// Terminal state owns Finished outright; a live member owns In progress
/// even when queued work or a wait stands beside it; everything else
/// lands in Pending with its honest label — needs attention with its
/// cause, needs follow-up for an owed decision, queued, waiting with its
/// recorded reason, not started, or simply open.
fn boss_outcome_status(
    row: &boss::BossOutcomeRow,
    sessions: &HashMap<Uuid, &AgentSession>,
    now: u64,
) -> (BossGoalBucket, BossGoalStatus) {
    match row.outcome.state {
        waku_protocol::boss::OutcomeState::Completed => {
            return (BossGoalBucket::Finished, BossGoalStatus::Complete);
        }
        waku_protocol::boss::OutcomeState::Cancelled => {
            return (BossGoalBucket::Finished, BossGoalStatus::Cancelled);
        }
        waku_protocol::boss::OutcomeState::Open => {}
    }
    if let Some(member) = boss_outcome_live_member(row) {
        let (member_bucket, status) = boss_member_status(
            &member.employee,
            sessions.get(&member.employee.session_id).copied(),
        );
        // A live member whose thread goal already reads complete is still
        // in flight — its settle has not landed on the outcome, so the
        // row reads Finishing rather than Completed.
        let status = if member_bucket == BossGoalBucket::Finished
            && matches!(status, BossGoalStatus::Complete | BossGoalStatus::Finished)
        {
            BossGoalStatus::Finishing
        } else {
            status
        };
        return (BossGoalBucket::Running, status);
    }
    // Nothing is running — the Pending precedence orders the most
    // actionable signal first.
    if row.attention {
        return (BossGoalBucket::Pending, BossGoalStatus::Blocked);
    }
    if row.outcome.pending_handoffs().next().is_some() {
        return (BossGoalBucket::Pending, BossGoalStatus::FollowUp);
    }
    if row.members.iter().any(boss_outcome_queued) {
        return (BossGoalBucket::Pending, BossGoalStatus::Queued);
    }
    if boss_outcome_wait_label(&row.outcome, now).is_some() {
        return (BossGoalBucket::Pending, BossGoalStatus::Waiting);
    }
    if row.outcome.assignments.is_empty() {
        return (BossGoalBucket::Pending, BossGoalStatus::NotStarted);
    }
    (BossGoalBucket::Pending, BossGoalStatus::Open)
}

/// One expanded assignment entry's status — the durable settle verdict
/// when the attempt resolved, else the live roster and session evidence.
/// An unsettled row no roster covers can only read as unavailable: it is
/// either still in flight somewhere the snapshot cannot see, or its
/// settle was never recorded — inventing a result would be worse.
fn boss_outcome_entry_status(
    row: &boss::BossOutcomeRow,
    assignment: &waku_protocol::boss::OutcomeAssignment,
    sessions: &HashMap<Uuid, &AgentSession>,
) -> BossGoalStatus {
    use waku_protocol::boss::AssignmentVerdict;
    if let Some(settle) = &assignment.settled {
        return match settle.verdict {
            AssignmentVerdict::Finished => BossGoalStatus::Finished,
            AssignmentVerdict::Failed => BossGoalStatus::Failed,
            AssignmentVerdict::Cancelled => BossGoalStatus::Cancelled,
            AssignmentVerdict::Superseded => BossGoalStatus::Superseded,
            AssignmentVerdict::Unavailable => BossGoalStatus::UnavailableHistory,
        };
    }
    let member = row
        .members
        .iter()
        .find(|member| member.employee.session_id == assignment.session);
    let Some(member) = member else {
        return BossGoalStatus::UnavailableHistory;
    };
    boss_member_status(
        &member.employee,
        sessions.get(&member.employee.session_id).copied(),
    )
    .1
}

/// Whether a recorded prerequisite still blocks an in-flight attempt:
/// its sibling row is unsettled or settled without success. A
/// prerequisite that failed or was cancelled shows the dependency as
/// blocked rather than ordinary waiting.
fn boss_outcome_prerequisite_failed(
    row: &boss::BossOutcomeRow,
    assignment: &waku_protocol::boss::OutcomeAssignment,
) -> bool {
    assignment.prerequisites.iter().any(|prerequisite| {
        row.outcome
            .assignments
            .iter()
            .filter(|sibling| sibling.session == *prerequisite)
            .next_back()
            .is_some_and(|sibling| {
                sibling.settled.as_ref().is_some_and(|settle| {
                    !matches!(
                        settle.verdict,
                        waku_protocol::boss::AssignmentVerdict::Finished
                    )
                })
            })
    })
}

/// The cause a Needs-attention row names — the unresolved member's own
/// blocker text or reporting settle cause first, then the newest settled
/// failure or blocked attempt no roster record still answers. Mirrors
/// `boss_outcome_attention`'s evidence; `None` when nothing more specific
/// than the status itself is recorded.
fn boss_outcome_attention_cause(row: &boss::BossOutcomeRow) -> Option<String> {
    for member in &row.members {
        let employee = &member.employee;
        if !waku_protocol::boss::BossOutcome::assignment_unresolved(employee) {
            continue;
        }
        if let Some(blocker) = employee
            .blocker
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(blocker.to_owned());
        }
        if let Some(cause) = employee
            .expiry
            .as_ref()
            .and_then(|expiry| boss_expiry_cause_label(expiry.cause))
        {
            return Some(cause);
        }
    }
    let mut latest: HashMap<Uuid, &waku_protocol::boss::OutcomeAssignment> = HashMap::new();
    for assignment in &row.outcome.assignments {
        latest.insert(assignment.session, assignment);
    }
    latest
        .values()
        .filter(|assignment| {
            assignment.settled.as_ref().is_some_and(|settle| {
                matches!(
                    settle.verdict,
                    waku_protocol::boss::AssignmentVerdict::Failed
                ) || settle.blocked
            }) && !row
                .members
                .iter()
                .any(|member| member.employee.session_id == assignment.session)
        })
        .max_by_key(|assignment| assignment.settled.as_ref().and_then(|settle| settle.at))
        .and_then(|assignment| {
            let settle = assignment.settled.as_ref()?;
            settle
                .cause
                .and_then(boss_expiry_cause_label)
                .or_else(|| settle.blocked.then(|| tr!("boss.goals_entry_blocked")))
        })
}

/// An outcome row resolved against the cached session snapshot —
/// everything the list item builder paints, prepared once per panel
/// refresh so the virtualized builder touches only prepared data.
struct BossOutcomePanelRow {
    outcome: Uuid,
    /// The row's stable focus handle, held by `boss_ui.goals_focus_handles`
    /// — a section move remounts the row between the two virtualized
    /// lists, and the shared handle keeps it the same focused element.
    focus: FocusHandle,
    height: f32,
    bucket: BossGoalBucket,
    status: BossGoalStatus,
    /// Queued members waiting behind the running work — the In progress
    /// row's "with any queued work also indicated" detail.
    queued_extra: usize,
    title: String,
    /// The member the detail line speaks for — the running or queued
    /// assignee when one exists, else the latest recorded attempt.
    member_name: Option<String>,
    avatar: Option<Arc<gpui::RenderImage>>,
    project_label: Option<String>,
    /// The state detail a member-less row shows instead — the recorded
    /// wait, an owed decision, or the unresolved cause.
    detail: Option<String>,
    worktree: bool,
    updated_label: Option<String>,
    attention: bool,
    expanded: bool,
    aria: String,
    updated_sort: u64,
    created_sort: u64,
}

/// One line inside an expanded outcome — a chronological assignment
/// attempt, an owed handoff, or a recorded wait/conflict note. Entries
/// that name a conversation navigate to it; notes stay put.
struct BossOutcomeEntry {
    /// The signature key distinguishing this entry across refreshes.
    key: String,
    /// The entry's stable focus handle when it is a navigable
    /// destination — kept by `boss_ui.goals_focus_handles` across its
    /// outcome's section moves.
    focus: Option<FocusHandle>,
    /// The conversation the entry opens — assignment sessions and
    /// handoff results navigate when their session is known.
    session: Option<Uuid>,
    icon: &'static str,
    tone: BossGoalTone,
    spin: bool,
    title: String,
    detail: String,
    aria: String,
    destination: bool,
}

enum BossOutcomeItem {
    Header {
        section: boss::BossGoalSection,
        label: String,
        attention: usize,
        collapsed: bool,
        top_gap: bool,
        /// The header's stable focus handle, held by
        /// `boss_ui.goals_focus_handles` — a reset that scrolls it out of
        /// the painted range keeps it rendering instead of dropping focus.
        focus: FocusHandle,
    },
    Row(Arc<BossOutcomePanelRow>),
    /// A row nested under its expanded outcome — the item's own height
    /// keeps the list's uniform geometry.
    Entry(Arc<BossOutcomeEntry>),
}

/// Replace a Goals list's items like `reset_with_uniform_height`, also
/// seeding each item's focus handle into the list's item records — a
/// focused row, entry, or header scrolled out of the painted range keeps
/// rendering, so a remount into the sibling list never drops focus.
fn reset_boss_goals_list(list_state: &ListState, items: &[BossOutcomeItem], row_height: f32) {
    list_state.reset(items.len());
    list_state.splice_focusable(
        0..items.len(),
        items.iter().map(|item| match item {
            BossOutcomeItem::Row(row) => Some(row.focus.clone()),
            BossOutcomeItem::Entry(entry) => entry.focus.clone(),
            BossOutcomeItem::Header { focus, .. } => Some(focus.clone()),
        }),
    );
    list_state.set_size_hints(
        0..items.len(),
        std::iter::repeat_n(Some(px(row_height)), items.len()),
    );
}

/// The `boss_ui.goals_focus_handles` key a focusable panel item owns —
/// each item's stable id, so a remount re-registers the same element.
fn boss_goals_item_focus_key(item: &BossOutcomeItem) -> Option<String> {
    match item {
        BossOutcomeItem::Header { section, .. } => Some(format!("header:{section:?}")),
        BossOutcomeItem::Row(row) => Some(format!("row:{}", row.outcome)),
        BossOutcomeItem::Entry(entry) => {
            entry.destination.then(|| format!("entry:{}", entry.key))
        }
    }
}

/// One disclosure header — quiet label, its trailing chevron, and the
/// "needs attention" tally folded sections carry in place of their hidden
/// rows.
#[track_caller]
fn boss_goal_section_header(
    section: boss::BossGoalSection,
    label: &str,
    attention: usize,
    collapsed: bool,
    top_gap: bool,
    focus: &FocusHandle,
    key: waku_client::DaemonKey,
    waku: &WeakEntity<Waku>,
    theme: &Theme,
) -> Stateful<Div> {
    let toggle_waku = waku.clone();
    let arrow_waku = waku.clone();
    div()
        .id(SharedString::from(format!(
            "boss-goal-header-{key:?}-{section:?}"
        )))
        .track_focus(focus)
        .when(top_gap, |element| element.mt(px(GOALS_PANEL_SECTION_GAP)))
        .h(px(28.0))
        .w_full()
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded(px(6.0))
        .cursor_default()
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .hover(|style| style.bg(theme.overlay))
        .active(|style| style.bg(theme.overlay_strong))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(sp(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_secondary)
                .child(label.to_owned()),
        )
        .child(icon(
            if collapsed {
                "icons/chevron-right.svg"
            } else {
                "icons/chevron-down.svg"
            },
            11.0,
            theme.text_tertiary,
        ))
        .child(div().flex_1())
        .when(
            attention > 0 && collapsed && section != boss::BossGoalSection::Finished,
            |element| {
                element.child(
                    div()
                        .flex_none()
                        .text_size(sp(11.0))
                        .text_color(theme.warning)
                        .child(tr!("boss.goals_needs_attention", count = attention)),
                )
            },
        )
        .on_activation_app(move |_, cx| {
            let _ = toggle_waku.update(cx, |this, cx| {
                let id = (key, section);
                if !this.boss_ui.goals_collapsed.remove(&id) {
                    this.boss_ui.goals_collapsed.insert(id);
                }
                cx.notify();
            });
        })
        .on_key_down(move |event, _, cx| {
            let collapse = match event.keystroke.key.as_str() {
                "left" => true,
                "right" => false,
                _ => return,
            };
            let _ = arrow_waku.update(cx, |this, cx| {
                let id = (key, section);
                if collapse {
                    this.boss_ui.goals_collapsed.insert(id);
                } else {
                    this.boss_ui.goals_collapsed.remove(&id);
                }
                cx.notify();
            });
            cx.stop_propagation();
        })
}

/// The compact two-line outcome row: status icon and outcome title with
/// a disclosure chevron, then an indented detail line — project folder
/// and name, a "·" separator, the current member's avatar and name, and
/// the worktree fork hugging the trailing relative update time. A row
/// with no members shows its recorded wait or follow-up detail instead.
/// Activation toggles the assignment detail; the expanded entries carry
/// the conversation navigation.
/// Activation defers to `on_activation_app` so the list item builder never
/// re-leases Waku.
#[track_caller]
fn boss_goal_panel_row_element(
    row: &Arc<BossOutcomePanelRow>,
    key: waku_client::DaemonKey,
    waku: &WeakEntity<Waku>,
    cx: &App,
) -> Stateful<Div> {
    let theme = Theme::current(cx);
    let (icon_path, tone, spin) = row.status.marker();
    let status_icon = if spin {
        motion::spin_slow(icon(icon_path, 12.0, tone.color(&theme)))
    } else {
        icon(icon_path, 12.0, tone.color(&theme)).into_any_element()
    };
    let outcome = row.outcome;
    let activate_waku = waku.clone();
    let arrow_waku = waku.clone();
    div()
        .id(SharedString::from(format!("boss-goal-{outcome}")))
        .track_focus(&row.focus)
        .w_full()
        .h(px(row.height))
        .flex_none()
        .px(px(8.0))
        .py(px(7.0))
        .rounded(px(8.0))
        .flex()
        .flex_col()
        .gap(px(3.0))
        .aria_label(row.aria.clone())
        .cursor_default()
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .hover(|style| style.bg(theme.overlay))
        .active(|style| style.bg(theme.overlay_strong))
        .on_activation_app(move |_, cx| {
            let _ = activate_waku.update(cx, |this, cx| {
                let id = (key, outcome);
                if !this.boss_ui.goals_row_expanded.remove(&id) {
                    this.boss_ui.goals_row_expanded.insert(id);
                }
                cx.notify();
            });
        })
        .on_key_down(move |event, _, cx| {
            let collapse = match event.keystroke.key.as_str() {
                "left" => true,
                "right" => false,
                _ => return,
            };
            let _ = arrow_waku.update(cx, |this, cx| {
                let id = (key, outcome);
                if collapse {
                    this.boss_ui.goals_row_expanded.remove(&id);
                } else {
                    this.boss_ui.goals_row_expanded.insert(id);
                }
                cx.notify();
            });
            cx.stop_propagation();
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .flex_none()
                        .size(px(12.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(status_icon),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(12.5))
                        .line_height(sp(16.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(row.title.clone()),
                )
                .child(icon(
                    if row.expanded {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    },
                    11.0,
                    theme.text_tertiary,
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                // Indent past the status icon and its gap so the detail line
                // opens under the title, leaving the marker column clear.
                .pl(px(18.0))
                .when_some(row.project_label.clone(), |element, project| {
                    element
                        .child(icon("icons/folder.svg", 11.0, theme.text_tertiary))
                        .child(
                            div()
                                .min_w_0()
                                .max_w(px(160.0))
                                .truncate()
                                .text_size(sp(11.0))
                                .line_height(sp(15.0))
                                .text_color(theme.text_tertiary)
                                .child(project),
                        )
                })
                .when_some(
                    row.member_name.clone().map(|name| {
                        let avatar = row.avatar.clone().map_or_else(
                            || {
                                div()
                                    .size(px(GOALS_PANEL_AVATAR))
                                    .rounded(px(6.0))
                                    .bg(theme.overlay)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(sp(10.0))
                                    .text_color(theme.text_secondary)
                                    .child(name.chars().next().unwrap_or('B').to_string())
                                    .into_any_element()
                            },
                            |image| {
                                img(image)
                                    .size(px(GOALS_PANEL_AVATAR))
                                    .rounded(px(6.0))
                                    .into_any_element()
                            },
                        );
                        (name, avatar)
                    }),
                    |element, (name, avatar)| {
                        element
                            .when(row.project_label.is_some(), |element| {
                                element.child(
                                    div()
                                        .flex_none()
                                        .text_size(sp(11.0))
                                        .line_height(sp(15.0))
                                        .text_color(theme.text_tertiary)
                                        .child("·"),
                                )
                            })
                            .child(div().flex_none().child(avatar))
                            .child(
                                div()
                                    .min_w_0()
                                    .max_w(px(112.0))
                                    .truncate()
                                    .text_size(sp(11.5))
                                    .line_height(sp(15.0))
                                    .text_color(theme.text_secondary)
                                    .child(name),
                            )
                    },
                )
                .when(row.queued_extra > 0, |element| {
                    element.child(
                        div()
                            .flex_none()
                            .text_size(sp(11.0))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .child(tr!(
                                "boss.goals_more_queued",
                                count = row.queued_extra
                            )),
                    )
                })
                .when_some(row.detail.clone(), |element, detail| {
                    element.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(11.0))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .child(detail),
                    )
                })
                .child(div().flex_1())
                .when(row.worktree, |element| {
                    element.child(icon("icons/fork.svg", 11.0, theme.text_tertiary))
                })
                .when_some(row.updated_label.clone(), |element, label| {
                    element.child(
                        div()
                            .flex_none()
                            .text_size(sp(11.0))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .child(label),
                    )
                }),
        )
}

/// One line inside an expanded outcome — an indented status marker, the
/// attempt's assignee and job or the note's label, then its detail and
/// relative time. Entries that name a known conversation activate into
/// it; notes render inert.
/// Activation defers to `on_activation_app` so the list item builder never
/// re-leases Waku.
#[track_caller]
fn boss_goal_entry_element(
    entry: &Arc<BossOutcomeEntry>,
    waku: &WeakEntity<Waku>,
    cx: &App,
) -> Stateful<Div> {
    let theme = Theme::current(cx);
    let marker = if entry.spin {
        motion::spin_slow(icon(entry.icon, 11.0, entry.tone.color(&theme)))
    } else {
        icon(entry.icon, 11.0, entry.tone.color(&theme)).into_any_element()
    };
    let session = entry.session;
    let activate_waku = waku.clone();
    div()
        .id(SharedString::from(format!(
            "boss-goal-entry-{}",
            entry.key
        )))
        .when_some(entry.focus.clone(), |element, focus| {
            element.track_focus(&focus)
        })
        .w_full()
        .h_full()
        .flex_none()
        .pl(px(26.0))
        .pr(px(8.0))
        .py(px(7.0))
        .rounded(px(8.0))
        .flex()
        .flex_col()
        .gap(px(3.0))
        .aria_label(entry.aria.clone())
        .when(entry.destination, |element| {
            element
                .cursor_default()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .hover(|style| style.bg(theme.overlay))
                .active(|style| style.bg(theme.overlay_strong))
                .on_activation_app(move |_, cx| {
                    let Some(session_id) = session else {
                        return;
                    };
                    let _ = activate_waku.update(cx, |this, cx| {
                        this.request_session_activation(
                            session_id,
                            SessionActivationTransition::Visit,
                            cx,
                        );
                    });
                })
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .flex_none()
                        .size(px(11.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(marker),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(11.5))
                        .line_height(sp(15.0))
                        .text_color(theme.text_secondary)
                        .child(entry.title.clone()),
                ),
        )
        .when(!entry.detail.is_empty(), |element| {
            element.child(
                div()
                    .pl(px(17.0))
                    .truncate()
                    .text_size(sp(11.0))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .child(entry.detail.clone()),
            )
        })
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_deliverables_default_to_reading_without_changing_file_preferences() {
        assert!(markdown_preview_mode(true, true, false, false));
        assert!(!markdown_preview_mode(true, true, true, true));
        assert!(!markdown_preview_mode(true, false, false, false));
        assert!(markdown_preview_mode(true, false, true, true));
        assert!(!markdown_preview_mode(false, true, false, true));
    }

    struct FinishedGoalsLayoutHarness {
        ongoing: bool,
    }

    impl Render for FinishedGoalsLayoutHarness {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row_height = boss_goal_row_height(f32::from(window.rem_size()));
            let rows = ListState::new(5, gpui::ListAlignment::Top, px(row_height))
                .with_uniform_item_height(px(row_height));
            div()
                .w(px(400.0))
                .h(px(700.0))
                .flex()
                .flex_col()
                .child(div().h(px(28.0)).flex_none())
                .child(
                    div()
                        .when(self.ongoing, |element| {
                            element.flex_none().h(px(5.0 * row_height))
                        })
                        .when(!self.ongoing, |element| {
                            element.flex_1().min_h_0().max_h(px(5.0 * row_height))
                        })
                        .debug_selector(|| "finished-viewport".into())
                        .child(
                            list(rows, move |index, _, cx| {
                                let row = Arc::new(BossOutcomePanelRow {
                                    outcome: Uuid::from_u128(index as u128 + 1),
                                    focus: cx.focus_handle().tab_stop(true),
                                    height: row_height,
                                    bucket: BossGoalBucket::Finished,
                                    status: BossGoalStatus::Attention,
                                    queued_extra: 0,
                                    title: "A completed goal with a long task title".into(),
                                    member_name: Some("Dinah".into()),
                                    avatar: None,
                                    project_label: Some("Goddard".into()),
                                    detail: None,
                                    worktree: true,
                                    updated_label: Some("23h".into()),
                                    attention: true,
                                    expanded: false,
                                    aria: "Show details".into(),
                                    updated_sort: 0,
                                    created_sort: 0,
                                });
                                boss_goal_panel_row_element(
                                    &row,
                                    waku_client::DaemonKey::Local,
                                    &WeakEntity::new_invalid(),
                                    cx,
                                )
                                .debug_selector(move || format!("finished-row-{index}"))
                                .into_any_element()
                            })
                            .size_full(),
                        ),
                )
                .child(
                    div()
                        .h(px(28.0))
                        .mt(px(4.0))
                        .flex_none()
                        .debug_selector(|| "history-toggle".into()),
                )
                .when(self.ongoing, |element| {
                    element.child(div().flex_1().min_h_0())
                })
        }
    }

    #[gpui::test]
    fn finished_preview_keeps_the_fifth_row_above_show_more(cx: &mut gpui::TestAppContext) {
        for ongoing in [false, true] {
            let (_, cx) = cx.add_window_view(|_, _| FinishedGoalsLayoutHarness { ongoing });
            for font_size in [11.0, 14.0, 20.0] {
                cx.update(|window, _| {
                    window.set_rem_size(px(font_size));
                    window.refresh();
                });
                cx.run_until_parked();
                let viewport = cx
                    .debug_bounds("finished-viewport")
                    .expect("viewport painted");
                let last_row = cx
                    .debug_bounds("finished-row-4")
                    .expect("fifth row painted");
                let toggle = cx.debug_bounds("history-toggle").expect("toggle painted");
                let row_height = px((GOALS_PANEL_ROW_HEIGHT * font_size
                    / waku_client::persistence::DEFAULT_UI_FONT_SIZE)
                    .ceil()
                    .max(GOALS_PANEL_ROW_HEIGHT));
                assert!((f32::from(last_row.size.height - row_height)).abs() < 0.1);
                assert!(
                    (f32::from(viewport.bottom() - last_row.bottom())).abs() < 0.1,
                    "ongoing={ongoing}, font_size={font_size}, viewport={viewport:?}, last_row={last_row:?}",
                );
                assert_eq!(toggle.origin.y, viewport.bottom() + px(4.0));
            }
        }
    }

    /// The Goals panel's two virtualized lists reduced to what the focus
    /// contract needs: outcome rows keyed by id, signature-gated resets,
    /// and the focus-lost fallback the app installs on the window.
    struct GoalsRowFocusHarness {
        boss_ui: boss::BossUi,
        key: waku_client::DaemonKey,
        /// Outcome ids rendered in each section — moving an id between
        /// them is the section transition the panel performs.
        finished: Vec<Uuid>,
        ongoing: Vec<Uuid>,
        /// How many rows each list's viewport paints; zero overdraw on
        /// the states keeps anything past it genuinely unmounted.
        viewport_rows: usize,
        fallback_focus: FocusHandle,
        signature: u64,
    }

    impl GoalsRowFocusHarness {
        fn new(
            window: &mut Window,
            cx: &mut Context<Self>,
            viewport_rows: usize,
            ongoing: Vec<Uuid>,
        ) -> Self {
            let fallback_focus = cx.focus_handle();
            cx.on_focus_lost(window, |this: &mut Self, window, cx| {
                let focus = this.fallback_focus.clone();
                window.focus(&focus, cx);
            })
            .detach();
            let mut boss_ui = boss::BossUi::default();
            // One row of overdraw keeps the painted range nearly the
            // viewport so a row beyond it is genuinely unmounted.
            let overdraw = px(GOALS_PANEL_ROW_HEIGHT);
            boss_ui.goals_finished_list = ListState::new(0, ListAlignment::Top, overdraw);
            boss_ui.goals_ongoing_list = ListState::new(0, ListAlignment::Top, overdraw);
            Self {
                boss_ui,
                key: waku_client::DaemonKey::Local,
                finished: Vec::new(),
                ongoing,
                viewport_rows,
                fallback_focus,
                signature: 0,
            }
        }

        fn focus_handle(&self, outcome: Uuid) -> FocusHandle {
            self.boss_ui.goals_focus_handles[&(self.key, format!("row:{outcome}"))].clone()
        }
    }

    impl Render for GoalsRowFocusHarness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let Self {
                boss_ui,
                key,
                finished,
                ongoing,
                viewport_rows,
                signature,
                ..
            } = self;
            let key = *key;
            // Prune and seed handles exactly as the panel does.
            let live_focus_keys: HashSet<String> = finished
                .iter()
                .chain(ongoing.iter())
                .map(|id| format!("row:{id}"))
                .collect();
            boss_ui
                .goals_focus_handles
                .retain(|(daemon, item_key), _| {
                    *daemon != key || live_focus_keys.contains(item_key)
                });
            let mut build = |ids: &[Uuid], cx: &mut Context<Self>| -> Vec<BossOutcomeItem> {
                ids.iter()
                    .map(|id| {
                        let focus = boss_ui
                            .goals_focus_handles
                            .entry((key, format!("row:{id}")))
                            .or_insert_with(|| cx.focus_handle().tab_stop(true))
                            .clone();
                        BossOutcomeItem::Row(Arc::new(BossOutcomePanelRow {
                            outcome: *id,
                            focus,
                            height: GOALS_PANEL_ROW_HEIGHT,
                            bucket: BossGoalBucket::Pending,
                            status: BossGoalStatus::Open,
                            queued_extra: 0,
                            title: format!("Outcome {id}"),
                            member_name: None,
                            avatar: None,
                            project_label: None,
                            detail: None,
                            worktree: false,
                            updated_label: None,
                            attention: false,
                            expanded: false,
                            aria: "Show details".into(),
                            updated_sort: 0,
                            created_sort: 0,
                        }))
                    })
                    .collect()
            };
            let finished_items = Arc::new(build(finished, cx));
            let ongoing_items = Arc::new(build(ongoing, cx));
            let items_signature = {
                let mut hasher = DefaultHasher::new();
                finished.hash(&mut hasher);
                ongoing.hash(&mut hasher);
                hasher.finish()
            };
            if items_signature != *signature {
                *signature = items_signature;
                reset_boss_goals_list(
                    &boss_ui.goals_finished_list,
                    &finished_items,
                    GOALS_PANEL_ROW_HEIGHT,
                );
                reset_boss_goals_list(
                    &boss_ui.goals_ongoing_list,
                    &ongoing_items,
                    GOALS_PANEL_ROW_HEIGHT,
                );
            }
            let viewport = px(GOALS_PANEL_ROW_HEIGHT * (*viewport_rows).max(1) as f32);
            let finished_list = boss_ui.goals_finished_list.clone();
            let ongoing_list = boss_ui.goals_ongoing_list.clone();
            let weak = WeakEntity::new_invalid();
            let weak_ongoing = weak.clone();
            let render_items = |items: Arc<Vec<BossOutcomeItem>>,
                                weak: WeakEntity<Waku>|
             -> Arc<dyn Fn(usize, &mut Window, &mut App) -> AnyElement> {
                Arc::new(move |index, _window, cx| {
                    items.get(index).map_or_else(
                        || div().into_any_element(),
                        |item| match item {
                            BossOutcomeItem::Row(row) => {
                                let tracked = row.outcome;
                                boss_goal_panel_row_element(row, key, &weak, cx)
                                    .debug_selector(move || {
                                        if tracked == TRACKED_OUTCOME {
                                            "tracked-goal-row".into()
                                        } else {
                                            "goal-row".into()
                                        }
                                    })
                                    .into_any_element()
                            }
                            _ => div().into_any_element(),
                        },
                    )
                })
            };
            let finished_render = render_items(finished_items, weak);
            let ongoing_render = render_items(ongoing_items, weak_ongoing);
            div()
                .id("goals-harness")
                .w(px(320.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(viewport)
                        .flex_none()
                        .child(
                            list(finished_list, move |index, window, cx| {
                                finished_render(index, window, cx)
                            })
                            .size_full(),
                        ),
                )
                .child(
                    div()
                        .h(viewport)
                        .flex_none()
                        .child(
                            list(ongoing_list, move |index, window, cx| {
                                ongoing_render(index, window, cx)
                            })
                            .size_full(),
                        ),
                )
        }
    }

    const TRACKED_OUTCOME: Uuid = Uuid::from_u128(7);

    /// A focused outcome row keeps focus through the remount a section
    /// move forces, even when it lands outside the destination list's
    /// painted range; losing the row entirely sends focus to the app's
    /// focus-lost fallback.
    #[gpui::test]
    fn focused_goals_row_survives_section_moves(cx: &mut gpui::TestAppContext) {
        let others: Vec<Uuid> = (0..6).map(|i| Uuid::from_u128(i + 100)).collect();
        let mut ongoing = vec![TRACKED_OUTCOME];
        ongoing.extend(others.iter().copied());
        let (view, cx) = cx.add_window_view(|window, cx| {
            GoalsRowFocusHarness::new(window, cx, 2, ongoing)
        });
        cx.run_until_parked();

        let tracked = cx.read(|app| view.read(app).focus_handle(TRACKED_OUTCOME));
        cx.update(|window, cx| window.focus(&tracked, cx));
        cx.run_until_parked();
        assert!(cx.update(|window, _| tracked.is_focused(window)));
        assert!(cx.debug_bounds("tracked-goal-row").is_some());

        // The row's outcome finishes: it leaves the ongoing list and
        // mounts inside Finished's viewport — one state change, one
        // remount.
        let _ = view.update(cx, |this, cx| {
            this.ongoing.retain(|id| *id != TRACKED_OUTCOME);
            this.finished.insert(0, TRACKED_OUTCOME);
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.update(|window, _| tracked.is_focused(window)),
            "a row moving sections keeps focus"
        );
        assert!(cx.debug_bounds("tracked-goal-row").is_some());

        // A settled row that is still focused and lands below the new
        // scroll position keeps rendering — the list renders the focused
        // item outside the viewport rather than dropping it.
        let _ = view.update(cx, |this, cx| {
            this.finished.clear();
            this.finished.extend(others.iter().copied());
            this.finished.push(TRACKED_OUTCOME);
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.update(|window, _| tracked.is_focused(window)),
            "a focused row scrolled out of view keeps focus"
        );
        assert!(
            cx.debug_bounds("tracked-goal-row").is_some(),
            "the focused row keeps rendering off-viewport"
        );

        // Losing the outcome entirely releases focus to the window's
        // fallback — the row is unreachable, so focus must not linger.
        let _ = view.update(cx, |this, cx| {
            this.finished.clear();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.update(|window, _| !tracked.is_focused(window)));
        assert!(cx.update(|window, cx| {
            view.read(cx).fallback_focus.is_focused(window)
        }));
    }

    #[test]
    fn session_panels_allow_normal_surfaces_but_hide_goals() {
        let session_surfaces = [
            RightPanelSurface::Browser(Uuid::nil()),
            RightPanelSurface::Terminal(Uuid::nil()),
            RightPanelSurface::BackgroundWork {
                key: BackgroundWorkKey::new(BackgroundWorkKind::Process, "process"),
                title: String::new(),
            },
            RightPanelSurface::Files,
            RightPanelSurface::Diff,
            RightPanelSurface::File("README.md".into()),
            RightPanelSurface::GitHub(Uuid::nil()),
            RightPanelSurface::Goals,
        ];
        assert_eq!(
            session_surfaces
                .iter()
                .map(session_panel_surface)
                .collect::<Vec<_>>(),
            [true, true, true, true, true, true, true, false],
        );
    }

    #[test]
    fn boss_tasks_tab_is_added_by_default_and_survives_strip_restore() {
        let mut surfaces = Vec::new();
        let mut active_surface = None;

        assert_eq!(
            ensure_boss_tasks_tab(&mut surfaces, &mut active_surface),
            Some(true)
        );
        assert_eq!(surfaces, vec![RightPanelSurface::Goals]);
        assert_eq!(active_surface, Some(0));

        // Syncing a restored strip keeps Tasks, while adding it to a strip
        // with user tabs preserves the active user tab.
        assert_eq!(
            ensure_boss_tasks_tab(&mut surfaces, &mut active_surface),
            None
        );
        assert_eq!(surfaces, vec![RightPanelSurface::Goals]);
        let mut restored = vec![RightPanelSurface::Files];
        let mut restored_active = Some(0);
        assert_eq!(
            ensure_boss_tasks_tab(&mut restored, &mut restored_active),
            Some(false)
        );
        assert_eq!(
            restored,
            vec![RightPanelSurface::Goals, RightPanelSurface::Files]
        );
        assert_eq!(restored_active, Some(1));
    }

    #[test]
    fn boss_tasks_tab_cannot_be_closed_but_other_tabs_can() {
        assert!(!right_panel_surface_is_closable(&RightPanelSurface::Goals));
        assert!(!right_panel_surface_is_closable(&RightPanelSurface::Plan {
            session_id: Uuid::nil(),
            plan_file: "plans/plan.md".into(),
        }));
        assert!(right_panel_surface_is_closable(&RightPanelSurface::Files));
    }

    #[test]
    fn transcript_file_links_route_by_the_active_workspace() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project_file = workspace.join("src/app/right_panel.rs");
        let project_file_with_line = format!("{}:1596", project_file.display());
        let project_file_with_column = format!("{}:1596:8", project_file.display());
        let relative_project_file = Path::new("src")
            .join("app")
            .join("right_panel.rs")
            .to_string_lossy()
            .into_owned();

        assert_eq!(
            transcript_link_route(&project_file_with_line, Some(workspace)),
            TranscriptLinkRoute::ProjectFile(relative_project_file.clone(), None)
        );
        assert_eq!(
            transcript_link_route(&project_file_with_column, Some(workspace)),
            TranscriptLinkRoute::ProjectFile(relative_project_file, None)
        );

        let encoded_file_url =
            url::Url::from_file_path(workspace.join("My File.rs")).expect("absolute file path");
        assert_eq!(
            transcript_link_route(&format!("{encoded_file_url}#L12C4"), Some(workspace)),
            TranscriptLinkRoute::ProjectFile("My File.rs".into(), None)
        );

        let outside_file = workspace.join("../kero/src/app.rs");
        let outside_file_with_line = format!("{}:20", outside_file.display());
        assert_eq!(
            transcript_link_route(&outside_file_with_line, Some(workspace)),
            TranscriptLinkRoute::Finder(normalized_path(&outside_file))
        );
        assert_eq!(
            transcript_link_route("https://example.com/file.rs:12", Some(workspace)),
            TranscriptLinkRoute::External
        );
    }

    #[test]
    fn contextual_file_references_resolve_relative_paths_without_changing_other_links() {
        let context = crate::model::ReferenceContext {
            project_root: PathBuf::from("/work/project"),
            worktree: None,
        };
        let reference = context.encode_reference("src/main.rs:12:4");
        let (decoded_context, target) =
            crate::model::ReferenceContext::decode_reference(&reference).unwrap();

        assert_eq!(decoded_context, context);
        assert_eq!(
            reference_file_link_path(&target),
            Some(PathBuf::from("src/main.rs"))
        );
        assert_eq!(file_link_location(&target), Some((12, Some(4))));
        // Plain relative Markdown links keep their existing external behavior.
        assert_eq!(markdown_file_link_path("src/main.rs"), None);
        assert_eq!(
            transcript_link_route("src/main.rs", None),
            TranscriptLinkRoute::External
        );
    }

    #[test]
    fn task_links_route_an_optional_message_deep_link() {
        let task = Uuid::new_v4();
        let message = Uuid::new_v4();
        let prefix = waku_protocol::TASK_LINK_PREFIX;

        assert_eq!(
            transcript_link_route(&format!("{prefix}{task}"), None),
            TranscriptLinkRoute::Task(Some(task), None)
        );
        assert_eq!(
            transcript_link_route(&format!("{prefix}{task}?message={message}"), None),
            TranscriptLinkRoute::Task(Some(task), Some(message))
        );
        // A malformed message id degrades to a plain task link, and a
        // malformed task id still reports a bad task link.
        assert_eq!(
            transcript_link_route(&format!("{prefix}{task}?message=nope"), None),
            TranscriptLinkRoute::Task(Some(task), None)
        );
        assert_eq!(
            transcript_link_route(&format!("{prefix}nope?message={message}"), None),
            TranscriptLinkRoute::Task(None, Some(message))
        );
    }

    #[test]
    fn link_copy_names_what_it_copies() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
        let file = workspace.join("My File.rs");
        let encoded_file_url = url::Url::from_file_path(&file).expect("absolute file path");

        assert_eq!(
            transcript_link_copy(&format!("{}:12:4", file.display())),
            (file.to_string_lossy().into_owned(), "common.copy_file_path")
        );
        assert_eq!(
            transcript_link_copy(&format!("{encoded_file_url}#L12")),
            (file.to_string_lossy().into_owned(), "common.copy_file_path")
        );
        let url = "https://example.com/file.rs:12";
        assert_eq!(
            transcript_link_copy(url),
            (url.to_owned(), "common.copy_url")
        );
    }

    #[test]
    fn line_column_offsets_clamp_into_the_file() {
        let content = "ab\ncd\néf\n";

        assert_eq!(cursor_offset_for_line_column(content, 1, 1), 0);
        assert_eq!(cursor_offset_for_line_column(content, 2, 1), 3);
        assert_eq!(cursor_offset_for_line_column(content, 2, 3), 5);
        // Columns count characters, so the second column of `éf` is past é's
        // two bytes.
        assert_eq!(cursor_offset_for_line_column(content, 3, 2), 8);
        // A column past the line's end lands on the line break; a line past
        // the file's end lands at the end.
        assert_eq!(cursor_offset_for_line_column(content, 2, 99), 5);
        assert_eq!(cursor_offset_for_line_column(content, 99, 1), content.len());
        // Zero clamps up to the first line and column.
        assert_eq!(cursor_offset_for_line_column(content, 0, 0), 0);
    }

    /// Selection resolves rows by key, so a repeated key makes a drag jump
    /// between the duplicates. Numbered rows keep their number-derived keys
    /// (stable across Review's gap expansion); rows a provider never
    /// positioned fall back to the row index.
    #[test]
    fn diff_row_selection_keys_are_unique_even_without_line_numbers() {
        let positionless =
            crate::review_diff::from_file_changes(&[crate::model::ActivityFileChange {
                path: "a.md".into(),
                additions: Some(2),
                deletions: Some(0),
                status: None,
                diff: Some("@@\n+one\n+two\n \n+three\n".into()),
            }]);
        let keys = positionless
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                matches!(
                    line.kind,
                    crate::review_diff::LineKind::Context
                        | crate::review_diff::LineKind::Addition
                        | crate::review_diff::LineKind::Deletion
                )
            })
            .map(|(index, line)| diff_row_selection_key("activity", line, index))
            .collect::<Vec<_>>();
        let unique = keys.iter().collect::<HashSet<_>>();
        assert_eq!(unique.len(), keys.len(), "{keys:?}");

        let numbered = crate::review_diff::Line {
            file_index: 0,
            old_line: Some(4),
            new_line: Some(6),
            kind: crate::review_diff::LineKind::Context,
            content: "kept".into(),
            tokens: Vec::new(),
        };
        assert_eq!(
            diff_row_selection_key("review-diff", &numbered, 9),
            "review-diff-line-0-context-4-6",
        );
    }

    fn review_file(path: &str) -> crate::review_diff::File {
        crate::review_diff::File {
            path: path.into(),
            additions: 1,
            deletions: 0,
            status: crate::review_diff::FileStatus::Modified,
            diff_line: None,
        }
    }

    fn review_files() -> Vec<crate::review_diff::File> {
        [
            "README.md",
            "src/app/runtime.rs",
            "src/app/view.rs",
            "src/lib.rs",
            "tests/review.rs",
        ]
        .into_iter()
        .map(review_file)
        .collect()
    }

    #[test]
    fn review_gap_expansion_icons_match_pierre_visual_directions() {
        use crate::review_diff::{ExpansionDirection, GapPosition};

        assert_eq!(
            review_diff_gap_directions(GapPosition::Leading, true),
            &[ExpansionDirection::End]
        );
        assert_eq!(
            review_diff_gap_directions(GapPosition::Trailing, true),
            &[ExpansionDirection::Start]
        );
        assert_eq!(
            review_diff_gap_directions(GapPosition::Between, false),
            &[ExpansionDirection::Both]
        );
        assert_eq!(
            review_diff_gap_directions(GapPosition::Between, true),
            &[ExpansionDirection::Start, ExpansionDirection::End]
        );

        assert_eq!(
            review_diff_gap_icon_path(ExpansionDirection::Start),
            "icons/chevron-down.svg"
        );
        assert_eq!(
            review_diff_gap_icon_path(ExpansionDirection::End),
            "icons/chevron-up.svg"
        );
        assert_eq!(
            review_diff_gap_icon_path(ExpansionDirection::Both),
            "icons/chevrons-up-down.svg"
        );
    }

    #[test]
    fn review_tree_builds_shared_directories_once() {
        let files = review_files();
        let expanded = review_diff_directory_paths(&files);
        assert_eq!(
            review_diff_tree_rows(&files, &expanded, ""),
            vec![
                ReviewDiffTreeRow::File {
                    file_index: 0,
                    depth: 0,
                },
                ReviewDiffTreeRow::Directory {
                    path: "src".into(),
                    name: "src".into(),
                    depth: 0,
                    expanded: true,
                },
                ReviewDiffTreeRow::Directory {
                    path: "src/app".into(),
                    name: "app".into(),
                    depth: 1,
                    expanded: true,
                },
                ReviewDiffTreeRow::File {
                    file_index: 1,
                    depth: 2,
                },
                ReviewDiffTreeRow::File {
                    file_index: 2,
                    depth: 2,
                },
                ReviewDiffTreeRow::File {
                    file_index: 3,
                    depth: 1,
                },
                ReviewDiffTreeRow::Directory {
                    path: "tests".into(),
                    name: "tests".into(),
                    depth: 0,
                    expanded: true,
                },
                ReviewDiffTreeRow::File {
                    file_index: 4,
                    depth: 1,
                },
            ]
        );
    }

    #[test]
    fn review_tree_collapse_hides_only_descendants() {
        let files = review_files();
        let expanded = HashSet::from(["src".to_owned()]);
        let rows = review_diff_tree_rows(&files, &expanded, "");

        assert!(rows.contains(&ReviewDiffTreeRow::Directory {
            path: "src/app".into(),
            name: "app".into(),
            depth: 1,
            expanded: false,
        }));
        assert!(rows.contains(&ReviewDiffTreeRow::File {
            file_index: 3,
            depth: 1,
        }));
        assert!(!rows.iter().any(|row| {
            matches!(
                row,
                ReviewDiffTreeRow::File { file_index, .. } if *file_index == 1 || *file_index == 2
            )
        }));
    }

    #[test]
    fn review_tree_filter_reveals_matching_path_and_ancestors() {
        let rows = review_diff_tree_rows(&review_files(), &HashSet::new(), "RUNTIME");
        assert_eq!(
            rows,
            vec![
                ReviewDiffTreeRow::Directory {
                    path: "src".into(),
                    name: "src".into(),
                    depth: 0,
                    expanded: true,
                },
                ReviewDiffTreeRow::Directory {
                    path: "src/app".into(),
                    name: "app".into(),
                    depth: 1,
                    expanded: true,
                },
                ReviewDiffTreeRow::File {
                    file_index: 1,
                    depth: 2,
                },
            ]
        );
    }

    #[test]
    fn working_tree_only_descends_into_expanded_directories() {
        let root = std::env::temp_dir().join(format!("waku-working-tree-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("README.md"), "# Waku\n").unwrap();

        let collapsed = visible_working_tree_entries(&root, &HashSet::new());
        assert_eq!(
            collapsed
                .iter()
                .map(|entry| entry.relative_path.clone())
                .collect::<Vec<_>>(),
            vec!["src".to_owned(), "README.md".to_owned()]
        );

        let expanded = HashSet::from([root.join("src")]);
        let visible = visible_working_tree_entries(&root, &expanded);
        let nested = Path::new("src")
            .join("nested")
            .to_string_lossy()
            .into_owned();
        let main_rs = Path::new("src")
            .join("main.rs")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            visible
                .iter()
                .map(|entry| (entry.relative_path.clone(), entry.depth))
                .collect::<Vec<_>>(),
            vec![
                ("src".to_owned(), 0),
                (nested, 1),
                (main_rs, 1),
                ("README.md".to_owned(), 0)
            ]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_highlighter_language_follows_file_name_and_extension() {
        assert_eq!(file_highlighter_language("src/app.rs"), "rust");
        assert_eq!(file_highlighter_language("ui/panel.tsx"), "tsx");
        assert_eq!(file_highlighter_language("Sources/App.swift"), "swift");
        assert_eq!(file_highlighter_language("Makefile"), "make");
        assert_eq!(file_highlighter_language("src/native.hpp"), "cpp");
        assert_eq!(file_highlighter_language("lib/main.ex"), "elixir");
        assert_eq!(file_highlighter_language("src/main.zig"), "zig");
        assert_eq!(file_highlighter_language("web/page.astro"), "html");
        assert_eq!(file_highlighter_language("ui/card.tsrx"), "tsx");
        assert_eq!(file_highlighter_language("LICENSE"), "text");

        for (path, expected_language) in [
            ("bun.lock", "json"),
            ("package-lock.json", "json"),
            ("deno.lock", "json"),
            ("composer.lock", "json"),
            ("Pipfile.lock", "json"),
            ("Package.resolved", "json"),
            ("Cargo.lock", "toml"),
            ("uv.lock", "toml"),
            ("poetry.lock", "toml"),
            ("pnpm-lock.yaml", "yaml"),
            ("yarn.lock", "yaml"),
            ("Podfile.lock", "yaml"),
            ("Gemfile.lock", "yaml"),
            ("mix.lock", "elixir"),
        ] {
            assert_eq!(file_highlighter_language(path), expected_language, "{path}");
        }
    }

    /// The editor colours code with the in-house lexer, so what matters is that
    /// the names `file_highlighter_language` produces are ones the lexer knows.
    /// The few it does not are listed here deliberately: they render as plain
    /// monospace rather than silently looking broken.
    #[test]
    fn mapped_languages_resolve_in_the_in_house_lexer() {
        use crate::md::highlight::{Lang, lang_for_tag};

        for (language, expected) in [
            ("rust", Some(Lang::Rust)),
            ("tsx", Some(Lang::Script)),
            ("swift", Some(Lang::Swift)),
            ("json", Some(Lang::Json)),
            ("toml", Some(Lang::Toml)),
            ("yaml", Some(Lang::Yaml)),
            ("make", Some(Lang::Shell)),
            ("cpp", Some(Lang::C)),
            ("markdown", Some(Lang::Markdown)),
            ("elixir", Some(Lang::Elixir)),
            ("lua", Some(Lang::Lua)),
            ("php", Some(Lang::Php)),
            ("dart", Some(Lang::Dart)),
            ("zig", Some(Lang::Zig)),
            ("tsx", Some(Lang::Script)),
            ("html", Some(Lang::Html)),
            // Not yet lexed; these fall back to unhighlighted monospace.
            ("text", None),
        ] {
            assert_eq!(lang_for_tag(language), expected, "{language}");
        }
    }

    #[test]
    fn the_editor_lexer_colours_code_it_recognises() {
        use crate::md::highlight::{Carry, Lang, TokenClass, tokenize_line};

        let line = r#"export function Card({ title }: { title: string }) {"#;
        let spans = tokenize_line(Lang::Script, line, Carry::None)
            .0
            .into_iter()
            .map(|token| (&line[token.range], token.class))
            .collect::<Vec<_>>();

        assert!(spans.contains(&("export", TokenClass::Keyword)));
        assert!(spans.contains(&("function", TokenClass::Keyword)));
        assert!(spans.contains(&("Card", TokenClass::Function)));
    }

    #[test]
    fn working_tree_file_icons_follow_names_and_extensions() {
        assert_eq!(file_icon_for_name("main.rs"), "icons/file-types/rust.svg");
        assert_eq!(
            file_icon_for_name("Panel.tsx"),
            "icons/file-types/react.svg"
        );
        assert_eq!(
            file_icon_for_name("README.md"),
            "icons/file-types/readme.svg"
        );
        assert_eq!(
            file_icon_for_name("Dockerfile.dev"),
            "icons/file-types/docker.svg"
        );
        assert_eq!(file_icon_for_name("bun.lock"), "icons/file-types/bun.svg");
        assert_eq!(
            file_icon_for_name("pnpm-lock.yaml"),
            "icons/file-types/pnpm.svg"
        );
        assert_eq!(
            file_icon_for_name("vite.config.ts"),
            "icons/file-types/vite.svg"
        );
        assert_eq!(
            file_icon_for_name("unknown.data"),
            "icons/file-types/file.svg"
        );
    }

    #[test]
    fn files_tab_uses_the_selected_file_name_and_icon() {
        let files = RightPanelSurface::Files;
        assert_eq!(right_panel_tab_label(&files, None), "Files");
        assert_eq!(
            right_panel_tab_label(&files, Some("packages/desktop/bun.lock")),
            "bun.lock"
        );
        assert_eq!(
            right_panel_tab_icon(&files, Some("packages/desktop/bun.lock")),
            "icons/file-types/bun.svg"
        );

        let file = RightPanelSurface::File("src/main.rs".into());
        assert_eq!(right_panel_tab_label(&file, None), "main.rs");
        assert_eq!(
            right_panel_tab_icon(&file, None),
            "icons/file-types/rust.svg"
        );
    }

    #[test]
    fn right_panel_tab_titles_stay_on_one_line() {
        let background = RightPanelSurface::BackgroundWork {
            key: BackgroundWorkKey::new(BackgroundWorkKind::Process, "process-1"),
            title: "node -e '\n  const value = 1'".into(),
        };
        assert_eq!(
            right_panel_tab_label(&background, None),
            "node -e ' const value = 1'"
        );
    }

    #[test]
    fn only_reuses_single_instance_surface_tabs() {
        let browser = RightPanelSurface::new_browser();
        let terminal = RightPanelSurface::new_terminal();
        let background = RightPanelSurface::BackgroundWork {
            key: BackgroundWorkKey::new(BackgroundWorkKind::Process, "process-1"),
            title: "Process one".into(),
        };
        let project_id = Uuid::new_v4();
        let surfaces = vec![
            browser,
            terminal,
            background,
            RightPanelSurface::Files,
            RightPanelSurface::Diff,
            RightPanelSurface::GitHub(project_id),
        ];

        assert_eq!(
            reusable_surface_index(&surfaces, &RightPanelSurface::new_browser()),
            None
        );
        assert_eq!(
            reusable_surface_index(&surfaces, &RightPanelSurface::new_terminal()),
            None
        );
        assert_eq!(
            reusable_surface_index(
                &surfaces,
                &RightPanelSurface::BackgroundWork {
                    key: BackgroundWorkKey::new(BackgroundWorkKind::Process, "process-1"),
                    title: "Renamed process".into(),
                },
            ),
            Some(2)
        );
        assert_eq!(
            reusable_surface_index(&surfaces, &RightPanelSurface::Files),
            Some(3)
        );
        assert_eq!(
            reusable_surface_index(&surfaces, &RightPanelSurface::Diff),
            Some(4)
        );
        // The work-item tab is per project — a second item from the same
        // repo reuses it, another repo gets its own.
        assert_eq!(
            reusable_surface_index(&surfaces, &RightPanelSurface::GitHub(project_id)),
            Some(5)
        );
        assert_eq!(
            reusable_surface_index(&surfaces, &RightPanelSurface::GitHub(Uuid::new_v4()),),
            None
        );
    }

    #[test]
    fn right_panel_state_isolated_by_session() {
        let session_with_terminal = Uuid::new_v4();
        let other_session = Uuid::new_v4();
        let terminal_id = Uuid::new_v4();
        let mut states = HashMap::new();
        let mut terminal_state = RightPanelSessionState::empty(true);
        terminal_state.surfaces = vec![RightPanelSurface::Terminal(terminal_id)];
        terminal_state.active_surface = Some(0);
        terminal_state.file_tree_width = 248.0;
        states.insert(
            RightPanelOwner::Session(session_with_terminal),
            terminal_state,
        );

        let other_state = RightPanelSessionState::take_or_closed(
            &mut states,
            RightPanelOwner::Session(other_session),
        );
        assert!(!other_state.visible);
        assert!(other_state.surfaces.is_empty());
        assert_eq!(other_state.active_surface, None);
        assert_eq!(other_state.file_tree_width, DEFAULT_FILE_TREE_WIDTH);

        let restored = RightPanelSessionState::take_or_closed(
            &mut states,
            RightPanelOwner::Session(session_with_terminal),
        );
        assert!(restored.visible);
        assert_eq!(
            restored.surfaces,
            vec![RightPanelSurface::Terminal(terminal_id)]
        );
        assert_eq!(restored.active_surface, Some(0));
        assert_eq!(restored.file_tree_width, 248.0);
    }

    #[test]
    fn tab_scroll_fades_only_show_toward_hidden_content() {
        assert_eq!(
            tab_scroll_fade_visibility(px(0.0), px(120.0)),
            (false, true)
        );
        assert_eq!(
            tab_scroll_fade_visibility(px(-40.0), px(120.0)),
            (true, true)
        );
        assert_eq!(
            tab_scroll_fade_visibility(px(-120.0), px(120.0)),
            (true, false)
        );
        assert_eq!(tab_scroll_fade_visibility(px(0.0), px(0.0)), (false, false));
    }

    #[test]
    fn selected_tab_offset_clears_fade_overlays() {
        assert_eq!(
            fade_safe_tab_offset(
                px(-100.0),
                px(300.0),
                px(90.0),
                px(190.0),
                px(0.0),
                px(300.0),
            ),
            px(-66.0)
        );
        assert_eq!(
            fade_safe_tab_offset(
                px(-100.0),
                px(324.0),
                px(300.0),
                px(400.0),
                px(0.0),
                px(300.0),
            ),
            px(-124.0)
        );
        assert_eq!(
            fade_safe_tab_offset(px(0.0), px(0.0), px(0.0), px(100.0), px(0.0), px(300.0),),
            px(0.0)
        );
    }

    fn boss_employee(
        lifecycle: waku_protocol::boss::EmployeeLifecycle,
        blocker: Option<&str>,
    ) -> waku_protocol::boss::BossEmployee {
        waku_protocol::boss::BossEmployee {
            session_id: Uuid::new_v4(),
            supervisor_id: Uuid::new_v4(),
            identity: waku_protocol::boss::BossIdentity {
                id: Uuid::new_v4(),
                name: "Nina".into(),
                avatar_seed: String::new(),
                avatar_style: Default::default(),
            },
            job_title: "Reviewer".into(),
            persona_id: Uuid::new_v4(),
            work_goal: waku_protocol::boss::EmployeeGoal::Errand,
            created_at: Some(100),
            icon: None,
            permissions: Default::default(),
            pinned_files: Vec::new(),
            expired: lifecycle == waku_protocol::boss::EmployeeLifecycle::Expired,
            workspace_transition: false,
            expired_at: None,
            blocker: blocker.map(str::to_owned),
            cancelled: false,
            expiry: None,
            state: lifecycle,
            ticket: None,
            queued_at: None,
            request_id: None,
            request_fingerprint: None,
            plan_id: None,
            item_id: None,
            assignment: None,
        }
    }

    fn boss_outcome(state: waku_protocol::boss::OutcomeState) -> waku_protocol::boss::BossOutcome {
        waku_protocol::boss::BossOutcome {
            id: Uuid::new_v4(),
            outcome: "Ship it".into(),
            success_criteria: String::new(),
            state,
            finishing_assignment: None,
            handoffs: Vec::new(),
            completion_conflict: None,
            evidence: None,
            plan_id: None,
            waiting: None,
            snoozed_until: None,
            last_activity_at: 0,
            unattended_since: None,
            last_reminder: None,
            created_at: 0,
            completed_at: None,
            history: Vec::new(),
            assignments: Vec::new(),
        }
    }

    fn outcome_row(
        state: waku_protocol::boss::OutcomeState,
        members: Vec<waku_protocol::boss::BossEmployee>,
    ) -> boss::BossOutcomeRow {
        boss::BossOutcomeRow {
            outcome: boss_outcome(state),
            members: members
                .into_iter()
                .map(|employee| boss::BossOutcomeMember {
                    employee,
                    queue_rank: None,
                })
                .collect(),
            attention: false,
        }
    }

    /// The lifecycle contract the Goals panel's sectioning depends on:
    /// admission outranks stale thread state, a terminal thread goal
    /// graduates a live row to Finished, and neutral expiry never reads as
    /// success.
    #[test]
    fn goal_status_buckets_follow_lifecycle_then_terminal_evidence() {
        use waku_protocol::boss::EmployeeLifecycle;

        let goal_row = |lifecycle, blocker: Option<&str>| boss_employee(lifecycle, blocker);
        let session = |status| {
            let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
            session.status = status;
            session
        };
        let thread_goal = |status| crate::model::ThreadGoal {
            objective: "Ship it".into(),
            status,
            managed_since_message: None,
            managed_id: None,
            managed_last_turn: None,
            token_budget: None,
            tokens_used: 0,
            time_used_seconds: 0,
        };

        assert_eq!(
            boss_member_status(
                &goal_row(EmployeeLifecycle::Queued, None),
                Some(&session(SessionStatus::Working))
            ),
            (BossGoalBucket::Pending, BossGoalStatus::Queued)
        );
        assert_eq!(
            boss_member_status(
                &goal_row(EmployeeLifecycle::Dispatching, None),
                Some(&session(SessionStatus::Idle))
            ),
            (BossGoalBucket::Running, BossGoalStatus::Starting)
        );

        // Expired rows are always Finished; outcome evidence picks the label.
        let expired = goal_row(EmployeeLifecycle::Expired, None);
        let mut failed_session = session(SessionStatus::Failed);
        failed_session.thread_goal = Some(thread_goal(crate::model::ThreadGoalStatus::Complete));
        assert_eq!(
            boss_member_status(&expired, Some(&failed_session)),
            (BossGoalBucket::Finished, BossGoalStatus::Failed)
        );
        assert_eq!(
            boss_member_status(
                &goal_row(EmployeeLifecycle::Expired, Some("signing identity")),
                Some(&session(SessionStatus::Idle))
            ),
            (BossGoalBucket::Finished, BossGoalStatus::Attention)
        );
        let mut complete_session = session(SessionStatus::Idle);
        complete_session.thread_goal = Some(thread_goal(crate::model::ThreadGoalStatus::Complete));
        assert_eq!(
            boss_member_status(&expired, Some(&complete_session)),
            (BossGoalBucket::Finished, BossGoalStatus::Complete)
        );
        let mut budget_session = session(SessionStatus::Idle);
        budget_session.thread_goal =
            Some(thread_goal(crate::model::ThreadGoalStatus::BudgetLimited));
        assert_eq!(
            boss_member_status(&expired, Some(&budget_session)),
            (BossGoalBucket::Finished, BossGoalStatus::BudgetReached)
        );
        assert_eq!(
            boss_member_status(&expired, Some(&session(SessionStatus::Idle))),
            (BossGoalBucket::Finished, BossGoalStatus::Finished)
        );

        // A terminal thread goal graduates a live row to Finished; teardown
        // and attention states stay In progress.
        let working = goal_row(EmployeeLifecycle::Working, None);
        assert_eq!(
            boss_member_status(&working, Some(&complete_session)),
            (BossGoalBucket::Finished, BossGoalStatus::Complete)
        );
        assert_eq!(
            boss_member_status(&working, Some(&failed_session)),
            (BossGoalBucket::Running, BossGoalStatus::Failed)
        );
        assert_eq!(
            boss_member_status(
                &goal_row(EmployeeLifecycle::Working, Some("needs a decision")),
                Some(&session(SessionStatus::Idle))
            ),
            (BossGoalBucket::Running, BossGoalStatus::Attention)
        );
        assert_eq!(
            boss_member_status(
                &goal_row(EmployeeLifecycle::Finishing, None),
                Some(&session(SessionStatus::Working))
            ),
            (BossGoalBucket::Running, BossGoalStatus::Finishing)
        );
        assert_eq!(
            boss_member_status(&working, Some(&session(SessionStatus::Waiting))),
            (BossGoalBucket::Running, BossGoalStatus::NeedsInput)
        );
        assert_eq!(
            boss_member_status(&working, Some(&session(SessionStatus::Working))),
            (BossGoalBucket::Running, BossGoalStatus::Working)
        );
        assert_eq!(
            boss_member_status(&working, Some(&session(SessionStatus::Idle))),
            (BossGoalBucket::Running, BossGoalStatus::Active)
        );
        assert_eq!(
            boss_member_status(&working, None),
            (BossGoalBucket::Running, BossGoalStatus::Unavailable)
        );
    }


    /// The section rules: terminal state owns Finished; a live member owns
    /// In progress over queued or waiting work; and the member-less cases
    /// read Pending with their honest labels — attention, follow-up,
    /// queued, waiting, not started, or open.
    #[test]
    fn outcome_status_follows_terminal_then_live_then_pending_rules() {
        use waku_protocol::boss::{EmployeeLifecycle, OutcomeState};
        let sessions: HashMap<Uuid, &AgentSession> = HashMap::new();
        let now = 1_000u64;

        // Terminal states never read as in progress or pending — and a
        // cancelled outcome is never labeled completed.
        for (state, expected) in [
            (OutcomeState::Completed, BossGoalStatus::Complete),
            (OutcomeState::Cancelled, BossGoalStatus::Cancelled),
        ] {
            assert_eq!(
                boss_outcome_status(&outcome_row(state, Vec::new()), &sessions, now),
                (BossGoalBucket::Finished, expected)
            );
        }

        // A running member owns In progress even beside queued work.
        let mut row = outcome_row(
            OutcomeState::Open,
            vec![
                boss_employee(EmployeeLifecycle::Working, None),
                boss_employee(EmployeeLifecycle::Queued, None),
            ],
        );
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Running, BossGoalStatus::Unavailable)
        );
        let mut live_session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        live_session.id = row.members[0].employee.session_id;
        live_session.status = SessionStatus::Working;
        let sessions = HashMap::from([(live_session.id, &live_session)]);
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Running, BossGoalStatus::Working)
        );
        row.members[0].employee.blocker = Some("needs a decision".into());
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Running, BossGoalStatus::Attention)
        );

        // Queued members with nothing running read Pending · Queued.
        let row = outcome_row(
            OutcomeState::Open,
            vec![boss_employee(EmployeeLifecycle::Queued, None)],
        );
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Queued)
        );

        // A recorded wait reads Pending · Waiting, not ordinary queueing.
        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        row.outcome.assignments.push(waku_protocol::boss::OutcomeAssignment {
            session: Uuid::new_v4(),
            generation: 1,
            identity: None,
            job_title: None,
            finishes_outcome: None,
            after_success: None,
            prerequisites: Vec::new(),
            assigned_at: Some(10),
            settled: None,
        });
        row.outcome.waiting = Some(waku_protocol::boss::OutcomeWait::Until { at: now + 600 });
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Waiting)
        );
        // An elapsed defer no longer explains the pause.
        row.outcome.waiting = Some(waku_protocol::boss::OutcomeWait::Until { at: now - 1 });
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Open)
        );

        // An owed handoff reads Pending · Needs follow-up.
        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        row.outcome.handoffs.push(waku_protocol::boss::OutcomeHandoff {
            id: Uuid::new_v4(),
            assignment: Uuid::new_v4(),
            attempt: 1,
            intent: "Review the result".into(),
            created_at: 10,
            resolution: None,
        });
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::FollowUp)
        );

        // Unresolved failures read Pending · Needs attention and outrank
        // the waiting and queued labels.
        let mut row = outcome_row(
            OutcomeState::Open,
            vec![boss_employee(EmployeeLifecycle::Queued, None)],
        );
        row.attention = true;
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Blocked)
        );

        // Zero assignments reads Pending · Not started; settled work with
        // nothing outstanding reads Pending · Open.
        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::NotStarted)
        );
        row.outcome
            .assignments
            .push(waku_protocol::boss::OutcomeAssignment {
                session: Uuid::new_v4(),
                generation: 1,
                identity: None,
                job_title: None,
                finishes_outcome: None,
                after_success: None,
                prerequisites: Vec::new(),
                assigned_at: Some(10),
                settled: Some(waku_protocol::boss::AssignmentSettle {
                    verdict: waku_protocol::boss::AssignmentVerdict::Finished,
                    cause: None,
                    blocked: false,
                    at: Some(20),
                }),
            });
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Open)
        );
    }

    fn outcome_assignment(
        session: Uuid,
        generation: u64,
    ) -> waku_protocol::boss::OutcomeAssignment {
        waku_protocol::boss::OutcomeAssignment {
            session,
            generation,
            identity: None,
            job_title: None,
            finishes_outcome: None,
            after_success: None,
            prerequisites: Vec::new(),
            assigned_at: Some(10),
            settled: None,
        }
    }

    /// An expanded outcome keeps every attempt distinct and honest: a
    /// settled failure carries its recorded cause, a queued dependent
    /// names its failed prerequisite and its own admission wait, a
    /// flagged member shows its blocker text, and only entries whose
    /// conversation the snapshot knows become destinations.
    #[test]
    fn outcome_entries_track_each_attempt_and_its_cause() {
        use waku_protocol::boss::{
            AssignmentSettle, AssignmentVerdict, EmployeeLifecycle, ExpiryCause, OutcomeState,
        };
        let now = 1_000u64;
        let failed = Uuid::new_v4();
        let dependent = Uuid::new_v4();
        let flagged = Uuid::new_v4();

        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        // A settled failure with a recorded cause — its roster record is
        // gone, so only the durable row can describe it.
        let mut attempt = outcome_assignment(failed, 1);
        attempt.settled = Some(AssignmentSettle {
            verdict: AssignmentVerdict::Failed,
            cause: Some(ExpiryCause::ExitedMidTurn),
            blocked: false,
            at: Some(20),
        });
        // A queued dependent of the failed attempt — blocked, never
        // ordinary progress.
        let mut waiting = outcome_assignment(dependent, 1);
        waiting.prerequisites = vec![failed];
        // An unsettled attempt whose roster record carries a blocker.
        let blocked_attempt = outcome_assignment(flagged, 1);
        row.outcome.assignments = vec![attempt, waiting, blocked_attempt];

        let mut queued_member = boss_employee(EmployeeLifecycle::Queued, None);
        queued_member.session_id = dependent;
        let mut expired_member =
            boss_employee(EmployeeLifecycle::Expired, Some("missing credentials"));
        expired_member.session_id = flagged;
        row.members = [queued_member, expired_member]
            .into_iter()
            .map(|employee| boss::BossOutcomeMember {
                employee,
                queue_rank: None,
            })
            .collect();

        let mut session = AgentSession::new(dependent, ProviderKind::Codex);
        session.id = dependent;
        let sessions: HashMap<Uuid, &AgentSession> = HashMap::from([(dependent, &session)]);
        let queued: HashMap<Uuid, String> =
            HashMap::from([(dependent, "Waiting for earlier work".to_owned())]);

        // The flagged member's blocker is also the row's named cause.
        assert_eq!(
            boss_outcome_attention_cause(&row).as_deref(),
            Some("missing credentials")
        );

        let entries = boss_outcome_goal_entries(&row, &sessions, &queued, now);
        assert_eq!(entries.len(), 3);

        // Admission order is the display order.
        let failure = &entries[0];
        assert_eq!(failure.key, format!("{}:{}:1", row.outcome.id, failed));
        assert!(failure.detail.contains("Failed"), "{}", failure.detail);
        assert!(
            failure.detail.contains("provider exited mid-turn"),
            "{}",
            failure.detail
        );
        assert!(!failure.destination);
        assert!(
            !failure.aria.starts_with("Open conversation"),
            "an unknown conversation does not promise navigation"
        );

        let dependent_entry = &entries[1];
        assert!(
            dependent_entry.detail.contains("Queued"),
            "{}",
            dependent_entry.detail
        );
        assert!(
            dependent_entry
                .detail
                .contains("Waiting for earlier work"),
            "{}",
            dependent_entry.detail
        );
        assert!(
            dependent_entry
                .detail
                .contains("Blocked — earlier work failed"),
            "{}",
            dependent_entry.detail
        );
        assert!(dependent_entry.destination);
        assert!(dependent_entry.aria.starts_with("Open conversation"));

        let blocked_entry = &entries[2];
        assert!(
            blocked_entry.detail.contains("blocked"),
            "{}",
            blocked_entry.detail
        );
        assert!(
            blocked_entry.detail.contains("missing credentials"),
            "{}",
            blocked_entry.detail
        );
    }

    /// The expanded view's non-attempt lines and its member joins: two
    /// running assignments share the outcome and both appear, owed
    /// handoffs and recorded waits get their own lines, a settled row
    /// nobody can describe reads as unavailable, and an empty outcome
    /// says so rather than inventing history.
    #[test]
    fn outcome_entries_cover_live_work_notes_and_unavailable_rows() {
        use waku_protocol::boss::{
            AssignmentSettle, AssignmentVerdict, EmployeeLifecycle, OutcomeState,
        };
        let now = 1_000u64;
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let ghost = Uuid::new_v4();

        // Two running assignments, one row, both visible on expansion.
        let mut row = outcome_row(
            OutcomeState::Open,
            vec![
                boss_employee(EmployeeLifecycle::Working, None),
                boss_employee(EmployeeLifecycle::Working, None),
            ],
        );
        row.members[0].employee.session_id = first;
        row.members[1].employee.session_id = second;
        row.outcome.assignments = vec![
            outcome_assignment(first, 1),
            outcome_assignment(second, 1),
            // A dangling attempt — unsettled, no roster record, nothing
            // recoverable. It reads as unavailable, never as success.
            outcome_assignment(ghost, 1),
        ];
        let mut first_session = AgentSession::new(first, ProviderKind::Codex);
        first_session.status = SessionStatus::Working;
        let mut second_session = AgentSession::new(second, ProviderKind::Codex);
        second_session.status = SessionStatus::Working;
        let sessions: HashMap<Uuid, &AgentSession> =
            HashMap::from([(first, &first_session), (second, &second_session)]);
        let queued: HashMap<Uuid, String> = HashMap::new();
        let entries = boss_outcome_goal_entries(&row, &sessions, &queued, now);
        assert_eq!(entries.len(), 3);
        for entry in &entries[..2] {
            assert!(entry.detail.contains("Working"), "{}", entry.detail);
        }
        assert_eq!(
            entries[2].detail.split(" · ").next().unwrap(),
            tr!("boss.goals_history_unavailable")
        );

        // Owed decisions and recorded pauses become their own lines.
        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        row.outcome.assignments = vec![outcome_assignment(first, 1)];
        row.outcome.handoffs.push(waku_protocol::boss::OutcomeHandoff {
            id: Uuid::new_v4(),
            assignment: first,
            attempt: 1,
            intent: "Review the result".into(),
            created_at: now - 60,
            resolution: None,
        });
        row.outcome.waiting = Some(waku_protocol::boss::OutcomeWait::Dependency {
            note: "upstream deploy".into(),
        });
        let entries = boss_outcome_goal_entries(&row, &sessions, &queued, now);
        assert!(
            entries
                .iter()
                .any(|entry| entry.title == tr!("boss.goals_handoff_pending")
                    && entry.detail.contains("Review the result")),
            "{}",
            entries
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.title.contains("upstream deploy")),
            "{}",
            entries
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        );

        // An empty outcome names the gap instead of fabricating a row.
        let row = outcome_row(OutcomeState::Open, Vec::new());
        let entries = boss_outcome_goal_entries(&row, &sessions, &queued, now);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, tr!("boss.goals_no_assignments"));
        assert!(!entries[0].destination);

        // A recorded close-out conflict gets its own note.
        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        row.outcome.assignments = vec![outcome_assignment(first, 1)];
        row.outcome.completion_conflict = Some(waku_protocol::boss::CompletionConflict {
            assignment: first,
            attempt: 1,
            reason: "evidence missing".into(),
            at: now - 30,
        });
        let entries = boss_outcome_goal_entries(&row, &sessions, &queued, now);
        let conflict = entries.last().unwrap();
        assert_eq!(conflict.title, tr!("boss.goals_conflict"));
        assert_eq!(conflict.detail, "evidence missing");

        // A settle nothing survived beyond the reference reads
        // unavailable rather than finished.
        let mut row = outcome_row(OutcomeState::Completed, Vec::new());
        let mut recovered = outcome_assignment(ghost, 1);
        recovered.settled = Some(AssignmentSettle {
            verdict: AssignmentVerdict::Unavailable,
            cause: None,
            blocked: false,
            at: None,
        });
        row.outcome.assignments = vec![recovered];
        let entries = boss_outcome_goal_entries(&row, &sessions, &queued, now);
        assert!(entries[0].detail.contains("Details unavailable"));
    }

    /// Recorded snoozes read as deliberate pauses until they elapse, and
    /// a reopened outcome leaves Finished behind without dropping a
    /// single settled attempt from its history.
    #[test]
    fn outcome_waits_reopen_and_snoozes_stay_honest() {
        use waku_protocol::boss::{AssignmentSettle, AssignmentVerdict, OutcomeState};
        let sessions: HashMap<Uuid, &AgentSession> = HashMap::new();
        let queued: HashMap<Uuid, String> = HashMap::new();
        let now = 1_000u64;

        // A snooze is the outcome's other recorded pause — it reads as
        // waiting until its own expiry, never as untracked open work.
        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        row.outcome.assignments.push(outcome_assignment(Uuid::new_v4(), 1));
        row.outcome.snoozed_until = Some(now + 600);
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Waiting)
        );
        assert_eq!(
            boss_outcome_goal_entries(&row, &sessions, &queued, now)
                .last()
                .unwrap()
                .key,
            format!("{}:wait", row.outcome.id)
        );
        // An elapsed snooze stops explaining the pause.
        row.outcome.snoozed_until = Some(now - 1);
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Open)
        );
        // A tracked wait still wins over the snooze's quieter signal.
        row.outcome.snoozed_until = Some(now + 600);
        row.outcome.waiting = Some(waku_protocol::boss::OutcomeWait::Dependency {
            note: "upstream deploy".into(),
        });
        assert_eq!(
            boss_outcome_wait_label(&row.outcome, now).as_deref(),
            Some("Waiting on upstream deploy")
        );

        // Reopening a completed outcome returns it to Pending with every
        // settled attempt intact — failed history is not rewritten.
        let mut row = outcome_row(OutcomeState::Open, Vec::new());
        let first = outcome_assignment(Uuid::new_v4(), 1);
        let mut second = outcome_assignment(Uuid::new_v4(), 1);
        second.settled = Some(AssignmentSettle {
            verdict: AssignmentVerdict::Finished,
            cause: None,
            blocked: false,
            at: Some(30),
        });
        let mut first = first;
        first.settled = Some(AssignmentSettle {
            verdict: AssignmentVerdict::Failed,
            cause: Some(waku_protocol::boss::ExpiryCause::Restarted),
            blocked: false,
            at: Some(20),
        });
        row.outcome.assignments = vec![first, second];
        // The reopen is an audited transition back to Open; the panel
        // derives everything from it.
        row.outcome.history.push(waku_protocol::boss::OutcomeTransition {
            state: OutcomeState::Completed,
            at: 40,
            actor: waku_protocol::boss::PlanActor::Boss,
        });
        row.outcome.history.push(waku_protocol::boss::OutcomeTransition {
            state: OutcomeState::Open,
            at: 50,
            actor: waku_protocol::boss::PlanActor::Boss,
        });
        // The unrostered failure keeps the row flagged — reopening does
        // not erase it. The flag itself derives in `boss_outcome_attention`,
        // covered by `outcome_attention_outlives_the_roster_record`.
        row.attention = true;
        assert_eq!(
            boss_outcome_status(&row, &sessions, now),
            (BossGoalBucket::Pending, BossGoalStatus::Blocked)
        );
        assert_eq!(
            boss_outcome_attention_cause(&row).as_deref(),
            Some("interrupted by a daemon restart")
        );
        let entries = boss_outcome_goal_entries(&row, &sessions, &queued, now);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].detail.contains("Failed"));
        assert!(
            entries[0].detail.contains("interrupted by a daemon restart"),
            "{:?}",
            entries[0].detail
        );
        assert!(entries[1].detail.contains("Finished"));
    }
}

impl Waku {
    pub(super) fn drain_boss_browse_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok((key, _request_id, session_id, url, title)) =
            self.boss_browse_events.try_recv()
        {
            if self.daemons.session_owner(session_id) != key
                || !self.boss_ui.managed.contains(&session_id)
            {
                continue;
            }
            self.request_session_activation(session_id, SessionActivationTransition::Visit, cx);
            self.pending_boss_browse.push_back((session_id, url, title));
            changed = true;
        }
        changed
    }

    pub(super) fn open_transcript_link(&mut self, target: &str, cx: &mut Context<Self>) -> bool {
        if let Some((context, target)) = crate::model::ReferenceContext::decode_reference(target) {
            let Some(path) = reference_file_link_path(&target) else {
                return true;
            };
            let Some(client) = self.workspace_client_for_path(context.workspace()) else {
                return true;
            };
            let session_id = self.selected_session().map(|session| session.id);
            let location = file_link_location(&target);
            let heading = markdown_file_link_heading(&target);
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        client.request(
                            waku_protocol::workspace::WorkspaceOperation::ResolveReferenceFile {
                                context,
                                path,
                            },
                        )
                    })
                    .await;
                let _ = this.update(cx, |this, cx| match result {
                    Ok(waku_protocol::workspace::WorkspaceResult::ReferenceWorkspace { path }) => {
                        if this.selected_session().map(|session| session.id) != session_id {
                            return;
                        }
                        let mut target = path.to_string_lossy().into_owned();
                        if let Some((line, column)) = location {
                            target.push_str(&format!(":{line}"));
                            if let Some(column) = column {
                                target.push_str(&format!(":{column}"));
                            }
                        } else if let Some(heading) = heading {
                            target.push_str(&format!("#{heading}"));
                        }
                        this.open_transcript_link(&target, cx);
                    }
                    Err(error) => {
                        this.show_toast(error.to_string());
                        cx.notify();
                    }
                    _ => {}
                });
            })
            .detach();
            return true;
        }
        let files_root = self.resolve_right_panel_files_root(cx);
        let route = transcript_link_route(target, files_root.as_deref());
        let linked_path = match &route {
            TranscriptLinkRoute::ProjectFile(relative_path, _) => {
                files_root.as_ref().map(|root| root.join(relative_path))
            }
            TranscriptLinkRoute::Finder(path) => Some(path.clone()),
            TranscriptLinkRoute::Task(..) | TranscriptLinkRoute::External => None,
        };
        if linked_path
            .as_deref()
            .is_some_and(|path| self.is_quarantined_transfer_path(path))
        {
            self.show_toast(tr!("friends.transfer_open_quarantined"));
            cx.notify();
            return true;
        }
        match route {
            TranscriptLinkRoute::ProjectFile(relative_path, heading) => {
                // A `file:line` target rides the same pending slot the finder
                // uses: the editor takes focus and the jump lands once the
                // file's first read does.
                let location = file_link_location(target);
                self.open_right_panel_surface(RightPanelSurface::Files, cx);
                self.open_right_panel_file(relative_path.clone(), cx);
                self.right_panel_file_tree_visible = false;
                cx.notify();
                if location.is_some() || heading.is_some() {
                    self.right_panel_pending_file_focus = Some(PendingFileFocus {
                        path: relative_path,
                        position: location.map(|(line, column)| (line, column.unwrap_or(1))),
                        heading,
                    });
                }
            }
            TranscriptLinkRoute::Finder(path) => {
                if self.is_remote_path(&path) {
                    self.show_toast(tr!("errors.remote_host_path"));
                    cx.notify();
                } else {
                    crate::platform::reveal_in_file_manager(&path, cx);
                }
            }
            TranscriptLinkRoute::Task(task_id, message_id) => {
                let known = task_id
                    .is_some_and(|id| self.state.sessions.iter().any(|session| session.id == id));
                match (task_id, known) {
                    (Some(id), true) => {
                        // A `?message=` link reveals its row on landing —
                        // the needle is unknown here, so the flash carries
                        // no glyph washes, just the row.
                        if let Some(message_id) = message_id {
                            self.pending_transcript_match = Some(PendingTranscriptMatch {
                                session_id: id,
                                message_id,
                                query: String::new(),
                            });
                        }
                        self.select_session(id, cx)
                    }
                    _ => {
                        self.show_toast(tr!("errors.task_link_unknown"));
                        cx.notify();
                    }
                }
            }
            TranscriptLinkRoute::External => return false,
        }
        true
    }

    fn is_quarantined_transfer_path(&self, target: &Path) -> bool {
        let Some(session) = self.selected_session() else {
            return false;
        };
        if session.friend_peer_id.is_none() || !session.quarantined {
            return false;
        }
        let Some(delivery_directory) =
            session
                .messages
                .iter()
                .find_map(|message| match message.notice.as_ref() {
                    Some(crate::model::TranscriptNotice::TransferReceived { path, .. }) => {
                        path.parent().map(Path::to_path_buf)
                    }
                    _ => None,
                })
        else {
            return false;
        };
        let target = if target.is_absolute() {
            target.to_path_buf()
        } else if let Some(workspace) = self.selected_workspace_path() {
            workspace.join(target)
        } else {
            return false;
        };
        target.starts_with(delivery_directory)
    }

    /// The link-specific actions a right-clicked URL contributes before the
    /// surface's usual context-menu items. Web destinations also offer the
    /// built-in browser and, where the default browser has a known incognito
    /// flag, a private window; other schemes keep the same open and copy
    /// every destination gets. The copy item names what it puts on the
    /// clipboard: "Copy URL" for link targets, "Copy file path" when the
    /// target resolves to a file.
    pub(super) fn transcript_link_menu_items(
        &self,
        url: &str,
        cx: &mut Context<Self>,
    ) -> Vec<MenuItem> {
        let open_target = url.to_owned();
        let waku = cx.entity().downgrade();
        let mut items = vec![MenuItem::new(tr!("common.open_link"), move |_, cx| {
            let handled = waku
                .update(cx, |this, cx| this.open_transcript_link(&open_target, cx))
                .unwrap_or(false);
            if !handled {
                cx.open_url(&open_target);
            }
        })];
        let web =
            url::Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"));
        if web {
            let tab_target = url.to_owned();
            let waku = cx.entity().downgrade();
            items.push(MenuItem::new(
                tr!("common.open_link_in_browser_tab"),
                move |window, cx| {
                    let _ = waku.update(cx, |this, cx| {
                        this.settings_page = None;
                        this.open_url_in_browser_tab(tab_target.clone(), window, cx);
                    });
                },
            ));
            if crate::platform::can_open_url_in_private_window() {
                let private_target = url.to_owned();
                items.push(MenuItem::new(
                    tr!("common.open_link_in_private_window"),
                    move |_, _| crate::platform::open_url_in_private_window(&private_target),
                ));
            }
        }
        let unscoped =
            crate::model::ReferenceContext::decode_reference(url).map(|(_, target)| target);
        let (copy_target, copy_key) = transcript_link_copy(unscoped.as_deref().unwrap_or(url));
        items.push(MenuItem::new(tr!(copy_key), move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copy_target.clone()));
        }));
        items
    }

    /// Open a path a tool reported, from an activity in the transcript.
    ///
    /// Providers name a changed file however they like — absolute, or relative
    /// to the session's workspace — so resolve it before routing. Inside the
    /// workspace it opens in the file viewer; anywhere else it goes to the file
    /// manager, the same split a file link in the transcript takes.
    pub(super) fn open_activity_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let path = Path::new(path.trim());
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(root) = self.resolve_right_panel_files_root(cx) {
            root.join(path)
        } else {
            return;
        };
        self.open_transcript_link(&resolved.to_string_lossy(), cx);
    }

    /// Open a named file in its OS default app — the user's editor for
    /// source, Preview for images, Finder for directories.
    ///
    /// Paths resolve the same way [`Self::open_activity_file`] does:
    /// absolute paths are taken as-is, anything else joins the selected
    /// workspace. A remote host's path cannot be opened locally, so it
    /// toasts instead of silently doing nothing.
    pub(super) fn open_path_in_default_app(&mut self, path: &str, cx: &mut Context<Self>) {
        let path = Path::new(path.trim());
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(root) = self.resolve_right_panel_files_root(cx) {
            root.join(path)
        } else {
            return;
        };
        if self.is_remote_path(&resolved) {
            self.show_toast(tr!("errors.remote_host_path"));
            cx.notify();
            return;
        }
        crate::platform::open_with_default_app(&resolved, cx);
    }

    /// Route a trusted delivery entry through the right-panel file preview or
    /// the OS default app. A directory opens in the existing Files tree and
    /// starts expanded at the clicked path.
    pub(super) fn open_transfer_path(
        &mut self,
        session_id: Uuid,
        message_id: Uuid,
        path: PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((current_session_id, current_message_id, payload_path, payload_is_dir)) =
            self.selected_transfer_notice()
        else {
            return;
        };
        if current_session_id != session_id || current_message_id != message_id {
            return;
        }

        let allowed_path = if payload_is_dir {
            path.strip_prefix(&payload_path).is_ok_and(|relative| {
                (path != payload_path || is_dir)
                    && relative
                        .components()
                        .all(|component| matches!(component, Component::Normal(_)))
            })
        } else {
            path == payload_path && !is_dir
        };
        if !allowed_path {
            return;
        }

        let files_root = if payload_is_dir {
            payload_path
        } else {
            let Some(parent) = payload_path.parent() else {
                return;
            };
            parent.to_path_buf()
        };
        if self.is_remote_path(&path) {
            self.show_toast(tr!("errors.remote_host_path"));
            cx.notify();
            return;
        }

        self.sync_right_panel_files_root(cx);
        if self.right_panel_files_root.as_deref() != Some(files_root.as_path()) {
            return;
        }
        if is_dir {
            if path != files_root {
                self.right_panel_expanded_paths.insert(path);
            }
            self.open_right_panel_surface(RightPanelSurface::Files, cx);
            return;
        }

        if transfer_file_is_previewable(&path)
            && let Ok(relative_path) = path.strip_prefix(&files_root)
            && let Some(relative_path) = relative_path.to_str()
        {
            self.open_right_panel_file(relative_path.to_owned(), cx);
        } else {
            crate::platform::open_with_default_app(&path, cx);
        }
    }

    fn selected_transfer_notice(&self) -> Option<(Uuid, Uuid, PathBuf, bool)> {
        let session = self.selected_session()?;
        if session.friend_peer_id.is_none()
            || session.quarantined
            || !session.detail_loaded
            || self.is_remote_session(session.id)
        {
            return None;
        }
        session
            .messages
            .iter()
            .find_map(|message| match message.notice.as_ref() {
                Some(crate::model::TranscriptNotice::TransferReceived { path, is_dir, .. }) => {
                    Some((session.id, message.id, path.clone(), *is_dir))
                }
                _ => None,
            })
    }

    /// Which place the live strip belongs to right now, derived from the
    /// same flags `navigation_location` reads — a page beats the selection
    /// parked underneath it.
    pub(super) fn active_right_panel_owner(&self) -> RightPanelOwner {
        if let Some(key) = self.boss_chat_key() {
            RightPanelOwner::Boss(key)
        } else if let Some((key, _)) = self.boss_ui.page {
            RightPanelOwner::Boss(key)
        } else if self.notifications.open {
            RightPanelOwner::Inbox
        } else if self.automations_page {
            RightPanelOwner::Automations
        } else if self.drafts_page {
            RightPanelOwner::Drafts
        } else if let Some(project_id) = self.projects_page {
            RightPanelOwner::Projects(project_id)
        } else if let Some(session_id) = self.state.selected_session {
            RightPanelOwner::Session(session_id)
        } else if let Some(terminal_id) = self.selected_terminal {
            RightPanelOwner::Terminal(terminal_id)
        } else {
            RightPanelOwner::Bare
        }
    }

    /// Park the live strip under its owner and mount the incoming owner's —
    /// the panel is context property, so a transition leaves every place's
    /// tabs and visibility exactly as they were left. A no-op when the owner
    /// did not change; call after the flags that decide the owner settle.
    pub(super) fn sync_right_panel_owner(&mut self, cx: &mut Context<Self>) {
        if self.sync_voice_briefing_navigation() {
            if let Some(session_id) = self.state.selected_session {
                self.maybe_voice_brief(session_id, cx);
            }
        }
        let owner = self.active_right_panel_owner();
        if owner == self.right_panel_live_owner {
            self.sync_boss_tasks_panel(cx);
            return;
        }
        let parked = self.take_active_right_panel_state();
        self.right_panel_states
            .insert(self.right_panel_live_owner, parked);
        let incoming = RightPanelSessionState::take_or_closed(&mut self.right_panel_states, owner);
        self.right_panel_live_owner = owner;
        self.restore_right_panel_state(incoming, cx);
        self.sync_boss_tasks_panel(cx);
    }

    /// Ensure the Boss chat's Tasks tab exists. It is strip furniture that
    /// is reconstructed on restore and remains present while user tabs come
    /// and go. Boss pages do not claim the chat's strip.
    pub(super) fn sync_boss_tasks_panel(&mut self, cx: &mut Context<Self>) {
        let Some(_key) = self.boss_chat_key().filter(|_| self.boss_ui.page.is_none()) else {
            return;
        };
        let Some(first_tab) = ensure_boss_tasks_tab(
            &mut self.right_panel_surfaces,
            &mut self.right_panel_active_surface,
        ) else {
            return;
        };
        if first_tab {
            if self.right_panel_width == DEFAULT_RIGHT_PANEL_WIDTH {
                self.right_panel_width = RIGHT_PANEL_MIN_WIDTH;
            }
            self.set_right_panel_visible(true, cx);
        }
        cx.notify();
    }

    /// Whether the live strip belongs to a boss-managed session — an
    /// employee's, or the boss's own chat. (The Boss page's strip still
    /// admits nothing.)
    pub(super) fn managed_panel_owner(&self) -> bool {
        match self.active_right_panel_owner() {
            RightPanelOwner::Session(id) => self.boss_ui.managed.contains(&id),
            RightPanelOwner::Boss(_) => {
                self.boss_ui.page.is_none() && self.boss_chat_key().is_some()
            }
            _ => false,
        }
    }

    /// The experimental Git panel is available in employee sessions, whose
    /// strip otherwise follows the managed-session surface rules. The boss's
    /// own chat remains restricted to its managed panel surfaces.
    pub(super) fn git_panel_owner_allowed(&self) -> bool {
        if !self.managed_panel_owner() {
            return true;
        }
        let RightPanelOwner::Session(id) = self.active_right_panel_owner() else {
            return false;
        };
        self.state
            .sessions
            .iter()
            .find(|session| session.id == id)
            .is_some_and(|session| self.session_is_employee(session))
    }

    /// What the current owner lets into its strip: sessions and main-area
    /// terminals take everything except Boss-only goals, employees take the
    /// same surfaces as other sessions except goals, a project page takes its
    /// issue/PR details and files rooted at the project, and pages without
    /// their own surface take nothing.
    fn right_panel_owner_allows(&self, surface: &RightPanelSurface) -> bool {
        let owner = self.active_right_panel_owner();
        if let RightPanelOwner::Session(id) = owner {
            if self.boss_ui.managed.contains(&id) {
                return session_panel_surface(surface);
            }
        }
        if self.managed_panel_owner() && !self.git_panel_owner_allowed() {
            return match surface {
                RightPanelSurface::Goals => {
                    self.boss_ui.page.is_none() && self.boss_chat_key().is_some()
                }
                _ => managed_panel_surface(surface),
            };
        }
        match owner {
            RightPanelOwner::Session(_) | RightPanelOwner::Terminal(_) | RightPanelOwner::Bare => {
                session_panel_surface(surface)
            }
            RightPanelOwner::Projects(_) => matches!(
                surface,
                RightPanelSurface::GitHub(_)
                    | RightPanelSurface::Files
                    | RightPanelSurface::File(_)
                    | RightPanelSurface::FileAtRef { .. }
            ),
            RightPanelOwner::Inbox => matches!(surface, RightPanelSurface::GitHub(_)),
            RightPanelOwner::Boss(key) => {
                self.boss_ui.page.is_none()
                    && self.boss_chat_key() == Some(key)
                    && matches!(surface, RightPanelSurface::Goals)
            }
            RightPanelOwner::Drafts | RightPanelOwner::Automations => false,
        }
    }

    pub(super) fn restore_right_panel_state(
        &mut self,
        mut state: RightPanelSessionState,
        cx: &mut Context<Self>,
    ) {
        // A strip parked before its owner became managed — or written by a
        // build without the gate — sheds whatever managed sessions cannot
        // host rather than re-mounting a disallowed tab.
        if self.managed_panel_owner() && !self.git_panel_owner_allowed() {
            let active = state
                .active_surface
                .and_then(|index| state.surfaces.get(index).cloned());
            state
                .surfaces
                .retain(|surface| self.right_panel_owner_allows(surface));
            state.active_surface = active
                .and_then(|surface| state.surfaces.iter().position(|entry| *entry == surface));
            state.git_panel_open = false;
            state.git_panel = None;
            state.git_panel_commit_diff = None;
            state.drop_dead_fullscreen();
        }
        self.replace_active_right_panel_state(state);
        // A planning session's plan tab is strip furniture, not a user tab —
        // restore inserts it before whatever the parked strip holds, but only
        // once the fetched document has real contents: until the session's
        // first write lands, the strip stays without the tab rather than
        // opening the panel on a loading screen. The fetch below re-arms on
        // each Boss state revision, so the write mounts the tab one sync
        // later. A strip meeting its session for the first time opens on the
        // plan; a parked one gains the tab without moving the user's
        // selection.
        if let RightPanelOwner::Session(session_id) = self.right_panel_live_owner
            && let Some(plan_file) = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| session.planning.as_ref())
                .map(|planning| planning.plan_file.clone())
        {
            if self
                .plan_docs
                .get(&session_id)
                .is_some_and(PlanDoc::has_content)
            {
                self.mount_plan_tab(session_id, plan_file);
            } else {
                let key = self.daemons.session_owner(session_id);
                self.ensure_plan_doc(key, session_id, &plan_file, true, cx);
            }
        }
        // A Git panel that parked with the strip comes back whole — refresh
        // it for whatever moved underneath while it was away, since fetches
        // in flight at park time failed their landing check. A relaunch
        // restore carries only the flag, so the panel rebuilds on the window
        // a tick out — without claiming focus like a user-opened one does.
        if self.git_panel_visible {
            if self.git_panel.is_some() {
                if let Some(panel) = self.git_panel.as_mut() {
                    panel.snapshot_loading = false;
                    panel.commits_loading = false;
                    panel.upstream_commits_loading = false;
                }
                self.refresh_git_panel(cx);
                self.refresh_git_panel_commits(cx);
            } else {
                let waku = cx.entity();
                let window_handle = self.window_handle;
                cx.defer(move |cx| {
                    let _ = window_handle.update(cx, move |_, window, cx| {
                        let _ = waku.update(cx, move |this, cx| {
                            // A faster second swap may have parked a real
                            // panel or closed the slot again — only rebuild
                            // when the flag still stands with none mounted.
                            if this.git_panel_visible && this.git_panel.is_none() {
                                this.open_git_panel(window, cx, false);
                            }
                        });
                    });
                });
            }
        }
        // The draft restored ahead of this swap holds the session's file
        // annotations. Hand each returning editor its share; an editor whose
        // path has none keeps an empty set — the draft is the authority, not
        // whatever the parked store still held.
        for (path, editor) in self.right_panel_file_editors.iter_mut() {
            editor.annotations.borrow_mut().items = self
                .pending_file_annotations
                .remove(path)
                .unwrap_or_default();
        }
        self.sync_right_panel_diff_tree_rows(cx);
        // A read in flight when this session was switched away from had its
        // result dropped, and the flag it left behind would stop the editor
        // ever asking again. Clear it and read afresh, which also picks up
        // edits made while another session was on screen.
        for editor in self.right_panel_file_editors.values_mut() {
            editor.reading = false;
            // A `path:line` jump that never landed dies with the swap too —
            // firing it sessions later would look like the caret moved on
            // its own.
            editor.pending_position = None;
        }
        // A blob read dropped by the swap left `requested` armed; clearing
        // it lets the restored surface's render ask again. An editor whose
        // read already landed just re-fetches the same blob.
        for editor in self.right_panel_ref_editors.values_mut() {
            editor.requested = false;
        }
        // The find bar pointed into the editors that were just swapped out;
        // its match list means nothing here, and restored editors may carry
        // washes stored mid-search. A pending finder focus handoff names one
        // of the outgoing editors too.
        self.reset_file_search_for_session(cx);
        self.reset_go_to_line_for_session(cx);
        self.right_panel_pending_file_focus = None;
        // A reveal armed just before the swap names a path in the incoming
        // session's tree, not this one's — drop it rather than scroll to a
        // coincidental same-named row.
        self.right_panel_pending_tree_reveal = None;
        self.right_panel_tree_scroll_to = None;
        self.reload_clean_right_panel_file_editors(cx);
        // Visibility persists as the next launch's default only for owners
        // that actually host a panel — an Automations visit must not write
        // the hidden strip it mounts over the user's last real value.
        if !matches!(
            self.right_panel_live_owner,
            RightPanelOwner::Boss(_)
                | RightPanelOwner::Drafts
                | RightPanelOwner::Automations
                | RightPanelOwner::Inbox
        ) {
            self.state.right_panel_visible = self.right_panel_visible;
        }
        if self.active_right_panel_surface() == Some(&RightPanelSurface::Diff) {
            self.refresh_right_panel_diff(cx);
        }
        if matches!(
            self.active_right_panel_surface(),
            Some(RightPanelSurface::Files | RightPanelSurface::File(_))
        ) {
            self.refresh_right_panel_working_tree(cx);
        }
        self.ensure_right_panel_terminals(cx);
        // A restored state's recorded root may no longer resolve — a
        // worktree moved, or the detached strip comes back to a terminal
        // that has since `cd`'d. Reconcile before anything reads the slice.
        self.sync_right_panel_files_root(cx);
        self.retain_right_panel_browsers();
        // A side-chat tab persists only while its session does — an unsent
        // one is never catalogued, and one deleted elsewhere stays gone.
        let dead: Vec<Uuid> = self
            .right_panel_surfaces
            .iter()
            .filter_map(|surface| match surface {
                RightPanelSurface::SideChat(id) => {
                    (!self.state.sessions.iter().any(|session| session.id == *id)).then_some(*id)
                }
                _ => None,
            })
            .collect();
        for id in dead {
            self.remove_side_chat_surface(id, cx);
        }
        if self.right_panel_visible {
            self.request_active_terminal_focus();
            self.request_active_browser_focus();
        }
    }

    /// Inserts a planning session's plan tab ahead of the strip's user tabs —
    /// furniture, not a user tab. A strip meeting its session for the first
    /// time opens on the plan; one already holding tabs gains the tab without
    /// moving the selection. A no-op unless the session owns the live strip
    /// and the tab is not already mounted — callers gate the mount on the
    /// fetched document holding real contents. Returns whether the strip
    /// changed so a landing read can skip repainting an identical doc.
    fn mount_plan_tab(&mut self, session_id: Uuid, plan_file: String) -> bool {
        if self.right_panel_live_owner != RightPanelOwner::Session(session_id) {
            return false;
        }
        let surface = RightPanelSurface::Plan {
            session_id,
            plan_file,
        };
        if self.right_panel_surfaces.contains(&surface) {
            return false;
        }
        let first_visit = self.right_panel_surfaces.is_empty();
        self.right_panel_surfaces.insert(0, surface);
        self.right_panel_active_surface = if first_visit {
            Some(0)
        } else {
            self.right_panel_active_surface.map(|index| index + 1)
        };
        if first_visit {
            self.right_panel_visible = true;
        }
        true
    }

    pub(super) fn remove_right_panel_session_state(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let state = if self.right_panel_live_owner == RightPanelOwner::Session(session_id) {
            let state = self.take_active_right_panel_state();
            self.replace_active_right_panel_state(RightPanelSessionState::empty(false));
            // The emptied strip has no owner — parking it under the dead
            // session would leave a junk entry behind.
            self.right_panel_live_owner = RightPanelOwner::Bare;
            Some(state)
        } else {
            self.right_panel_states
                .remove(&RightPanelOwner::Session(session_id))
        };
        if let Some(state) = state {
            for surface in &state.surfaces {
                if let Some(terminal_id) = surface.terminal_id() {
                    self.drop_terminal(terminal_id, cx);
                }
                if let Some(browser_id) = surface.browser_id() {
                    self.right_panel_browsers.remove(&browser_id);
                }
                if let RightPanelSurface::GitHub(project_id) = surface
                    && let Some(browser) = self.github_browsers.get_mut(project_id)
                {
                    browser.detail = None;
                }
                if let RightPanelSurface::SideChat(id) = surface {
                    self.side_chat_views.remove(id);
                    self.side_chat_composers.remove(id);
                }
            }
        }
        self.plan_docs.remove(&session_id);
        self.plan_annotations.remove(&session_id);
        self.right_panel_pr_states
            .retain(|(owner, _), _| *owner != session_id);
    }

    fn take_active_right_panel_state(&mut self) -> RightPanelSessionState {
        // Parked file annotations keep their items — the pinned highlights
        // belong to the session — but the transient hover/editing flags must
        // not resurface pointing at an editor that closed during the swap.
        for editor in self.right_panel_file_editors.values() {
            let mut annotations = editor.annotations.borrow_mut();
            annotations.hovered = None;
            annotations.editing = None;
        }
        // A parked strip keeps only what cannot be re-derived: dirty buffers
        // and annotation pins are user data, while a clean editor is a disk
        // read away — the surface's lazy `ensure_right_panel_file_editor`
        // recreates it on return, same as a relaunch restore. Ref editors
        // and the parsed diff shed the same way; activating the Diff tab
        // refetches a missing snapshot.
        let file_editors = std::mem::take(&mut self.right_panel_file_editors)
            .into_iter()
            .filter(|(_, editor)| editor.dirty || !editor.annotations.borrow().items.is_empty())
            .collect();
        self.right_panel_ref_editors.clear();
        self.right_panel_diff_snapshot = None;
        // The Git panel is this owner's too: park its live state — draft
        // message included — and shed the hover/prompt leftovers the same
        // way an outright close does.
        let git_panel_open = self.git_panel_visible;
        let git_panel = self.git_panel.take();
        let git_panel_commit_diff = self.git_panel_commit_diff.take();
        self.close_git_panel_state();
        RightPanelSessionState {
            visible: self.right_panel_visible,
            surfaces: std::mem::take(&mut self.right_panel_surfaces),
            active_surface: self.right_panel_active_surface.take(),
            last_focused_terminal: self.right_panel_last_focused_terminal.take(),
            tabs_scroll_handle: std::mem::replace(
                &mut self.right_panel_tabs_scroll_handle,
                ScrollHandle::new(),
            ),
            pending_tab_reveal: self.right_panel_pending_tab_reveal.take(),
            expanded_paths: std::mem::take(&mut self.right_panel_expanded_paths),
            files_selected_path: self.right_panel_files_selected_path.take(),
            file_tree_width: self.right_panel_file_tree_width,
            file_tree_visible: self.right_panel_file_tree_visible,
            file_editors,
            ref_editors: HashMap::new(),
            files_root: self.right_panel_files_root.take(),
            diff_source: self.right_panel_diff_source,
            diff_snapshot: None,
            diff_selected_file: self.right_panel_diff_selected_file.take(),
            diff_expanded_paths: std::mem::take(&mut self.right_panel_diff_expanded_paths),
            git_panel_open,
            git_panel,
            git_panel_commit_diff,
            // The maximized surface is this strip's too — it parks with the
            // owner instead of ending on the swap.
            fullscreen: self.fullscreen_surface.take(),
        }
    }

    fn replace_active_right_panel_state(&mut self, state: RightPanelSessionState) {
        // The incoming owner's remembered maximized surface takes over —
        // usually none. A stale entry, like a tab that closed while parked,
        // is reconciled per frame in `settle_panel_slides`.
        self.fullscreen_surface = state.fullscreen;
        self.panel_fullscreen_slide = None;
        self.right_panel_visible = state.visible;
        if state.visible {
            // A restored-visible panel wins the slot back from the Git panel.
            self.close_git_panel_state();
        } else if self.state.git_panel_enabled {
            // The slot's other tenant: this owner's parked Git panel comes
            // back — flag, live state, and open commit view together.
            self.git_panel_visible = state.git_panel_open;
            if state.git_panel_open {
                self.git_panel = state.git_panel;
                self.git_panel_commit_diff = state.git_panel_commit_diff;
            }
        }
        self.right_panel_surfaces = state.surfaces;
        self.right_panel_active_surface = state.active_surface;
        self.right_panel_last_focused_terminal = state.last_focused_terminal;
        self.right_panel_tabs_scroll_handle = state.tabs_scroll_handle;
        self.right_panel_pending_tab_reveal = state.pending_tab_reveal;
        self.right_panel_expanded_paths = state.expanded_paths;
        self.right_panel_files_selected_path = state.files_selected_path;
        self.right_panel_file_tree_width = state.file_tree_width;
        self.right_panel_file_tree_visible = state.file_tree_visible;
        self.right_panel_file_editors = state.file_editors;
        self.right_panel_ref_editors = state.ref_editors;
        self.right_panel_files_root = state.files_root;
        self.right_panel_diff_generation = self.right_panel_diff_generation.wrapping_add(1);
        self.right_panel_diff_selection.clear();
        self.right_panel_diff_source = state.diff_source;
        self.right_panel_diff_snapshot = state.diff_snapshot;
        self.right_panel_diff_loading = false;
        self.right_panel_diff_error = None;
        self.right_panel_diff_selected_file = state.diff_selected_file;
        self.right_panel_diff_expanded_paths = state.diff_expanded_paths;
        self.right_panel_diff_tree_cursor = None;
        self.right_panel_diff_tree_rows.borrow_mut().clear();
        self.right_panel_diff_tree_list_state.reset(0);
        let line_count = self
            .right_panel_diff_snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.lines.len());
        self.right_panel_diff_list_state.reset(line_count);
    }

    /// Drop a project's work-item tab from every parked strip's surface
    /// list. The detail it renders is keyed by project, so a second copy in
    /// another owner's strip would show the same item. The active strip's
    /// copy — if any — is the caller's to handle.
    pub(super) fn remove_parked_github_surfaces(&mut self, project_id: Uuid) {
        for state in self.right_panel_states.values_mut() {
            let Some(index) = state.surfaces.iter().position(
                |surface| matches!(surface, RightPanelSurface::GitHub(id) if *id == project_id),
            ) else {
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
        }
    }

    pub(super) fn reveal_right_panel_tab(&mut self, index: usize) {
        self.right_panel_pending_tab_reveal = Some(index);
        self.right_panel_tabs_scroll_handle.scroll_to_item(index);
    }

    pub(super) fn active_right_panel_surface(&self) -> Option<&RightPanelSurface> {
        self.right_panel_active_surface
            .and_then(|index| self.right_panel_surfaces.get(index))
    }

    pub(super) fn request_active_terminal_focus(&mut self) {
        self.right_panel_pending_terminal_focus = self
            .active_right_panel_surface()
            .and_then(RightPanelSurface::terminal_id);
    }

    /// The panel terminal currently holding keyboard focus, if the strip's
    /// active tab is one — ⌘T's "stay in the panel" signal. A terminal tab
    /// that is merely active while focus sits elsewhere (composer, sidebar)
    /// does not count.
    pub(super) fn focused_right_panel_terminal(&self, window: &Window, cx: &App) -> Option<Uuid> {
        if !self.right_panel_visible {
            return None;
        }
        let terminal_id = self.active_right_panel_surface()?.terminal_id()?;
        self.right_panel_terminals
            .get(&terminal_id)
            .is_some_and(|terminal| terminal.read(cx).focus_handle(cx).is_focused(window))
            .then_some(terminal_id)
    }

    pub(super) fn request_active_browser_focus(&mut self) {
        self.right_panel_pending_browser_focus = self
            .active_right_panel_surface()
            .and_then(RightPanelSurface::browser_id);
    }

    /// The panel-open half of the file slot: the active editor's input takes
    /// focus on the first frame it mounts — `position: None`, so no caret
    /// jump rides along. An in-flight `file:line` request wins the slot.
    pub(super) fn request_active_file_focus(&mut self) {
        if self.right_panel_pending_file_focus.is_none()
            && let Some(path) = self.visible_right_panel_file_path()
        {
            self.right_panel_pending_file_focus = Some(PendingFileFocus {
                path,
                position: None,
                heading: None,
            });
        }
    }

    /// `secondary-j`: put keyboard focus back in this session's terminal —
    /// the one that last had it, the most recently opened one otherwise, or a
    /// fresh surface when the session has no terminal yet. The panel opens if
    /// it was hidden; without a selected session there is no working
    /// directory to spawn into, so the chord does nothing.
    ///
    /// When the terminal already holds focus the chord becomes a toggle: the
    /// panel hides (surfaces and their sessions keep running) and focus
    /// returns to the composer.
    pub(super) fn focus_terminal_action(
        &mut self,
        _: &FocusTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        if self.state.selected_session.is_none() {
            // Terminal mode owns the main area; ⌘J refocuses the selected
            // terminal there rather than opening the panel's strip.
            if let Some(terminal_id) = self.selected_terminal
                && let Some(terminal) = self.right_panel_terminals.get(&terminal_id)
            {
                let focus = terminal.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            cx.notify();
            return;
        }
        let terminal_focused = self.focused_right_panel_terminal(window, cx).is_some();
        if terminal_focused {
            self.set_right_panel_visible(false, cx);
            let focus_handle = self.composer_focus(cx);
            window.focus(&focus_handle, cx);
            cx.notify();
            return;
        }
        let target = self
            .right_panel_last_focused_terminal
            .and_then(|terminal_id| {
                self.right_panel_surfaces
                    .iter()
                    .position(|surface| surface.terminal_id() == Some(terminal_id))
            })
            .or_else(|| {
                self.right_panel_surfaces
                    .iter()
                    .rposition(|surface| surface.terminal_id().is_some())
            });
        if let Some(index) = target {
            if let Some(terminal_id) = self.right_panel_surfaces[index].terminal_id() {
                self.ensure_right_panel_terminal(terminal_id, cx);
            }
            self.right_panel_active_surface = Some(index);
            self.reveal_right_panel_tab(index);
            self.request_active_terminal_focus();
            self.set_right_panel_visible(true, cx);
        } else {
            self.open_right_panel_surface(RightPanelSurface::new_terminal(), cx);
        }
        cx.notify();
    }

    /// The file surface shows `relative_path`'s rendered markdown preview
    /// rather than its source editor. Deliverable pages use a per-file
    /// reading preference; other files use the global toggle. Callers use this to
    /// pick which selection surface (and which annotation anchors) a file
    /// action should read.
    pub(super) fn file_markdown_preview_active(&self, relative_path: &str) -> bool {
        let deliverable_page = self.live_deliverable_page().is_some_and(|(key, id)| {
            self.boss_ui.states.get(&key).is_some_and(|state| {
                state.deliverables.iter().any(|deliverable| {
                    deliverable.id == id
                        && Path::new(&deliverable.path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            == Some(relative_path)
                })
            })
        });
        markdown_preview_mode(
            file_highlighter_language(relative_path) == "markdown",
            deliverable_page,
            self.right_panel_file_editors
                .get(relative_path)
                .is_some_and(|editor| editor.show_source),
            self.state.markdown_preview,
        )
    }

    /// The file the active editor surface is showing, whether via a File tab
    /// or the Files browser's selection — regardless of whether the panel is
    /// currently visible, which is a per-caller decision: save works on a
    /// hidden panel, find does not.
    pub(super) fn visible_right_panel_file_path(&self) -> Option<String> {
        match self.active_right_panel_surface() {
            Some(RightPanelSurface::Files) => self.right_panel_files_selected_path.clone(),
            Some(RightPanelSurface::File(path)) => Some(path.clone()),
            _ => None,
        }
    }

    fn right_panel_file_is_dirty(&self, relative_path: &str) -> bool {
        self.right_panel_file_editors
            .get(relative_path)
            .is_some_and(|editor| editor.dirty)
    }

    fn right_panel_surface_is_dirty(&self, surface: &RightPanelSurface) -> bool {
        match surface {
            RightPanelSurface::Files => self
                .right_panel_files_selected_path
                .as_deref()
                .is_some_and(|path| self.right_panel_file_is_dirty(path)),
            RightPanelSurface::File(path) => self.right_panel_file_is_dirty(path),
            _ => false,
        }
    }

    fn ensure_initial_right_panel_file_editor_width(&mut self) {
        if self.right_panel_file_editors.is_empty() {
            self.right_panel_width = widened_panel_width_for_file_editor(
                self.right_panel_width,
                self.right_panel_file_tree_width,
            );
        }
    }

    pub(super) fn open_right_panel_surface(
        &mut self,
        surface: RightPanelSurface,
        cx: &mut Context<Self>,
    ) {
        self.add_right_panel_surface(surface, true, cx);
    }

    /// `open_right_panel_surface` split on whether the panel should reveal:
    /// custom commands add their terminal quietly so the run can report
    /// through a toast instead of a slide-open.
    fn add_right_panel_surface(
        &mut self,
        surface: RightPanelSurface,
        reveal: bool,
        cx: &mut Context<Self>,
    ) {
        // The active owner decides what its strip may host — a page that
        // takes nothing leaves the request dead rather than leaking a tab.
        if !self.right_panel_owner_allows(&surface) {
            return;
        }
        let reusable_index = reusable_surface_index(&self.right_panel_surfaces, &surface);
        if matches!(
            &surface,
            RightPanelSurface::File(_) | RightPanelSurface::FileAtRef { .. }
        ) {
            self.ensure_initial_right_panel_file_editor_width();
        }
        if surface == RightPanelSurface::Diff {
            if reusable_index.is_none() {
                self.right_panel_width = widened_panel_width_for_review(self.right_panel_width);
            }
            self.refresh_right_panel_diff(cx);
        }
        if matches!(
            surface,
            RightPanelSurface::Files | RightPanelSurface::File(_)
        ) {
            self.refresh_right_panel_working_tree(cx);
        }
        if let Some(terminal_id) = surface.terminal_id() {
            self.ensure_right_panel_terminal(terminal_id, cx);
            // A strip terminal carries no directory of its own — `None`
            // resolves to the owning session's workspace at spawn, and the
            // terminal follows the workspace when it moves.
            self.register_terminal(terminal_id, self.state.selected_session, None);
        }
        if let RightPanelSurface::SideChat(session_id) = surface {
            self.ensure_session_loaded(session_id, cx);
            self.right_panel_pending_side_chat_focus = Some(session_id);
        }
        // Browser views are created on the surface's first render, which has
        // the `Window` their webview must attach to.
        let is_fresh_terminal = reusable_index.is_none() && surface.terminal_id().is_some();
        let index = match reusable_index {
            Some(index) => index,
            None => {
                self.right_panel_surfaces.push(surface);
                self.right_panel_surfaces.len() - 1
            }
        };
        if is_fresh_terminal {
            self.analytics
                .track(crate::analytics::Event::TerminalOpened { kind: "panel" });
        }
        self.right_panel_active_surface = Some(index);
        self.reveal_right_panel_tab(index);
        if reveal {
            self.request_active_terminal_focus();
            self.request_active_browser_focus();
            self.set_right_panel_visible(true, cx);
        }
        cx.notify();
    }

    pub(super) fn open_turn_diff(
        &mut self,
        turn_id: Uuid,
        file: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some((session_id, turn_count)) = self.selected_session().and_then(|session| {
            session
                .turns
                .iter()
                .find(|turn| turn.id == turn_id)
                .map(|turn| (session.id, turn.turn_count))
        }) else {
            return;
        };
        // A file row hands its path to the snapshot landing, which selects it
        // the same way a Git panel row does.
        self.right_panel_pending_diff_file = file;
        self.right_panel_diff_source = ReviewDiffSource::LastTurn {
            session_id,
            turn_id,
            turn_count,
        };
        self.right_panel_diff_selection.clear();
        self.right_panel_diff_snapshot = None;
        self.right_panel_diff_selected_file = None;
        self.open_right_panel_surface(RightPanelSurface::Diff, cx);
    }

    pub(super) fn open_right_panel_file(&mut self, relative_path: String, cx: &mut Context<Self>) {
        // The Files-active path below reuses the tab rather than opening a
        // surface, so the owner's admission check has to cover this entry
        // too — files can't open where the owner takes none.
        if !self.right_panel_owner_allows(&RightPanelSurface::File(relative_path.clone())) {
            return;
        }
        self.ensure_initial_right_panel_file_editor_width();
        let Some(active) = self.right_panel_active_surface else {
            self.open_right_panel_surface(RightPanelSurface::File(relative_path), cx);
            return;
        };
        match self.right_panel_surfaces.get(active).cloned() {
            Some(RightPanelSurface::Files) => {
                let dirty_file_would_be_replaced = self
                    .right_panel_files_selected_path
                    .as_deref()
                    .is_some_and(|current_path| {
                        current_path != relative_path
                            && self.right_panel_file_is_dirty(current_path)
                    });
                if dirty_file_would_be_replaced {
                    self.open_right_panel_surface(RightPanelSurface::File(relative_path), cx);
                    return;
                }

                self.right_panel_files_selected_path = Some(relative_path);
                self.set_right_panel_visible(true, cx);
                cx.notify();
            }
            Some(RightPanelSurface::File(current_path)) => {
                if current_path == relative_path {
                    return;
                }
                if self.right_panel_file_is_dirty(&current_path) {
                    self.open_right_panel_surface(RightPanelSurface::File(relative_path), cx);
                    return;
                }

                let requested = RightPanelSurface::File(relative_path);
                if let Some(existing) =
                    reusable_surface_index(&self.right_panel_surfaces, &requested)
                {
                    self.right_panel_surfaces.remove(active);
                    let existing = if existing > active {
                        existing - 1
                    } else {
                        existing
                    };
                    self.right_panel_active_surface = Some(existing);
                    self.reveal_right_panel_tab(existing);
                } else {
                    self.right_panel_surfaces[active] = requested;
                    self.reveal_right_panel_tab(active);
                }
                self.set_right_panel_visible(true, cx);
                cx.notify();
            }
            _ => self.open_right_panel_surface(RightPanelSurface::File(relative_path), cx),
        }
    }

    fn activate_right_panel_working_tree_entry(
        &mut self,
        relative_path: String,
        absolute_path: PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) {
        if is_dir {
            if !self.right_panel_expanded_paths.remove(&absolute_path) {
                self.right_panel_expanded_paths
                    .insert(absolute_path.clone());
            }
            self.refresh_right_panel_working_tree(cx);
            cx.notify();
        } else if let Some((session_id, message_id, _, _)) = self.selected_transfer_notice() {
            self.open_transfer_path(session_id, message_id, absolute_path, false, cx);
        } else {
            self.open_right_panel_file(relative_path, cx);
        }
    }

    /// Select `relative_path` in the Files surface's working tree, expanding
    /// every ancestor directory so the row exists — the Git panel's "Reveal
    /// in Files". The scroll lands via `right_panel_tree_scroll_to` once the
    /// refreshed tree's entries arrive.
    pub(super) fn reveal_right_panel_file_in_tree(
        &mut self,
        relative_path: String,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        self.reveal_right_panel_file_in_tree_at_root(relative_path, workspace, cx);
    }

    pub(super) fn reveal_right_panel_file_in_tree_at_root(
        &mut self,
        relative_path: String,
        workspace: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let absolute = workspace.join(&relative_path);
        // Every directory between the workspace root and the file must be
        // expanded for the row to render at all.
        let mut dir = absolute.parent();
        while let Some(path) = dir {
            if path == workspace.as_path() {
                break;
            }
            self.right_panel_expanded_paths.insert(path.to_path_buf());
            dir = path.parent();
        }
        self.right_panel_pending_tree_reveal = Some(relative_path.clone());
        self.right_panel_files_selected_path = Some(relative_path);
        self.open_right_panel_surface(RightPanelSurface::Files, cx);
    }

    /// The tree's next entries resolve the pending reveal to a row index —
    /// or drop it when the file is not in the tree at all.
    fn resolve_right_panel_pending_tree_reveal(&mut self) {
        let Some(path) = self.right_panel_pending_tree_reveal.take() else {
            return;
        };
        self.right_panel_tree_scroll_to = self
            .right_panel_working_tree
            .iter()
            .position(|entry| entry.relative_path == path);
    }

    pub(super) fn close_right_panel_surface(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.right_panel_surfaces.len() {
            return;
        }
        // A planning session's plan tab is part of the session — it leaves
        // only when the session does, never through a close gesture.
        if !right_panel_surface_is_closable(&self.right_panel_surfaces[index]) {
            return;
        }
        if let Some(terminal_id) = self.right_panel_surfaces[index].terminal_id() {
            self.drop_terminal(terminal_id, cx);
        }
        if let Some(browser_id) = self.right_panel_surfaces[index].browser_id() {
            self.right_panel_browsers.remove(&browser_id);
        }
        if let RightPanelSurface::GitHub(project_id) = self.right_panel_surfaces[index]
            && let Some(browser) = self.github_browsers.get_mut(&project_id)
        {
            browser.detail = None;
        }
        if let (Some(session_id), RightPanelSurface::PullRequest { number }) = (
            self.state.selected_session,
            &self.right_panel_surfaces[index],
        ) {
            self.right_panel_pr_states.remove(&(session_id, *number));
        }
        // A side-chat tab IS the session: closing it deletes the chat. The
        // removal runs after the strip update so the re-entrant surface sweep
        // in `remove_session_inner` finds this tab already gone.
        let side_chat_id = match self.right_panel_surfaces[index] {
            RightPanelSurface::SideChat(id) => Some(id),
            _ => None,
        };
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
        if let Some(id) = side_chat_id {
            self.side_chat_views.remove(&id);
            self.side_chat_composers.remove(&id);
            self.remove_side_chat_session(id, cx);
        }
        cx.notify();
    }

    /// Drop a side chat's tab wherever it lives — the active strip or a
    /// parked session's surface list — without deleting the session; the
    /// caller owns that. The `close_terminal`/`remove_parked_github_surfaces`
    /// shape.
    pub(super) fn remove_side_chat_surface(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if let Some(index) = self.right_panel_surfaces.iter().position(
            |surface| matches!(surface, RightPanelSurface::SideChat(id) if *id == session_id),
        ) {
            self.close_right_panel_surface(index, cx);
        }
        for state in self.right_panel_states.values_mut() {
            let Some(index) = state.surfaces.iter().position(
                |surface| matches!(surface, RightPanelSurface::SideChat(id) if *id == session_id),
            ) else {
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
        }
    }

    pub(super) fn close_window_or_right_panel_tab_action(
        &mut self,
        _: &CloseWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The Git panel has no tabs; ⌘W on it just gives the slot back.
        if self.git_panel_visible {
            self.set_git_panel_visible(false, window, cx);
            return;
        }
        // A terminal filling the main area is the active surface: ⌘W kills
        // it, asking first while a command is still running inside.
        if let Some(terminal_id) = self.selected_terminal {
            self.close_main_terminal(terminal_id, window, cx);
            return;
        }
        if let Some(active) = self.right_panel_active_surface {
            // The same running-command guard the main terminal gets: a
            // busy tab's shell dies with the surface, so it asks first.
            if let Some(terminal_id) = self
                .right_panel_surfaces
                .get(active)
                .and_then(|surface| surface.terminal_id())
                && self
                    .right_panel_terminals
                    .get(&terminal_id)
                    .is_some_and(|terminal| terminal.read(cx).command_running())
            {
                let focus = self.open_terminal_close_dialog(terminal_id, cx);
                window.focus(&focus, cx);
                return;
            }
            self.close_right_panel_surface(active, cx);
            if self.right_panel_surfaces.is_empty() {
                let focus_handle = self.composer_focus(cx);
                window.focus(&focus_handle, cx);
            }
        } else {
            self.request_window_close(window, cx);
        }
    }

    pub(super) fn render_right_panel_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id("toggle-right-panel")
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon("icons/panel-right.svg", 14.0, theme.text_tertiary))
            .tooltip(|window, cx| {
                Tooltip::new(tr!("right_panel.toggle"))
                    .action(&ToggleRightPanel)
                    .build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.set_right_panel_visible(!this.right_panel_visible, cx);
            }))
    }

    // The toggles ride whichever header owns the window's top-right corner;
    // the fixed gap keeps the window header's spacing between them no
    // matter the host's own rhythm.
    pub(super) fn render_panel_toggles(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .when(
                self.state.git_panel_enabled && self.git_panel_owner_allowed(),
                |element| element.child(self.render_git_panel_toggle(cx)),
            )
            .child(self.render_right_panel_toggle(cx))
    }

    pub(super) fn render_right_panel(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let pending_count = self.pending_boss_browse.len();
        for _ in 0..pending_count {
            let Some((session_id, url, title)) = self.pending_boss_browse.pop_front() else {
                break;
            };
            if self.state.selected_session == Some(session_id) && self.managed_panel_owner() {
                let surface = RightPanelSurface::new_browser();
                let Some(browser_id) = surface.browser_id() else {
                    continue;
                };
                self.open_right_panel_surface(surface, cx);
                let browser = self.ensure_right_panel_browser(browser_id, window, cx);
                if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                    self.right_panel_browser_titles.insert(browser_id, title);
                }
                cx.spawn(async move |_, cx| {
                    let _ = browser.update(cx, |view, cx| view.navigate_to_url(url, cx));
                })
                .detach();
            } else {
                self.pending_boss_browse.push_back((session_id, url, title));
            }
        }
        let theme = Theme::current(cx);
        // The open-time fallback for surfaces with nothing focusable of
        // their own — held until a frame actually shows the panel so a
        // cached hidden render cannot spend it. Runs before the surface
        // pendings below so a deeper request wins when both fire on the
        // same frame.
        if self.right_panel_visible
            && let Some(focus) = self.right_panel_pending_focus.take()
        {
            // Deferred like the file-editor pending: render runs inside
            // prepaint, and moving focus here would let a second element
            // claim the frame's a11y focus after an earlier one already did.
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        }
        let active_terminal_id = self
            .active_right_panel_surface()
            .and_then(RightPanelSurface::terminal_id);
        if self.right_panel_pending_terminal_focus == active_terminal_id
            && let Some(terminal_id) = active_terminal_id
            && let Some(terminal) = self.right_panel_terminals.get(&terminal_id)
        {
            let focus_handle = terminal.read(cx).focus_handle(cx);
            window.on_next_frame(move |window, cx| window.focus(&focus_handle, cx));
            self.right_panel_pending_terminal_focus = None;
        }
        // Record whichever terminal actually holds focus — pending requests
        // land here, and so do direct clicks into the grid on a later frame.
        if let Some(terminal_id) = active_terminal_id
            && self
                .right_panel_terminals
                .get(&terminal_id)
                .is_some_and(|terminal| terminal.read(cx).focus_handle(cx).is_focused(window))
        {
            self.right_panel_last_focused_terminal = Some(terminal_id);
            // Holding focus is having seen the completion — the sidebar's
            // unread dot retires here rather than on tab activation.
            if self.unseen_terminal_completions.remove(&terminal_id) {
                cx.notify();
            }
        }
        let body = match self.active_right_panel_surface().cloned() {
            None => self.render_right_panel_chooser(cx).into_any_element(),
            Some(RightPanelSurface::BackgroundWork { key, .. }) => self
                .render_background_work_surface(&key, cx)
                .into_any_element(),
            Some(RightPanelSurface::Files) => self
                .render_right_panel_files(width, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::Diff) => self
                .render_right_panel_diff(width, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::Terminal(terminal_id)) => self
                .right_panel_terminals
                .get(&terminal_id)
                .cloned()
                .inspect(|terminal| {
                    terminal.update(cx, |terminal, _| terminal.set_panel_width(width));
                })
                .map(IntoElement::into_any_element)
                .unwrap_or_else(|| {
                    self.render_right_panel_empty_message(
                        tr!("right_panel.terminal_unavailable"),
                        tr!("right_panel.terminal_unavailable_description"),
                        cx,
                    )
                    .into_any_element()
                }),
            Some(RightPanelSurface::File(path)) => self
                .render_right_panel_file(path, width, true, None, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::FileAtRef { path, git_ref }) => self
                .render_right_panel_ref_file(&path, &git_ref, width, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::GitHub(project_id)) => self
                .render_github_detail(project_id, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::PullRequest { number }) => self
                .render_pull_request_panel(number, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::SideChat(session_id)) => self
                .render_side_chat_panel(session_id, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::Plan {
                session_id,
                plan_file,
            }) => self
                .render_plan_preview(session_id, &plan_file, window, cx)
                .into_any_element(),
            Some(RightPanelSurface::Goals) => self.render_boss_goals_panel(cx).into_any_element(),
            Some(RightPanelSurface::Browser(browser_id)) => {
                let browser = self.ensure_right_panel_browser(browser_id, window, cx);
                if self
                    .right_panel_pending_browser_focus
                    .take_if(|pending| *pending == browser_id)
                    .is_some()
                {
                    let browser = browser.clone();
                    window.on_next_frame(move |window, cx| {
                        browser.update(cx, |view, cx| view.focus_default(window, cx));
                    });
                }
                browser.into_any_element()
            }
        };

        div()
            .id("right-panel")
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .min_w_0()
            .key_context("RightPanel")
            .track_focus(&self.right_panel_focus)
            .border_l(hairline())
            .border_color(theme.separator)
            .bg(theme.surface)
            .relative()
            .child(self.render_right_panel_header(window, cx))
            .child(body)
            // The fullscreen layer owns the window's width; dragging the
            // panel edge would fight it until the mode exits.
            .when(!self.panel_fullscreen_active(), |element| {
                element.child(self.render_panel_resize_handle(
                    "right-panel-resize-handle",
                    PanelResizeTarget::RightPanel,
                    cx,
                ))
            })
    }

    /// One side-chat composer per session, created on the tab's first
    /// render. Submissions route to that session — never the selected one —
    /// through the ordinary submission path, so queueing and steering behave
    /// the way they do in the main composer.
    fn ensure_side_chat_composer(
        &mut self,
        session_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ComposerInput> {
        if let Some(chat) = self.side_chat_composers.get(&session_id) {
            return chat.composer.clone();
        }
        let composer = cx.new(|cx| {
            ComposerInput::new(window, cx)
                .padding_x(px(14.0), cx)
                .collapsed_paste(cx)
        });
        composer.update(cx, |composer, cx| {
            composer.set_placeholder(tr!("side_chat.placeholder"), cx);
        });
        cx.subscribe(
            &composer,
            move |this: &mut Self, _, event: &ComposerEvent, cx| match event {
                ComposerEvent::Submit(prompt) => {
                    let empty_draft = prompt.trim().is_empty()
                        && this
                            .side_chat_composers
                            .get(&session_id)
                            .is_none_or(|chat| chat.atoms.is_empty())
                        && !this.side_chat_has_annotations(session_id);
                    if empty_draft {
                        // Same affordance as the session column's empty
                        // Enter over a stopped turn.
                        this.continue_interrupted_session_to(session_id, cx);
                    } else {
                        this.submit_side_chat_prompt(session_id, prompt.clone(), cx);
                    }
                }
                ComposerEvent::SubmitSteer(prompt) => {
                    let empty_draft = prompt.trim().is_empty()
                        && this
                            .side_chat_composers
                            .get(&session_id)
                            .is_none_or(|chat| chat.atoms.is_empty())
                        && !this.side_chat_has_annotations(session_id);
                    if !empty_draft
                        && let Some(submission) =
                            this.side_chat_submission(session_id, prompt.clone(), cx)
                    {
                        this.steer_session_submission(session_id, submission, cx);
                    }
                }
                ComposerEvent::Edited => cx.notify(),
                ComposerEvent::Focus => {
                    this.last_focused_side_chat_composer = Some(session_id);
                }
                ComposerEvent::BackspaceOnEmpty => {
                    // Chat idiom: pop the last staged atom, the way the
                    // session column pops attachments and atoms.
                    if let Some(chat) = this.side_chat_composers.get_mut(&session_id)
                        && chat.atoms.pop().is_some()
                    {
                        this.sync_side_chat_atoms(session_id, cx);
                        cx.notify();
                    }
                }
                ComposerEvent::InlineAtomActivated(marker) => {
                    this.activate_side_chat_atom(session_id, *marker, cx);
                }
                _ => {}
            },
        )
        .detach();
        // Every splice the field applies can move or delete the markers the
        // chat's atoms anchor to — the same contract the session column's
        // composer keeps.
        cx.subscribe(
            &composer,
            move |this: &mut Self, _, event: &ComposerSplice, cx| {
                this.remap_side_chat_atoms(session_id, event, cx);
            },
        )
        .detach();
        // A large paste folds into the field as a marker chip; the text
        // stays beside the composer as an atom until submit.
        cx.subscribe(
            &composer,
            move |this: &mut Self, _, event: &ComposerTextPaste, cx| {
                this.stage_side_chat_pasted_text(session_id, event.0.clone(), cx);
            },
        )
        .detach();
        self.side_chat_composers.insert(
            session_id,
            SideChatComposer {
                composer: composer.clone(),
                autocomplete: autocomplete::AutocompleteUi::new(),
                atoms: Vec::new(),
                commands: Rc::new(Vec::new()),
                command_key: None,
                commands_loading: false,
                files: Rc::new(Vec::new()),
                file_key: None,
                files_loading: false,
            },
        );
        composer
    }

    /// A `/side` chat lane in its parent's panel: the compact transcript a
    /// Big Picture card draws, bottom-pinned, plus its own composer. The
    /// session is a sibling — never the selected one — so every row is built
    /// from `session_id` rather than the lane's selected-session helpers.
    fn render_side_chat_panel(
        &mut self,
        session_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned()
        else {
            return self
                .render_right_panel_empty_message(
                    tr!("right_panel.side_chat_unavailable"),
                    tr!("right_panel.side_chat_unavailable_description"),
                    cx,
                )
                .into_any_element();
        };
        self.ensure_session_loaded(session_id, cx);
        let composer = self.ensure_side_chat_composer(session_id, window, cx);
        self.refresh_side_chat_sources(session_id, cx);
        self.prune_side_chat_annotations(session_id, cx);
        let pending_side_chat_focus = if self
            .right_panel_pending_side_chat_focus
            .take_if(|pending| *pending == session_id)
            .is_some()
        {
            Some(composer.read(cx).focus())
        } else {
            None
        };

        // The row kinds are fingerprinted and spliced exactly like a card's:
        // appends keep position, a refold re-measures, and the tail re-measures
        // while the session works so fresh text is never clipped.
        let pending_turn = self.blocked_checkpoint_turn(session.id);
        // The transcript's resolved-commit set is shared so SHAs underline
        // and hit-test in the lane the way they do in the session column.
        let resolved_commits = self.transcript_selection.resolved_commits.clone();
        let view = self
            .side_chat_views
            .entry(session_id)
            .or_insert_with(move || {
                let mut selection = TranscriptSelection::default();
                selection.resolved_commits = resolved_commits;
                SideChatView {
                    rows: {
                        let rows = ListState::new(0, ListAlignment::Bottom, px(2048.0));
                        rows.set_scroll_handler(|_, window, _| window.refresh());
                        rows
                    },
                    scrollbar: ScrollbarState::new(),
                    kinds: (0, Rc::new(Vec::new())),
                    response_footers: HashMap::new(),
                    expanded_turns: HashSet::new(),
                    expanded_activity_blocks: HashMap::new(),
                    expanded_changed_files: HashSet::new(),
                    hovered_response_turn: None,
                    selection,
                }
            });
        let expanded_turns = view.expanded_turns.clone();
        let fingerprint = transcript_rows_fingerprint(&session, &expanded_turns, pending_turn);
        let previous_kinds = view.kinds.1.clone();
        let (kinds, refolded) = if view.kinds.0 != fingerprint {
            let mut folded = folded_transcript_row_kinds(&session, &expanded_turns, pending_turn);
            view.response_footers = folded
                .iter()
                .filter_map(|kind| {
                    let TranscriptRowKind::ResponseFooter(_, message_index) = *kind else {
                        return None;
                    };
                    let message = session.messages.get(message_index)?;
                    let copy_content =
                        super::transcript::assistant_response_footer(&session, message_index)?;
                    let timestamp =
                        super::transcript::assistant_response_footer_time(&session, message_index)
                            .unwrap_or(message.created_at);
                    Some((message_index, (SharedString::from(copy_content), timestamp)))
                })
                .collect();
            // A footer turn hosts its pending card inside its footer row on
            // the main transcript, so append it at the tail in side chats.
            if let Some(turn_id) = pending_turn
                && !folded.contains(&TranscriptRowKind::ChangedFiles(turn_id))
            {
                folded.push(TranscriptRowKind::ChangedFiles(turn_id));
            }
            let kinds = Rc::new(folded);
            view.kinds = (fingerprint, kinds.clone());
            (kinds, true)
        } else {
            (view.kinds.1.clone(), false)
        };
        let count = kinds.len();
        let current = view.rows.item_count();
        if refolded {
            if let Some((range, new_count)) = transcript_row_splice(&previous_kinds, &kinds) {
                view.rows.splice(range, new_count);
            } else if current != count {
                view.rows.reset(count);
            } else {
                view.rows.remeasure_items(0..count);
            }
        } else if count > current {
            view.rows.splice(current..current, count - current);
        } else if count < current {
            view.rows.reset(count);
        }
        if session.status.is_busy() {
            view.rows
                .remeasure_items(count.saturating_sub(STREAM_REMEASURE_TAIL_ROWS)..count);
        }
        let rows_state = view.rows.clone();
        let scrollbar = view.scrollbar.clone();
        let selection = view.selection.clone();
        let selection_input = selection.clone();
        let annotation_selection = selection.clone();
        let waku = cx.entity().downgrade();
        let entity = waku.clone();
        let annotation_offer = self.render_side_chat_annotation_offer(session_id, window, cx);
        let annotation_editor = self.render_side_chat_annotation_editor(session_id, cx);
        let annotation_tooltip = self.render_side_chat_annotation_tooltip(session_id, cx);
        let annotation_ref_tooltip = self.render_side_chat_annotation_ref_tooltip(session_id, cx);
        let workspace_footer =
            self.render_side_chat_workspace_footer(session_id, composer.clone(), cx);
        // The lane is a transcript surface: clicks hand the region a
        // programmatic focus so ⌘L's Transcript-context binding reaches
        // `add_to_chat` here the way it does in the session column.
        let lane_focus = self.transcript_control_focus(format!("side-chat-{session_id}"), cx);
        let lane_focus_click = lane_focus.clone();
        let panel = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .key_context("Transcript")
                    .track_focus(&lane_focus)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _, window, cx| window.focus(&lane_focus_click, cx)),
                    )
                    // Painted before any row, so the frame's registry holds
                    // exactly the lane's visible text, in order — the same
                    // contract the transcript's reset keeps.
                    .child(md::render::frame_reset(selection))
                    .child(
                        list(rows_state.clone(), move |index, window, cx| {
                            entity
                                .upgrade()
                                .map(|entity| {
                                    entity.update(cx, |this, cx| {
                                        this.side_chat_row(session_id, index, window, cx)
                                    })
                                })
                                .unwrap_or_else(|| div().into_any_element())
                        })
                        .size_full(),
                    )
                    .child(scrollbar::edge_fade(
                        rows_state.clone(),
                        scrollbar::FadeEdge::Top,
                        theme.surface,
                    ))
                    .child(scrollbar::edge_fade(
                        rows_state.clone(),
                        scrollbar::FadeEdge::Bottom,
                        theme.surface,
                    ))
                    .child(scrollbar::vertical(&rows_state, &scrollbar))
                    .child(
                        canvas(
                            |bounds, window, _| {
                                window.insert_hitbox(bounds, HitboxBehavior::Normal).id
                            },
                            move |_, region, window, _| {
                                // ⌥-click/drag annotates through the same
                                // `AddToChat` settle the transcript arms.
                                md::render::install_selection_input(
                                    region,
                                    window,
                                    &selection_input,
                                    Some(Box::new(AddToChat)),
                                )
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    // Annotation hit-tests paint after the selection's
                    // listeners, the same ordering the transcript keeps.
                    .child(
                        canvas(
                            |bounds, window, _| {
                                window.insert_hitbox(bounds, HitboxBehavior::Normal).id
                            },
                            move |_, region, window, cx| {
                                Self::install_annotation_input(
                                    region,
                                    window,
                                    cx,
                                    &annotation_selection,
                                    &waku,
                                    annotations::AnnotationTarget::SideChat(session_id),
                                )
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .children(annotation_offer)
                    .children(annotation_editor)
                    .children(annotation_tooltip)
                    .children(annotation_ref_tooltip),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(10.0))
                    .pt(px(8.0))
                    // Parked follow-ups tuck against the composer card's top
                    // edge, inset the same 14px the session column gives the
                    // queue card inside its composer column. The bounds probe
                    // lets the autocomplete popup clear the card rather than
                    // cover it — cleared on every miss, the cell goes stale
                    // the moment the card unmounts.
                    .children({
                        let queue_bounds = self
                            .side_chat_composers
                            .get(&session_id)
                            .map(|chat| chat.autocomplete.queue_bounds_cell());
                        match self.queued_messages_card(&session, cx) {
                            Some(card) => Some(
                                div()
                                    .px(px(14.0))
                                    .relative()
                                    .children(
                                        queue_bounds
                                            .map(super::autocomplete::composer_card_bounds_probe),
                                    )
                                    .child(card),
                            ),
                            None => {
                                if let Some(cell) = queue_bounds {
                                    cell.set(None);
                                }
                                None
                            }
                        }
                    })
                    .child(self.render_composer_card(
                        &composer::ComposerCard::SideChat {
                            session_id,
                            composer,
                        },
                        window,
                        cx,
                    )),
            )
            .child(workspace_footer);
        if let Some(focus) = pending_side_chat_focus {
            // The composer card joins the dispatch tree with the panel. Wait
            // for that deferred mount before handing it keyboard focus.
            window.on_next_frame(move |window, _| {
                window.on_next_frame(move |window, cx| window.focus(&focus, cx));
            });
        }
        panel.into_any_element()
    }

    fn toggle_side_chat_turn_fold(
        &mut self,
        session_id: Uuid,
        turn_id: Uuid,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.side_chat_views.get_mut(&session_id) else {
            return;
        };
        if expanded {
            view.expanded_turns.remove(&turn_id);
        } else {
            view.expanded_turns.insert(turn_id);
        }
        cx.notify();
    }

    fn toggle_side_chat_changed_files(
        &mut self,
        session_id: Uuid,
        turn_id: Uuid,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.side_chat_views.get_mut(&session_id) else {
            return;
        };
        if expanded {
            view.expanded_changed_files.remove(&turn_id);
        } else {
            view.expanded_changed_files.insert(turn_id);
        }
        if let Some(row_index) = view.kinds.1.iter().position(|kind| {
            matches!(
                kind,
                TranscriptRowKind::ResponseFooter(id, _) | TranscriptRowKind::ChangedFiles(id)
                    if *id == turn_id
            )
        }) {
            let scroll_top = view.rows.logical_scroll_top();
            view.rows.remeasure_items(row_index..row_index + 1);
            view.rows.scroll_to(scroll_top);
        }
        cx.notify();
    }

    fn render_side_chat_changed_files_row(
        &self,
        session_id: Uuid,
        session: &AgentSession,
        turn_id: Uuid,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let checkpoint = session
            .turns
            .iter()
            .find(|turn| turn.id == turn_id)
            .and_then(|turn| turn.checkpoint.as_ref())
            .filter(|checkpoint| checkpoint.status == CheckpointStatus::Ready)
            .filter(|checkpoint| !checkpoint.files.is_empty())?;
        let expanded = self
            .side_chat_views
            .get(&session_id)
            .is_some_and(|view| view.expanded_changed_files.contains(&turn_id));
        const PREVIEW_LIMIT: usize = 3;
        const EXPANDED_LIMIT: usize = 12;
        let visible_limit = if expanded {
            EXPANDED_LIMIT
        } else {
            PREVIEW_LIMIT
        };
        let visible_count = checkpoint.files.len().min(visible_limit);
        let title = if checkpoint.files.len() == 1 {
            tr!("transcript.changed_file", count = checkpoint.files.len())
        } else {
            tr!("transcript.changed_files", count = checkpoint.files.len())
        };
        let mut card = div()
            .w_full()
            .min_w_0()
            .rounded(px(15.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.overlay)
            .overflow_hidden()
            .child(
                div()
                    .min_h(px(58.0))
                    .px(px(12.0))
                    .py(px(9.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .size(px(36.0))
                            .flex_none()
                            .rounded(px(11.0))
                            .bg(theme.overlay_strong)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/file-diff.svg", 16.0, theme.text_tertiary)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(14.0))
                                    .child(
                                        div()
                                            .text_color(theme.success)
                                            .child(format!("+{}", checkpoint.additions)),
                                    )
                                    .child(
                                        div()
                                            .text_color(theme.danger)
                                            .child(format!("-{}", checkpoint.deletions)),
                                    ),
                            ),
                    ),
            );
        let mut file_rows = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .border_t(hairline())
            .border_color(theme.separator);
        for file in checkpoint.files.iter().take(visible_count) {
            file_rows = file_rows.child(
                div()
                    .h(px(29.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(file.path.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.5))
                            .text_color(theme.success)
                            .child(format!("+{}", file.additions)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.5))
                            .text_color(theme.danger)
                            .child(format!("-{}", file.deletions)),
                    ),
            );
        }
        card = card.child(file_rows);
        if checkpoint.files.len() > PREVIEW_LIMIT {
            let focus = self.transcript_control_focus(
                format!("side-chat-changed-files-toggle-{session_id}-{turn_id}"),
                cx,
            );
            let label = if expanded {
                tr!("transcript.show_fewer_files")
            } else {
                tr!(
                    "transcript.show_more_files",
                    count = checkpoint.files.len() - PREVIEW_LIMIT
                )
            };
            card = card.child(
                div()
                    .id(SharedString::from(format!(
                        "side-chat-changed-files-toggle-{session_id}-{turn_id}"
                    )))
                    .track_focus(&focus)
                    .tab_index(0)
                    .h(px(34.0))
                    .px(px(12.0))
                    .border_t(hairline())
                    .border_color(theme.separator)
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_default()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .hover(|style| style.bg(theme.overlay_strong).text_color(theme.text))
                    .active(|style| style.bg(theme.overlay))
                    .child(SharedString::from(label))
                    .when(expanded && checkpoint.files.len() > EXPANDED_LIMIT, |row| {
                        row.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme.text_ghost)
                                .child(tr!(
                                    "transcript.showing_first_files",
                                    count = EXPANDED_LIMIT,
                                    total = checkpoint.files.len()
                                )),
                        )
                    })
                    .child(div().flex_1())
                    .child(icon(
                        if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        },
                        11.0,
                        theme.affordance_icon(),
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_side_chat_changed_files(session_id, turn_id, expanded, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.toggle_side_chat_changed_files(session_id, turn_id, expanded, cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }
        Some(card.into_any_element())
    }

    fn render_side_chat_turn_fold_row(
        &self,
        session_id: Uuid,
        session: &AgentSession,
        turn_id: Uuid,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self
            .side_chat_views
            .get(&session_id)
            .is_some_and(|view| view.expanded_turns.contains(&turn_id));
        let label = turn_fold_label(session, turn_id);
        let control_id = format!("side-chat-turn-fold-{session_id}-{turn_id}");
        let focus = self.transcript_control_focus(control_id.clone(), cx);
        div()
            .w_full()
            .h(px(24.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(div().h(hairline()).flex_1().bg(theme.separator))
            .child(
                div()
                    .id(SharedString::from(control_id))
                    .track_focus(&focus)
                    .tab_index(0)
                    .h(px(24.0))
                    .px(px(2.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .cursor_default()
                    .text_size(sp(13.5))
                    .line_height(sp(18.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .hover(|style| style.text_color(theme.text_secondary))
                    .child(SharedString::from(label))
                    .child(icon(
                        if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        },
                        11.5,
                        theme.affordance_icon(),
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_side_chat_turn_fold(session_id, turn_id, expanded, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.toggle_side_chat_turn_fold(session_id, turn_id, expanded, cx);
                            cx.stop_propagation();
                        }
                    })),
            )
            .child(div().h(hairline()).flex_1().bg(theme.separator))
            .into_any_element()
    }

    /// One row of a side chat's transcript. `index` is a position in the
    /// view's `kinds` vector, synced this frame by `render_side_chat_panel`.
    /// Row bodies reuse the card renderers — the panel is the same compact
    /// read of a non-selected session.
    fn side_chat_row(
        &self,
        session_id: Uuid,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return div().into_any_element();
        };
        let (row_count, kind, starts_followup_turn) = {
            let Some(view) = self.side_chat_views.get(&session_id) else {
                return div().into_any_element();
            };
            (
                view.kinds.1.len(),
                view.kinds
                    .1
                    .get(index)
                    .copied()
                    .unwrap_or(TranscriptRowKind::Message(index)),
                row_starts_followup_turn(session, &view.kinds.1, index),
            )
        };
        let inner = match kind {
            TranscriptRowKind::Message(message_index) => session
                .messages
                .get(message_index)
                .cloned()
                .map(|message| {
                    let copied = self.copied_message_feedback.contains_key(&message.id);
                    let menu = self.menu_handle(format!("side-chat-message-{}", message.id), cx);
                    let attachment_menus = (0..message.attachments.len())
                        .map(|index| {
                            self.menu_handle(
                                format!("side-chat-message-{}-attachment-{index}", message.id),
                                cx,
                            )
                        })
                        .collect();
                    let attachment_images = message
                        .attachments
                        .iter()
                        .map(|attachment| {
                            if !attachment.is_image {
                                return None;
                            }
                            let reference = attachment.blob_reference.as_deref()?;
                            self.image_for_reference(
                                reference,
                                Some(&attachment.path),
                                Some(&attachment.name),
                                cx,
                            )
                        })
                        .collect();
                    let metrics =
                        self.scaled_markdown_metrics(if message.role == MessageRole::User {
                            MarkdownMetrics::USER_MESSAGE
                        } else {
                            MarkdownMetrics::BODY
                        });
                    let animate_streaming = message.streaming && !cx.reduce_motion();
                    let selection = self
                        .side_chat_views
                        .get(&session_id)
                        .map(|view| view.selection.clone())
                        .unwrap_or_default();
                    let mut ctx = self
                        .markdown_ctx(
                            format!("side-chat-message-{}", message.id),
                            &palette,
                            metrics,
                            animate_streaming,
                            Some(session.id),
                            &selection,
                            cx,
                        )
                        .with_context_menu(menu.clone());
                    // Replies may cite their prompt's annotations as
                    // "Annotation N" — the chat's own sent set bounds the
                    // affordance, like the transcript's.
                    if message.role == MessageRole::Assistant
                        && let Some(set) = self.side_chat_annotation_ref_set(session.id, message.id)
                    {
                        ctx = ctx.with_annotation_labels(set.len());
                    }
                    if message.role == MessageRole::User {
                        // Tables in a user bubble fade into its raised fill,
                        // not the panel surface.
                        ctx = ctx.with_surface(theme.raised);
                    }
                    let work_item_refs = (message.role == MessageRole::User)
                        .then(|| {
                            self.work_item_refs_for_content(
                                self.workspace_path_for_session(session),
                                message.visible_content(),
                            )
                        })
                        .unwrap_or_default();
                    let mut markdown = self.message_markdown.borrow_mut();
                    let view = matches!(message.role, MessageRole::User | MessageRole::Assistant)
                        .then(|| {
                            // Seeded like a card's: the side chat's replies
                            // arrive while its tab may not be on screen, so
                            // they paint at full opacity rather than
                            // dissolving on first open.
                            let view = markdown
                                .entry(message.id)
                                .or_insert_with(MarkdownView::seeded);
                            view.set_transcript_text(
                                message.visible_content(),
                                message.streaming,
                                message.role == MessageRole::User,
                            );
                            &*view
                        });
                    let rendered = render_message(
                        MessageRender {
                            theme: &theme,
                            message: &message,
                            provider: session.provider,
                            assistant_footer_copy_content: None,
                            assistant_footer_time: None,
                            copied,
                            show_response_token_speed: false,
                            assistant_message_action: None,
                            user_message_action: None,
                            voice_briefing: None,
                            user_message_viewport: None,
                            user_message_expanded: false,
                            user_message_expand_focus: None,
                            message_edit_input: None,
                            attachment_menus,
                            attachment_images,
                            pasted_text_attachments: (0..message.attachments.len())
                                .map(|index| self.pasted_text_attachment_view(message.id, index))
                                .collect(),
                            attachments_can_reveal: !self.is_remote_session(session_id),
                            markdown: view,
                            display_content: message.visible_content(),
                            work_item_refs,
                            ctx: &ctx,
                            menu,
                            sent_by_task_link: message
                                .sent_by_task
                                .filter(|id| self.sent_by_task_openable(*id)),
                            sent_by_boss: message.sent_by_task.and_then(|id| {
                                self.boss_session_identity(id).map(|identity| {
                                    (self.boss_avatar(&identity, 18.0, cx), identity.name.clone())
                                })
                            }),
                            auto_prompt_rule: self
                                .enabled_auto_prompt_for_content(message.visible_content()),
                            waku: cx.entity().downgrade(),
                            composer: self.composer.clone(),
                            landed_notice: None,
                            transfer_notice: self.transfer_notice_state(session, &message, cx),
                        },
                        cx,
                    );
                    if animate_streaming && view.is_some_and(MarkdownView::is_fading) {
                        motion::pulse_lease(window.current_view(), cx);
                    }
                    rendered
                })
                .unwrap_or_else(|| div().into_any_element()),
            TranscriptRowKind::TurnBlock(block_index) => session
                .transcript_blocks
                .get(block_index)
                .map(|block| {
                    self.render_activities_row(
                        session_id,
                        &block.activities,
                        block_index,
                        &theme,
                        window,
                        cx,
                    )
                })
                .unwrap_or_else(|| div().into_any_element()),
            TranscriptRowKind::TurnFold(turn_id) => {
                self.render_side_chat_turn_fold_row(session_id, session, turn_id, &theme, cx)
            }
            TranscriptRowKind::BossHistory(..) | TranscriptRowKind::BossHistoryBoundary(_) => {
                div().into_any_element()
            }
            TranscriptRowKind::WorkingIndicator => {
                self.render_card_working_indicator_row(session, &theme)
            }
            // The side chat keeps the wake marker to one quiet line.
            TranscriptRowKind::BossTrigger(anchor) => {
                let line = boss_trigger_group(session, anchor)
                    .first()
                    .and_then(|message| message.report_trigger.as_ref())
                    .map(|trigger| {
                        boss_trigger_entry_label(
                            trigger,
                            trigger.boundary == crate::model::ReportTriggerBoundary::Steer,
                        )
                    })
                    .unwrap_or_default();
                div()
                    .truncate()
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(line))
                    .into_any_element()
            }
            TranscriptRowKind::ChangedFiles(turn_id) => {
                if self.blocked_checkpoint_turn(session.id) == Some(turn_id) {
                    self.render_card_checkpoint_pending_row(turn_id, &theme)
                } else {
                    self.render_side_chat_changed_files_row(
                        session_id, session, turn_id, &theme, cx,
                    )
                    .unwrap_or_else(|| div().into_any_element())
                }
            }
            TranscriptRowKind::ResponseFooter(turn_id, message_index) => {
                let Some(message) = session.messages.get(message_index) else {
                    return div().into_any_element();
                };
                let Some((copy_content, timestamp)) = self
                    .side_chat_views
                    .get(&session_id)
                    .and_then(|view| view.response_footers.get(&message_index))
                else {
                    return div().into_any_element();
                };
                let group_name =
                    SharedString::from(format!("side-chat-response-footer-{session_id}-{turn_id}"));
                let force_visible = self
                    .side_chat_views
                    .get(&session_id)
                    .is_some_and(|view| view.hovered_response_turn == Some(turn_id));
                let changed_files = self
                    .render_side_chat_changed_files_row(session_id, session, turn_id, &theme, cx);
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .group(group_name.clone())
                    .when_some(changed_files, |column, card| {
                        column.child(div().w_full().mb(px(3.0)).child(card))
                    })
                    .child(super::components::render_message_footer(
                        &theme,
                        message,
                        *timestamp,
                        copy_content.clone(),
                        self.copied_message_feedback.contains_key(&message.id),
                        false,
                        group_name,
                        force_visible,
                        false,
                        None,
                        None,
                        None,
                        cx.entity().downgrade(),
                    ))
                    .into_any_element()
            }
        };
        let response_turn_id = super::transcript::response_row_turn_id(session, kind);
        div()
            .id(SharedString::from(format!(
                "side-chat-row-{session_id}-{index}"
            )))
            .w_full()
            .px(px(20.0))
            .py(px(8.0))
            .when(index == 0, |element| element.pt(px(22.0)))
            .when(starts_followup_turn, |element| {
                element.pt(px(FOLLOWUP_TURN_TOP_GAP))
            })
            .when(index + 1 == row_count, |element| element.pb(px(22.0)))
            .child(inner)
            .when_some(response_turn_id, |row, turn_id| {
                row.on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                    let Some(view) = this.side_chat_views.get_mut(&session_id) else {
                        return;
                    };
                    let previous = view.hovered_response_turn;
                    if *hovering {
                        view.hovered_response_turn = Some(turn_id);
                    } else if previous == Some(turn_id) {
                        view.hovered_response_turn = None;
                    }
                    if view.hovered_response_turn != previous {
                        cx.notify();
                    }
                }))
            })
            .into_any_element()
    }

    fn ensure_right_panel_browser(
        &mut self,
        browser_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<crate::browser::BrowserView> {
        if let Some(browser) = self.right_panel_browsers.get(&browser_id) {
            return browser.clone();
        }
        let browser = cx.new(|cx| crate::browser::BrowserView::new(window, cx));
        // Tab titles and toolbar state live on the browser entity; the panel
        // chrome re-renders when they move.
        cx.observe(&browser, |_, _, cx| cx.notify()).detach();
        self.right_panel_browsers
            .insert(browser_id, browser.clone());
        browser
    }

    /// Open a URL in a fresh built-in browser tab — the toast's shift-modified
    /// open path and any future "preview this site" entry point.
    pub(super) fn open_url_in_browser_tab(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let surface = RightPanelSurface::new_browser();
        let Some(browser_id) = surface.browser_id() else {
            return;
        };
        self.open_right_panel_surface(surface, cx);
        let browser = self.ensure_right_panel_browser(browser_id, window, cx);
        browser.update(cx, |view, cx| view.navigate_to_url(url, cx));
    }

    /// Drop browser views whose tab no longer exists in any session.
    pub(super) fn retain_right_panel_browsers(&mut self) {
        let retained_browser_ids = self
            .right_panel_surfaces
            .iter()
            .filter_map(RightPanelSurface::browser_id)
            .chain(self.right_panel_states.values().flat_map(|state| {
                state
                    .surfaces
                    .iter()
                    .filter_map(RightPanelSurface::browser_id)
            }))
            .collect::<HashSet<_>>();
        self.right_panel_browsers
            .retain(|browser_id, _| retained_browser_ids.contains(browser_id));
        self.right_panel_browser_titles
            .retain(|browser_id, _| retained_browser_ids.contains(browser_id));
    }

    /// Whether any GPUI overlay that could float above the right panel is
    /// open. The native webview always draws over GPUI, so while this holds
    /// the live page swaps for a frozen snapshot.
    fn any_overlay_open(&self, cx: &App) -> bool {
        self.menus.borrow().values().any(ContextMenuHandle::is_open)
            || self.selected_runtime().is_some_and(|runtime| {
                runtime.computer_use_previews.iter().any(|preview| {
                    preview.visible
                        && preview.target.is_some()
                        && preview.phase != ComputerUsePhase::AwaitingApproval
                })
            })
            || self.command_palette.is_open()
            || self.task_switcher.is_open()
            || self.project_switcher.is_open()
            || self.commit_dialog.is_some()
            || self.archive_dialog.is_some()
            || self.full_access_dialog.is_some()
            || self.incognito_dialog.is_some()
            || self.provider_switch_dialog.is_some()
            || self.shortcuts_dialog.is_some()
            || self.image_preview.is_some()
            || self.composer.read(cx).context_menu_open(cx)
            || self
                .right_panel_browsers
                .values()
                .any(|browser| browser.read(cx).overlay_open(cx))
    }

    /// Once per frame, from the very top of the app's render: push down to
    /// every browser whether its native view belongs on screen. This is the
    /// single authority — tab switches, panel toggles, session switches, the
    /// settings page and overlay menus all funnel through here, so a webview
    /// can never linger over unrelated UI.
    pub(super) fn sync_browser_webviews(&mut self, cx: &mut Context<Self>) {
        if self.right_panel_browsers.is_empty() {
            return;
        }
        // With the scene overlay compositing GPUI's deferred draws above
        // native views, open menus never occlude the webview — the snapshot
        // swap is purely the fallback for a window where enabling it failed.
        let overlay_open = !self.scene_overlay_enabled && self.any_overlay_open(cx);
        // A webview composites above the GPUI scene, so the panel's clip does
        // not apply to it: shown mid-slide it would hang over the transcript
        // at full width. Keep it down until the panel has finished moving.
        let active_browser = if self.settings_page.is_none()
            && self.right_panel_content_visible()
            && self.right_panel_slide.is_none()
        {
            self.active_right_panel_surface()
                .and_then(RightPanelSurface::browser_id)
        } else {
            None
        };
        for (browser_id, browser) in &self.right_panel_browsers {
            let surface_visible = active_browser == Some(*browser_id);
            browser.update(cx, |view, cx| {
                view.sync_native_state(surface_visible, overlay_open, cx);
            });
        }
    }

    /// Run a user-defined custom command: a fresh terminal tab whose shell
    /// sources the command's materialized script. The command rides along in
    /// `right_panel_terminal_commands` so the tab keeps its launch settings
    /// if the PTY is ever respawned for a changed workspace.
    ///
    /// The panel stays closed: the run reports through a spinner toast that
    /// resolves to a check or a red x when the launch line's sentinel hands
    /// back the script's exit code, and a failure reveals the terminal.
    /// cmd cannot emit the sentinel, so Windows keeps the reveal-on-run
    /// behavior; a remote daemon has no PTY at all, so the panel must carry
    /// whatever the surface shows there too.
    pub(super) fn run_custom_command(&mut self, command: CustomCommand, cx: &mut Context<Self>) {
        self.run_custom_command_journaled(
            command,
            action_predictions::JournalAction::TerminalRun,
            cx,
        );
    }

    /// `run_custom_command` with the journal entry the caller's action
    /// actually was — the sync strip's Git move is not a terminal run.
    pub(super) fn run_custom_command_journaled(
        &mut self,
        command: CustomCommand,
        journal: action_predictions::JournalAction,
        cx: &mut Context<Self>,
    ) {
        if self.selected_workspace_path().is_none() {
            self.show_toast(tr!("commands.no_workspace"));
            cx.notify();
            return;
        }
        self.record_action(self.state.selected_session, journal);
        let surface = RightPanelSurface::new_terminal();
        let terminal_id = surface.terminal_id();
        if let Some(terminal_id) = terminal_id {
            self.right_panel_terminal_commands
                .insert(terminal_id, command.clone());
        }
        if cfg!(windows)
            || self
                .selected_workspace_path()
                .is_some_and(|path| self.is_remote_path(path))
        {
            self.open_right_panel_surface(surface, cx);
            return;
        }
        if let Some(terminal_id) = terminal_id {
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
        self.add_right_panel_surface(surface, false, cx);
    }

    /// A custom command's launch line reported the script's exit code:
    /// settle the run's toast — in place under its spinner while that is
    /// still the visible toast, as a fresh toast once it is not — and on
    /// failure reveal the terminal, which stayed open at the error.
    ///
    /// A run whose terminal left the active tab strip belongs to a session
    /// that is no longer on screen; its result stays silent rather than
    /// toasting into another session's context. Switching back before the
    /// finish restores the surface and the report.
    pub(super) fn custom_command_finished(
        &mut self,
        terminal_id: Uuid,
        exit_code: Option<i32>,
        cx: &mut Context<Self>,
    ) {
        let Some(mut run) = self.custom_command_runs.remove(&terminal_id) else {
            return;
        };
        // One last read — output written between the final dirty poll and
        // the finish event is exactly what a failure wants to show.
        if let Some(view) = self.right_panel_terminals.get(&terminal_id) {
            let tail = view.read(cx).output_tail(COMMAND_RUN_TAIL_LINES);
            if !tail.is_empty() {
                run.tail = tail;
            }
        }
        let Some(index) = self
            .right_panel_surfaces
            .iter()
            .position(|surface| surface.terminal_id() == Some(terminal_id))
        else {
            return;
        };
        let (message, tone) = match exit_code {
            Some(0) => (
                tr!("commands.succeeded", name = run.name),
                ToastTone::Success,
            ),
            Some(code) => (
                tr!("commands.failed", name = run.name, code = code),
                ToastTone::Failure,
            ),
            // The shell or PTY went away without reporting a status —
            // a signal kill, or a spawn that never reached the script.
            None => (
                tr!("commands.failed_no_code", name = run.name),
                ToastTone::Failure,
            ),
        };
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| toast.id == run.toast_id)
        {
            self.update_toast(message, tone);
        } else {
            self.show_toast_with_tone(message, tone, None);
        }
        // The last thing a failed run printed is usually the error —
        // keep it under the result.
        if exit_code != Some(0)
            && !run.tail.is_empty()
            && let Some(toast) = self.toast.as_mut()
        {
            toast.detail = Some(run.tail.clone());
        }
        if exit_code != Some(0) {
            self.right_panel_active_surface = Some(index);
            self.reveal_right_panel_tab(index);
            self.request_active_terminal_focus();
            self.set_right_panel_visible(true, cx);
        }
        cx.notify();
    }

    /// Mirror the bottom of a running command's screen into the toast it
    /// owns. The terminal view notifies on every PTY dirty flag — up to
    /// the 24ms poll cadence — so publishes are throttled to the
    /// stream-commit cadence with one trailing flush that lands whatever
    /// the last burst left.
    pub(super) fn refresh_command_run_tail(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        let Some(tail) = self
            .right_panel_terminals
            .get(&terminal_id)
            .map(|view| view.read(cx).output_tail(COMMAND_RUN_TAIL_LINES))
        else {
            return;
        };
        let Some(run) = self.custom_command_runs.get_mut(&terminal_id) else {
            return;
        };
        if tail == run.tail {
            return;
        }
        run.tail = tail;
        let elapsed = run.tail_published_at.map(|published| published.elapsed());
        if elapsed.is_none_or(|elapsed| elapsed >= COMMAND_RUN_TAIL_INTERVAL) {
            run.tail_published_at = Some(Instant::now());
            self.publish_command_run_tail(terminal_id, cx);
            return;
        }
        if run.tail_flush_armed {
            return;
        }
        run.tail_flush_armed = true;
        let delay = COMMAND_RUN_TAIL_INTERVAL.saturating_sub(elapsed.unwrap_or_default());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                let Some(run) = this.custom_command_runs.get_mut(&terminal_id) else {
                    return;
                };
                run.tail_flush_armed = false;
                run.tail_published_at = Some(Instant::now());
                this.publish_command_run_tail(terminal_id, cx);
            });
        })
        .detach();
    }

    /// Copy a run's buffered tail onto the toast it owns. A dismissed or
    /// superseded toast id means the run outlived it — nothing to update.
    fn publish_command_run_tail(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        let Some(run) = self.custom_command_runs.get(&terminal_id) else {
            return;
        };
        let detail = (!run.tail.is_empty()).then(|| run.tail.clone());
        let Some(toast) = self.toast.as_mut().filter(|toast| toast.id == run.toast_id) else {
            return;
        };
        if toast.detail != detail {
            toast.detail = detail;
            cx.notify();
        }
    }

    /// Close the tab a finished terminal belongs to, wherever it sits — the
    /// active session's tab strip, a background session's saved surfaces,
    /// or the Terminals group's global list.
    pub(super) fn close_terminal_view_surface(
        &mut self,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) {
        let Some(terminal_id) = self
            .right_panel_terminals
            .iter()
            .find_map(|(id, terminal)| (terminal == view).then_some(*id))
        else {
            return;
        };
        self.close_terminal(terminal_id, cx);
    }

    fn ensure_right_panel_terminal(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        // Where the terminal belongs: the directory its record carries — a
        // cwd the caller chose — or the workspace it tracks. `None` falls
        // through to the selected session's workspace, which also covers a
        // surface not registered yet.
        let workspace_bound = self
            .terminal_records
            .get(&terminal_id)
            .is_some_and(|record| record.working_directory.is_none());
        let Some(working_directory) = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| self.terminal_spawn_directory(record))
            .or_else(|| {
                self.selected_workspace_path()
                    .map(std::path::Path::to_path_buf)
            })
        else {
            self.right_panel_terminals.remove(&terminal_id);
            return;
        };
        if !self.is_remote_path(&working_directory) && !working_directory.is_dir() {
            // The directory is gone — typically an archived session's
            // worktree awaiting restore. A PTY launched now would fall back
            // to the filesystem root and, once the directory returns, look
            // current to the spawn check while its shell sits in the
            // wrong place. The restore's completion re-runs this ensure.
            self.right_panel_terminals.remove(&terminal_id);
            return;
        }
        let spawned_at = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.workspace_directory.clone())
            .or_else(|| {
                self.right_panel_terminals
                    .get(&terminal_id)
                    .map(|terminal| terminal.read(cx).spawn_directory().to_path_buf())
            });
        match spawned_at {
            None => self.spawn_terminal_entity(terminal_id, working_directory, cx),
            // A workspace-tracking terminal follows the workspace when it
            // moves — its workspace anchor is the test, never the live cwd,
            // so a `cd` inside the shell can't read as a move. A terminal
            // spawned at a recorded directory stays where it was put.
            Some(spawned_at) if workspace_bound && spawned_at != working_directory => {
                // Respawning is invisible for a terminal that has only ever
                // shown a prompt, but one that has run anything holds work
                // the PTY kill would destroy — or a launch line it would
                // re-run — so it detaches to the Terminals group instead.
                let has_run = self
                    .right_panel_terminals
                    .get(&terminal_id)
                    .is_some_and(|terminal| terminal.read(cx).last_command_started_at().is_some())
                    || self
                        .right_panel_terminal_programs
                        .contains_key(&terminal_id);
                if has_run {
                    self.detach_terminal_to_group(terminal_id, cx);
                } else {
                    self.spawn_terminal_entity(terminal_id, working_directory, cx);
                }
            }
            Some(_) => {}
        }
    }

    pub(super) fn ensure_right_panel_terminals(&mut self, cx: &mut Context<Self>) {
        let active_terminal_ids = self
            .right_panel_surfaces
            .iter()
            .filter_map(RightPanelSurface::terminal_id)
            .collect::<Vec<_>>();
        let retained_terminal_ids = active_terminal_ids
            .iter()
            .copied()
            .chain(self.right_panel_states.values().flat_map(|state| {
                state
                    .surfaces
                    .iter()
                    .filter_map(RightPanelSurface::terminal_id)
            }))
            // Global terminals belong to no surface list; their records are
            // what keep their entities alive.
            .chain(self.terminal_records.keys().copied())
            .collect::<HashSet<_>>();
        self.right_panel_terminals
            .retain(|terminal_id, _| retained_terminal_ids.contains(terminal_id));
        self.right_panel_terminal_commands
            .retain(|terminal_id, _| retained_terminal_ids.contains(terminal_id));
        self.right_panel_terminal_programs
            .retain(|terminal_id, _| retained_terminal_ids.contains(terminal_id));
        self.sandbox_sign_in_tabs
            .retain(|terminal_id, _| retained_terminal_ids.contains(terminal_id));
        self.custom_command_runs
            .retain(|terminal_id, _| retained_terminal_ids.contains(terminal_id));
        for terminal_id in active_terminal_ids {
            self.ensure_right_panel_terminal(terminal_id, cx);
        }
    }

    fn render_right_panel_header(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let active_surface = self.right_panel_active_surface;
        let mut tabs = div()
            .id("right-panel-tabs")
            .h_full()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(4.0))
            .overflow_x_scroll()
            .track_scroll(&self.right_panel_tabs_scroll_handle);
        for (index, surface) in self.right_panel_surfaces.iter().cloned().enumerate() {
            let active = active_surface == Some(index);
            let dirty = self.right_panel_surface_is_dirty(&surface);
            let label = SharedString::from(match &surface {
                // Browser tabs read like browser tabs: the page title once
                // known, the address until then.
                RightPanelSurface::Browser(browser_id) => self
                    .right_panel_browser_titles
                    .get(browser_id)
                    .cloned()
                    .or_else(|| {
                        self.right_panel_browsers
                            .get(browser_id)
                            .and_then(|browser| browser.read(cx).tab_label())
                    })
                    .unwrap_or_else(|| surface.label()),
                // Work-item tabs name the open item: "#123".
                RightPanelSurface::GitHub(project_id) => self.github_surface_label(*project_id),
                // A side-chat tab names its session once a title exists.
                RightPanelSurface::SideChat(session_id) => self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == *session_id)
                    .map(|session| session.display_title().to_owned())
                    .filter(|title| title != AgentSession::DEFAULT_TITLE)
                    .unwrap_or_else(|| surface.label()),
                _ => {
                    right_panel_tab_label(&surface, self.right_panel_files_selected_path.as_deref())
                }
            });
            let icon_path = match &surface {
                // A command's terminal tab wears the icon it was configured
                // with; every other surface resolves its own.
                RightPanelSurface::Terminal(terminal_id) => self
                    .right_panel_terminal_commands
                    .get(terminal_id)
                    .map(|command| crate::custom_commands::icon_path(command.icon))
                    .unwrap_or_else(|| {
                        right_panel_tab_icon(
                            &surface,
                            self.right_panel_files_selected_path.as_deref(),
                        )
                    }),
                // Work-item tabs wear the open item's state glyph.
                RightPanelSurface::GitHub(project_id) => self.github_surface_icon(*project_id),
                _ => {
                    right_panel_tab_icon(&surface, self.right_panel_files_selected_path.as_deref())
                }
            };
            let uses_file_icon = matches!(
                &surface,
                RightPanelSurface::File(_)
                    | RightPanelSurface::FileAtRef { .. }
                    | RightPanelSurface::Plan { .. }
            ) || matches!(&surface, RightPanelSurface::Files)
                && self.right_panel_files_selected_path.is_some();
            // A plan tab is the session's own document — it stays mounted
            // for the session's life, so the strip draws no close control.
            let closable = right_panel_surface_is_closable(&surface);
            let activate_weak = cx.entity().downgrade();
            let close_weak = cx.entity().downgrade();
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("right-panel-tab-{index}")))
                    .h(px(28.0))
                    .min_w(px(100.0))
                    .max_w(px(176.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_default()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .when(active, |element| element.bg(theme.overlay_strong))
                    .when(!active, |element| {
                        element.hover(|element| element.bg(theme.overlay))
                    })
                    .child(if uses_file_icon {
                        file_icon(icon_path, 13.0).into_any_element()
                    } else {
                        icon(icon_path, 13.0, theme.text_secondary).into_any_element()
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(if active {
                                theme.text
                            } else {
                                theme.text_secondary
                            })
                            .child(label),
                    )
                    .when(dirty, |element| {
                        element.child(
                            div()
                                .id(SharedString::from(format!("right-panel-tab-dirty-{index}")))
                                .size(px(7.0))
                                .flex_none()
                                .rounded_full()
                                .bg(theme.warning)
                                .tooltip(|window, cx| {
                                    Tooltip::new(tr!(
                                        "files.unsaved_changes",
                                        shortcut =
                                            crate::platform::primary_shortcut("⌘S", "Ctrl+S")
                                    ))
                                    .build(window, cx)
                                }),
                        )
                    })
                    .when(closable, |element| {
                        element.child(
                            div()
                                .id(SharedString::from(format!("close-right-panel-tab-{index}")))
                                .w(px(16.0))
                                .h(px(16.0))
                                .rounded(px(4.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|element| element.bg(theme.overlay_strong))
                                .child(icon("icons/x.svg", 10.0, theme.text_tertiary))
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = close_weak.update(cx, |this, cx| {
                                        this.close_right_panel_surface(index, cx);
                                    });
                                }),
                        )
                    })
                    .on_click(move |_, _, cx| {
                        let _ = activate_weak.update(cx, |this, cx| {
                            this.right_panel_active_surface = Some(index);
                            this.reveal_right_panel_tab(index);
                            // A parked strip sheds its diff snapshot — the
                            // first activation after restore refetches it.
                            if this.right_panel_surfaces.get(index)
                                == Some(&RightPanelSurface::Diff)
                                && this.right_panel_diff_snapshot.is_none()
                                && !this.right_panel_diff_loading
                            {
                                this.refresh_right_panel_diff(cx);
                            }
                            this.request_active_terminal_focus();
                            cx.notify();
                        });
                    }),
            );
        }
        // The add button trails the rightmost tab so it reads as part of the
        // strip; it scrolls with the tabs and opens its menu on a short hover.
        if !self.right_panel_surfaces.is_empty() {
            let weak = cx.entity().downgrade();
            let existing_surfaces = self.right_panel_surfaces.clone();
            // Managed sessions get no terminal, file-tree, or review tabs —
            // the menu only offers what the owner's strip can host.
            let options: Vec<RightPanelSurface> = [
                RightPanelSurface::new_browser(),
                RightPanelSurface::new_terminal(),
                RightPanelSurface::Files,
                RightPanelSurface::Diff,
                RightPanelSurface::Goals,
            ]
            .into_iter()
            .filter(|surface| self.right_panel_owner_allows(surface))
            .collect();
            let handle = self.menu_handle("add-right-panel-surface", cx);
            tabs = tabs.child(
                div()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(dropdown_menu_on_hover(
                        icon_button("add-right-panel-surface", "icons/plus.svg", theme),
                        "add-right-panel-surface-menu",
                        &handle,
                        MenuAlign::BelowLeft,
                        move |_| {
                            options
                                .clone()
                                .into_iter()
                                .map(|surface| {
                                    let weak = weak.clone();
                                    let open_surface = surface.clone();
                                    let already_open =
                                        reusable_surface_index(&existing_surfaces, &surface)
                                            .is_some();
                                    MenuItem::new(surface.label(), move |_, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.open_right_panel_surface(open_surface.clone(), cx);
                                        });
                                    })
                                    .icon(surface.icon_path())
                                    .selected(already_open)
                                })
                                .collect()
                        },
                    )),
            );
        }
        tabs = tabs.child(div().w(px(TAB_SCROLL_FADE_WIDTH)).h(px(1.0)).flex_none());

        let fullscreen = self.panel_fullscreen_active();
        // The maximized layer only reaches the window's left edge — and runs
        // under the traffic lights — once the sidebar is fully hidden; while
        // the sidebar holds the edge it owns the clearance instead.
        let covers_window_chrome = fullscreen && self.sidebar_rendered_width <= 0.0;
        let mut header = div()
            .id("right-panel-header")
            .h(px(48.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            // Maximized over a hidden sidebar, the strip's leading edge runs
            // under the traffic lights; the same clearance the sidebar
            // reserves keeps the tabs clickable.
            .pl(px(if covers_window_chrome {
                TRAFFIC_LIGHT_CLEARANCE
            } else {
                10.0
            }))
            .pr(px(14.0))
            // On client-decorated platforms the maximized layer also covers
            // the sidebar's window controls, so the header hosts them while
            // it owns the window's top edge. A no-op where the OS draws them.
            .when(covers_window_chrome, |header| {
                header.children(self.render_client_window_controls(
                    super::window_chrome::WindowControlSide::Left,
                    window,
                    cx,
                ))
            })
            .child(
                div()
                    .relative()
                    .h_full()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .child(tabs)
                    .when_some(self.right_panel_pending_tab_reveal, |element, tab_index| {
                        element.child(tab_scroll_reveal_guard(
                            self.right_panel_tabs_scroll_handle.clone(),
                            tab_index,
                            cx.entity().downgrade(),
                        ))
                    })
                    .child(tab_scroll_fade(
                        self.right_panel_tabs_scroll_handle.clone(),
                        TabScrollFadeSide::Left,
                        theme.surface,
                    ))
                    .child(tab_scroll_fade(
                        self.right_panel_tabs_scroll_handle.clone(),
                        TabScrollFadeSide::Right,
                        theme.surface,
                    )),
            );

        if self.active_right_panel_surface().is_some() {
            let maximized = self.fullscreen_surface.is_some();
            let focus = self.transcript_control_focus("right-panel-maximize-toggle", cx);
            let (icon_path, label) = if maximized {
                ("icons/minimize.svg", tr!("right_panel.restore"))
            } else {
                ("icons/maximize.svg", tr!("right_panel.maximize"))
            };
            header = header.child(
                div()
                    .id("right-panel-maximize-toggle")
                    .track_focus(&focus)
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
                    .child(icon(icon_path, 13.0, theme.text_tertiary))
                    .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_panel_fullscreen(cx)))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.toggle_panel_fullscreen(cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }

        self.window_drag_region(
            header.child(self.render_panel_toggles(cx)).children(
                self.render_client_window_controls(
                    super::window_chrome::WindowControlSide::Right,
                    window,
                    cx,
                ),
            ),
            cx,
        )
    }

    fn render_right_panel_chooser(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        // The owner decides which cards the chooser shows — a managed
        // session offers just the webview.
        let cards: Vec<(RightPanelSurface, String)> = [
            (
                RightPanelSurface::new_browser(),
                tr!("right_panel.browser_description"),
            ),
            (
                RightPanelSurface::new_terminal(),
                tr!("right_panel.terminal_description"),
            ),
            (
                RightPanelSurface::Files,
                tr!("right_panel.files_description"),
            ),
            (RightPanelSurface::Diff, tr!("right_panel.diff_description")),
            (
                RightPanelSurface::Goals,
                tr!("right_panel.goals_description"),
            ),
        ]
        .into_iter()
        .filter(|(surface, _)| self.right_panel_owner_allows(surface))
        .collect();
        div()
            .id("right-panel-chooser")
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .px(px(20.0))
            .pb(px(32.0))
            .child(
                div()
                    .w_full()
                    .max_w(sp(420.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!("right_panel.open_surface")),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(tr!("right_panel.choose_surface")),
                    )
                    .children(cards.chunks(2).enumerate().map(|(row, cards)| {
                        div()
                            .mt(px(if row == 0 { 18.0 } else { 8.0 }))
                            .w_full()
                            .flex()
                            .gap(px(8.0))
                            .children(cards.iter().map(|(surface, description)| {
                                self.render_right_panel_card(
                                    surface.clone(),
                                    description.clone(),
                                    cx,
                                )
                            }))
                    })),
            )
    }

    fn render_right_panel_card(
        &self,
        surface: RightPanelSurface,
        description: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let icon_path = surface.icon_path();
        let label = surface.label();
        div()
            .id(SharedString::from(format!(
                "right-panel-card-{}",
                label.to_lowercase()
            )))
            .h(sp(112.0))
            .flex_1()
            .min_w_0()
            .p(sp(14.0))
            .rounded(px(10.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .bg(theme.composer)
            .flex()
            .flex_col()
            .items_start()
            .cursor_default()
            .hover(|element| element.bg(theme.raised).border_color(theme.text_ghost))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon(icon_path, 18.0, theme.text_secondary))
            .child(
                div()
                    .mt(sp(12.0))
                    .w_full()
                    .min_w_0()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(label),
            )
            .child(
                div()
                    .mt(sp(4.0))
                    .w_full()
                    .min_w_0()
                    .text_size(sp(12.5))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .whitespace_normal()
                    .line_clamp(2)
                    .text_overflow(gpui::TextOverflow::Truncate("...".into()))
                    .child(description),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_right_panel_surface(surface.clone(), cx);
            }))
    }

    fn render_right_panel_files(
        &mut self,
        panel_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if let Some(relative_path) = self.right_panel_files_selected_path.clone() {
            self.render_right_panel_file(relative_path, panel_width, true, None, window, cx)
        } else {
            self.render_right_panel_working_tree(None, cx)
        }
    }

    fn render_right_panel_working_tree(
        &self,
        selected_path: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let Some(project) = self.selected_project() else {
            return self.render_right_panel_empty_message(
                tr!("files.no_project_open"),
                tr!("files.no_project_open_description"),
                cx,
            );
        };
        let project_name = project.display_name();
        // Read only. The walk is filesystem I/O, so it happens in
        // `refresh_right_panel_working_tree`, never in a frame.
        let entries = self.right_panel_working_tree.clone();
        let waku = cx.entity().downgrade();

        let mut list = div().flex().flex_col().py(px(6.0));
        for entry in entries {
            let relative_path = entry.relative_path.clone();
            let absolute_path = entry.absolute_path.clone();
            let is_dir = entry.is_dir;
            let selected = selected_path == Some(relative_path.as_str());
            let menu_id = SharedString::from(format!("file-menu-{relative_path}"));
            let menu = self.menu_handle(menu_id.clone(), cx);
            let menu_path = absolute_path.clone();
            let menu_name = entry.name.clone();
            let row_focus = self.transcript_control_focus(
                format!("right-panel-tree-row-{}", absolute_path.display()),
                cx,
            );
            let row = div()
                .id(SharedString::from(format!(
                    "right-panel-file-{relative_path}"
                )))
                .h(px(30.0))
                .mx(px(8.0))
                .pl(px(8.0 + entry.depth as f32 * 16.0))
                .pr(px(8.0))
                .rounded(px(8.0))
                .flex()
                .track_focus(&row_focus)
                .tab_index(0)
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .when(selected, |element| element.bg(theme.overlay_strong))
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|element| element.bg(theme.focus_highlight()))
                .child(if is_dir {
                    icon(
                        if entry.expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        },
                        10.0,
                        theme.text_ghost,
                    )
                    .into_any_element()
                } else {
                    div().w(px(10.0)).h(px(10.0)).flex_none().into_any_element()
                })
                .when_some(entry.file_icon, |element, file_icon_path| {
                    element.child(file_icon(file_icon_path, 14.0))
                })
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(entry.name),
                );
            let click_relative_path = relative_path.clone();
            let click_absolute_path = absolute_path.clone();
            let key_relative_path = relative_path.clone();
            let key_absolute_path = absolute_path;
            let row = row
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.activate_right_panel_working_tree_entry(
                        click_relative_path.clone(),
                        click_absolute_path.clone(),
                        is_dir,
                        cx,
                    );
                }))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                    if !event.keystroke.modifiers.modified()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        this.activate_right_panel_working_tree_entry(
                            key_relative_path.clone(),
                            key_absolute_path.clone(),
                            is_dir,
                            cx,
                        );
                        cx.stop_propagation();
                    }
                }));
            let waku_menu = waku.clone();
            list = list.child(context_menu(
                div().w_full().child(row),
                menu_id,
                &menu,
                move |cx| {
                    waku_menu
                        .read_with(cx, |this, _| {
                            this.working_tree_row_menu(&waku_menu, &menu_path, &menu_name, is_dir)
                        })
                        .ok()
                        .unwrap_or_default()
                },
            ));
        }

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(42.0))
                    .flex_none()
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b(hairline())
                    .border_color(theme.separator)
                    .child(icon("icons/folder.svg", 13.0, theme.text_tertiary))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child(project_name),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("right-panel-files-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.right_panel_files_scroll_handle)
                            .child(list),
                    )
                    .child(scrollbar::vertical(
                        &self.right_panel_files_scroll_handle,
                        &self.right_panel_files_scrollbar,
                    ))
                    .children(self.right_panel_tree_scroll_to.map(|index| {
                        // Rows are `h(30)` under the list's `py(6)` — index to
                        // pixels directly rather than waiting a second frame
                        // for measured bounds.
                        let scroll = self.right_panel_files_scroll_handle.clone();
                        let waku = cx.entity().downgrade();
                        canvas(
                            move |_, window, _| {
                                let viewport = scroll.bounds();
                                let offset = scroll.offset();
                                let row_top = px(6.0 + index as f32 * 30.0);
                                let row_bottom = row_top + px(30.0);
                                let visible_top = -offset.y;
                                let visible_bottom =
                                    visible_top + (viewport.bottom() - viewport.top());
                                let mut y = offset.y;
                                if row_top < visible_top {
                                    y = -row_top;
                                } else if row_bottom > visible_bottom {
                                    y = -(row_bottom - (viewport.bottom() - viewport.top()));
                                }
                                if y != offset.y {
                                    scroll.set_offset(point(offset.x, y));
                                }
                                window.on_next_frame(move |_, cx| {
                                    let _ = waku.update(cx, |this, cx| {
                                        if this.right_panel_tree_scroll_to == Some(index) {
                                            this.right_panel_tree_scroll_to = None;
                                            cx.notify();
                                        }
                                    });
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    })),
            )
    }

    /// The right-click menu on a working-tree row.
    ///
    /// Every action acts on a local path, so a remote session — whose paths
    /// live on the daemon host — gets an empty list and the menu never opens.
    fn working_tree_row_menu(
        &self,
        waku: &WeakEntity<Self>,
        absolute_path: &Path,
        name: &str,
        is_dir: bool,
    ) -> Vec<MenuItem> {
        if self.is_remote_path(absolute_path) {
            return Vec::new();
        }
        let mut items = Vec::new();
        if let Some(app) = self.preferred_open_in_app() {
            let (label, image, bundle_id) = (app.label, app.icon.clone(), app.bundle_id);
            let path = absolute_path.to_path_buf();
            items.push(
                MenuItem::new(tr!("files.open_in", app = label), move |_, _| {
                    crate::platform::open_path_in_app(&path, bundle_id);
                })
                .image(image),
            );
        }
        if !self.open_in_apps.is_empty() {
            let apps = self.open_in_apps.clone();
            let path = absolute_path.to_path_buf();
            items.push(MenuItem::Submenu {
                label: tr!("files.open_with").into(),
                value: None,
                items: Rc::new(move |_| {
                    apps.iter()
                        .map(|app| {
                            let path = path.clone();
                            let bundle_id = app.bundle_id;
                            MenuItem::new(app.label, move |_, _| {
                                crate::platform::open_path_in_app(&path, bundle_id);
                            })
                            .image(app.icon.clone())
                        })
                        .collect()
                }),
            });
        }
        if !items.is_empty() {
            items.push(MenuItem::Separator);
        }
        if !is_dir {
            let directory = absolute_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let suggested = name.to_owned();
            let source = absolute_path.to_path_buf();
            let waku = waku.clone();
            items.push(
                MenuItem::new(tr!("files.save_as"), move |_, cx| {
                    let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
                    let waku = waku.clone();
                    let source = source.clone();
                    cx.spawn(async move |cx| {
                        let Ok(Ok(Some(destination))) = receiver.await else {
                            return;
                        };
                        let result = cx
                            .background_executor()
                            .spawn(async move { std::fs::copy(&source, &destination) })
                            .await;
                        waku.update(cx, |this, cx| {
                            if let Err(error) = result {
                                this.show_toast(tr!(
                                    "files.save_as_failed",
                                    error = error.to_string()
                                ));
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                    .detach();
                })
                .icon("icons/download.svg"),
            );
        }
        {
            let path = absolute_path.to_string_lossy().into_owned();
            let waku = waku.clone();
            items.push(
                MenuItem::new(tr!("files.copy_path"), move |_, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                    waku.update(cx, |this, cx| {
                        this.show_toast(tr!("common.copied"));
                        cx.notify();
                    })
                    .ok();
                })
                .icon("icons/copy.svg"),
            );
        }
        if !is_dir {
            let path = absolute_path.to_path_buf();
            let waku = waku.clone();
            items.push(
                MenuItem::new(tr!("files.copy_file_contents"), move |_, cx| {
                    let path = path.clone();
                    let waku = waku.clone();
                    cx.spawn(async move |cx| {
                        let contents = cx
                            .background_executor()
                            .spawn(async move { std::fs::read_to_string(&path) })
                            .await;
                        waku.update(cx, |this, cx| {
                            match contents {
                                Ok(contents) => {
                                    cx.write_to_clipboard(ClipboardItem::new_string(contents));
                                    this.show_toast(tr!("common.copied"));
                                }
                                Err(_) => {
                                    this.show_toast(tr!("files.copy_contents_failed"));
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                })
                .icon("icons/copy.svg"),
            );
        }
        {
            let path = absolute_path.to_path_buf();
            items.push(
                MenuItem::new(tr!("common.reveal_in_finder"), move |_, cx| {
                    crate::platform::reveal_in_file_manager(&path, cx);
                })
                .icon("icons/folder-open.svg"),
            );
        }
        items
    }

    /// The `working_tree_row_menu` for a `file_link` target — the same
    /// actions, resolved from the link's possibly workspace-relative path.
    /// Runs at menu-open time, a one-shot user action, so the `is_dir`
    /// probe's stat is allowed where a frame's would not be. A remote path
    /// resolves to no items, matching the tree rows.
    pub(super) fn file_link_menu(
        &self,
        waku: &WeakEntity<Self>,
        path: &str,
        cx: &App,
    ) -> Vec<MenuItem> {
        if crate::model::ReferenceContext::decode_reference(path).is_some() {
            let target = path.to_owned();
            let waku = waku.clone();
            return vec![MenuItem::new(tr!("common.open_link"), move |_, cx| {
                let _ = waku.update(cx, |this, cx| {
                    this.open_transcript_link(&target, cx);
                });
            })];
        }
        let path = Path::new(path.trim());
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(root) = self.resolve_right_panel_files_root(cx) {
            root.join(path)
        } else {
            return Vec::new();
        };
        if self.is_remote_path(&absolute_path) || !absolute_path.exists() {
            return Vec::new();
        }
        let name = absolute_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        self.working_tree_row_menu(waku, &absolute_path, &name, absolute_path.is_dir())
    }

    /// The armed deliverable's own page: a previewable file renders with the
    /// file viewer's machinery at the chat column's full width — the
    /// deliverable's page, not the boss chat's right panel. The composer rides
    /// underneath with the boss's chip, so a send from here lands on the
    /// boss chat with the deliverable attached. `None` whenever no armed deliverable
    /// is holding a page open, which drops stale state as it is found.
    pub(super) fn render_deliverable_preview_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some((key, deliverable_id)) = self.live_deliverable_page() else {
            self.unmount_deliverable_page(cx);
            return None;
        };
        let deliverable = self.boss_ui.states.get(&key).and_then(|state| {
            state
                .deliverables
                .iter()
                .find(|deliverable| deliverable.id == deliverable_id && !deliverable.directory)
        });
        let Some((deliverable_title, relative_path)) = deliverable.and_then(|deliverable| {
            std::path::Path::new(&deliverable.path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|file_name| (deliverable.name.clone(), file_name.to_owned()))
        }) else {
            self.unmount_deliverable_page(cx);
            return None;
        };
        Some(
            self.render_right_panel_file(
                relative_path,
                self.chat_viewport_width(window),
                false,
                Some(deliverable_title),
                window,
                cx,
            )
            .into_any_element(),
        )
    }

    /// `show_tree` mounts the working-tree column beside the editor — the
    /// strip's own browsing surface. A deliverable's preview page leaves it
    /// out: the page previews one published file, not its directory.
    /// `deliverable_title` lays the surface out as a page — the markdown preview
    /// centers its column like the maximized panel, a bottom fade marks
    /// content scrolling under the composer as the transcript's does, and the
    /// header carries the deliverable's name like a top bar title.
    fn render_right_panel_file(
        &mut self,
        relative_path: String,
        panel_width: f32,
        show_tree: bool,
        deliverable_title: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let deliverable_page = deliverable_title.is_some();
        let fullscreen = self.panel_fullscreen_active();
        let file_tree_width = if !show_tree || fullscreen {
            0.0
        } else if !self.right_panel_file_tree_visible {
            0.0
        } else {
            fitted_file_tree_width(panel_width, self.right_panel_file_tree_width)
        };
        let (editor_state, writable, _) =
            self.ensure_right_panel_file_editor(&relative_path, window, cx);

        // Deliverables use a per-file reading toggle; other Markdown files
        // carry the global source/preview toggle. Every other
        // language always shows source. Image files render pixels instead of
        // text — SVGs alone keep a source view behind the toggle, since
        // their text stays editable. `.wireframe.json` previews as themed
        // elements behind the wireframes experiment; with the flag off, or
        // flipped to source, it is just a JSON file in the editor.
        let is_markdown = file_highlighter_language(&relative_path) == "markdown";
        let is_svg =
            image_preview::image_format_for_name(&relative_path) == Some(gpui::ImageFormat::Svg);
        let is_wireframe =
            self.state.wireframes_experiment_enabled && is_wireframe_document(&relative_path);
        let image_mode = self
            .right_panel_file_editors
            .get(&relative_path)
            .is_some_and(|editor| file_shows_image(editor, &relative_path));
        let wireframe_mode = is_wireframe
            && self
                .right_panel_file_editors
                .get(&relative_path)
                .is_none_or(|editor| !editor.show_source);
        let preview = !image_mode
            && !wireframe_mode
            && markdown_preview_mode(
                is_markdown,
                deliverable_page,
                self.right_panel_file_editors
                    .get(&relative_path)
                    .is_some_and(|editor| editor.show_source),
                self.state.markdown_preview,
            );
        let body = if image_mode {
            self.render_file_image_preview(&relative_path, cx)
        } else if wireframe_mode {
            self.render_file_wireframe_preview(&relative_path, &editor_state, cx)
        } else if preview {
            self.render_file_markdown_preview(
                &relative_path,
                &editor_state,
                panel_width - file_tree_width,
                deliverable_page,
                window,
                cx,
            )
        } else {
            self.render_file_editor_body(
                &relative_path,
                &editor_state,
                panel_width - file_tree_width,
                writable,
                window,
                cx,
            )
        };
        let github_url = self.right_panel_files_root.clone().and_then(|workspace| {
            let snapshot = self.branch_snapshot_for_workspace(&workspace, cx)?;
            match self.remote_file_for(&workspace, &relative_path, cx) {
                // Verified against the remote-tracking refs — the remote
                // serves the file at this ref + repo-relative path.
                Some(Ok(Some(remote))) => {
                    let base = branches::github_remote_base(snapshot.origin_url.as_deref()?)?;
                    Some(format!(
                        "{base}/blob/{}/{}",
                        branches::github_url_path_encode(&remote.reference),
                        branches::github_url_path_encode(&remote.path)
                    ))
                }
                // Verified absent — untracked, uncommitted, or unpushed.
                Some(Ok(None)) => None,
                // Unverifiable — keep the optimistic guess rather than
                // drop an affordance that used to render unconditionally.
                Some(Err(_)) => branches::github_file_url(&snapshot, &relative_path),
                // Answer in flight; drawing the old guess here would
                // flash a button that can vanish a frame later.
                None => None,
            }
        });
        let github_button = github_url.map(|url| {
            let focus = self.transcript_control_focus("file-open-on-github", cx);
            let label = tr!("files.open_on_github");
            let click_url = url.clone();
            let key_url = url;
            div()
                .id("file-open-on-github")
                .track_focus(&focus)
                .tab_index(0)
                .size(px(26.0))
                .rounded(px(9.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .hover(|style| style.bg(theme.overlay))
                .child(icon("icons/github.svg", 12.0, theme.text_tertiary))
                .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
                .on_click(move |_, _, cx| cx.open_url(&click_url))
                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.open_url(&key_url);
                        cx.stop_propagation();
                    }
                })
        });
        let tree_toggle = (show_tree && !fullscreen).then(|| {
            let focus = self.transcript_control_focus("file-tree-toggle", cx);
            let label = if self.right_panel_file_tree_visible {
                tr!("files.hide_tree")
            } else {
                tr!("files.show_tree")
            };
            div()
                .id("file-tree-toggle")
                .track_focus(&focus)
                .tab_index(0)
                .size(px(26.0))
                .rounded(px(9.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .hover(|style| style.bg(theme.overlay))
                .child(icon("icons/panel-right.svg", 12.0, theme.text_tertiary))
                .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.right_panel_file_tree_visible = !this.right_panel_file_tree_visible;
                    cx.notify();
                }))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.right_panel_file_tree_visible = !this.right_panel_file_tree_visible;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
        });
        let preview_toggle = (is_markdown || is_svg || is_wireframe).then(|| {
            let focus = self.transcript_control_focus("file-preview-toggle", cx);
            let (icon_path, label) = if preview || image_mode || wireframe_mode {
                ("icons/pencil.svg", tr!("files.edit_markdown_source"))
            } else if is_svg {
                ("icons/eye.svg", tr!("files.preview_image"))
            } else if is_wireframe {
                ("icons/eye.svg", tr!("files.preview_wireframe"))
            } else {
                ("icons/eye.svg", tr!("files.preview_markdown"))
            };
            div()
                .id("file-preview-toggle")
                .track_focus(&focus)
                .tab_index(0)
                .size(px(26.0))
                .rounded(px(9.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .hover(|style| style.bg(theme.overlay))
                .child(icon(icon_path, 12.0, theme.text_tertiary))
                .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
                .on_click(cx.listener({
                    let relative_path = relative_path.clone();
                    move |this, _, _, cx| {
                        if is_svg || is_wireframe || deliverable_page {
                            this.toggle_file_source_view(&relative_path, cx);
                        } else {
                            this.toggle_markdown_preview(cx);
                        }
                    }
                }))
                .on_key_down(cx.listener({
                    let relative_path = relative_path.clone();
                    move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if is_svg || is_wireframe || deliverable_page {
                                this.toggle_file_source_view(&relative_path, cx);
                            } else {
                                this.toggle_markdown_preview(cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                }))
        });
        let editor = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(42.0))
                    .flex_none()
                    // A deliverable's page owns the chat column, so its top
                    // bar is the surface under the window's left edge — it
                    // keeps the same traffic-light clearance the chat header
                    // does while the sidebar is too narrow to host them.
                    .pl(px(if deliverable_page {
                        if self.sidebar_visible {
                            16.0 + (TRAFFIC_LIGHT_CLEARANCE - self.sidebar_rendered_width).max(0.0)
                        } else {
                            TRAFFIC_LIGHT_CLEARANCE
                        }
                    } else {
                        16.0
                    }))
                    .pr(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b(hairline())
                    .border_color(theme.separator)
                    .children(
                        (deliverable_page && !self.sidebar_visible)
                            .then(|| {
                                self.render_client_window_controls(
                                    super::window_chrome::WindowControlSide::Left,
                                    window,
                                    cx,
                                )
                            })
                            .flatten(),
                    )
                    .when_some(deliverable_title, |header, title| {
                        header.child(
                            div()
                                .min_w_0()
                                .flex_shrink(1.0)
                                .truncate()
                                .text_size(sp(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(title),
                        )
                    })
                    .child(file_icon(file_icon_for_path(&relative_path), 13.0))
                    .child(file_link(
                        div()
                            .id(SharedString::from(format!(
                                "file-viewer-path-{relative_path}"
                            )))
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(relative_path.clone()),
                        &self.transcript_control_focus(
                            format!("file-viewer-path-{relative_path}"),
                            cx,
                        ),
                        relative_path.clone(),
                        self,
                        &cx.entity().downgrade(),
                        format!("file-link-menu-viewer-{relative_path}"),
                        cx,
                    ))
                    .children(github_button)
                    .children(tree_toggle)
                    .children(preview_toggle),
            )
            .child(body)
            // A deliverable's page rides above the composer like the transcript:
            // its rows dissolve into the surface where more waits below.
            .when_some(
                deliverable_page
                    .then(|| {
                        if image_mode {
                            None
                        } else if preview {
                            self.preview_list_state(&relative_path)
                                .map(|state| -> Box<dyn scrollbar::Scrollable> { Box::new(state) })
                        } else {
                            Some(Box::new(self.right_panel_editor_scroll_handle.clone())
                                as Box<dyn scrollbar::Scrollable>)
                        }
                    })
                    .flatten(),
                |editor, handle| {
                    editor.child(scrollbar::edge_fade(
                        handle,
                        scrollbar::FadeEdge::Bottom,
                        theme.surface,
                    ))
                },
            );

        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .child(editor)
            // Fullscreen drops the file tree entirely; its resize handle
            // would fight a surface that owns the window's width. A page
            // render with `show_tree` off leaves the column out too.
            .when(
                show_tree && !fullscreen && self.right_panel_file_tree_visible,
                |element| {
                    element.child(
                        div()
                            .w(px(file_tree_width))
                            .min_w(px(FILE_TREE_MIN_WIDTH))
                            .h_full()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .relative()
                            .border_l(hairline())
                            .border_color(theme.separator)
                            .child(self.render_right_panel_working_tree(Some(&relative_path), cx))
                            .child(self.render_panel_resize_handle(
                                "right-panel-file-tree-resize-handle",
                                PanelResizeTarget::FileTree,
                                cx,
                            )),
                    )
                },
            )
    }

    /// The `FileAtRef` surface: a file's blob at a git ref, read-only — the
    /// working-tree editor's chrome minus everything that needs the file on
    /// disk (save, reveal, annotations, the tree column).
    fn render_right_panel_ref_file(
        &mut self,
        path: &str,
        git_ref: &str,
        _panel_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let (editor_state, scroll, scrollbar) =
            self.ensure_right_panel_ref_editor(path, git_ref, window, cx);
        // Cheap every frame — the read self-guards once started.
        self.read_right_panel_ref_file(path.to_owned(), git_ref.to_owned(), cx);
        let key = format!("{git_ref}:{path}");

        let text_size = self.state.code_font_size;
        let line_height = (text_size * 1.5).round();

        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(42.0))
                    .flex_none()
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b(hairline())
                    .border_color(theme.separator)
                    .child(file_icon(file_icon_for_path(path), 13.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(format!("{path} ({git_ref})")),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .bg(theme.surface)
                    .font_family(crate::fonts::current(cx).code)
                    .text_size(px(text_size))
                    .line_height(px(line_height))
                    .child(
                        div()
                            .id(SharedString::from(format!("ref-file-editor-{key}")))
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .child(div().w_full().px(px(16.0)).py(px(6.0)).child(editor_state)),
                    )
                    .child(scrollbar::vertical(&scroll, &scrollbar)),
            )
    }

    /// The ref surface's editor, created on first render: empty and locked,
    /// with the blob read kicked off to the background executor — the same
    /// shape the working-tree editor takes because `render` cannot touch
    /// the filesystem.
    fn ensure_right_panel_ref_editor(
        &mut self,
        path: &str,
        git_ref: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<TextInput>, ScrollHandle, Rc<ScrollbarState>) {
        let key = format!("{git_ref}:{path}");
        if let Some(editor) = self.right_panel_ref_editors.get(&key) {
            return (
                editor.state.clone(),
                editor.scroll.clone(),
                editor.scrollbar.clone(),
            );
        }
        let language = file_highlighter_language(path);
        let state = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .syntax(Some(language))
                .read_only(true)
                .accessibility_label(format!("{path} ({git_ref})"))
        });
        let editor = RightPanelRefEditor {
            state: state.clone(),
            requested: false,
            scroll: ScrollHandle::new(),
            scrollbar: ScrollbarState::new(),
        };
        let (scroll, scrollbar) = (editor.scroll.clone(), editor.scrollbar.clone());
        self.right_panel_ref_editors.insert(key, editor);
        (state, scroll, scrollbar)
    }

    /// `git show <ref>:<path>` into the surface's read-only editor. The read
    /// runs on the background executor; the landing drops when the session
    /// or workspace moved on, mirroring `read_right_panel_file_into_editor`.
    fn read_right_panel_ref_file(&mut self, path: String, git_ref: String, cx: &mut Context<Self>) {
        let key = format!("{git_ref}:{path}");
        let (Some(project_path), Some(session_id)) = (
            self.selected_workspace_path()
                .map(std::path::Path::to_path_buf),
            self.state.selected_session,
        ) else {
            return;
        };
        let Some(workspace) = self.workspace_client_for_path(&project_path) else {
            return;
        };
        let Some(editor) = self.right_panel_ref_editors.get_mut(&key) else {
            return;
        };
        // One shot: a second asker would only duplicate the read.
        if editor.requested {
            return;
        }
        editor.requested = true;
        cx.spawn(async move |waku, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let project_path = project_path.clone();
                    async move {
                        match workspace.request(waku_client::WorkspaceOperation::ReadFileAtRef {
                            cwd: project_path,
                            path,
                            git_ref,
                        }) {
                            Ok(waku_client::WorkspaceResult::TextFile { content }) => content,
                            Ok(_) => tr!(
                                "files.unable_to_edit",
                                error = "the daemon returned an invalid file response"
                            ),
                            Err(error) => {
                                tr!("files.unable_to_edit", error = error.to_string())
                            }
                        }
                    }
                })
                .await;
            waku.update(cx, |waku, cx| {
                if waku.state.selected_session != Some(session_id)
                    || waku
                        .selected_workspace_path()
                        .is_none_or(|current| current != project_path)
                {
                    // The editor swapped out with its session; whatever
                    // parked state it lives in keeps `requested` set, so a
                    // later restore skips a re-read — stale is acceptable
                    // for a committed blob.
                    return;
                }
                let Some(editor) = waku.right_panel_ref_editors.get_mut(&key) else {
                    return;
                };
                let state = editor.state.clone();
                state.update(cx, |state, cx| state.set_content(read, cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn ensure_right_panel_file_editor(
        &mut self,
        relative_path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<TextInput>, bool, bool) {
        // A `Cmd+P` confirm asks for editor focus here, on the first frame the
        // entity is known to exist; deferring once more lets this frame's
        // paint put the element in the dispatch tree before focus moves. Its
        // `path:line[:column]` target moves onto the editor itself — the caret
        // cannot land until the file's read does.
        let pending_open = if self
            .right_panel_pending_file_focus
            .as_ref()
            .is_some_and(|pending| pending.path == relative_path)
        {
            self.right_panel_pending_file_focus.take()
        } else {
            None
        };
        let focus_pending = pending_open.is_some();
        let (position_pending, heading_pending) = pending_open
            .map(|pending| (pending.position, pending.heading))
            .unwrap_or_default();
        if let Some(editor) = self.right_panel_file_editors.get_mut(relative_path) {
            if let Some(position) = position_pending {
                editor.pending_position = Some(position);
            }
            if heading_pending.is_some() {
                editor.pending_heading = heading_pending;
            }
            // An image or wireframe preview has no editor to focus — the
            // TextInput is not rendered while the pane shows the visual.
            if focus_pending
                && !file_shows_image(editor, relative_path)
                && !file_shows_wireframe(
                    editor,
                    relative_path,
                    self.state.wireframes_experiment_enabled,
                )
            {
                let focus = editor.state.read(cx).focus();
                window.on_next_frame(move |window, cx| window.focus(&focus, cx));
            }
            let found = (editor.state.clone(), editor.writable, editor.dirty);
            self.apply_pending_file_position(relative_path, window, cx);
            return found;
        }

        // Reached from `render`, so the file cannot be read here. The editor
        // starts empty and locked, and `read_right_panel_file_into_editor`
        // fills it in from the background executor a frame or two later.
        let language = file_highlighter_language(relative_path);
        let state = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .syntax(Some(language))
                .read_only(true)
                .accessibility_label(relative_path.to_owned())
        });

        // A draft-restored annotation for this file joins its editor now —
        // until this point it lived in `pending_file_annotations`, still
        // counted by the composer chip and drained into submissions.
        let annotations = Annotations {
            items: self
                .pending_file_annotations
                .remove(relative_path)
                .unwrap_or_default(),
            ..Default::default()
        };
        self.right_panel_file_editors.insert(
            relative_path.to_owned(),
            RightPanelFileEditor {
                state: state.clone(),
                disk_content: String::new(),
                writable: false,
                dirty: false,
                text_loaded: false,
                image: None,
                image_zoom: 0.0,
                image_pan_y: px(0.0),
                image_viewport: None,
                image_natural: None,
                show_source: false,
                wireframe: None,
                reading: false,
                read_epoch: 0,
                pending_position: position_pending,
                pending_heading: heading_pending,
                preview_list: ListState::new(0, ListAlignment::Top, px(1024.0)),
                annotations: Rc::new(RefCell::new(annotations)),
            },
        );

        // Dirty tracking follows content edits. Observing raw notifies would
        // also fire for caret blinks and selection drags, cloning the whole
        // file's text for each one.
        let subscribed_path = relative_path.to_owned();
        cx.subscribe(
            &state,
            move |this: &mut Self, state, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Edited) {
                    return;
                }
                let value = state.read(cx).content().to_owned();
                if let Some(editor) = this
                    .right_panel_file_editors
                    .get_mut(subscribed_path.as_str())
                {
                    let dirty = editor.writable && value != editor.disk_content;
                    if editor.dirty != dirty {
                        editor.dirty = dirty;
                        cx.notify();
                    }
                }
                // Any content change — typing, a replace, a reload from disk —
                // moves the text out from under an open find's match list.
                this.refresh_file_search_for_edit(subscribed_path.as_str(), cx);
            },
        )
        .detach();

        let focused_path = relative_path.to_owned();
        cx.subscribe(&state, move |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Focus) {
                this.reload_right_panel_file_if_clean(focused_path.as_str(), cx);
            }
        })
        .detach();

        self.read_right_panel_file_into_editor(relative_path.to_owned(), cx);
        self.apply_pending_file_position(relative_path, window, cx);
        // A fresh editor defaults to preview mode, so an image or a
        // wireframe never renders the TextInput the focus would land on.
        if focus_pending
            && image_preview::image_format_for_name(relative_path).is_none()
            && !(self.state.wireframes_experiment_enabled && is_wireframe_document(relative_path))
        {
            let focus = state.read(cx).focus();
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        }
        (state, false, false)
    }

    /// Lands a `path:line[:column]` jump the finder asked for: caret onto the
    /// line, then scrolled into view. `pending_position` waits inside the
    /// editor across renders because the file's read lands off the UI thread,
    /// so this runs from `ensure` until text exists to place the caret in.
    /// The reveal defers a frame for the same reason — `position_for_offset`
    /// measures the last painted layout.
    fn apply_pending_file_position(
        &mut self,
        relative_path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.right_panel_file_editors.get_mut(relative_path) else {
            return;
        };
        if !editor.reading {
            let state = editor.state.clone();
            let content = state.read(cx).content().to_owned();
            let heading_line = editor
                .pending_heading
                .take()
                .and_then(|heading| markdown_heading_line(&content, &heading));
            let position = editor
                .pending_position
                .take()
                .or_else(|| heading_line.map(|line| (line, 1)));
            let Some((line, column)) = position else {
                return;
            };
            let offset = cursor_offset_for_line_column(state.read(cx).content(), line, column);
            let preview_heading_offset =
                heading_line.map(|line| cursor_offset_for_line_column(&content, line, 1));
            state.update(cx, |state, cx| state.select_range(offset..offset, cx));
            let weak = cx.entity().downgrade();
            let path = relative_path.to_owned();
            window.on_next_frame(move |window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.reveal_editor_offset(&state, offset, cx);
                    if let Some(offset) = preview_heading_offset {
                        this.reveal_markdown_heading(&path, offset, 0, window, cx);
                    }
                });
            });
        }
    }

    /// The virtualized scroll state backing `relative_path`'s markdown
    /// preview. `None` while the file has no editor — the preview renders
    /// only after [`ensure_right_panel_file_editor`] has run, so a missing
    /// entry means the preview is gone too.
    pub(super) fn preview_list_state(&self, relative_path: &str) -> Option<ListState> {
        self.right_panel_file_editors
            .get(relative_path)
            .map(|editor| editor.preview_list.clone())
    }

    fn reveal_markdown_heading(
        &mut self,
        relative_path: &str,
        source_offset: usize,
        attempt: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.file_markdown_preview_active(relative_path) {
            return;
        }
        let Some(list) = self.preview_list_state(relative_path) else {
            return;
        };
        let target_block = {
            let preview = self.file_preview_markdown.borrow();
            let Some((_, view)) = preview.as_ref().filter(|(path, _)| path == relative_path) else {
                return;
            };
            let mut block_index = 0;
            loop {
                let Some(range) = view.block_source_range(block_index) else {
                    return;
                };
                if range.contains(&source_offset) {
                    break block_index;
                }
                block_index += 1;
            }
        };
        let target_bounds = self
            .file_preview_selection
            .registry
            .borrow()
            .entries()
            .iter()
            .filter(|entry| {
                !entry.geometry.is_missing()
                    && md::render::block_index_of_ordinal(entry.key.index) == target_block
            })
            .find_map(|entry| {
                md::render::text_range_bounds(&entry.geometry, &(0..entry.text.len()))
                    .into_iter()
                    .next()
            });
        let Some(target_bounds) = target_bounds else {
            // The preview mounts only the viewport's blocks, so the heading's
            // glyphs register only after its list item scrolls into view.
            if attempt < 4 {
                list.scroll_to_reveal_item(target_block);
                cx.notify();
                let path = relative_path.to_owned();
                cx.on_next_frame(window, move |this, window, cx| {
                    this.reveal_markdown_heading(&path, source_offset, attempt + 1, window, cx)
                });
            }
            return;
        };
        let viewport = list.viewport_bounds();
        let current = list.scroll_px_offset_for_scrollbar();
        let delta = target_bounds.top() - viewport.top();
        list.set_offset_from_scrollbar(point(current.x, current.y - delta));
        cx.notify();
    }

    /// Reads a file into its editor off the UI thread.
    ///
    /// One `read_to_string` of an arbitrarily large file — hundreds of frames
    /// for a big one — so it never runs in a frame. The editor keeps whatever
    /// it is already showing until the read lands.
    ///
    /// The result is applied only if the same files root is still active and
    /// the editor is still the one that asked, so a read started before a
    /// session switch or a terminal re-root cannot write another directory's
    /// text into the view.
    fn read_right_panel_file_into_editor(&mut self, relative_path: String, cx: &mut Context<Self>) {
        let Some(project_path) = self.right_panel_files_root.clone() else {
            // Nothing to read from. Say so in the editor rather than leaving it
            // looking like an empty file.
            if let Some(editor) = self.right_panel_file_editors.get_mut(&relative_path) {
                editor.reading = false;
                if file_shows_image(editor, &relative_path) {
                    editor.image = Some(Err(tr!("files.no_project_is_open")));
                } else {
                    editor.disk_content = tr!("files.no_project_is_open");
                    editor.writable = false;
                    editor.text_loaded = true;
                    let state = editor.state.clone();
                    let content = editor.disk_content.clone();
                    state.update(cx, |state, cx| state.set_content(content, cx));
                }
            }
            return;
        };
        let Some(workspace) = self.workspace_client_for_path(&project_path) else {
            return;
        };
        let Some(editor) = self.right_panel_file_editors.get_mut(&relative_path) else {
            return;
        };
        // A second asker would only duplicate the read and race to apply it.
        if editor.reading {
            return;
        }
        editor.reading = true;
        editor.read_epoch += 1;
        let epoch = editor.read_epoch;
        let image_format = if file_shows_image(editor, &relative_path) {
            image_preview::image_format_for_name(&relative_path)
        } else {
            None
        };

        cx.spawn(async move |waku, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let project_path = project_path.clone();
                    let relative_path = relative_path.clone();
                    async move {
                        match image_format {
                            Some(format) => RightPanelFileRead::Image(
                                read_right_panel_binary_file(
                                    &workspace,
                                    &project_path,
                                    &relative_path,
                                )
                                .map(|bytes| Arc::new(gpui::Image::from_bytes(format, bytes))),
                            ),
                            None => {
                                let (content, writable) = read_right_panel_file(
                                    &workspace,
                                    &project_path,
                                    &relative_path,
                                );
                                RightPanelFileRead::Text(content, writable)
                            }
                        }
                    }
                })
                .await;
            waku.update(cx, |waku, cx| {
                if waku.right_panel_files_root.as_deref() != Some(project_path.as_path()) {
                    // The editor moved into another context's stored state, or
                    // the files root changed. Clear the flag so a later reload
                    // can ask again, and drop the text.
                    if let Some(editor) = waku.right_panel_file_editors.get_mut(&relative_path) {
                        editor.reading = false;
                    }
                    return;
                }
                let Some(editor) = waku.right_panel_file_editors.get_mut(&relative_path) else {
                    return;
                };
                // A save landed while the read was in flight, so this text
                // describes the file as it was before that save.
                if editor.read_epoch != epoch {
                    return;
                }
                editor.reading = false;
                match read {
                    RightPanelFileRead::Image(result) => {
                        editor.image = Some(result);
                    }
                    RightPanelFileRead::Text(content, writable) => {
                        // An edit landed while the read was in flight; the
                        // user's text wins over the copy on disk.
                        if editor.dirty {
                            return;
                        }
                        if editor.text_loaded
                            && editor.disk_content == content
                            && editor.writable == writable
                        {
                            return;
                        }
                        editor.disk_content = content.clone();
                        editor.writable = writable;
                        editor.dirty = false;
                        editor.text_loaded = true;
                        let state = editor.state.clone();
                        state.update(cx, |state, cx| {
                            state.set_read_only(!writable);
                            state.set_content(content, cx);
                        });
                    }
                }
                cx.notify();
                // Toggling an SVG's source/preview mid-read leaves the other
                // payload missing; queue its read so the new view fills in.
                let needs_read = waku
                    .right_panel_file_editors
                    .get(&relative_path)
                    .is_some_and(|editor| {
                        (editor.show_source && !editor.text_loaded)
                            || (file_shows_image(editor, &relative_path) && editor.image.is_none())
                    });
                if needs_read {
                    waku.read_right_panel_file_into_editor(relative_path.clone(), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The editor body: a line-number gutter beside soft-wrapped text.
    ///
    /// The gutter is *painted*, not laid out — one canvas that shapes only the
    /// numbers currently on screen, the way Zed's editor element does. A div per
    /// line would put one layout node per line of the file in every frame, which
    /// is what made large files crawl.
    ///
    /// Row heights come from the text's measured layout rather than a nominal
    /// line height, so a soft-wrapped line still gets exactly one number and the
    /// two columns cannot drift apart down a long file.
    fn render_file_editor_body(
        &mut self,
        relative_path: &str,
        editor_state: &Entity<TextInput>,
        pane_width: f32,
        writable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        const GUTTER_PAD_RIGHT: f32 = 8.0;
        const CONTENT_PAD_TOP: f32 = 6.0;

        let text_size = self.state.code_font_size;
        let line_height = (text_size * 1.5).round();

        // An open find bar follows whichever file this body is showing; a
        // cheap comparison every frame, one recompute on the frame after the
        // visible file actually changes. The go-to-line bar instead closes —
        // its live preview would be a surprise in a file it never targeted.
        self.sync_file_search_target(relative_path, cx);
        self.sync_go_to_line_target(relative_path, cx);
        let find_bar = self.render_file_search_bar(pane_width, writable, window, cx);

        let theme = Theme::current(cx);
        let (line_count, heights) = {
            let field = editor_state.read(cx);
            (
                field.content().split('\n').count().max(1),
                field.wrapped_line_heights(),
            )
        };
        let reading = self
            .right_panel_file_editors
            .get(relative_path)
            .is_some_and(|editor| editor.reading);
        let go_to_line_bar = self.render_go_to_line_bar(line_count, reading, window, cx);
        // A mono digit advances ~0.6em, so the gutter tracks the font size.
        let digit_width = (text_size * 0.6).ceil();
        let gutter_width = 20.0 + digit_width * (line_count.to_string().len() as f32);
        let content_height = if heights.is_empty() {
            px(line_height) * line_count as f32
        } else {
            heights.iter().fold(Pixels::ZERO, |total, h| total + *h)
        };

        let viewport = self.right_panel_editor_scroll_handle.clone();
        let number_color = theme.text_ghost;
        let gutter = canvas(
            |_, _, _| (),
            move |bounds: gpui::Bounds<Pixels>, _, window: &mut Window, cx: &mut App| {
                let visible = viewport.bounds();
                let mut y = bounds.origin.y;
                for number in 1..=line_count {
                    let height = heights
                        .get(number - 1)
                        .copied()
                        .unwrap_or_else(|| px(line_height));
                    // Everything below the viewport is unreachable from here on.
                    if y > visible.bottom() {
                        break;
                    }
                    if y + height >= visible.top() {
                        let text = SharedString::from(number.to_string());
                        let run = gpui::TextRun {
                            len: text.len(),
                            font: gpui::font(crate::fonts::current(cx).code),
                            color: number_color,
                            ..Default::default()
                        };
                        let line =
                            window
                                .text_system()
                                .shape_line(text, px(text_size), &[run], None);
                        let origin = point(bounds.right() - line.width, y);
                        let _ = line.paint(
                            origin,
                            px(line_height),
                            gpui::TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                    y += height;
                }
            },
        )
        .flex_none()
        .w(px(gutter_width - GUTTER_PAD_RIGHT))
        .h(content_height);

        // Pinned annotation washes live inside the field's own paint — push
        // the live set in before building the body so this frame already
        // shows any change.
        self.sync_file_annotation_washes(relative_path, cx);
        let annotation_offer = self.render_file_annotation_offer(relative_path, window, cx);
        let annotation_editor = self.render_file_annotation_editor(relative_path, cx);
        let annotation_tooltip = self.render_file_annotation_tooltip(relative_path, cx);

        // The find bar sits in normal flow above the scroll region — Zed's
        // buffer-search arrangement — so an open bar pushes the content and
        // its line-number gutter down instead of covering the first lines.
        div()
            .key_context("FileEditorPane")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .font_family(crate::fonts::current(cx).code)
            .text_size(px(text_size))
            .line_height(px(line_height))
            .children(find_bar)
            .children(go_to_line_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id(SharedString::from(format!("file-editor-{relative_path}")))
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.right_panel_editor_scroll_handle)
                            .child(
                                div()
                                    .w_full()
                                    .pt(px(CONTENT_PAD_TOP))
                                    .pb(px(CONTENT_PAD_TOP + line_height * FILE_SCROLL_PAD_LINES))
                                    .flex()
                                    .items_start()
                                    .child(gutter)
                                    .child(div().w(px(GUTTER_PAD_RIGHT)).flex_none())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .pr(px(10.0))
                                            .child(editor_state.clone()),
                                    ),
                            ),
                    )
                    .child(scrollbar::vertical(
                        &self.right_panel_editor_scroll_handle,
                        &self.right_panel_editor_scrollbar,
                    ))
                    // The annotation listeners' region covers the text area;
                    // their hit-tests stay glyph-precise inside it.
                    .child(self.file_annotation_input(relative_path, cx))
                    .children(annotation_offer)
                    .children(annotation_editor)
                    .children(annotation_tooltip),
            )
    }

    /// Flips the global markdown source/preview mode and persists it, so the
    /// choice follows the user across files and sessions.
    fn toggle_markdown_preview(&mut self, cx: &mut Context<Self>) {
        self.set_markdown_preview(!self.state.markdown_preview, cx);
    }

    /// Flips one file between rendered preview and editable source — per
    /// file, including Markdown deliverables. SVGs reload because their
    /// bytes are only read once source is asked for; `.wireframe.json`
    /// already holds its text, so the reload is a no-op there.
    fn toggle_file_source_view(&mut self, relative_path: &str, cx: &mut Context<Self>) {
        let Some(editor) = self.right_panel_file_editors.get_mut(relative_path) else {
            return;
        };
        editor.show_source = !editor.show_source;
        cx.notify();
        self.read_right_panel_file_into_editor(relative_path.to_owned(), cx);
    }

    /// The `.wireframe.json` alternative to the editor body: each screen
    /// drawn as themed flex elements by [`wireframe::wireframe_screen_element`]
    /// — HTML/CSS semantics in the element tree, so the preview follows the
    /// selected theme like real UI. The parse is memoized on the editor by
    /// content hash, keeping the frame's work proportional to the visible
    /// screens rather than the file's size.
    fn render_file_wireframe_preview(
        &mut self,
        relative_path: &str,
        editor_state: &Entity<TextInput>,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        // `text_loaded` separates "still reading" from "empty file" — the
        // parse runs only on real content, never on the pre-read blank.
        let text_loaded = self
            .right_panel_file_editors
            .get(relative_path)
            .is_some_and(|editor| editor.text_loaded);
        let parsed = if !text_loaded {
            None
        } else {
            let content = editor_state.read(cx).content().to_owned();
            let fingerprint = {
                let mut hasher = DefaultHasher::new();
                content.hash(&mut hasher);
                hasher.finish()
            };
            self.right_panel_file_editors
                .get_mut(relative_path)
                .and_then(|editor| {
                    if !matches!(&editor.wireframe, Some((cached, _)) if *cached == fingerprint) {
                        editor.wireframe = Some((
                            fingerprint,
                            waku_protocol::wireframe::Wireframe::parse(&content),
                        ));
                    }
                    editor.wireframe.as_ref().map(|(_, result)| result)
                })
        };

        let message = |text: String, color: Hsla| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.0))
                .text_size(sp(12.5))
                .text_color(color)
                .child(text)
        };
        let body: AnyElement = match parsed {
            Some(Ok(wireframe)) if wireframe.screens.is_empty() => {
                message(tr!("files.wireframe_empty"), theme.text_tertiary).into_any_element()
            }
            Some(Ok(wireframe)) => div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(40.0))
                .px(px(24.0))
                .py(px(32.0))
                .children(wireframe.screens.iter().map(|screen| {
                    div()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .text_size(sp(11.0))
                                .text_color(theme.text_tertiary)
                                .child(screen.name.clone()),
                        )
                        .child(wireframe::wireframe_screen_element(screen, &theme))
                }))
                .into_any_element(),
            // Parse errors render verbatim — the surface names the problem
            // rather than blanking, like the schema intends.
            Some(Err(error)) => message(error.to_string(), theme.danger).into_any_element(),
            // No cached parse means the file's read has not landed yet.
            None => message(tr!("files.loading"), theme.text_tertiary).into_any_element(),
        };

        div()
            .key_context("FileEditorPane")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .font_family(crate::fonts::current(cx).ui)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "file-wireframe-{relative_path}"
                            )))
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.wireframe_preview_scroll_handle)
                            .child(body),
                    )
                    .child(scrollbar::vertical(
                        &self.wireframe_preview_scroll_handle,
                        &self.wireframe_preview_scrollbar,
                    )),
            )
    }

    /// The image alternative to the editor body: bytes read through
    /// `ReadBinaryFile` and wrapped as a `gpui::Image` off the UI thread. The
    /// frame path reads only the editor entry — `None` is "still loading"
    /// and a stored error is the fallback, never a reason to re-request.
    ///
    /// The viewport pans vertically and zooms on ⌘+scroll. It is a manual
    /// pan, not a GPUI scroll region: panning is one-dimensional, so a
    /// `ScrollHandle`'s clamp-and-offset bookkeeping would only add a second
    /// axis to get wrong. Position is computed in `canvas` prepaint, where
    /// the pane's bounds and the decoded pixel size are both known.
    fn render_file_image_preview(&mut self, relative_path: &str, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let image = self
            .right_panel_file_editors
            .get(relative_path)
            .and_then(|editor| editor.image.clone());

        let message = |text: String| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.0))
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary)
                .child(text)
        };

        let body: AnyElement = match image {
            Some(Ok(image)) => {
                let path = relative_path.to_owned();
                let entity = cx.entity();
                let entity_id = entity.entity_id();
                let weak = entity.downgrade();
                div()
                    .id("file-image-viewport")
                    .size_full()
                    .overflow_hidden()
                    .cursor_default()
                    .on_scroll_wheel({
                        let path = path.clone();
                        let weak = weak.clone();
                        move |event: &gpui::ScrollWheelEvent, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.handle_file_image_scroll(entity_id, &path, event, cx)
                            });
                        }
                    })
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                let natural =
                                    image
                                        .clone()
                                        .use_render_image(window, cx)
                                        .and_then(|render| {
                                            let size = render.size(0);
                                            (size.width.0 > 0 && size.height.0 > 0).then(|| {
                                                (size.width.0 as f32, size.height.0 as f32)
                                            })
                                        });
                                weak.update(cx, |this, _| {
                                    let editor = this.right_panel_file_editors.get_mut(&path)?;
                                    editor.image_viewport = Some(bounds);
                                    editor.image_natural = natural;
                                    if editor.image_zoom == 0.0
                                        && let Some((width, height)) = natural
                                    {
                                        // First view fits the whole image, but
                                        // never upscales past its pixel size.
                                        editor.image_zoom = (f32::from(bounds.size.width) / width)
                                            .min(f32::from(bounds.size.height) / height)
                                            .min(1.0);
                                    }
                                    let (width, height) = natural?;
                                    let scaled_w = px(width * editor.image_zoom);
                                    let scaled_h = px(height * editor.image_zoom);
                                    let left = (bounds.size.width - scaled_w) / 2.0;
                                    let top = if scaled_h > bounds.size.height {
                                        editor
                                            .image_pan_y
                                            .clamp(bounds.size.height - scaled_h, px(0.0))
                                    } else {
                                        (bounds.size.height - scaled_h) / 2.0
                                    };
                                    Some((left, top, scaled_w, scaled_h))
                                })
                                .ok()
                                .flatten()
                                .map(
                                    |(left, top, width, height)| {
                                        let mut element = div()
                                            .size_full()
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(left)
                                                    .top(top)
                                                    .w(width)
                                                    .h(height)
                                                    .child(
                                                        img(image.clone())
                                                            .id(SharedString::from(format!(
                                                                "file-image-{path}"
                                                            )))
                                                            .size_full(),
                                                    ),
                                            )
                                            .into_any_element();
                                        element.prepaint_as_root(
                                            bounds.origin,
                                            bounds.size.into(),
                                            window,
                                            cx,
                                        );
                                        element
                                    },
                                )
                            },
                            |_, element, window, cx| {
                                if let Some(mut element) = element {
                                    element.paint(window, cx);
                                }
                            },
                        )
                        .size_full(),
                    )
                    .into_any_element()
            }
            Some(Err(error)) => message(error).into_any_element(),
            None => message(tr!("files.loading_file")).into_any_element(),
        };

        div()
            .key_context("FileEditorPane")
            .flex_1()
            .min_h_0()
            .bg(theme.surface)
            .child(body)
    }

    /// Wheel handling for the image viewport: ⌘/Ctrl+scroll zooms around the
    /// cursor, a bare scroll pans vertically. The event is always consumed —
    /// a short image dead-zoning the transcript's scroll would feel broken.
    fn handle_file_image_scroll(
        &mut self,
        entity_id: EntityId,
        relative_path: &str,
        event: &gpui::ScrollWheelEvent,
        cx: &mut App,
    ) {
        cx.stop_propagation();
        let Some(editor) = self.right_panel_file_editors.get_mut(relative_path) else {
            return;
        };
        let (Some(bounds), Some((_, natural_h))) = (editor.image_viewport, editor.image_natural)
        else {
            return;
        };
        let delta_y = match event.delta {
            gpui::ScrollDelta::Pixels(delta) => f32::from(delta.y),
            gpui::ScrollDelta::Lines(lines) => lines.y * FILE_IMAGE_SCROLL_LINE_PX,
        };
        let viewport_h = f32::from(bounds.size.height);
        if event.modifiers.platform || event.modifiers.control {
            let old_zoom = editor.image_zoom.max(0.001);
            let zoom_factor = if delta_y > 0.0 {
                1.0 + delta_y * FILE_IMAGE_ZOOM_PER_PIXEL
            } else {
                1.0 / (1.0 - delta_y * FILE_IMAGE_ZOOM_PER_PIXEL)
            };
            let new_zoom = (old_zoom * zoom_factor).clamp(FILE_IMAGE_MIN_ZOOM, FILE_IMAGE_MAX_ZOOM);
            // Keep the image point under the cursor fixed: the content offset
            // at the cursor scales by the zoom ratio.
            let scaled_old = natural_h * old_zoom;
            let top_old = if scaled_old > viewport_h {
                f32::from(editor.image_pan_y)
            } else {
                (viewport_h - scaled_old) / 2.0
            };
            let cursor_y = f32::from(event.position.y) - f32::from(bounds.origin.y);
            let scaled_new = natural_h * new_zoom;
            let top = cursor_y - (cursor_y - top_old) * (new_zoom / old_zoom);
            editor.image_zoom = new_zoom;
            editor.image_pan_y = px(top.clamp((viewport_h - scaled_new).min(0.0), 0.0));
        } else {
            let scaled_h = natural_h * editor.image_zoom.max(0.001);
            if scaled_h <= viewport_h {
                return;
            }
            editor.image_pan_y =
                px((f32::from(editor.image_pan_y) + delta_y).clamp(viewport_h - scaled_h, 0.0));
        }
        cx.notify(entity_id);
    }

    /// The maximized panel layer is on screen this frame — the mode is
    /// active or its exit slide is still traveling.
    pub(super) fn panel_fullscreen_active(&self) -> bool {
        self.live_deliverable_page().is_none()
            && (self.fullscreen_surface.is_some() || self.panel_fullscreen_slide.is_some())
    }

    /// Cover the window with the active right-panel surface, or dock it back.
    /// The flag parks with the owner's strip on a swap, and every other way
    /// the surface goes away (tab close, surface switch, panel hide) is
    /// reconciled per frame in `settle_panel_slides`.
    fn toggle_panel_fullscreen(&mut self, cx: &mut Context<Self>) {
        let entering = self.fullscreen_surface.is_none();
        self.fullscreen_surface = if entering {
            self.active_right_panel_surface()
                .cloned()
                .map(|surface| (surface, self.visible_right_panel_file_path()))
        } else {
            None
        };
        let from = if entering && self.panel_fullscreen_slide.is_none() {
            self.right_panel_rendered_width
        } else {
            // Leaving, or reversing a slide still in flight: start where the
            // layer's edge actually is.
            self.panel_fullscreen_rendered_width
        };
        self.panel_fullscreen_slide = self.begin_panel_slide(from, cx);
        cx.notify();
    }

    /// Escape inside the maximized layer. The binding's PanelFullscreen
    /// context sits deeper than Waku's CancelTurn and shallower than
    /// FileEditorPane's close-find, so it only fires once no find bar has
    /// claimed the keystroke — and it excludes Terminal, leaving Escape to
    /// a focused pty even while maximized.
    pub(super) fn exit_panel_fullscreen_action(
        &mut self,
        _: &ExitPanelFullscreen,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.fullscreen_surface.is_some() {
            self.toggle_panel_fullscreen(cx);
        }
    }

    /// Pulls a planning session's `plans/<name>.md` out of the owning
    /// daemon's boss files root. The document lives outside every project
    /// workspace, so neither the file-tree reads a `File` editor uses nor
    /// the workspace client can reach it — only `BossOperation::ReadFile`
    /// resolves the `plans/` prefix. The read re-arms whenever the boss
    /// document's revision moves — `drain_boss_events` calls in on each
    /// received state — so agent writes stream into the preview one sync
    /// later. A re-arm keeps the fetched text on screen until its
    /// replacement lands and an identical landing never repaints: the
    /// preview flickers only when the plan itself does. `retry_failed`
    /// callers — state events, not render paths — may re-issue a read that
    /// already answered with an error; render callers leave a failure up
    /// rather than fetching again on every frame.
    pub(super) fn ensure_plan_doc(
        &mut self,
        key: waku_client::DaemonKey,
        session_id: Uuid,
        plan_file: &str,
        retry_failed: bool,
        cx: &mut Context<Self>,
    ) {
        let key = plan_doc_host(&self.boss_ui.states, key, session_id);
        let revision = self
            .boss_ui
            .states
            .get(&key)
            .map(|state| state.revision)
            .unwrap_or(0);
        if self
            .plan_docs
            .get(&session_id)
            .is_some_and(|doc| doc.settled(key, revision, retry_failed))
        {
            return;
        }
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            self.plan_docs.insert(
                session_id,
                PlanDoc {
                    key,
                    revision,
                    requested: false,
                    content: Some(Err(tr!("boss.unreachable"))),
                },
            );
            return;
        };
        // The last fetched text stays up while its replacement is read —
        // blanking to loading on every revision bump is the flash.
        let content = self
            .plan_docs
            .get(&session_id)
            .and_then(|doc| doc.content.clone());
        self.plan_docs.insert(
            session_id,
            PlanDoc {
                key,
                revision,
                requested: true,
                content,
            },
        );
        let path = plan_file.to_owned();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::Boss {
                            operation: waku_client::boss::BossOperation::ReadFile { path },
                        },
                    )
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let Some(doc) = waku.plan_docs.get_mut(&session_id) else {
                    return;
                };
                // A newer state re-armed the read while this one was in
                // flight — its own request supersedes this landing.
                if doc.key != key || doc.revision != revision {
                    return;
                }
                doc.requested = false;
                let next = match result {
                    Ok(waku_client::ResponsePayload::Boss {
                        result: waku_client::boss::BossResult::File { content, .. },
                    }) => Ok(content),
                    Ok(_) => Err(tr!("boss.unexpected_response")),
                    Err(error) => Err(error.to_string()),
                };
                // An identical landing leaves nothing to repaint.
                let changed = doc.content.as_ref() != Some(&next);
                doc.content = Some(next);
                // The document's first real contents mount the plan tab —
                // until this read the strip stayed without it.
                let mounted = doc.has_content()
                    && waku
                        .state
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                        .and_then(|session| session.planning.as_ref())
                        .map(|planning| planning.plan_file.clone())
                        .is_some_and(|plan_file| waku.mount_plan_tab(session_id, plan_file));
                if changed || mounted {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The scrollable read-only document behind a plan tab and the Boss
    /// page's plan detail — the transcript's markdown engine over the
    /// daemon-fetched text, with drag selection and, on the session's own
    /// plan tab (`annotatable`), the comment layer the file preview uses:
    /// pins live in `plan_annotations` keyed by the owning session and ship
    /// as quoted passages with that session's next submission. The Boss
    /// page's detail view passes `None` for `window` and stays read-only —
    /// an annotation staged there would have no composer in sight.
    pub(super) fn plan_document_view(
        &mut self,
        session_id: Uuid,
        text: &str,
        annotatable: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);
        let (block_count, content_changed) = {
            let mut cache = self.plan_markdown.borrow_mut();
            if !matches!(cache.as_ref(), Some((cached, _)) if *cached == session_id) {
                *cache = Some((session_id, MarkdownView::document()));
                // The list's measured heights describe the previous
                // session's blocks — this document starts from the top.
                self.plan_preview_list_state.reset(0);
            }
            let (_, view) = cache.as_mut().expect("entry ensured above");
            let changed = view.source() != text;
            view.set_text(text, false);
            (view.block_count(), changed)
        };
        // Same contract as the file preview: one top-level block per list
        // item keeps per-frame work proportional to the viewport.
        let list_state = self.plan_preview_list_state.clone();
        if list_state.item_count() != block_count {
            list_state.splice(0..list_state.item_count(), block_count);
        } else if content_changed {
            list_state.remeasure();
        }
        let mut preview_selection = self.plan_preview_selection.clone();
        // The session's pins paint here exactly as a file's do on its
        // markdown preview — the same store the hover and editor read.
        preview_selection.annotations =
            self.plan_annotations.entry(session_id).or_default().clone();
        let metrics = MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size);
        let reader_selection = preview_selection.clone();
        let reader_source = text.to_owned();
        let reader_title = tr!("speed_reader.preview_title", path = "plan");
        let reader_waku = cx.entity().downgrade();
        let ctx = MarkdownCtx::new(
            format!("plan-preview-{session_id}"),
            &palette,
            metrics,
            preview_selection.clone(),
        )
        .with_families(crate::fonts::current(cx))
        .with_math_enabled(self.state.render_math)
        .with_guided_reading(self.guided_reading())
        .with_standalone_context_menu(self.menu_handle("plan-preview-math", cx))
        .with_context_menu_items(Rc::new(move |_| {
            let source = reader_selection
                .selection
                .borrow()
                .selected_text()
                .unwrap_or_else(|| reader_source.clone());
            let waku = reader_waku.clone();
            let title = reader_title.clone();
            vec![MenuItem::new(
                tr!("speed_reader.go_fast"),
                move |window, cx| {
                    let _ = waku.update(cx, |this, cx| {
                        this.open_speed_reader(title.clone(), source.clone(), window, cx);
                    });
                },
            )]
        }))
        .with_link_items(self.markdown_link_menu_items.clone())
        .with_link_handler(self.markdown_link_handler.clone());
        let item_waku = cx.entity().downgrade();
        let document = md::render::standalone_context_menu(
            div().size_full().child(
                list(list_state.clone(), move |index, _window, cx| {
                    item_waku
                        .update(cx, |this, cx| {
                            this.render_plan_preview_block(session_id, index, cx)
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full(),
            ),
            &ctx,
        );

        let preview_focus = self.transcript_control_focus("plan-preview", cx);
        let preview_focus_click = preview_focus.clone();
        let selection_input = {
            let selection = preview_selection.clone();
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
                move |_, region, window, _| {
                    // ⌥-click arms the pressed line as the release fallback
                    // and fires ⌘L on mouse-up — the transcript's "select and
                    // annotate" gesture, here pinning on the rendered plan.
                    md::render::install_selection_input(
                        region,
                        window,
                        &selection,
                        annotatable.then(|| Box::new(AddToChat) as Box<dyn gpui::Action>),
                    )
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let annotation_offer = window
            .as_deref()
            .filter(|_| annotatable)
            .and_then(|window| self.render_plan_annotation_offer(session_id, window, cx));
        let annotation_editor = annotatable
            .then(|| self.render_plan_annotation_editor(session_id, cx))
            .flatten();
        let annotation_tooltip = annotatable
            .then(|| self.render_plan_annotation_tooltip(session_id, cx))
            .flatten();

        div()
            .when(annotatable, |element| {
                // The document pane carries the file pane's context so ⌘L's
                // "Add to chat" binding reaches `add_to_chat` here the way it
                // does in a file editor or its preview.
                element.key_context("FileEditorPane")
            })
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .child(
                div()
                    .track_focus(&preview_focus)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _, window, cx| {
                            window.focus(&preview_focus_click, cx);
                        }),
                    )
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // Painted before the document, so the frame's selection
                    // registry holds exactly this frame's text elements.
                    .child(md::render::frame_reset(preview_selection.clone()))
                    .child(document)
                    .child(selection_input)
                    // After the selection canvas so its hit-tests see this
                    // frame's registry — the file preview's ordering.
                    .when(annotatable, |element| {
                        element.child(self.plan_annotation_input(
                            &preview_selection,
                            session_id,
                            cx,
                        ))
                    })
                    .child(scrollbar::vertical(
                        &self.plan_preview_list_state,
                        &self.plan_preview_scrollbar,
                    ))
                    .children(annotation_offer)
                    .children(annotation_editor)
                    .children(annotation_tooltip),
            )
    }

    /// One top-level block of the session's plan document — the `list` item
    /// renderer. [`Self::plan_document_view`] feeds the parser and owns the
    /// item count; this rebuilds the render context for the block the
    /// viewport asks about.
    fn render_plan_preview_block(
        &mut self,
        session_id: Uuid,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);
        let centered = self.panel_fullscreen_active();
        let metrics = MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size);
        let mut preview_selection = self.plan_preview_selection.clone();
        preview_selection.annotations =
            self.plan_annotations.entry(session_id).or_default().clone();
        let reader_selection = preview_selection.clone();
        let reader_title = tr!("speed_reader.preview_title", path = "plan");
        let reader_waku = cx.entity().downgrade();
        let (block, count) = {
            let cache = self.plan_markdown.borrow();
            let Some((_, view)) = cache.as_ref().filter(|(id, _)| *id == session_id) else {
                return div().into_any_element();
            };
            let reader_source = view.source().to_owned();
            let ctx = MarkdownCtx::new(
                format!("plan-preview-{session_id}"),
                &palette,
                metrics,
                preview_selection,
            )
            .with_families(crate::fonts::current(cx))
            .with_math_enabled(self.state.render_math)
            .with_guided_reading(self.guided_reading())
            .with_standalone_context_menu(self.menu_handle("plan-preview-math", cx))
            .with_context_menu_items(Rc::new(move |_| {
                let source = reader_selection
                    .selection
                    .borrow()
                    .selected_text()
                    .unwrap_or_else(|| reader_source.clone());
                let waku = reader_waku.clone();
                let title = reader_title.clone();
                vec![MenuItem::new(
                    tr!("speed_reader.go_fast"),
                    move |window, cx| {
                        let _ = waku.update(cx, |this, cx| {
                            this.open_speed_reader(title.clone(), source.clone(), window, cx);
                        });
                    },
                )]
            }))
            .with_link_items(self.markdown_link_menu_items.clone())
            .with_link_handler(self.markdown_link_handler.clone());
            match md::render::markdown_block(view, &ctx, index) {
                Some(block) => (block, view.block_count()),
                None => return div().into_any_element(),
            }
        };
        self.markdown_document_item(block, index, count, centered, metrics, theme.text)
    }

    /// The Plan surface's body: the session's fetched document, a loading
    /// or failure note while it is in flight, or the markdown preview
    /// itself once the boss answers.
    fn render_plan_preview(
        &mut self,
        session_id: Uuid,
        plan_file: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = self.daemons.session_owner(session_id);
        self.ensure_plan_doc(key, session_id, plan_file, false, cx);
        let doc = self.plan_docs.get(&session_id);
        match doc.and_then(|doc| doc.content.as_ref()) {
            Some(Ok(text)) => {
                let text = text.clone();
                self.plan_document_view(session_id, &text, true, Some(window), cx)
            }
            Some(Err(error)) => self.render_right_panel_empty_message(
                tr!("boss.plan_unavailable"),
                error.clone(),
                cx,
            ),
            None => {
                let theme = Theme::current(cx);
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text_tertiary)
                            .child(tr!("boss.loading")),
                    )
            }
        }
    }

    /// The rendered-markdown alternative to the editor body, shown while the
    /// global preview toggle is on. It renders the editor's current text —
    /// unsaved edits included — with the transcript's markdown engine; the
    /// parse is cached per path, so re-rendering an unchanged document costs
    /// `Rc` clones, not a re-parse. Reads only in-memory editor state: the
    /// render path may not touch the filesystem.
    ///
    /// Selection and comments work like the transcript's: the render context
    /// runs on `file_preview_selection` with the editor's annotation store
    /// swapped in, so the file's pinned highlights paint, hover and reopen
    /// here exactly as they do in the source view — where
    /// [`Self::sync_file_annotation_washes`] feeds the same set to the field.
    fn render_file_markdown_preview(
        &mut self,
        relative_path: &str,
        editor_state: &Entity<TextInput>,
        pane_width: f32,
        deliverable_page: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // An open find bar follows this surface: re-aim it if the file or
        // the source/preview mode flipped, and run a pending scroll reveal
        // here where the frame still owns a `Window` for the retry path.
        self.sync_file_search_target(relative_path, cx);
        self.apply_pending_preview_search_reveal(relative_path, window, cx);
        let search_highlights = self.file_preview_search_highlights(relative_path);
        let find_bar = self.render_file_search_bar(pane_width, false, window, cx);

        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);
        // The maximized panel and a deliverable's page both own a full-width
        // column — the document centers at the content measure either way.
        let centered = self.panel_fullscreen_active() || deliverable_page;
        let (block_count, content_changed) = {
            let mut cache = self.file_preview_markdown.borrow_mut();
            if !matches!(cache.as_ref(), Some((cached, _)) if cached == relative_path) {
                *cache = Some((relative_path.to_owned(), MarkdownView::document()));
            }
            let (_, view) = cache.as_mut().expect("entry ensured above");
            let content = editor_state.read(cx).content();
            let changed = view.source() != content;
            view.set_text(content, false);
            (view.block_count(), changed)
        };
        // The document mounts one top-level block per list item, so a large
        // file's flatten and layout stay proportional to the viewport instead
        // of the document. A block-count change splices; a same-count edit
        // only invalidates measured heights.
        let list_state = if deliverable_page {
            self.boss_ui.deliverable_page.map(|page| {
                self.boss_ui
                    .deliverable_page_scroll
                    .entry(page)
                    .or_insert_with(|| ListState::new(0, ListAlignment::Top, px(1024.0)))
                    .clone()
            })
        } else {
            None
        }
        .or_else(|| self.preview_list_state(relative_path))
        .unwrap_or_else(|| ListState::new(0, ListAlignment::Top, px(1024.0)));
        if list_state.item_count() != block_count {
            list_state.splice(0..list_state.item_count(), block_count);
        } else if content_changed {
            list_state.remeasure();
        }
        let mut preview_selection = self.file_preview_selection.clone();
        if let Some(editor) = self.right_panel_file_editors.get(relative_path) {
            preview_selection.annotations = editor.annotations.clone();
        }
        let metrics = MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size);
        let reader_selection = preview_selection.clone();
        let reader_editor_state = editor_state.clone();
        let reader_title = tr!("speed_reader.preview_title", path = relative_path);
        let reader_waku = cx.entity().downgrade();
        let menu_composer = self.composer.clone();
        let mut ctx = MarkdownCtx::new(
            format!("file-preview-{relative_path}"),
            &palette,
            metrics,
            preview_selection.clone(),
        )
        .with_families(crate::fonts::current(cx))
        .with_math_enabled(self.state.render_math)
        .with_guided_reading(self.guided_reading())
        // The page and a panel preview can be live together — each needs its
        // own handle or one surface's right-click items leak into the other's.
        .with_standalone_context_menu(self.menu_handle(
            if deliverable_page {
                "deliverable-page-math"
            } else {
                "file-preview-math"
            },
            cx,
        ))
        .with_context_menu_items(Rc::new(move |cx| {
            // A deliverable's page borrows the chat message's menu — copy,
            // search, and friends act on the published document's text.
            if deliverable_page {
                let content = reader_editor_state.read(cx).content().to_owned();
                return components::deliverable_menu_items(
                    &content,
                    &reader_selection,
                    &menu_composer,
                    &reader_waku,
                    cx,
                );
            }
            let source = reader_selection
                .selection
                .borrow()
                .selected_text()
                .unwrap_or_else(|| reader_editor_state.read(cx).content().to_owned());
            let waku = reader_waku.clone();
            let title = reader_title.clone();
            vec![MenuItem::new(
                tr!("speed_reader.go_fast"),
                move |window, cx| {
                    let _ = waku.update(cx, |this, cx| {
                        this.open_speed_reader(title.clone(), source.clone(), window, cx);
                    });
                },
            )]
        }))
        .with_link_items(self.markdown_link_menu_items.clone())
        .with_link_handler(self.markdown_link_handler.clone());
        if let Some(highlights) = search_highlights {
            ctx = ctx.with_search_highlights(highlights);
        }
        let item_waku = cx.entity().downgrade();
        let item_path = relative_path.to_owned();
        let document = md::render::standalone_context_menu(
            div().size_full().child(
                list(list_state.clone(), move |index, _window, cx| {
                    item_waku
                        .update(cx, |this, cx| {
                            this.render_file_preview_block(&item_path, index, centered, cx)
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full(),
            ),
            &ctx,
        );

        let preview_focus = self.transcript_control_focus("file-preview", cx);
        let preview_focus_click = preview_focus.clone();
        let selection_input = {
            let selection = preview_selection.clone();
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
                move |_, region, window, _| {
                    // ⌥-click arms the pressed line as the release fallback
                    // and fires ⌘L on mouse-up — the transcript's "select and
                    // annotate" gesture, here pinning on the rendered file.
                    md::render::install_selection_input(
                        region,
                        window,
                        &selection,
                        Some(Box::new(AddToChat)),
                    )
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let annotation_offer = self.render_preview_annotation_offer(relative_path, window, cx);
        let annotation_editor = self.render_preview_annotation_editor(relative_path, cx);
        let annotation_tooltip = self.render_preview_annotation_tooltip(relative_path, cx);

        div()
            .key_context("FileEditorPane")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.surface)
            // The find bar sits in normal flow above the scroll region —
            // the same arrangement the source view uses — so an open bar
            // pushes the document down instead of covering it.
            .children(find_bar)
            .child(
                div()
                    .track_focus(&preview_focus)
                    .on_action(cx.listener({
                        let selection = preview_selection.clone();
                        move |_, _: &CopySelection, _, cx| {
                            // This document has its own registry; the workspace
                            // copy handler otherwise reads the hidden transcript.
                            match selection.selection.borrow().clipboard_text() {
                                Some(text) => {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                                None => cx.propagate(),
                            }
                        }
                    }))
                    // The focus claim lives on the document container, not
                    // the pane: a click in the find bar must not pull the
                    // caret back out of its query field.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _, window, cx| {
                            window.focus(&preview_focus_click, cx);
                        }),
                    )
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // Painted before the document, so the frame's selection
                    // registry holds exactly this frame's text elements.
                    .child(md::render::frame_reset(preview_selection.clone()))
                    .child(document)
                    .child(selection_input)
                    // After the selection canvas so its hit-tests see this
                    // frame's registry; bubble dispatch runs listeners in
                    // reverse paint order, so a press on a highlight reaches
                    // the annotation handlers before the selection's drag
                    // begins.
                    .child(self.preview_annotation_input(&preview_selection, relative_path, cx))
                    .child(scrollbar::vertical(
                        &list_state,
                        &self.file_preview_scrollbar,
                    ))
                    .children(annotation_offer)
                    .children(annotation_editor)
                    .children(annotation_tooltip),
            )
    }

    /// One top-level block of the file's markdown preview — the `list` item
    /// renderer. [`Self::render_file_markdown_preview`] feeds the parser and
    /// owns the item count; this rebuilds the render context for the block
    /// the viewport asks about, so a block outside the overdraw costs
    /// nothing.
    fn render_file_preview_block(
        &mut self,
        relative_path: &str,
        index: usize,
        centered: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);
        let metrics = MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size);
        let mut preview_selection = self.file_preview_selection.clone();
        let Some(editor_state) = self
            .right_panel_file_editors
            .get(relative_path)
            .map(|editor| {
                preview_selection.annotations = editor.annotations.clone();
                editor.state.clone()
            })
        else {
            return div().into_any_element();
        };
        let reader_selection = preview_selection.clone();
        let reader_editor_state = editor_state;
        let reader_title = tr!("speed_reader.preview_title", path = relative_path);
        let reader_waku = cx.entity().downgrade();
        let mut ctx = MarkdownCtx::new(
            format!("file-preview-{relative_path}"),
            &palette,
            metrics,
            preview_selection,
        )
        .with_families(crate::fonts::current(cx))
        .with_math_enabled(self.state.render_math)
        .with_guided_reading(self.guided_reading())
        .with_standalone_context_menu(self.menu_handle("file-preview-math", cx))
        .with_context_menu_items(Rc::new(move |cx| {
            let source = reader_selection
                .selection
                .borrow()
                .selected_text()
                .unwrap_or_else(|| reader_editor_state.read(cx).content().to_owned());
            let waku = reader_waku.clone();
            let title = reader_title.clone();
            vec![MenuItem::new(
                tr!("speed_reader.go_fast"),
                move |window, cx| {
                    let _ = waku.update(cx, |this, cx| {
                        this.open_speed_reader(title.clone(), source.clone(), window, cx);
                    });
                },
            )]
        }))
        .with_link_items(self.markdown_link_menu_items.clone())
        .with_link_handler(self.markdown_link_handler.clone());
        if let Some(highlights) = self.file_preview_search_highlights(relative_path) {
            ctx = ctx.with_search_highlights(highlights);
        }
        let (block, count) = {
            let cache = self.file_preview_markdown.borrow();
            let Some((_, view)) = cache.as_ref().filter(|(path, _)| path == relative_path) else {
                return div().into_any_element();
            };
            match md::render::markdown_block(view, &ctx, index) {
                Some(block) => (block, view.block_count()),
                None => return div().into_any_element(),
            }
        };
        self.markdown_document_item(block, index, count, centered, metrics, theme.text)
    }

    /// The `list` item shell a virtualized markdown document wraps each
    /// top-level block in: the content column's centering and padding, plus
    /// the inter-block gap the unvirtualized container applied as `gap`.
    fn markdown_document_item(
        &self,
        block: AnyElement,
        index: usize,
        count: usize,
        centered: bool,
        metrics: MarkdownMetrics,
        text_color: Hsla,
    ) -> AnyElement {
        div()
            .w_full()
            .when(centered, |element| element.flex().justify_center())
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .when(centered, |element| {
                        element.max_w(px(CONTENT_MAX_WIDTH))
                    })
                    .px(px(16.0))
                    .when(index == 0, |element| element.pt(px(14.0)))
                    .pb(if index + 1 == count {
                        px(24.0 + metrics.line_height * FILE_SCROLL_PAD_LINES)
                    } else {
                        px(metrics.block_gap)
                    })
                    .text_color(text_color)
                    .child(block),
            )
            .into_any_element()
    }

    /// Picks up an external edit to a file the user has not modified here.
    ///
    /// Reaches the filesystem, so it queues a background read rather than
    /// blocking; the editor keeps showing its current text until that lands.
    fn reload_right_panel_file_if_clean(&mut self, relative_path: &str, cx: &mut Context<Self>) {
        if self
            .right_panel_file_editors
            .get(relative_path)
            .is_none_or(|editor| editor.dirty)
        {
            return;
        }
        self.read_right_panel_file_into_editor(relative_path.to_owned(), cx);
    }

    pub(super) fn reload_clean_right_panel_file_editors(&mut self, cx: &mut Context<Self>) {
        let paths = self
            .right_panel_file_editors
            .iter()
            .filter(|(_, editor)| !editor.dirty)
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        for path in paths {
            self.reload_right_panel_file_if_clean(&path, cx);
        }
    }

    pub(super) fn save_right_panel_file_action(
        &mut self,
        _: &SaveFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // ⌘S doubles as "Sync branch…" outside the file editor: with no file
        // surface active the chord opens the branch picker instead — unless
        // the visible surface is a document the user cannot save anyway.
        let Some(relative_path) = self.visible_right_panel_file_path() else {
            if matches!(
                self.active_right_panel_surface(),
                Some(RightPanelSurface::Plan { .. })
            ) {
                return;
            }
            self.open_sync_branch(window, cx);
            return;
        };
        let Some(project_path) = self.right_panel_files_root.clone() else {
            return;
        };
        let Some(editor) = self.right_panel_file_editors.get(&relative_path) else {
            return;
        };
        if !editor.writable {
            self.show_toast(if editor.reading {
                tr!("files.could_not_save_opening", path = relative_path)
            } else {
                tr!("files.could_not_save_read_only", path = relative_path)
            });
            cx.notify();
            return;
        }

        let content = editor.state.read(cx).content().to_owned();
        let epoch = if let Some(editor) = self.right_panel_file_editors.get_mut(&relative_path) {
            editor.reading = false;
            editor.read_epoch += 1;
            editor.read_epoch
        } else {
            return;
        };
        let Some(workspace) = self.workspace_client_for_path(&project_path) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let project_path = project_path.clone();
                    let relative_path = relative_path.clone();
                    let content = content.clone();
                    async move {
                        match workspace.request(waku_client::WorkspaceOperation::WriteTextFile {
                            root: project_path,
                            relative_path: PathBuf::from(relative_path),
                            content,
                        })? {
                            waku_client::WorkspaceResult::Ack => Ok(()),
                            _ => anyhow::bail!("the daemon returned an invalid file response"),
                        }
                    }
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.right_panel_files_root.as_deref() != Some(project_path.as_path()) {
                    return;
                }
                match result {
                    Ok(()) => {
                        if let Some(editor) = waku.right_panel_file_editors.get_mut(&relative_path)
                            && editor.read_epoch == epoch
                        {
                            let current = editor.state.read(cx).content();
                            editor.disk_content = content.clone();
                            editor.dirty = current != content;
                            // The saved text is now what preview mode would
                            // read from disk — refresh a cached image from
                            // these bytes instead of re-reading the file.
                            if let Some(format) =
                                image_preview::image_format_for_name(&relative_path)
                            {
                                editor.image = Some(Ok(Arc::new(gpui::Image::from_bytes(
                                    format,
                                    content.clone().into_bytes(),
                                ))));
                            }
                        }
                    }
                    Err(error) => waku.show_toast(tr!(
                        "files.could_not_save",
                        path = relative_path,
                        error = error.to_string()
                    )),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_right_panel_diff(
        &mut self,
        panel_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let toolbar = self.render_right_panel_diff_toolbar(cx);
        let content = match self.right_panel_diff_snapshot.clone() {
            Some(snapshot) => {
                let tree_width = fitted_file_tree_width(
                    panel_width,
                    self.right_panel_file_tree_width.max(220.0),
                );
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .child(self.render_right_panel_unified_diff(snapshot.clone(), cx))
                    .child(
                        div()
                            .w(px(tree_width))
                            .min_w(px(FILE_TREE_MIN_WIDTH))
                            .h_full()
                            .flex_none()
                            .relative()
                            .border_l(hairline())
                            .border_color(theme.separator)
                            .child(self.render_right_panel_diff_tree(window, cx))
                            .child(self.render_panel_resize_handle(
                                "right-panel-diff-tree-resize-handle",
                                PanelResizeTarget::FileTree,
                                cx,
                            )),
                    )
                    .into_any_element()
            }
            None if self.right_panel_diff_loading => self
                .render_right_panel_empty_message(
                    tr!("diff.loading"),
                    tr!("diff.loading_description"),
                    cx,
                )
                .into_any_element(),
            None if self.right_panel_diff_error.is_some() => self
                .render_right_panel_empty_message(
                    tr!("diff.unavailable"),
                    self.right_panel_diff_error.clone().unwrap_or_default(),
                    cx,
                )
                .into_any_element(),
            None => self
                .render_right_panel_empty_message(
                    tr!("diff.no_changes"),
                    tr!("diff.no_changes_description"),
                    cx,
                )
                .into_any_element(),
        };

        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .flex()
            .flex_col()
            // Anything focusable inside the review surface — the file tree,
            // its filter field, the toolbar controls — resolves here, so the
            // ⌘± chords know to move the code font size.
            .key_context("ReviewDiff")
            .child(md::render::frame_reset(
                self.right_panel_diff_selection.clone(),
            ))
            .child(toolbar)
            .child(content)
            .child(self.right_panel_diff_selection_input())
    }

    fn render_right_panel_diff_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let selected = self.right_panel_diff_source;
        let latest_turn = self.latest_review_turn_source();
        let source_label = self.review_diff_source_label(selected);
        let weak = cx.entity().downgrade();
        let handle = self.menu_handle("right-panel-diff-source", cx);
        let source = dropdown_menu(
            MenuChip::new("right-panel-diff-source")
                .label(source_label)
                .height(px(28.0))
                .background(theme.surface)
                .selected(handle.is_open()),
            "right-panel-diff-source-menu",
            &handle,
            MenuAlign::BelowLeft,
            move |_| {
                let mut items = Vec::new();
                let last_turn_source = latest_turn.unwrap_or_default();
                let last_turn_weak = weak.clone();
                items.push(
                    MenuItem::new(tr!("diff.source_last_turn"), move |_, cx| {
                        let _ = last_turn_weak.update(cx, |this, cx| {
                            this.set_right_panel_diff_source(last_turn_source, cx)
                        });
                    })
                    .selected(latest_turn == Some(selected))
                    .disabled(latest_turn.is_none()),
                );
                items.push(MenuItem::Separator);
                for (choice, label) in [
                    (
                        ReviewDiffSource::Uncommitted,
                        tr!("diff.source_uncommitted"),
                    ),
                    (ReviewDiffSource::Unstaged, tr!("diff.source_unstaged")),
                    (ReviewDiffSource::Staged, tr!("diff.source_staged")),
                ] {
                    let choice_weak = weak.clone();
                    items.push(
                        MenuItem::new(label, move |_, cx| {
                            let _ = choice_weak.update(cx, |this, cx| {
                                this.set_right_panel_diff_source(choice, cx)
                            });
                        })
                        .selected(choice == selected),
                    );
                }
                items.push(MenuItem::Separator);
                for (choice, label) in [
                    (ReviewDiffSource::Committed, tr!("diff.source_committed")),
                    (ReviewDiffSource::Branch, tr!("diff.source_branch")),
                ] {
                    let choice_weak = weak.clone();
                    items.push(
                        MenuItem::new(label, move |_, cx| {
                            let _ = choice_weak.update(cx, |this, cx| {
                                this.set_right_panel_diff_source(choice, cx)
                            });
                        })
                        .selected(choice == selected),
                    );
                }
                items
            },
        );

        let (additions, deletions, truncated) = self
            .right_panel_diff_snapshot
            .as_ref()
            .map_or((0, 0, false), |snapshot| {
                (snapshot.additions, snapshot.deletions, snapshot.truncated)
            });
        let refresh_focus = self.transcript_control_focus("right-panel-diff-refresh", cx);
        let refresh_icon: AnyElement = if self.right_panel_diff_loading {
            motion::spin(icon("icons/loader-circle.svg", 12.0, theme.text_tertiary))
        } else {
            icon("icons/rotate-cw.svg", 12.0, theme.text_tertiary).into_any_element()
        };
        let refresh = div()
            .id("right-panel-diff-refresh")
            .track_focus(&refresh_focus)
            .tab_index(0)
            .size(px(28.0))
            .rounded(px(9.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .child(refresh_icon)
            .tooltip(|window, cx| Tooltip::new(tr!("diff.refresh")).build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.refresh_right_panel_diff(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.refresh_right_panel_diff(cx);
                    cx.stop_propagation();
                }
            }));

        div()
            .h(px(44.0))
            .flex_none()
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .border_b(hairline())
            .border_color(theme.separator)
            .child(source)
            .child(
                div()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.success)
                    .child(format!("+{additions}")),
            )
            .child(
                div()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.danger)
                    .child(format!("-{deletions}")),
            )
            .when(truncated, |row| {
                row.child(
                    div()
                        .text_size(sp(12.5))
                        .text_color(theme.warning)
                        .child(tr!("diff.truncated")),
                )
            })
            .child(div().flex_1())
            .child(refresh)
            .into_any_element()
    }

    fn render_right_panel_unified_diff(
        &self,
        snapshot: Arc<ReviewDiffSnapshot>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if snapshot.files.is_empty() {
            return self
                .render_right_panel_empty_message(
                    tr!("diff.no_changes"),
                    tr!("diff.no_changes_description"),
                    cx,
                )
                .into_any_element();
        }
        let sticky_header = self.render_right_panel_diff_sticky_header(&snapshot, cx);
        let entity = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .overflow_hidden()
            .child(
                list(
                    self.right_panel_diff_list_state.clone(),
                    move |index, _window, cx| {
                        entity
                            .upgrade()
                            .map(|entity| {
                                entity.update(cx, |this, cx| {
                                    this.render_right_panel_diff_line(index, cx)
                                })
                            })
                            .unwrap_or_else(|| div().into_any_element())
                    },
                )
                .size_full(),
            )
            .when_some(sticky_header, |container, header| container.child(header))
            .child(scrollbar::vertical(
                &self.right_panel_diff_list_state,
                &self.right_panel_diff_scrollbar,
            ))
            .into_any_element()
    }

    fn render_right_panel_diff_sticky_header(
        &self,
        snapshot: &ReviewDiffSnapshot,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let scroll_top = self.right_panel_diff_list_state.logical_scroll_top();
        let (header_index, next_header_index) = snapshot.file_headers_around(scroll_top.item_ix)?;
        let needs_sticky = header_index < scroll_top.item_ix
            || (header_index == scroll_top.item_ix && scroll_top.offset_in_item > px(0.));
        if !needs_sticky {
            return None;
        }

        let line = snapshot.lines.get(header_index)?;
        let file = snapshot.files.get(line.file_index)?;
        let top_offset = next_header_index
            .and_then(|next_header_index| {
                let bounds = self
                    .right_panel_diff_list_state
                    .bounds_for_item(next_header_index)?;
                let viewport = self.right_panel_diff_list_state.viewport_bounds();
                let y_in_viewport = bounds.origin.y - viewport.origin.y;
                (y_in_viewport < bounds.size.height).then_some(y_in_viewport - bounds.size.height)
            })
            .unwrap_or(px(0.));

        Some(
            div()
                .absolute()
                .top(top_offset)
                .left_0()
                .w_full()
                .child(self.render_right_panel_diff_file_header(header_index, file, true, cx))
                .into_any_element(),
        )
    }

    fn render_right_panel_diff_file_header(
        &self,
        index: usize,
        file: &crate::review_diff::File,
        sticky: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let id_prefix = if sticky {
            "review-diff-sticky-file"
        } else {
            "review-diff-file"
        };
        div()
            .id(SharedString::from(format!("{id_prefix}-{index}")))
            .w_full()
            .min_w_0()
            .h(px(REVIEW_DIFF_FILE_HEADER_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .border_b(hairline())
            .border_color(theme.separator)
            .bg(theme.surface)
            .when(sticky, |header| header.block_mouse_except_scroll())
            .child(file_icon(file_icon_for_path(&file.path), 14.0))
            .child(file_link(
                div()
                    .id(SharedString::from(format!("{id_prefix}-path-{index}")))
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .tooltip(Tooltip::text(file.path.clone()))
                    .child(file.path.clone()),
                &self.transcript_control_focus(format!("{id_prefix}-path-{index}"), cx),
                file.path.clone(),
                self,
                &cx.entity().downgrade(),
                format!("file-link-menu-{id_prefix}-{index}"),
                cx,
            ))
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.success)
                    .child(format!("+{}", file.additions)),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.danger)
                    .child(format!("-{}", file.deletions)),
            )
            .into_any_element()
    }

    fn render_right_panel_diff_line(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(snapshot) = self.right_panel_diff_snapshot.as_ref() else {
            return div().into_any_element();
        };
        let Some(line) = snapshot.lines.get(index) else {
            return div().into_any_element();
        };
        let Some(file) = snapshot.files.get(line.file_index) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let style = DiffRowStyle::review(self.state.code_font_size, crate::fonts::current(cx).code);
        // Chrome rows keep their gutters flush with the code rows'.
        let gutter_width = style.gutter_width();

        match &line.kind {
            crate::review_diff::LineKind::FileHeader => {
                self.render_right_panel_diff_file_header(index, file, false, cx)
            }
            crate::review_diff::LineKind::Gap(gap) => {
                let expandable = gap.is_expandable();
                let chunked = gap.count() > crate::review_diff::DEFAULT_EXPANSION_LINE_COUNT as u32;
                let directions = review_diff_gap_directions(gap.position, chunked);
                let two_directions = directions.len() > 1;
                let gutter = div()
                    .w(px(gutter_width))
                    .h_full()
                    .flex_none()
                    .flex()
                    .when(two_directions, |gutter| gutter.flex_col())
                    .border_r(hairline())
                    .border_color(theme.separator)
                    .bg(theme.overlay)
                    .when(expandable, |mut gutter| {
                        for (button_index, direction) in directions.iter().copied().enumerate() {
                            gutter = gutter.child(self.render_right_panel_diff_gap_action(
                                index,
                                gap.id,
                                direction,
                                review_diff_gap_icon_path(direction),
                                review_diff_gap_tooltip(direction),
                                two_directions,
                                two_directions && button_index == 0,
                                cx,
                            ));
                        }
                        gutter
                    });
                let label_focus = self
                    .transcript_control_focus(format!("right-panel-diff-gap-{}-label", gap.id), cx);
                let label = div()
                    .id(SharedString::from(format!(
                        "right-panel-diff-gap-{}-label",
                        gap.id
                    )))
                    .track_focus(&label_focus)
                    .h_full()
                    .min_w_0()
                    .flex_1()
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .bg(theme.overlay)
                    .child(tr!("diff.unmodified_lines", count = gap.count()))
                    .when(expandable, |label| {
                        label
                            .tab_index(0)
                            .cursor_default()
                            .focus_visible(|style| style.bg(theme.focus_highlight()))
                            .hover(|style| {
                                style
                                    .bg(theme.overlay_strong)
                                    .text_color(theme.text_secondary)
                            })
                            .active(|style| style.bg(theme.overlay))
                            .tooltip(Tooltip::text(tr!("diff.expand_context")))
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                let direction = if event.modifiers().shift {
                                    crate::review_diff::ExpansionDirection::All
                                } else {
                                    crate::review_diff::ExpansionDirection::Both
                                };
                                this.expand_right_panel_diff_gap(index, direction, cx);
                                cx.stop_propagation();
                            }))
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    let direction = if event.keystroke.modifiers.shift {
                                        crate::review_diff::ExpansionDirection::All
                                    } else {
                                        crate::review_diff::ExpansionDirection::Both
                                    };
                                    this.expand_right_panel_diff_gap(index, direction, cx);
                                    cx.stop_propagation();
                                }
                            }))
                    });
                div()
                    .h(px(32.0))
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .text_size(px(12.5))
                    .text_color(theme.text_tertiary)
                    .child(gutter)
                    .child(label)
                    .into_any_element()
            }
            crate::review_diff::LineKind::HunkHeader => div()
                .min_h(px(24.0))
                .w_full()
                .min_w_0()
                .flex()
                .items_stretch()
                .font_family(style.code_family.clone())
                .text_size(px(12.5))
                .line_height(px(16.0))
                .text_color(theme.text_tertiary)
                .child(
                    div()
                        .w(px(gutter_width))
                        .min_h(px(24.0))
                        .self_stretch()
                        .flex_none()
                        .border_r(hairline())
                        .border_color(theme.separator)
                        .bg(theme.overlay),
                )
                .child(
                    div()
                        .min_h(px(24.0))
                        .min_w_0()
                        .flex_1()
                        .px(px(12.0))
                        .py(px(4.0))
                        .flex()
                        .items_start()
                        .overflow_hidden()
                        .whitespace_normal()
                        .bg(theme.overlay)
                        .child(line.content.clone()),
                )
                .into_any_element(),
            crate::review_diff::LineKind::Meta => div()
                .min_h(px(24.0))
                .w_full()
                .min_w_0()
                .flex()
                .items_stretch()
                .font_family(style.code_family.clone())
                .text_size(px(12.5))
                .line_height(px(16.0))
                .text_color(theme.text_tertiary)
                .child(
                    div()
                        .w(px(gutter_width))
                        .min_h(px(24.0))
                        .self_stretch()
                        .flex_none(),
                )
                .child(
                    div()
                        .min_h(px(24.0))
                        .min_w_0()
                        .flex_1()
                        .py(px(4.0))
                        .overflow_hidden()
                        .whitespace_normal()
                        .pr(px(10.0))
                        .child(line.content.clone()),
                )
                .into_any_element(),
            crate::review_diff::LineKind::Context
            | crate::review_diff::LineKind::Addition
            | crate::review_diff::LineKind::Deletion => render_diff_code_row(
                line,
                index,
                "review-diff",
                &self.right_panel_diff_selection,
                style,
                &theme,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_right_panel_diff_gap_action(
        &self,
        line_index: usize,
        gap_id: u64,
        direction: crate::review_diff::ExpansionDirection,
        icon_path: &'static str,
        tooltip: String,
        compact_half: bool,
        border_bottom: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let direction_name = match direction {
            crate::review_diff::ExpansionDirection::Start => "start",
            crate::review_diff::ExpansionDirection::End => "end",
            crate::review_diff::ExpansionDirection::Both => "both",
            crate::review_diff::ExpansionDirection::All => "all",
        };
        let focus = self.transcript_control_focus(
            format!("right-panel-diff-gap-{gap_id}-button-{direction_name}"),
            cx,
        );
        div()
            .id(SharedString::from(format!(
                "right-panel-diff-gap-{gap_id}-button-{direction_name}"
            )))
            .track_focus(&focus)
            .tab_index(0)
            .w_full()
            .h_full()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .when(compact_half, |button| button.h(px(16.0)).flex_none())
            .when(border_bottom, |button| {
                button.border_b(hairline()).border_color(theme.separator)
            })
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay_strong))
            .active(|style| style.bg(theme.overlay))
            .tooltip(Tooltip::text(tooltip))
            .child(icon(icon_path, 11.0, theme.text_tertiary))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                let direction = if event.modifiers().shift {
                    crate::review_diff::ExpansionDirection::All
                } else {
                    direction
                };
                this.expand_right_panel_diff_gap(line_index, direction, cx);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    let direction = if event.keystroke.modifiers.shift {
                        crate::review_diff::ExpansionDirection::All
                    } else {
                        direction
                    };
                    this.expand_right_panel_diff_gap(line_index, direction, cx);
                    cx.stop_propagation();
                }
            }))
    }

    /// Resolves one cached outcome row against the cached session
    /// snapshot — status, title, the member the detail line speaks for,
    /// project, workspace, and the relative update label — so the list
    /// item builders paint only prepared data.
    fn prepare_boss_outcome_row(
        &self,
        row: &boss::BossOutcomeRow,
        key: waku_client::DaemonKey,
        sessions: &HashMap<Uuid, &AgentSession>,
        now: u64,
        focus: FocusHandle,
    ) -> BossOutcomePanelRow {
        let (bucket, status) = boss_outcome_status(row, sessions, now);
        let status_label = status.label();
        let title = {
            let outcome = row.outcome.outcome.trim();
            if outcome.is_empty() {
                tr!("boss.goals_untitled")
            } else {
                outcome.to_owned()
            }
        };
        // The member the detail line speaks for: a live assignee first,
        // then the next queued admission, then the latest recorded
        // attempt — a finished row still names its last assignee.
        let queued: Vec<&boss::BossOutcomeMember> =
            row.members.iter().filter(|m| boss_outcome_queued(m)).collect();
        let speaker_member = boss_outcome_live_member(row).or_else(|| {
            queued
                .iter()
                .min_by_key(|member| member.queue_rank.unwrap_or(usize::MAX))
                .copied()
        });
        let last_assignment = row.outcome.assignments.last();
        let speaker_session = speaker_member
            .map(|member| member.employee.session_id)
            .or_else(|| last_assignment.map(|assignment| assignment.session));
        let speaker_name = speaker_member
            .map(|member| member.employee.identity.name.clone())
            .or_else(|| {
                last_assignment.and_then(|assignment| {
                    assignment
                        .identity
                        .as_ref()
                        .map(|identity| identity.name.clone())
                })
            });
        let speaker_session_ref = speaker_session.and_then(|id| sessions.get(&id).copied());
        let project = speaker_session_ref.and_then(|session| {
            self.state
                .projects
                .iter()
                .find(|project| project.id == session.project_id)
        });
        // The queued ticket's declared project stands in until the
        // assignment's session shell exists.
        let project_label = project.map(Project::display_name).or_else(|| {
            speaker_member.and_then(|member| {
                member
                    .employee
                    .ticket
                    .as_ref()
                    .map(|ticket| ticket.project.trim())
                    .filter(|project| !project.is_empty())
                    .map(str::to_owned)
            })
        });
        let worktree = speaker_session_ref.is_some_and(|session| {
            matches!(session.workspace, SessionWorkspace::Worktree { .. })
        });
        // Last-updated: the outcome's own activity stamp, lifted by a
        // newer member session stamp when the snapshot leads it.
        let updated_at = [
            Some(row.outcome.last_activity_at),
            row.outcome.completed_at,
            speaker_session_ref.map(|session| session.updated_at),
            speaker_member.and_then(|member| member.employee.queued_at),
            speaker_member.and_then(|member| member.employee.expired_at),
        ]
        .into_iter()
        .flatten()
        .max();
        let updated_label = updated_at.map(|updated| {
            let elapsed = now.saturating_sub(updated);
            if elapsed < 60 {
                sidebar::format_time_ago(elapsed)
            } else {
                tr!(
                    "boss.goals_updated_ago",
                    ago = sidebar::format_time_ago(elapsed)
                )
            }
        });
        // Queued work beside the speaker stays indicated on the row —
        // the In progress section's "also queued" detail.
        let queued_extra = queued
            .len()
            .saturating_sub(usize::from(speaker_member.is_some_and(|member| {
                member.employee.lifecycle() == waku_protocol::boss::EmployeeLifecycle::Queued
            })));
        // The member-less row's honest detail: a flagged row names its
        // cause first — an open conflict's reason or the unresolved
        // blocker/failure — then the recorded pause and an owed
        // decision's count.
        let pending_handoffs = row.outcome.pending_handoffs().count();
        let detail = row
            .attention
            .then(|| {
                row.outcome
                    .completion_conflict
                    .as_ref()
                    .map(|conflict| conflict.reason.clone())
                    .or_else(|| boss_outcome_attention_cause(row))
            })
            .flatten()
            .or_else(|| boss_outcome_wait_label(&row.outcome, now))
            .or_else(|| {
                (pending_handoffs > 0).then(|| {
                    tr!("boss.goals_handoffs_owed", count = pending_handoffs)
                })
            });
        let avatar = speaker_member
            .map(|member| &member.employee.identity)
            .or_else(|| {
                last_assignment.and_then(|assignment| assignment.identity.as_ref())
            })
            .and_then(|identity| self.boss_avatar_image(identity, GOALS_PANEL_AVATAR));
        let expanded = self
            .boss_ui
            .goals_row_expanded
            .contains(&(key, row.outcome.id));
        BossOutcomePanelRow {
            outcome: row.outcome.id,
            focus,
            height: boss_goal_row_height(self.state.ui_font_size),
            bucket,
            status,
            queued_extra,
            member_name: speaker_name,
            avatar,
            project_label,
            detail,
            worktree,
            updated_label,
            attention: row.attention || status.attention(),
            expanded,
            aria: if expanded {
                tr!(
                    "boss.goals_hide_details",
                    title = title,
                    status = status_label
                )
            } else {
                tr!(
                    "boss.goals_show_details",
                    title = title,
                    status = status_label
                )
            },
            title,
            updated_sort: updated_at.unwrap_or(0),
            created_sort: row.outcome.created_at,
        }
    }
}

/// The expanded outcome's detail lines — every recorded assignment
/// attempt in admission order with its live status or settle verdict,
/// then the owed-decision and wait notes that explain the current state.
/// Attempts never drop off: retries and failures stay visible beside the
/// latest work. Entries that name a conversation the session snapshot
/// knows become keyboard-navigable destinations; the rest stay inert and
/// say so.
fn boss_outcome_goal_entries(
    row: &boss::BossOutcomeRow,
    sessions: &HashMap<Uuid, &AgentSession>,
    queued: &HashMap<Uuid, String>,
    now: u64,
) -> Vec<BossOutcomeEntry> {
    let mut entries = Vec::new();
    for assignment in &row.outcome.assignments {
        let status = boss_outcome_entry_status(row, assignment, sessions);
        let member = row
            .members
            .iter()
            .find(|member| member.employee.session_id == assignment.session);
        let name = assignment
            .identity
            .as_ref()
            .map(|identity| identity.name.clone())
            .or_else(|| member.map(|member| member.employee.identity.name.clone()));
        let job = assignment
            .job_title
            .as_deref()
            .map(str::trim)
            .filter(|job| !job.is_empty())
            .or_else(|| {
                member
                    .map(|member| member.employee.job_title.trim())
                    .filter(|job| !job.is_empty())
            });
        let title = match (name, job) {
            (Some(name), Some(job)) => format!("{name} — {job}"),
            (Some(name), None) => name,
            (None, job) => job
                .map(|job| format!("{} — {job}", tr!("boss.goals_unknown_employee")))
                .unwrap_or_else(|| tr!("boss.goals_entry_unavailable")),
        };
        let mut meta = vec![status.label()];
        if assignment.generation > 1 {
            meta.push(tr!(
                "boss.goals_entry_attempt",
                number = assignment.generation
            ));
        }
        if assignment.finishes_outcome == Some(true) {
            meta.push(tr!("boss.goals_entry_finisher"));
        }
        let blocked = assignment
            .settled
            .as_ref()
            .map(|settle| settle.blocked)
            .or_else(|| member.map(|member| member.employee.blocker.is_some()))
            .unwrap_or(false);
        if blocked {
            meta.push(tr!("boss.goals_entry_blocked"));
        }
        // The actual recorded cause behind a flagged or interrupted
        // result — the member's own blocker text while its record lives,
        // else the settle's finer-grained expiry cause.
        let cause = member
            .and_then(|member| member.employee.blocker.as_deref())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                assignment
                    .settled
                    .as_ref()
                    .and_then(|settle| settle.cause)
                    .and_then(boss_expiry_cause_label)
            });
        if let Some(cause) = cause {
            meta.push(cause);
        }
        if assignment.settled.is_none() {
            if let Some(wait) =
                member.and_then(|member| queued.get(&member.employee.session_id))
            {
                meta.push(wait.clone());
            }
            if boss_outcome_prerequisite_failed(row, assignment) {
                meta.push(tr!("boss.goals_prereq_failed"));
            }
        }
        if let Some(stamp) = assignment
            .settled
            .as_ref()
            .and_then(|settle| settle.at)
            .or(assignment.assigned_at)
        {
            meta.push(sidebar::format_time_ago(now.saturating_sub(stamp)));
        }
        let (icon, tone, spin) = status.marker();
        let detail = meta.join(" · ");
        let destination = sessions.contains_key(&assignment.session);
        entries.push(BossOutcomeEntry {
            focus: None,
            key: format!(
                "{}:{}:{}",
                row.outcome.id, assignment.session, assignment.generation
            ),
            session: Some(assignment.session),
            icon,
            tone,
            spin,
            aria: if destination {
                tr!(
                    "boss.goals_open_assignment",
                    label = title,
                    status = detail.clone()
                )
            } else {
                tr!(
                    "boss.goals_assignment_status",
                    label = title,
                    status = detail.clone()
                )
            },
            title,
            detail,
            destination,
        });
    }
    if let Some(conflict) = &row.outcome.completion_conflict {
        entries.push(BossOutcomeEntry {
            focus: None,
            key: format!("{}:conflict", row.outcome.id),
            session: None,
            icon: "icons/alert.svg",
            tone: BossGoalTone::Warning,
            spin: false,
            title: tr!("boss.goals_conflict"),
            aria: conflict.reason.clone(),
            detail: conflict.reason.clone(),
            destination: false,
        });
    }
    for handoff in row.outcome.pending_handoffs() {
        let assignee = row
            .outcome
            .assignments
            .iter()
            .rev()
            .find(|assignment| assignment.session == handoff.assignment)
            .and_then(|assignment| assignment.identity.as_ref())
            .map(|identity| identity.name.clone());
        let title = match assignee {
            Some(name) => format!("{} — {name}", tr!("boss.goals_handoff_pending")),
            None => tr!("boss.goals_handoff_pending"),
        };
        let mut detail = handoff.intent.trim().to_owned();
        let elapsed = now.saturating_sub(handoff.created_at);
        if elapsed > 0 {
            if !detail.is_empty() {
                detail.push_str(" · ");
            }
            detail.push_str(&sidebar::format_time_ago(elapsed));
        }
        let destination = sessions.contains_key(&handoff.assignment);
        entries.push(BossOutcomeEntry {
            focus: None,
            key: format!("{}:handoff:{}", row.outcome.id, handoff.id),
            session: Some(handoff.assignment),
            icon: "icons/bell.svg",
            tone: BossGoalTone::Warning,
            spin: false,
            aria: if destination {
                tr!(
                    "boss.goals_open_assignment",
                    label = title,
                    status = detail.clone()
                )
            } else {
                tr!(
                    "boss.goals_assignment_status",
                    label = title,
                    status = detail.clone()
                )
            },
            title,
            detail,
            destination,
        });
    }
    if let Some(wait) = boss_outcome_wait_label(&row.outcome, now) {
        entries.push(BossOutcomeEntry {
            focus: None,
            key: format!("{}:wait", row.outcome.id),
            session: None,
            icon: "icons/hourglass.svg",
            tone: BossGoalTone::Secondary,
            spin: false,
            aria: wait.clone(),
            title: wait,
            detail: String::new(),
            destination: false,
        });
    }
    if entries.is_empty() {
        entries.push(BossOutcomeEntry {
            focus: None,
            key: format!("{}:empty", row.outcome.id),
            session: None,
            icon: "icons/circle-dot.svg",
            tone: BossGoalTone::Tertiary,
            spin: false,
            aria: tr!("boss.goals_no_assignments"),
            title: tr!("boss.goals_no_assignments"),
            detail: String::new(),
            destination: false,
        });
    }
    entries
}

impl Waku {
    fn render_boss_goals_panel(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let Some(key) = self.boss_chat_key() else {
            let (title, description) = if self.boss_ui.states.is_empty() {
                (tr!("boss.goals_loading"), tr!("boss.goals_loading_body"))
            } else {
                (
                    tr!("boss.goals_unavailable"),
                    tr!("boss.goals_unavailable_body"),
                )
            };
            return div()
                .id("boss-goals-panel")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .px(px(12.0))
                .child(self.render_right_panel_empty_message(title, description, cx));
        };
        let rows = self
            .boss_ui
            .outcome_rows
            .get(&key)
            .cloned()
            .unwrap_or_else(|| Arc::new(Vec::new()));
        if rows.is_empty() {
            return div()
                .id("boss-goals-panel")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .px(px(12.0))
                .child(self.render_right_panel_empty_message(
                    tr!("boss.goals_empty_title"),
                    tr!("boss.goals_empty_body"),
                    cx,
                ));
        }
        let sessions: HashMap<Uuid, &AgentSession> = self
            .state
            .sessions
            .iter()
            .map(|session| (session.id, session))
            .collect();
        let now = unix_time();
        let mut finished: Vec<Arc<BossOutcomePanelRow>> = Vec::new();
        let mut running = Vec::new();
        let mut pending = Vec::new();
        // Expanded entries are prepared beside their rows so the item
        // lists and signatures below carry them; a row moving sections
        // keeps its expansion and its focus-friendly outcome id.
        let mut entries: HashMap<Uuid, Vec<Arc<BossOutcomeEntry>>> = HashMap::new();
        for row in rows.iter() {
            let focus = self
                .boss_ui
                .goals_focus_handles
                .entry((key, format!("row:{}", row.outcome.id)))
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let prepared = Arc::new(self.prepare_boss_outcome_row(
                row, key, &sessions, now, focus,
            ));
            if prepared.expanded {
                entries.insert(
                    row.outcome.id,
                    boss_outcome_goal_entries(row, &sessions, &self.boss_ui.queued, now)
                        .into_iter()
                        .map(|entry| {
                            let mut entry = entry;
                            if entry.destination {
                                entry.focus = Some(
                                    self.boss_ui
                                        .goals_focus_handles
                                        .entry((key, format!("entry:{}", entry.key)))
                                        .or_insert_with(|| {
                                            cx.focus_handle().tab_stop(true)
                                        })
                                        .clone(),
                                );
                            }
                            Arc::new(entry)
                        })
                        .collect(),
                );
            }
            match prepared.bucket {
                BossGoalBucket::Finished => finished.push(prepared),
                BossGoalBucket::Running => running.push(prepared),
                BossGoalBucket::Pending => pending.push(prepared),
            }
        }
        // Finished is newest first; In progress and Pending lead with
        // actionable items, then outcome age.
        finished.sort_by(|a, b| {
            b.updated_sort
                .cmp(&a.updated_sort)
                .then_with(|| a.outcome.cmp(&b.outcome))
        });
        running.sort_by(|a, b| {
            b.attention
                .cmp(&a.attention)
                .then_with(|| a.created_sort.cmp(&b.created_sort))
                .then_with(|| a.outcome.cmp(&b.outcome))
        });
        pending.sort_by(|a, b| {
            b.attention
                .cmp(&a.attention)
                .then_with(|| a.created_sort.cmp(&b.created_sort))
                .then_with(|| a.outcome.cmp(&b.outcome))
        });
        let finished_collapsed = self
            .boss_ui
            .goals_collapsed
            .contains(&(key, boss::BossGoalSection::Finished));
        let in_progress_collapsed = self
            .boss_ui
            .goals_collapsed
            .contains(&(key, boss::BossGoalSection::InProgress));
        let pending_collapsed = self
            .boss_ui
            .goals_collapsed
            .contains(&(key, boss::BossGoalSection::Pending));
        let history_expanded = self.boss_ui.goals_history_expanded.contains(&key);
        let recent_limit = if history_expanded {
            finished.len()
        } else {
            GOALS_PANEL_RECENT_LIMIT.min(finished.len())
        };
        // A row and its expanded entries travel together — the finished
        // viewport and the ongoing sections share the item shape so both
        // lists paint the same disclosure content.
        let expand = |row: &Arc<BossOutcomePanelRow>,
                      entries: &HashMap<Uuid, Vec<Arc<BossOutcomeEntry>>>| {
            let mut items = Vec::with_capacity(1 + entries.get(&row.outcome).map_or(0, Vec::len));
            items.push(BossOutcomeItem::Row(row.clone()));
            if let Some(list) = entries.get(&row.outcome) {
                items.extend(list.iter().cloned().map(BossOutcomeItem::Entry));
            }
            items
        };
        let visible_finished: Vec<BossOutcomeItem> = finished
            .iter()
            .take(recent_limit)
            .flat_map(|row| expand(row, &entries))
            .collect();
        let older_count = finished.len().saturating_sub(recent_limit);
        let mut ongoing_items: Vec<BossOutcomeItem> = Vec::new();
        for (section, label, section_rows, collapsed) in [
            (
                boss::BossGoalSection::InProgress,
                tr!("boss.goals_in_progress"),
                &running,
                in_progress_collapsed,
            ),
            (
                boss::BossGoalSection::Pending,
                tr!("boss.goals_section_pending"),
                &pending,
                pending_collapsed,
            ),
        ] {
            if section_rows.is_empty() {
                continue;
            }
            ongoing_items.push(BossOutcomeItem::Header {
                section,
                label,
                attention: section_rows.iter().filter(|row| row.attention).count(),
                collapsed,
                top_gap: !ongoing_items.is_empty(),
                focus: self
                    .boss_ui
                    .goals_focus_handles
                    .entry((key, format!("header:{section:?}")))
                    .or_insert_with(|| cx.focus_handle().tab_stop(true))
                    .clone(),
            });
            if !collapsed {
                ongoing_items.extend(section_rows.iter().flat_map(|row| expand(row, &entries)));
            }
        }
        // Focus handles live only while their item renders — a removed
        // outcome, a collapsed section's rows, or an entry line that
        // stopped navigating drop out and cannot hold a dead focus.
        let mut live_focus_keys: HashSet<String> = visible_finished
            .iter()
            .chain(ongoing_items.iter())
            .filter_map(boss_goals_item_focus_key)
            .collect();
        // The Finished header mounts beside its list rather than inside
        // it — account for it by hand.
        if !finished.is_empty() {
            live_focus_keys.insert(format!("header:{:?}", boss::BossGoalSection::Finished));
        }
        self.boss_ui
            .goals_focus_handles
            .retain(|(daemon, item_key), _| *daemon != key || live_focus_keys.contains(item_key));
        // Reset a viewport when its item sequence or scaled row height
        // changes — count-preserving reorders reset too, but title and
        // timestamp ticks never do.
        let row_height = boss_goal_row_height(self.state.ui_font_size);
        // The item signature names each row's outcome and each expanded
        // entry's own key — an expansion or a membership change in the
        // sequence resets the viewport; a status label tick does not.
        let item_signature = |hasher: &mut DefaultHasher, item: &BossOutcomeItem| match item {
            BossOutcomeItem::Header {
                section,
                attention,
                collapsed,
                ..
            } => {
                0u8.hash(hasher);
                section.hash(hasher);
                attention.hash(hasher);
                collapsed.hash(hasher);
            }
            BossOutcomeItem::Row(row) => {
                1u8.hash(hasher);
                row.outcome.hash(hasher);
            }
            BossOutcomeItem::Entry(entry) => {
                2u8.hash(hasher);
                entry.key.hash(hasher);
            }
        };
        let finished_signature = {
            let mut hasher = DefaultHasher::new();
            row_height.to_bits().hash(&mut hasher);
            for item in &visible_finished {
                item_signature(&mut hasher, item);
            }
            hasher.finish()
        };
        let ongoing_signature = {
            let mut hasher = DefaultHasher::new();
            row_height.to_bits().hash(&mut hasher);
            for item in &ongoing_items {
                item_signature(&mut hasher, item);
            }
            hasher.finish()
        };
        let owner_changed = self.boss_ui.goals_list_owner != Some(key);
        let finished_list = self.boss_ui.goals_finished_list.clone();
        let ongoing_list = self.boss_ui.goals_ongoing_list.clone();
        if owner_changed || self.boss_ui.goals_finished_signature != Some(finished_signature) {
            self.boss_ui.goals_finished_signature = Some(finished_signature);
            reset_boss_goals_list(&finished_list, &visible_finished, row_height);
        }
        if owner_changed || self.boss_ui.goals_ongoing_signature != Some(ongoing_signature) {
            self.boss_ui.goals_ongoing_signature = Some(ongoing_signature);
            reset_boss_goals_list(&ongoing_list, &ongoing_items, row_height);
        }
        self.boss_ui.goals_list_owner = Some(key);
        let offline = matches!(key, waku_client::DaemonKey::Remote(host) if !self.remote_host_connected(host));
        let waku = cx.entity().downgrade();
        let mut panel = div()
            .id("boss-goals-panel")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .px(px(12.0));
        if offline {
            panel = panel.child(
                div()
                    .flex_none()
                    .pb(px(4.0))
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("boss.goals_offline")),
            );
        }
        if !finished.is_empty() {
            let finished_header_focus = self
                .boss_ui
                .goals_focus_handles
                .entry((key, format!("header:{:?}", boss::BossGoalSection::Finished)))
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            panel = panel.child(boss_goal_section_header(
                boss::BossGoalSection::Finished,
                &tr!("boss.goals_section_finished"),
                0,
                finished_collapsed,
                false,
                &finished_header_focus,
                key,
                &waku,
                &theme,
            ));
            if !finished_collapsed {
                // The preview reserves five complete rows while ongoing
                // work exists; expanding hands history the
                // larger flex share, bounded by its real height, so Show
                // more visibly reveals older rows instead of only
                // lengthening a hidden scroll area.
                let estimate = visible_finished.len() as f32 * row_height;
                let ongoing_exists = !ongoing_items.is_empty();
                let scrollbar = self.boss_ui.goals_finished_scrollbar.clone();
                let list_state = finished_list.clone();
                let items = Arc::new(visible_finished);
                let weak = waku.clone();
                panel = panel.child(
                    div()
                        .relative()
                        .when(ongoing_exists && !history_expanded, |element| {
                            element.flex_none().h(px(estimate))
                        })
                        .when(ongoing_exists && history_expanded, |element| {
                            element
                                .flex_1()
                                .flex_grow(2.0)
                                .min_h_0()
                                .max_h(px(estimate))
                        })
                        .when(!ongoing_exists, |element| {
                            // Let long history scroll, but keep the toggle
                            // directly below short lists instead of consuming
                            // all unused panel space.
                            element.flex_1().min_h_0().max_h(px(estimate))
                        })
                        .child(
                            list(list_state.clone(), move |index, _window, cx| {
                                items.get(index).map_or_else(
                                    || div().into_any_element(),
                                    |item| match item {
                                        BossOutcomeItem::Row(row) => {
                                            boss_goal_panel_row_element(row, key, &weak, cx)
                                                .into_any_element()
                                        }
                                        BossOutcomeItem::Entry(entry) => {
                                            boss_goal_entry_element(entry, &weak, cx)
                                                .into_any_element()
                                        }
                                        BossOutcomeItem::Header { .. } => {
                                            div().into_any_element()
                                        }
                                    },
                                )
                            })
                            .size_full(),
                        )
                        .child(scrollbar::vertical(&list_state, &scrollbar)),
                );
                if finished.len() > GOALS_PANEL_RECENT_LIMIT {
                    let weak = waku.clone();
                    panel = panel.child(
                        div()
                            .id("boss-goals-history-toggle")
                            .tab_index(0)
                            .flex_none()
                            .h(px(28.0))
                            .mt(px(4.0))
                            // Align with the rows' text column — the row's
                            // inset plus the status-icon indent.
                            .pl(px(26.0))
                            .flex()
                            .items_center()
                            .cursor_default()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .focus_visible(|style| style.bg(theme.focus_highlight()))
                            .hover(|style| style.text_color(theme.text))
                            .child(if history_expanded {
                                tr!("boss.goals_show_less")
                            } else {
                                tr!("boss.goals_show_older", count = older_count)
                            })
                            .on_activation_app(move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    if !this.boss_ui.goals_history_expanded.remove(&key) {
                                        this.boss_ui.goals_history_expanded.insert(key);
                                    }
                                    cx.notify();
                                });
                            }),
                    );
                }
            }
        }
        if !ongoing_items.is_empty() {
            let scrollbar = self.boss_ui.goals_ongoing_scrollbar.clone();
            let list_state = ongoing_list.clone();
            let items = Arc::new(ongoing_items);
            let weak = waku.clone();
            panel = panel.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .when(!finished.is_empty(), |element| {
                        element.mt(px(GOALS_PANEL_SECTION_GAP))
                    })
                    .child(
                        list(list_state.clone(), move |index, _window, cx| {
                            items.get(index).map_or_else(
                                || div().into_any_element(),
                                |item| match item {
                                    BossOutcomeItem::Header {
                                        section,
                                        label,
                                        attention,
                                        collapsed,
                                        top_gap,
                                        focus,
                                    } => {
                                        let theme = Theme::current(cx);
                                        boss_goal_section_header(
                                            *section, label, *attention, *collapsed, *top_gap,
                                            focus, key, &weak, &theme,
                                        )
                                        .into_any_element()
                                    }
                                    BossOutcomeItem::Row(row) => {
                                        boss_goal_panel_row_element(row, key, &weak, cx)
                                            .into_any_element()
                                    }
                                    BossOutcomeItem::Entry(entry) => {
                                        boss_goal_entry_element(entry, &weak, cx)
                                            .into_any_element()
                                    }
                                },
                            )
                        })
                        .size_full(),
                    )
                    .child(scrollbar::vertical(&list_state, &scrollbar)),
            );
        }
        panel
    }

    fn expand_right_panel_diff_gap(
        &mut self,
        line_index: usize,
        direction: crate::review_diff::ExpansionDirection,
        cx: &mut Context<Self>,
    ) {
        let expansion = self
            .right_panel_diff_snapshot
            .as_mut()
            .and_then(|snapshot| Arc::make_mut(snapshot).expand_gap(line_index, direction));
        let Some(expansion) = expansion else {
            return;
        };
        self.right_panel_diff_list_state
            .splice(line_index..line_index + 1, expansion.replacement_count);
        cx.notify();
    }

    /// One listener set covers every selectable code line registered while
    /// the virtualized Review list paints this frame.
    fn right_panel_diff_selection_input(&self) -> impl IntoElement {
        let selection = self.right_panel_diff_selection.clone();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
            move |_, region, window, _| {
                md::render::install_selection_input(region, window, &selection, None)
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    fn render_right_panel_diff_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let focus = self.transcript_control_focus("right-panel-diff-tree", cx);
        let tree_focused = focus.is_focused(window);
        let entity = cx.entity().downgrade();
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(44.0))
                    .flex_none()
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .border_b(hairline())
                    .border_color(theme.separator)
                    .child(
                        TextField::new(
                            "right-panel-diff-filter",
                            self.right_panel_diff_filter.clone(),
                        )
                        .icon("icons/search.svg", 13.0)
                        .w_full(),
                    ),
            )
            .child(
                div()
                    .id("right-panel-diff-tree")
                    .track_focus(&focus)
                    .tab_index(0)
                    .key_context("ReviewDiffTree")
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        this.right_panel_diff_tree_key_down(event, window, cx)
                    }))
                    .child(
                        list(
                            self.right_panel_diff_tree_list_state.clone(),
                            move |index, _window, cx| {
                                entity
                                    .upgrade()
                                    .map(|entity| {
                                        entity.update(cx, |this, cx| {
                                            this.render_right_panel_diff_tree_row(
                                                index,
                                                tree_focused,
                                                cx,
                                            )
                                        })
                                    })
                                    .unwrap_or_else(|| div().into_any_element())
                            },
                        )
                        .size_full()
                        .py(px(4.0)),
                    )
                    .child(scrollbar::vertical(
                        &self.right_panel_diff_tree_list_state,
                        &self.right_panel_diff_tree_scrollbar,
                    )),
            )
    }

    fn render_right_panel_diff_tree_row(
        &self,
        index: usize,
        tree_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.right_panel_diff_tree_rows.borrow().get(index).cloned() else {
            return div().h(px(30.0)).into_any_element();
        };
        let theme = Theme::current(cx);
        let cursor = tree_focused && self.right_panel_diff_tree_cursor == Some(index);
        match row {
            ReviewDiffTreeRow::Directory {
                path,
                name,
                depth,
                expanded,
            } => div()
                .w_full()
                .h(px(30.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .child(
                    div()
                        .id(SharedString::from(format!("review-diff-directory-{path}")))
                        .h(px(26.0))
                        .flex_1()
                        .min_w_0()
                        .pl(px(7.0 + depth as f32 * 14.0))
                        .pr(px(7.0))
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .cursor_default()
                        .when(cursor, |row| row.bg(theme.overlay_strong))
                        .when(!cursor, |row| row.hover(|row| row.bg(theme.overlay)))
                        .child(icon(
                            if expanded {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            },
                            10.0,
                            theme.text_ghost,
                        ))
                        .child(icon("icons/folder.svg", 13.0, theme.text_tertiary))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_secondary)
                                .child(name),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let focus = this.transcript_control_focus("right-panel-diff-tree", cx);
                            focus.focus(window, cx);
                            this.right_panel_diff_tree_cursor = Some(index);
                            this.toggle_right_panel_diff_directory(path.clone(), cx);
                        })),
                )
                .into_any_element(),
            ReviewDiffTreeRow::File { file_index, depth } => {
                let Some(snapshot) = self.right_panel_diff_snapshot.as_ref() else {
                    return div().h(px(30.0)).into_any_element();
                };
                let Some(file) = snapshot.files.get(file_index) else {
                    return div().h(px(30.0)).into_any_element();
                };
                let path = file.path.clone();
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                let selected = self.right_panel_diff_selected_file == Some(file_index);
                let (status, status_color) = match file.status {
                    crate::review_diff::FileStatus::Added => ("A", theme.success),
                    crate::review_diff::FileStatus::Deleted => ("D", theme.danger),
                    crate::review_diff::FileStatus::Binary => ("B", theme.warning),
                    crate::review_diff::FileStatus::Modified => ("M", theme.warning),
                };
                div()
                    .w_full()
                    .h(px(30.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id(SharedString::from(format!("review-diff-tree-file-{path}")))
                            .h(px(26.0))
                            .flex_1()
                            .min_w_0()
                            .pl(px(23.0 + depth as f32 * 14.0))
                            .pr(px(7.0))
                            .rounded(px(5.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_default()
                            .when(selected && cursor, |row| row.bg(theme.overlay_strong))
                            .when(selected ^ cursor, |row| row.bg(theme.overlay))
                            .when(!selected && !cursor, |row| {
                                row.hover(|row| row.bg(theme.overlay))
                            })
                            .child(file_icon(file_icon_for_path(&path), 13.0))
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "review-diff-tree-file-path-{file_index}"
                                    )))
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .text_color(if selected {
                                        theme.text
                                    } else {
                                        theme.text_secondary
                                    })
                                    .tooltip(Tooltip::text(path.clone()))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .w(px(18.0))
                                    .h(px(18.0))
                                    .flex_none()
                                    .rounded(px(4.0))
                                    .border(hairline())
                                    .border_color(status_color.opacity(0.65))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(sp(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(status_color)
                                    .child(status),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let focus =
                                    this.transcript_control_focus("right-panel-diff-tree", cx);
                                focus.focus(window, cx);
                                this.right_panel_diff_tree_cursor = Some(index);
                                this.select_right_panel_diff_file(file_index, cx);
                            })),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_right_panel_empty_message(
        &self,
        title: String,
        description: String,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .pb(px(32.0))
            .child(
                div()
                    .text_size(sp(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(title),
            )
            .child(
                div()
                    .mt(px(6.0))
                    .max_w(px(300.0))
                    .text_center()
                    .text_size(sp(12.5))
                    .line_height(sp(17.0))
                    .text_color(theme.text_tertiary)
                    .child(description),
            )
    }

    /// Re-reads whichever workspace surface is on screen.
    pub(super) fn refresh_workspace_surfaces(&mut self, cx: &mut Context<Self>) {
        match self.active_right_panel_surface() {
            Some(RightPanelSurface::Diff) => self.refresh_right_panel_diff(cx),
            Some(RightPanelSurface::Files | RightPanelSurface::File(_)) => {
                self.refresh_right_panel_working_tree(cx)
            }
            _ => {}
        }
    }

    /// The directory the Files surface and its editors are rooted at right
    /// now, from the owner on screen: a trusted Friends delivery's payload,
    /// the selected session's workspace, the selected terminal's live cwd,
    /// or the project a Projects page is scoped to. Pages that admit no files
    /// and quarantined deliveries resolve to `None`.
    pub(super) fn resolve_right_panel_files_root(&self, cx: &App) -> Option<PathBuf> {
        match self.active_right_panel_owner() {
            RightPanelOwner::Session(session_id) => {
                let session = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)?;
                if session.quarantined
                    || (session.friend_peer_id.is_some() && !session.detail_loaded)
                {
                    return None;
                }
                if session.friend_peer_id.is_some()
                    && let Some((payload_path, is_dir)) =
                        session
                            .messages
                            .iter()
                            .find_map(|message| match message.notice.as_ref() {
                                Some(crate::model::TranscriptNotice::TransferReceived {
                                    path,
                                    is_dir,
                                    ..
                                }) => Some((path, *is_dir)),
                                _ => None,
                            })
                {
                    return if is_dir {
                        Some(payload_path.clone())
                    } else {
                        payload_path.parent().map(Path::to_path_buf)
                    };
                }
                self.selected_workspace_path()
                    .map(std::path::Path::to_path_buf)
            }
            RightPanelOwner::Terminal(terminal_id) => self.terminal_cwd(terminal_id, cx),
            RightPanelOwner::Projects(project_id) => self
                .state
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone()),
            RightPanelOwner::Boss(key) => {
                // An armed sidebar deliverable roots the boss chat's file
                // surfaces at the path the boss published — its preview
                // page reads through the same root. Remote deliverables have
                // no local slice.
                if key != waku_client::DaemonKey::Local {
                    return None;
                }
                if let Some((deliverable_key, deliverable_id)) = self.boss_ui.command_deliverable
                    && deliverable_key == key
                    && let Some(deliverable) = self.boss_ui.states.get(&key).and_then(|state| {
                        state
                            .deliverables
                            .iter()
                            .find(|deliverable| deliverable.id == deliverable_id)
                    })
                {
                    let path = PathBuf::from(&deliverable.path);
                    return if deliverable.directory {
                        Some(path)
                    } else {
                        path.parent().map(Path::to_path_buf)
                    };
                }
                // Otherwise the boss chat's file surfaces and the `Cmd+P`
                // finder root at the boss's own files — the `files/`
                // sibling of the workspace its project names. The Boss
                // page's strip admits no file tabs, so it resolves to
                // nothing rather than opening dead-end previews.
                if self.boss_ui.page.is_some() {
                    return None;
                }
                let state = self.boss_ui.states.get(&key)?;
                let project = self.boss_ui.projects.get(&state.identity.id)?;
                project.path.parent().map(|boss| boss.join("files"))
            }
            RightPanelOwner::Inbox
            | RightPanelOwner::Drafts
            | RightPanelOwner::Automations
            | RightPanelOwner::Bare => None,
        }
    }

    /// Re-root the files slice when the resolved root drifts — a `cd` in the
    /// selected terminal, a different terminal coming forward, a worktree
    /// moving. The outgoing slice parks under its root the way a session's
    /// panel state parks under its id, so a dirty editor is never pointed at
    /// a different file; the incoming root's slice returns exactly as it was
    /// left. Returns whether a swap ran.
    pub(super) fn sync_right_panel_files_root(&mut self, cx: &mut Context<Self>) -> bool {
        let resolved = self.resolve_right_panel_files_root(cx);
        if resolved == self.right_panel_files_root {
            return false;
        }
        let slice = self.take_right_panel_files_slice();
        if let Some(old_root) = self.right_panel_files_root.take() {
            self.right_panel_parked_files.insert(old_root, slice);
        }
        // The parked entry is consumed: the same root opening twice would
        // otherwise hand out a second copy of its editors.
        let slice = resolved
            .as_ref()
            .and_then(|root| self.right_panel_parked_files.remove(root))
            .unwrap_or_else(|| ParkedPanelFiles {
                surfaces: Vec::new(),
                active_surface: None,
                files_selected_path: None,
                expanded_paths: HashSet::new(),
                file_editors: HashMap::new(),
                file_tree_width: DEFAULT_FILE_TREE_WIDTH,
                file_tree_visible: true,
            });
        self.restore_right_panel_files_slice(slice);
        self.right_panel_files_root = resolved;
        // Search and jump state pointed into editors that just swapped out.
        self.reset_file_search_for_session(cx);
        self.reset_go_to_line_for_session(cx);
        self.right_panel_pending_file_focus = None;
        cx.notify();
        true
    }

    /// Pull every relative-path-keyed piece of the strip out for parking:
    /// the File/Files surfaces with their indices, then the stores those
    /// surfaces name. `active_surface` is re-pointed at the surface that
    /// slid into the removed slot, matching the close path's arithmetic.
    fn take_right_panel_files_slice(&mut self) -> ParkedPanelFiles {
        let mut surfaces = Vec::new();
        for index in (0..self.right_panel_surfaces.len()).rev() {
            if matches!(
                self.right_panel_surfaces[index],
                RightPanelSurface::Files | RightPanelSurface::File(_)
            ) {
                surfaces.push((index, self.right_panel_surfaces.remove(index)));
            }
        }
        surfaces.reverse();
        let parked_active = self
            .right_panel_active_surface
            .and_then(|active| surfaces.iter().position(|(index, _)| *index == active));
        if let Some(active) = self.right_panel_active_surface {
            let removed_before = surfaces.iter().filter(|(index, _)| *index < active).count();
            self.right_panel_active_surface = if self.right_panel_surfaces.is_empty() {
                None
            } else {
                Some((active - removed_before).min(self.right_panel_surfaces.len() - 1))
            };
        }
        ParkedPanelFiles {
            surfaces,
            active_surface: parked_active,
            files_selected_path: self.right_panel_files_selected_path.take(),
            expanded_paths: std::mem::take(&mut self.right_panel_expanded_paths),
            // Same shed as the strip park: clean editors re-read on restore;
            // dirty buffers and annotation pins stay.
            file_editors: std::mem::take(&mut self.right_panel_file_editors)
                .into_iter()
                .filter(|(_, editor)| editor.dirty || !editor.annotations.borrow().items.is_empty())
                .collect(),
            file_tree_width: self.right_panel_file_tree_width,
            file_tree_visible: self.right_panel_file_tree_visible,
        }
    }

    /// Return a parked slice to the strip. Surfaces re-insert in ascending
    /// order at their recorded indices, which reproduces their original
    /// positions; the strip's active index shifts once per surface landing
    /// at or before it, then a parked-active file tab takes the slot back.
    fn restore_right_panel_files_slice(&mut self, parked: ParkedPanelFiles) {
        self.right_panel_files_selected_path = parked.files_selected_path;
        self.right_panel_expanded_paths = parked.expanded_paths;
        self.right_panel_file_editors = parked.file_editors;
        self.right_panel_file_tree_width = parked.file_tree_width;
        self.right_panel_file_tree_visible = parked.file_tree_visible;
        let mut restored_active = None;
        for (position, (index, surface)) in parked.surfaces.into_iter().enumerate() {
            let index = index.min(self.right_panel_surfaces.len());
            self.right_panel_surfaces.insert(index, surface);
            if parked.active_surface == Some(position) {
                restored_active = Some(index);
            }
            if let Some(active) = self.right_panel_active_surface
                && index <= active
            {
                self.right_panel_active_surface = Some(active + 1);
            }
        }
        if let Some(active) = restored_active.or(self.right_panel_active_surface) {
            self.right_panel_active_surface = Some(active);
            self.reveal_right_panel_tab(active);
        } else if !self.right_panel_surfaces.is_empty() {
            self.right_panel_active_surface = Some(0);
        }
    }

    /// Re-walks the files root's working tree.
    ///
    /// `read_dir` plus a `stat` per entry, recursively over expanded
    /// directories — filesystem I/O, so it runs on the background executor and
    /// the panel keeps drawing the previous listing until the result lands.
    /// Called when the tree's inputs change, never from a frame.
    pub(super) fn refresh_right_panel_working_tree(&mut self, cx: &mut Context<Self>) {
        self.sync_right_panel_files_root(cx);
        let Some(project_path) = self.right_panel_files_root.clone() else {
            self.right_panel_working_tree.clear();
            return;
        };
        // The tree on disk moves under us, and the expanded set may just have
        // changed, so a cached listing is only good until something asks again.
        self.working_trees.invalidate(&project_path);
        match self.working_trees.read(&project_path) {
            Query::Ready(entries) => {
                self.right_panel_working_tree = (*entries).clone();
                self.resolve_right_panel_pending_tree_reveal();
            }
            Query::Pending => {}
            Query::Missing(token) => {
                let Some(workspace) = self.workspace_client_for_path(&project_path) else {
                    return;
                };
                let expanded = self.right_panel_expanded_paths.clone();
                cx.spawn(async move |waku, cx| {
                    let entries = cx
                        .background_executor()
                        .spawn({
                            let path = project_path.clone();
                            async move {
                                match workspace.request(waku_client::WorkspaceOperation::ListTree {
                                    root: path,
                                    expanded_paths: expanded.into_iter().collect(),
                                }) {
                                    Ok(waku_client::WorkspaceResult::WorkingTree { entries }) => {
                                        entries
                                            .into_iter()
                                            .map(|entry| WorkingTreeEntry {
                                                file_icon: (!entry.is_dir)
                                                    .then(|| file_icon_for_name(&entry.name)),
                                                relative_path: entry.relative_path,
                                                absolute_path: entry.absolute_path,
                                                name: entry.name,
                                                is_dir: entry.is_dir,
                                                expanded: entry.expanded,
                                                depth: entry.depth,
                                            })
                                            .collect()
                                    }
                                    Ok(_) | Err(_) => Vec::new(),
                                }
                            }
                        })
                        .await;
                    waku.update(cx, |waku, cx| {
                        if waku.working_trees.fulfill(token, entries.clone())
                            && waku.right_panel_files_root.as_deref()
                                == Some(project_path.as_path())
                        {
                            waku.right_panel_working_tree = entries;
                            waku.resolve_right_panel_pending_tree_reveal();
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    fn latest_review_turn_source(&self) -> Option<ReviewDiffSource> {
        let session = self.selected_session()?;
        session
            .turns
            .iter()
            .rev()
            .find(|turn| {
                turn.turn_count > 0
                    && turn
                        .checkpoint
                        .as_ref()
                        .is_some_and(|checkpoint| checkpoint.status == CheckpointStatus::Ready)
            })
            .map(|turn| ReviewDiffSource::LastTurn {
                session_id: session.id,
                turn_id: turn.id,
                turn_count: turn.turn_count,
            })
    }

    fn review_diff_source_label(&self, source: ReviewDiffSource) -> String {
        match source {
            ReviewDiffSource::LastTurn { .. }
                if self.latest_review_turn_source() == Some(source) =>
            {
                tr!("diff.source_last_turn")
            }
            ReviewDiffSource::LastTurn { turn_count, .. } => {
                tr!("diff.source_turn", turn = turn_count)
            }
            ReviewDiffSource::Uncommitted => tr!("diff.source_uncommitted"),
            ReviewDiffSource::Unstaged => tr!("diff.source_unstaged"),
            ReviewDiffSource::Staged => tr!("diff.source_staged"),
            ReviewDiffSource::Committed => tr!("diff.source_committed"),
            ReviewDiffSource::Branch => tr!("diff.source_branch"),
            ReviewDiffSource::Commit => tr!("diff.source_commit"),
        }
    }

    pub(super) fn set_right_panel_diff_source(
        &mut self,
        source: ReviewDiffSource,
        cx: &mut Context<Self>,
    ) {
        if self.right_panel_diff_source != source {
            self.right_panel_diff_selection.clear();
            self.right_panel_diff_source = source;
            self.right_panel_diff_snapshot = None;
            self.right_panel_diff_error = None;
            self.right_panel_diff_selected_file = None;
            self.right_panel_diff_expanded_paths.clear();
            self.right_panel_diff_tree_cursor = None;
            self.right_panel_diff_tree_rows.borrow_mut().clear();
            self.right_panel_diff_tree_list_state.reset(0);
            self.right_panel_diff_list_state.reset(0);
        }
        self.open_right_panel_surface(RightPanelSurface::Diff, cx);
    }

    /// Captures one stable Git range and turns it into render-ready rows. Git,
    /// patch parsing, and syntax tokenization all stay off the UI thread; the
    /// generation check prevents an old source or session from landing late.
    fn refresh_right_panel_diff(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            self.right_panel_diff_selection.clear();
            self.right_panel_diff_snapshot = None;
            self.right_panel_diff_loading = false;
            self.right_panel_diff_error = Some(tr!("diff.unavailable"));
            return;
        };
        let Some(project_path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            self.right_panel_diff_selection.clear();
            self.right_panel_diff_snapshot = None;
            self.right_panel_diff_loading = false;
            self.right_panel_diff_error = Some(tr!("diff.unavailable"));
            return;
        };

        self.right_panel_diff_generation = self.right_panel_diff_generation.wrapping_add(1);
        let generation = self.right_panel_diff_generation;
        let source = self.right_panel_diff_source;
        let had_snapshot = self.right_panel_diff_snapshot.is_some();
        let previous_directories = self
            .right_panel_diff_snapshot
            .as_ref()
            .map_or_else(HashSet::new, |snapshot| {
                review_diff_directory_paths(&snapshot.files)
            });
        let selected_path = self.right_panel_diff_selected_file.and_then(|index| {
            self.right_panel_diff_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.files.get(index))
                .map(|file| file.path.clone())
        });
        self.right_panel_diff_loading = true;
        self.right_panel_diff_error = None;
        cx.notify();

        let Some(workspace) = self.workspace_client_for_path(&project_path) else {
            self.right_panel_diff_loading = false;
            self.right_panel_diff_error = Some(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let project_path = project_path.clone();
                    async move {
                        match workspace.request(
                            waku_client::WorkspaceOperation::CollectReviewDiff {
                                cwd: project_path,
                                source: crate::review_diff::wire_source(source),
                            },
                        )? {
                            waku_client::WorkspaceResult::ReviewDiff { data } => {
                                Ok(crate::review_diff::parse_collected(
                                    source,
                                    &data.numstat,
                                    &data.patch,
                                    data.complete_context,
                                ))
                            }
                            _ => anyhow::bail!("the daemon returned an invalid diff response"),
                        }
                    }
                })
                .await;
            waku.update(cx, |waku, cx| {
                let still_current = waku.state.selected_session == Some(session_id)
                    && waku.right_panel_diff_generation == generation
                    && waku.right_panel_diff_source == source
                    && waku
                        .selected_workspace_path()
                        .is_some_and(|path| path == project_path);
                if !still_current {
                    return;
                }

                waku.right_panel_diff_loading = false;
                match result {
                    Ok(snapshot) => {
                        waku.right_panel_diff_selection.clear();
                        let directories = review_diff_directory_paths(&snapshot.files);
                        if had_snapshot {
                            waku.right_panel_diff_expanded_paths
                                .retain(|path| directories.contains(path));
                            waku.right_panel_diff_expanded_paths
                                .extend(directories.difference(&previous_directories).cloned());
                        } else {
                            waku.right_panel_diff_expanded_paths = directories;
                        }
                        // A file row sent over from the Git panel wins the
                        // selection over the previously selected path.
                        let pending_path = waku.right_panel_pending_diff_file.take();
                        waku.right_panel_diff_selected_file = pending_path
                            .as_deref()
                            .and_then(|path| {
                                snapshot.files.iter().position(|file| file.path == path)
                            })
                            .or_else(|| {
                                selected_path.as_deref().and_then(|path| {
                                    snapshot.files.iter().position(|file| file.path == path)
                                })
                            })
                            .or_else(|| (!snapshot.files.is_empty()).then_some(0));
                        let line_count = snapshot.lines.len();
                        waku.right_panel_diff_snapshot = Some(Arc::new(snapshot));
                        waku.right_panel_diff_error = None;
                        waku.right_panel_diff_list_state.reset(line_count);
                        waku.sync_right_panel_diff_tree_rows(cx);
                        if pending_path.is_some()
                            && let Some(index) = waku.right_panel_diff_selected_file
                        {
                            waku.select_right_panel_diff_file(index, cx);
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        if waku.right_panel_diff_snapshot.is_some() {
                            waku.show_toast(tr!("diff.refresh_failed", error = message));
                        } else {
                            waku.right_panel_diff_error = Some(message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn sync_right_panel_diff_tree_rows(&mut self, cx: &mut Context<Self>) {
        let filter = self.right_panel_diff_filter.read(cx).content().to_owned();
        let previous_cursor_row = self
            .right_panel_diff_tree_cursor
            .and_then(|index| self.right_panel_diff_tree_rows.borrow().get(index).cloned());
        let rows = self
            .right_panel_diff_snapshot
            .as_ref()
            .map_or_else(Vec::new, |snapshot| {
                review_diff_tree_rows(
                    &snapshot.files,
                    &self.right_panel_diff_expanded_paths,
                    &filter,
                )
            });
        let cursor = previous_cursor_row
            .as_ref()
            .and_then(|previous| {
                rows.iter().position(|row| match (previous, row) {
                    (
                        ReviewDiffTreeRow::Directory { path: left, .. },
                        ReviewDiffTreeRow::Directory { path: right, .. },
                    ) => left == right,
                    (
                        ReviewDiffTreeRow::File {
                            file_index: left, ..
                        },
                        ReviewDiffTreeRow::File {
                            file_index: right, ..
                        },
                    ) => left == right,
                    _ => false,
                })
            })
            .or_else(|| {
                self.right_panel_diff_selected_file.and_then(|selected| {
                    rows.iter().position(|row| {
                        matches!(
                            row,
                            ReviewDiffTreeRow::File { file_index, .. }
                                if *file_index == selected
                        )
                    })
                })
            })
            .or_else(|| (!rows.is_empty()).then_some(0));
        let row_count = rows.len();
        *self.right_panel_diff_tree_rows.borrow_mut() = rows;
        self.right_panel_diff_tree_cursor = cursor;
        self.right_panel_diff_tree_list_state
            .reset_with_uniform_height(row_count, px(30.0));
    }

    fn toggle_right_panel_diff_directory(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.right_panel_diff_expanded_paths.remove(&path) {
            self.right_panel_diff_expanded_paths.insert(path);
        }
        self.sync_right_panel_diff_tree_rows(cx);
        cx.notify();
    }

    pub(super) fn select_right_panel_diff_file(
        &mut self,
        file_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.right_panel_diff_selected_file = Some(file_index);
        if let Some(line) = self
            .right_panel_diff_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.files.get(file_index))
            .and_then(|file| file.diff_line)
        {
            // `scroll_to_reveal_item` bottom-aligns targets below the viewport,
            // which can reveal only the file header and leave its diff body
            // off-screen. A tree selection is an explicit jump, so top-anchor
            // the header and expose the content immediately below it.
            self.right_panel_diff_list_state
                .scroll_to(gpui::ListOffset {
                    item_ix: line,
                    offset_in_item: px(0.0),
                });
        }
        cx.notify();
    }

    fn right_panel_diff_tree_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.right_panel_diff_tree_rows.borrow().clone();
        if rows.is_empty() {
            return;
        }
        let current = self
            .right_panel_diff_tree_cursor
            .filter(|index| *index < rows.len())
            .unwrap_or(0);
        let key = event.keystroke.key.as_str();
        let target = match key {
            "up" => Some(current.saturating_sub(1)),
            "down" => Some((current + 1).min(rows.len() - 1)),
            "home" => Some(0),
            "end" => Some(rows.len() - 1),
            "left" => match &rows[current] {
                ReviewDiffTreeRow::Directory {
                    path,
                    expanded: true,
                    ..
                } => {
                    self.toggle_right_panel_diff_directory(path.clone(), cx);
                    None
                }
                ReviewDiffTreeRow::Directory { depth, .. }
                | ReviewDiffTreeRow::File { depth, .. } => {
                    rows[..current].iter().rposition(|row| {
                        matches!(
                            row,
                            ReviewDiffTreeRow::Directory {
                                depth: parent_depth,
                                ..
                            } if *parent_depth < *depth
                        )
                    })
                }
            },
            "right" => match &rows[current] {
                ReviewDiffTreeRow::Directory {
                    path,
                    expanded: false,
                    ..
                } => {
                    self.toggle_right_panel_diff_directory(path.clone(), cx);
                    None
                }
                ReviewDiffTreeRow::Directory { depth, .. }
                    if rows.get(current + 1).is_some_and(|row| match row {
                        ReviewDiffTreeRow::Directory {
                            depth: child_depth, ..
                        }
                        | ReviewDiffTreeRow::File {
                            depth: child_depth, ..
                        } => child_depth > depth,
                    }) =>
                {
                    Some(current + 1)
                }
                _ => None,
            },
            "enter" | "space" => {
                match &rows[current] {
                    ReviewDiffTreeRow::Directory { path, .. } => {
                        self.toggle_right_panel_diff_directory(path.clone(), cx)
                    }
                    ReviewDiffTreeRow::File { file_index, .. } => {
                        self.select_right_panel_diff_file(*file_index, cx)
                    }
                }
                None
            }
            _ => return,
        };
        if let Some(target) = target {
            self.right_panel_diff_tree_cursor = Some(target);
            self.right_panel_diff_tree_list_state
                .scroll_to_reveal_item(target);
            cx.notify();
        }
        cx.stop_propagation();
    }
}
