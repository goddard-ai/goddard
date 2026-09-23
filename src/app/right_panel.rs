use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::*;

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
    ProjectFile(String),
    Finder(PathBuf),
    /// A `goddard://task/<id>` reference — `None` when the id is malformed.
    Task(Option<Uuid>),
    External,
}

pub(super) fn positive_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<usize>().is_ok_and(|value| value > 0)
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
    let target = strip_file_location(target.trim());
    if target
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
    {
        return url::Url::parse(target).ok()?.to_file_path().ok();
    }

    let path = PathBuf::from(percent_decode_file_path(target));
    path.is_absolute().then_some(path)
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
        return TranscriptLinkRoute::Task(Uuid::parse_str(rest.trim_end_matches('/')).ok());
    }
    let Some(path) = markdown_file_link_path(target) else {
        return TranscriptLinkRoute::External;
    };
    let path = normalized_path(&path);
    if let Some(relative_path) =
        workspace.and_then(|workspace| workspace_relative_file_path(workspace, &path))
    {
        TranscriptLinkRoute::ProjectFile(relative_path)
    } else {
        TranscriptLinkRoute::Finder(path)
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
        }
    }

    fn icon_path(&self) -> &'static str {
        match self {
            Self::Browser(_) => "icons/globe.svg",
            Self::Terminal(_) => "icons/terminal.svg",
            Self::BackgroundWork { key, .. } => work_kind_icon(key.kind),
            Self::PullRequest { .. } => "icons/git-pull-request-arrow.svg",
            Self::Files => "icons/folder.svg",
            Self::Diff => "icons/file-diff.svg",
            Self::File(path) | Self::FileAtRef { path, .. } => file_icon_for_path(path),
            Self::GitHub(_) => "icons/github.svg",
            Self::SideChat(_) => "icons/chat.svg",
        }
    }
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

fn reusable_surface_index(
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
        RightPanelSurface::Files
        | RightPanelSurface::Diff
        | RightPanelSurface::File(_)
        | RightPanelSurface::FileAtRef { .. }
        | RightPanelSurface::PullRequest { .. } => {
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

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

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
            TranscriptLinkRoute::ProjectFile(relative_project_file.clone())
        );
        assert_eq!(
            transcript_link_route(&project_file_with_column, Some(workspace)),
            TranscriptLinkRoute::ProjectFile(relative_project_file)
        );

        let encoded_file_url =
            url::Url::from_file_path(workspace.join("My File.rs")).expect("absolute file path");
        assert_eq!(
            transcript_link_route(&format!("{encoded_file_url}#L12C4"), Some(workspace)),
            TranscriptLinkRoute::ProjectFile("My File.rs".into())
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
    fn review_render_path_only_reads_the_in_memory_snapshot() {
        let source = include_str!("right_panel.rs");
        let start = source
            .find("\n    fn render_right_panel_diff(")
            .expect("review render fn");
        let body = &source[start + 1..];
        let end = body
            .find("\n    fn render_right_panel_empty_message(")
            .expect("review render end");
        let body = &body[..end];

        for forbidden in [
            "Command::new",
            "std::fs::",
            "review_diff::collect",
            "capture_worktree_commit",
        ] {
            assert!(
                !body.contains(forbidden),
                "Review rendering must not call `{forbidden}`; prepare it in refresh_right_panel_diff"
            );
        }
    }

    /// A wrapped diff line must grow its row rather than be clipped by it.
    /// Both the panel's own rows and the shared code row have to hold this,
    /// and the shared one is also what the transcript's diff paints with.
    #[test]
    fn diff_text_rows_soft_wrap() {
        let source = include_str!("right_panel.rs");
        let panel = source
            .split_once("\n    fn render_right_panel_diff_line(")
            .expect("review diff line renderer")
            .1
            .split_once("\n    #[allow(clippy::too_many_arguments)]")
            .expect("review diff line renderer end")
            .0;
        let shared = source
            .split_once("\npub(super) fn render_diff_code_row(")
            .expect("shared diff code row")
            .1
            .split_once("\nfn review_diff_flat_text(")
            .expect("shared diff code row end")
            .0;

        for body in [panel, shared] {
            assert!(!body.contains(".whitespace_nowrap()"));
        }
        assert!(panel.matches(".whitespace_normal()").count() >= 2);
        assert!(shared.contains(".whitespace_normal()"));
        assert!(shared.contains(".min_h(px(style.row_height))"));
        assert!(!shared.contains(".h(px(style.row_height))"));
    }

    /// The render path must never reach the filesystem. This reads the source
    /// rather than the behaviour, because the cost of a regression here is a
    /// syscall per directory entry on every frame — invisible until a project
    /// is large or its volume is slow.
    #[test]
    fn the_working_tree_render_path_does_no_filesystem_work() {
        let source = include_str!("right_panel.rs");
        // Anchored on the definition's indentation so this test does not match
        // its own string literals.
        let start = source
            .find("\n    fn render_right_panel_working_tree(")
            .expect("render fn");
        let body = &source[start + 1..];
        let end = body.find("\n    fn ").unwrap_or(body.len());
        let body = &body[..end];

        for forbidden in [
            "visible_working_tree_entries",
            "read_dir",
            "std::fs::",
            "metadata(",
        ] {
            assert!(
                !body.contains(forbidden),
                "render_right_panel_working_tree must not call `{forbidden}`; \
                 walk the tree in refresh_right_panel_working_tree instead"
            );
        }
    }

    /// Same guard for the file editor, which `render_right_panel_file` reaches
    /// on every frame that draws a file tab. Opening a large file used to read
    /// it inline, so the frame that revealed the tab paid for the whole file.
    #[test]
    fn the_file_editor_render_path_does_no_filesystem_work() {
        let source = include_str!("right_panel.rs");
        let start = source
            .find("\n    fn ensure_right_panel_file_editor(")
            .expect("ensure fn");
        let body = &source[start + 1..];
        let end = body
            .find("\n    /// Reads a file into its editor")
            .unwrap_or(body.len());
        let body = &body[..end];

        for forbidden in ["read_right_panel_file(", "std::fs::", "metadata("] {
            assert!(
                !body.contains(forbidden),
                "ensure_right_panel_file_editor must not call `{forbidden}`; \
                 read the file in read_right_panel_file_into_editor instead"
            );
        }
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
        let source = include_str!("right_panel.rs");
        let header = source
            .split_once("\n    fn render_right_panel_header(")
            .expect("right panel header renderer")
            .1
            .split_once("\n    fn render_right_panel_chooser(")
            .expect("right panel header renderer end")
            .0;

        assert!(header.contains(".truncate()"));
        assert!(!header.contains(".line_clamp(1)"));

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
}

impl Waku {
    pub(super) fn open_transcript_link(&mut self, target: &str, cx: &mut Context<Self>) -> bool {
        let files_root = self.resolve_right_panel_files_root(cx);
        match transcript_link_route(target, files_root.as_deref()) {
            TranscriptLinkRoute::ProjectFile(relative_path) => {
                // A `file:line` target rides the same pending slot the finder
                // uses: the editor takes focus and the jump lands once the
                // file's first read does.
                let location = file_link_location(target);
                self.open_right_panel_surface(RightPanelSurface::Files, cx);
                self.open_right_panel_file(relative_path.clone(), cx);
                if let Some((line, column)) = location {
                    self.right_panel_pending_file_focus = Some(PendingFileFocus {
                        path: relative_path,
                        position: Some((line, column.unwrap_or(1))),
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
            TranscriptLinkRoute::Task(task_id) => {
                let known = task_id
                    .is_some_and(|id| self.state.sessions.iter().any(|session| session.id == id));
                match (task_id, known) {
                    (Some(id), true) => self.select_session(id, cx),
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

    /// Which place the live strip belongs to right now, derived from the
    /// same flags `navigation_location` reads — a page beats the selection
    /// parked underneath it.
    pub(super) fn active_right_panel_owner(&self) -> RightPanelOwner {
        if self.notifications.open {
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
        let owner = self.active_right_panel_owner();
        if owner == self.right_panel_live_owner {
            return;
        }
        let parked = self.take_active_right_panel_state();
        self.right_panel_states
            .insert(self.right_panel_live_owner, parked);
        let incoming = RightPanelSessionState::take_or_closed(&mut self.right_panel_states, owner);
        self.right_panel_live_owner = owner;
        self.restore_right_panel_state(incoming, cx);
    }

    /// What the current owner lets into its strip: sessions and main-area
    /// terminals take everything, a project page takes its issue/PR details
    /// and files rooted at the project, and pages without their own surface
    /// take nothing.
    fn right_panel_owner_allows(&self, surface: &RightPanelSurface) -> bool {
        match self.active_right_panel_owner() {
            RightPanelOwner::Session(_) | RightPanelOwner::Terminal(_) | RightPanelOwner::Bare => {
                true
            }
            RightPanelOwner::Projects(_) => matches!(
                surface,
                RightPanelSurface::GitHub(_)
                    | RightPanelSurface::Files
                    | RightPanelSurface::File(_)
                    | RightPanelSurface::FileAtRef { .. }
            ),
            RightPanelOwner::Inbox => matches!(surface, RightPanelSurface::GitHub(_)),
            RightPanelOwner::Drafts | RightPanelOwner::Automations => false,
        }
    }

    pub(super) fn restore_right_panel_state(
        &mut self,
        state: RightPanelSessionState,
        cx: &mut Context<Self>,
    ) {
        self.replace_active_right_panel_state(state);
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
            RightPanelOwner::Drafts | RightPanelOwner::Automations | RightPanelOwner::Inbox
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
            .filter(|(_, editor)| {
                editor.dirty || !editor.annotations.borrow().items.is_empty()
            })
            .collect();
        self.right_panel_ref_editors.clear();
        self.right_panel_diff_snapshot = None;
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
            file_editors,
            ref_editors: HashMap::new(),
            files_root: self.right_panel_files_root.take(),
            diff_source: self.right_panel_diff_source,
            diff_snapshot: None,
            diff_selected_file: self.right_panel_diff_selected_file.take(),
            diff_expanded_paths: std::mem::take(&mut self.right_panel_diff_expanded_paths),
        }
    }

    fn replace_active_right_panel_state(&mut self, state: RightPanelSessionState) {
        // Fullscreen belonged to the session's surfaces being swapped out;
        // even a restored session showing the same path starts docked.
        self.fullscreen_surface = None;
        self.panel_fullscreen_slide = None;
        self.right_panel_visible = state.visible;
        if state.visible {
            // A restored-visible panel wins the slot back from the Git panel.
            self.close_git_panel_state();
        }
        self.right_panel_surfaces = state.surfaces;
        self.right_panel_active_surface = state.active_surface;
        self.right_panel_last_focused_terminal = state.last_focused_terminal;
        self.right_panel_tabs_scroll_handle = state.tabs_scroll_handle;
        self.right_panel_pending_tab_reveal = state.pending_tab_reveal;
        self.right_panel_expanded_paths = state.expanded_paths;
        self.right_panel_files_selected_path = state.files_selected_path;
        self.right_panel_file_tree_width = state.file_tree_width;
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
    /// rather than its source editor — the toggle is global, so the preview
    /// is active for every markdown file while it is on. Callers use this to
    /// pick which selection surface (and which annotation anchors) a file
    /// action should read.
    pub(super) fn file_markdown_preview_active(&self, relative_path: &str) -> bool {
        file_highlighter_language(relative_path) == "markdown" && self.state.markdown_preview
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
            .when(self.state.git_panel_enabled, |element| {
                element.child(self.render_git_panel_toggle(cx))
            })
            .child(self.render_right_panel_toggle(cx))
    }

    pub(super) fn render_right_panel(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
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
                .render_right_panel_file(path, width, window, cx)
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
        if let Some(composer) = self.side_chat_composers.get(&session_id) {
            return composer.clone();
        }
        let composer = cx.new(|cx| {
            ComposerInput::new(window, cx)
                .padding_x(px(10.0), cx)
                .collapsed_paste(cx)
        });
        composer.update(cx, |composer, cx| {
            composer.set_placeholder(tr!("side_chat.placeholder"), cx);
        });
        let side_composer = composer.clone();
        cx.subscribe(
            &composer,
            move |this: &mut Self, _, event: &ComposerEvent, cx| match event {
                ComposerEvent::Submit(prompt) => {
                    // `/side` is reserved even here: a side chat cannot nest
                    // one, and the text must not reach the provider as a
                    // literal prompt.
                    if crate::composer_complete::parse_side_submission(prompt).is_some() {
                        side_composer.update(cx, |composer, cx| composer.clear(cx));
                        this.show_toast(tr!("side_chat.no_nesting"));
                        return;
                    }
                    this.submit_composer_submission_to(
                        session_id,
                        ComposerSubmission::plain(prompt.clone()),
                        cx,
                    );
                }
                ComposerEvent::SubmitSteer(prompt) => {
                    this.steer_session_submission(
                        session_id,
                        ComposerSubmission::plain(prompt.clone()),
                        cx,
                    );
                }
                _ => {}
            },
        )
        .detach();
        self.side_chat_composers
            .insert(session_id, composer.clone());
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
        if self
            .right_panel_pending_side_chat_focus
            .take_if(|pending| *pending == session_id)
            .is_some()
        {
            let focus = composer.read(cx).focus();
            window.focus(&focus, cx);
        }

        // The row kinds are fingerprinted and spliced exactly like a card's:
        // appends keep position, a refold re-measures, and the tail re-measures
        // while the session works so fresh text is never clipped.
        let pending_turn = self.blocked_checkpoint_turn(session.id);
        let fingerprint = transcript_rows_fingerprint(&session, &self.expanded_turns, pending_turn);
        let view = self
            .side_chat_views
            .entry(session_id)
            .or_insert_with(|| SideChatView {
                rows: {
                    let rows = ListState::new(0, ListAlignment::Bottom, px(2048.0));
                    rows.set_scroll_handler(|_, window, _| window.refresh());
                    rows
                },
                scrollbar: ScrollbarState::new(),
                kinds: (0, Rc::new(Vec::new())),
            });
        let (kinds, refolded) = if view.kinds.0 != fingerprint {
            let mut folded =
                folded_transcript_row_kinds(&session, &self.expanded_turns, pending_turn);
            // Panels have no footer or file summary, but a capture holding
            // a queued prompt still marks the settled turn.
            folded.retain(|kind| match kind {
                TranscriptRowKind::ResponseFooter(..) => false,
                TranscriptRowKind::ChangedFiles(turn_id) => pending_turn == Some(*turn_id),
                _ => true,
            });
            // A footer turn hosts its pending card inside the footer row,
            // which this surface filters out — append one at the tail.
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
        if count > current {
            view.rows.splice(current..current, count - current);
            if refolded {
                view.rows.remeasure_items(0..current);
            }
        } else if count < current {
            view.rows.reset(count);
        } else if refolded {
            view.rows.remeasure_items(0..count);
        }
        if session.status.is_busy() {
            view.rows
                .remeasure_items(count.saturating_sub(STREAM_REMEASURE_TAIL_ROWS)..count);
        }
        let rows_state = view.rows.clone();
        let scrollbar = view.scrollbar.clone();
        let entity = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .px(px(4.0))
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
                    .child(scrollbar::vertical(&rows_state, &scrollbar)),
            )
            .child(
                div()
                    .flex_none()
                    .border_t(hairline())
                    .border_color(theme.separator)
                    .px(px(10.0))
                    .py(px(6.0))
                    .child(composer),
            )
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
                    let ctx = self.markdown_ctx(
                        format!("side-chat-message-{}", message.id),
                        &palette,
                        metrics,
                        animate_streaming,
                        cx,
                    );
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
                            view.set_text(message.visible_content(), message.streaming);
                            &*view
                        });
                    let rendered = render_message(
                        MessageRender {
                            theme: &theme,
                            message: &message,
                            assistant_footer_copy_content: None,
                            assistant_footer_time: None,
                            copied,
                            show_response_token_speed: false,
                            assistant_message_action: None,
                            user_message_action: None,
                            user_message_viewport: None,
                            user_message_expanded: false,
                            user_message_expand_focus: None,
                            message_edit_input: None,
                            attachment_menus,
                            attachment_images,
                            attachments_can_reveal: !self.is_remote_session(session_id),
                            markdown: view,
                            work_item_refs,
                            ctx: &ctx,
                            menu,
                            sent_by_task_link: message
                                .sent_by_task
                                .filter(|id| self.sent_by_task_openable(*id)),
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
            TranscriptRowKind::TurnBlock(block_index) => {
                self.render_card_activities_row(session, block_index, &theme)
            }
            TranscriptRowKind::TurnFold(turn_id) => {
                self.render_card_turn_fold_row(session, turn_id, &theme)
            }
            TranscriptRowKind::WorkingIndicator => {
                self.render_card_working_indicator_row(session, &theme)
            }
            // Only a pending capture's ChangedFiles survives the fold retain;
            // it renders the compact pending row rather than the full card.
            TranscriptRowKind::ChangedFiles(turn_id)
                if self.blocked_checkpoint_turn(session.id) == Some(turn_id) =>
            {
                self.render_card_checkpoint_pending_row(&theme)
            }
            // Folded out of the kinds list entirely; the fallback renders
            // nothing.
            TranscriptRowKind::ResponseFooter(..) | TranscriptRowKind::ChangedFiles(_) => {
                div().into_any_element()
            }
        };
        div()
            .id(SharedString::from(format!(
                "side-chat-row-{session_id}-{index}"
            )))
            .w_full()
            .py(px(4.0))
            .when(index == 0, |element| element.pt(px(6.0)))
            .when(starts_followup_turn, |element| {
                element.pt(px(FOLLOWUP_TURN_TOP_GAP))
            })
            .when(index + 1 == row_count, |element| element.pb(px(6.0)))
            .child(inner)
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
            && self.right_panel_visible
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
        if self.is_remote_path(&working_directory) {
            // A desktop PTY would interpret the remote cwd on the wrong
            // machine. Keep the surface unavailable until the protocol grows
            // a daemon-owned streaming terminal.
            self.right_panel_terminals.remove(&terminal_id);
            return;
        }
        if !working_directory.is_dir() {
            // The directory is gone — typically an archived session's
            // worktree awaiting restore. A PTY launched now would fall back
            // to the filesystem root and, once the directory returns, look
            // current to the spawn check while its shell sits in the
            // wrong place. The restore's completion re-runs this ensure.
            self.right_panel_terminals.remove(&terminal_id);
            return;
        }
        let spawned_at = self
            .right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).spawn_directory().to_path_buf());
        match spawned_at {
            None => self.spawn_terminal_entity(terminal_id, working_directory, cx),
            // A workspace-tracking terminal follows the workspace when it
            // moves — the spawn directory is the test, never the live cwd,
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
                    .is_some_and(|terminal| {
                        terminal.read(cx).last_command_started_at().is_some()
                    })
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
                    .right_panel_browsers
                    .get(browser_id)
                    .and_then(|browser| browser.read(cx).tab_label())
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
                RightPanelSurface::File(_) | RightPanelSurface::FileAtRef { .. }
            ) || matches!(&surface, RightPanelSurface::Files)
                && self.right_panel_files_selected_path.is_some();
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
                    .child(
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
            let options = [
                RightPanelSurface::new_browser(),
                RightPanelSurface::new_terminal(),
                RightPanelSurface::Files,
                RightPanelSurface::Diff,
            ];
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
            header
                .child(self.render_panel_toggles(cx))
                .children(self.render_client_window_controls(
                    super::window_chrome::WindowControlSide::Right,
                    window,
                    cx,
                )),
            cx,
        )
    }

    fn render_right_panel_chooser(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
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
                    .child(
                        div()
                            .mt(px(18.0))
                            .w_full()
                            .flex()
                            .gap(px(8.0))
                            .child(self.render_right_panel_card(
                                RightPanelSurface::new_browser(),
                                tr!("right_panel.browser_description"),
                                cx,
                            ))
                            .child(self.render_right_panel_card(
                                RightPanelSurface::new_terminal(),
                                tr!("right_panel.terminal_description"),
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .w_full()
                            .flex()
                            .gap(px(8.0))
                            .child(self.render_right_panel_card(
                                RightPanelSurface::Files,
                                tr!("right_panel.files_description"),
                                cx,
                            ))
                            .child(self.render_right_panel_card(
                                RightPanelSurface::Diff,
                                tr!("right_panel.diff_description"),
                                cx,
                            )),
                    ),
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
            self.render_right_panel_file(relative_path, panel_width, window, cx)
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
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .when(selected, |element| element.bg(theme.overlay_strong))
                .hover(|element| element.bg(theme.overlay))
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
            let row = if is_dir {
                row.on_click(cx.listener(move |this, _, _, cx| {
                    if !this.right_panel_expanded_paths.remove(&absolute_path) {
                        this.right_panel_expanded_paths
                            .insert(absolute_path.clone());
                    }
                    this.refresh_right_panel_working_tree(cx);
                    cx.notify();
                }))
            } else {
                row.on_click(cx.listener(move |this, _, _, cx| {
                    this.open_right_panel_file(relative_path.clone(), cx);
                }))
            };
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

    fn render_right_panel_file(
        &mut self,
        relative_path: String,
        panel_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let fullscreen = self.panel_fullscreen_active();
        let file_tree_width = if fullscreen {
            0.0
        } else {
            fitted_file_tree_width(panel_width, self.right_panel_file_tree_width)
        };
        let (editor_state, writable, _) =
            self.ensure_right_panel_file_editor(&relative_path, window, cx);

        // Markdown files carry the global source/preview toggle; every other
        // language always shows source. Image files render pixels instead of
        // text — SVGs alone keep a source view behind the toggle, since
        // their text stays editable.
        let is_markdown = file_highlighter_language(&relative_path) == "markdown";
        let is_svg =
            image_preview::image_format_for_name(&relative_path) == Some(gpui::ImageFormat::Svg);
        let image_mode = self
            .right_panel_file_editors
            .get(&relative_path)
            .is_some_and(|editor| file_shows_image(editor, &relative_path));
        let preview = !image_mode && is_markdown && self.state.markdown_preview;
        let body = if image_mode {
            self.render_file_image_preview(&relative_path, cx)
        } else if preview {
            self.render_file_markdown_preview(&relative_path, &editor_state, window, cx)
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
        let preview_toggle = (is_markdown || is_svg).then(|| {
            let focus = self.transcript_control_focus("file-preview-toggle", cx);
            let (icon_path, label) = if preview || image_mode {
                ("icons/pencil.svg", tr!("files.edit_markdown_source"))
            } else if is_svg {
                ("icons/eye.svg", tr!("files.preview_image"))
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
                        if is_svg {
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
                            if is_svg {
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
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b(hairline())
                    .border_color(theme.separator)
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
                    .children(preview_toggle),
            )
            .child(body);

        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .child(editor)
            // Fullscreen drops the file tree entirely; its resize handle
            // would fight a surface that owns the window's width.
            .when(!fullscreen, |element| {
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
            })
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
        let position_pending = pending_open.and_then(|pending| pending.position);
        if let Some(editor) = self.right_panel_file_editors.get_mut(relative_path) {
            if let Some(position) = position_pending {
                editor.pending_position = Some(position);
            }
            // An image preview has no editor to focus — the TextInput is not
            // rendered while the pane shows pixels.
            if focus_pending && !file_shows_image(editor, relative_path) {
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
                reading: false,
                read_epoch: 0,
                pending_position: position_pending,
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
        if focus_pending && image_preview::image_format_for_name(relative_path).is_none() {
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
        if !editor.reading
            && let Some((line, column)) = editor.pending_position.take()
        {
            let state = editor.state.clone();
            let offset = cursor_offset_for_line_column(state.read(cx).content(), line, column);
            state.update(cx, |state, cx| state.select_range(offset..offset, cx));
            let weak = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.reveal_editor_offset(&state, offset, cx);
                });
            });
        }
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
                                    .pb(px(
                                        CONTENT_PAD_TOP
                                            + line_height * FILE_SCROLL_PAD_LINES,
                                    ))
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

    /// Flips one SVG between rendered preview and editable source — per file,
    /// unlike markdown's global toggle, because an image's bytes are only
    /// loaded as text once the source view asks for them.
    fn toggle_file_source_view(&mut self, relative_path: &str, cx: &mut Context<Self>) {
        let Some(editor) = self.right_panel_file_editors.get_mut(relative_path) else {
            return;
        };
        editor.show_source = !editor.show_source;
        cx.notify();
        self.read_right_panel_file_into_editor(relative_path.to_owned(), cx);
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
                    .child(canvas(
                        move |bounds, window, cx| {
                            let natural =
                                image
                                    .clone()
                                    .use_render_image(window, cx)
                                    .and_then(|render| {
                                        let size = render.size(0);
                                        (size.width.0 > 0 && size.height.0 > 0)
                                            .then(|| (size.width.0 as f32, size.height.0 as f32))
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
                            .map(|(left, top, width, height)| {
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
                            })
                        },
                        |_, element, window, cx| {
                            if let Some(mut element) = element {
                                element.paint(window, cx);
                            }
                        },
                    ))
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
        self.fullscreen_surface.is_some() || self.panel_fullscreen_slide.is_some()
    }

    /// Cover the window with the active right-panel surface, or dock it back.
    /// Runtime-only: nothing persists, and every other way the surface goes
    /// away (tab close, surface switch, panel hide, session swap) is
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);
        let fullscreen = self.panel_fullscreen_active();
        let mut cache = self.file_preview_markdown.borrow_mut();
        if !matches!(cache.as_ref(), Some((cached, _)) if cached == relative_path) {
            *cache = Some((relative_path.to_owned(), MarkdownView::new()));
        }
        let (_, view) = cache.as_mut().expect("entry ensured above");
        view.set_text(editor_state.read(cx).content(), false);
        let mut preview_selection = self.file_preview_selection.clone();
        if let Some(editor) = self.right_panel_file_editors.get(relative_path) {
            preview_selection.annotations = editor.annotations.clone();
        }
        let metrics = MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size);
        let ctx = MarkdownCtx::new(
            format!("file-preview-{relative_path}"),
            &palette,
            metrics,
            preview_selection.clone(),
        )
        .with_families(crate::fonts::current(cx))
        .with_math_enabled(self.state.render_math)
        .with_guided_reading(self.guided_reading())
        .with_standalone_context_menu(self.menu_handle("file-preview-math", cx))
        .with_link_handler(self.markdown_link_handler.clone());
        let document = md::render::markdown(view, &ctx);

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
            .bg(theme.surface)
            .child(
                div()
                    .id(SharedString::from(format!("file-preview-{relative_path}")))
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.file_preview_scroll_handle)
                    // Painted before the document, so the frame's selection
                    // registry holds exactly this frame's text elements.
                    .child(md::render::frame_reset(preview_selection.clone()))
                    .child(
                        div()
                            .when(fullscreen, |element| {
                                element.w_full().flex().justify_center()
                            })
                            .child(
                                div()
                                    .when(fullscreen, |element| {
                                        element.w_full().max_w(px(CONTENT_MAX_WIDTH)).min_w_0()
                                    })
                                    .px(px(16.0))
                                    .pt(px(14.0))
                                    .pb(px(
                                        24.0 + metrics.line_height * FILE_SCROLL_PAD_LINES,
                                    ))
                                    .text_color(theme.text)
                                    .children(document),
                            ),
                    ),
            )
            .child(selection_input)
            // After the selection canvas so its hit-tests see this frame's
            // registry; bubble dispatch runs listeners in reverse paint
            // order, so a press on a highlight reaches the annotation
            // handlers before the selection's drag begins.
            .child(self.preview_annotation_input(&preview_selection, relative_path, cx))
            .child(scrollbar::vertical(
                &self.file_preview_scroll_handle,
                &self.file_preview_scrollbar,
            ))
            .children(annotation_offer)
            .children(annotation_editor)
            .children(annotation_tooltip)
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
        // surface active the chord opens the branch picker instead.
        let Some(relative_path) = self.visible_right_panel_file_path() else {
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
    /// now, from the owner on screen: the selected session's workspace, the
    /// selected terminal's live cwd, or the project a Projects page is
    /// scoped to. Pages that admit no files resolve to `None`.
    pub(super) fn resolve_right_panel_files_root(&self, cx: &App) -> Option<PathBuf> {
        match self.active_right_panel_owner() {
            RightPanelOwner::Session(_) => self
                .selected_workspace_path()
                .map(std::path::Path::to_path_buf),
            RightPanelOwner::Terminal(terminal_id) => self.terminal_cwd(terminal_id, cx),
            RightPanelOwner::Projects(project_id) => self
                .state
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone()),
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
                .filter(|(_, editor)| {
                    editor.dirty || !editor.annotations.borrow().items.is_empty()
                })
                .collect(),
            file_tree_width: self.right_panel_file_tree_width,
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
