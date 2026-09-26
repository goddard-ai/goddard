//! The `Cmd+P` file finder: jump to a file in the selected task's workspace.
//!
//! The modal is a view over the composer's shared workspace file index —
//! `mention_files` mirrored into `mention_file_index` by
//! `refresh_composer_sources`, discovered on the background executor —
//! filtered into `results` per keystroke and per index arrival, so a frame
//! never walks the filesystem or re-runs the fuzzy match. The search field
//! keeps real focus while the row highlight is drawn, matching the command
//! palette. A `path:line[:column]` suffix jumps straight to that position.
//! Confirming opens the file in the right panel through the same
//! path a transcript file link takes; keyboard focus then lands on the file's
//! editor via `right_panel_pending_file_focus`, which the editor's ensure
//! path consumes on the first frame the entity exists.

use gpui::{Font, KeyBinding, deferred};
use nucleo_matcher::Matcher;

use crate::composer_complete::{self, Scored, highlight_byte_ranges};

use super::autocomplete::matched_text;
use super::command_palette::{
    Confirm, Dismiss, SelectFirst, SelectLast, SelectNext, SelectPageDown, SelectPageUp,
    SelectPrevious, next_selection_index,
};
use super::*;

/// List navigation lives beneath the focused one-line input; escape lives on
/// the card so the field's own clear-on-escape outranks it while it has text.
const SEARCH_CONTEXT: &str = "FileFinder > TextInput";
const CARD_CONTEXT: &str = "FileFinder";

const SEARCH_ROW_HEIGHT: f32 = 60.0;
const RESULT_ROW_HEIGHT: f32 = 34.0;
const EMPTY_RESULTS_HEIGHT: f32 = 180.0;
const RESULTS_BOTTOM_PADDING: f32 = 8.0;
const MAX_CARD_HEIGHT: f32 = 440.0;
const PAGE_STEP: isize = 7;

/// Bind the finder's keys. Registered with the other picker `init`s, after
/// the input's own bindings.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(SEARCH_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(SEARCH_CONTEXT)),
        KeyBinding::new("ctrl-n", SelectNext, Some(SEARCH_CONTEXT)),
        KeyBinding::new("ctrl-p", SelectPrevious, Some(SEARCH_CONTEXT)),
        KeyBinding::new("tab", SelectNext, Some(SEARCH_CONTEXT)),
        KeyBinding::new("shift-tab", SelectPrevious, Some(SEARCH_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(SEARCH_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(SEARCH_CONTEXT)),
        KeyBinding::new("pagedown", SelectPageDown, Some(SEARCH_CONTEXT)),
        KeyBinding::new("pageup", SelectPageUp, Some(SEARCH_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(SEARCH_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CARD_CONTEXT)),
    ]);
}

/// Cross-frame state for the finder. `results` is the drawn snapshot,
/// recomputed on a keystroke or when the shared index lands.
pub(super) struct FileFinderUi {
    search: Entity<TextInput>,
    open: bool,
    focus_generation: u64,
    previous_focus: Option<FocusHandle>,
    /// The root `files` was listed from — the selected session's workspace,
    /// or the selected terminal's cwd. A drift clears the projection; the
    /// next refresh relists under the new root.
    root: Option<PathBuf>,
    /// `Arc::as_ptr` identity of the index `files` was filtered out of, so the
    /// files-only projection is rebuilt once per index swap rather than per
    /// keystroke.
    index: usize,
    /// A listing for `root` is in flight — the placeholder draws its spinner.
    loading: bool,
    files: Rc<Vec<FileEntry>>,
    results: Vec<Scored<FileEntry>>,
    selected: usize,
    scroll: ScrollHandle,
    matcher: Matcher,
}

impl FileFinderUi {
    pub(super) fn new(search: Entity<TextInput>) -> Self {
        Self {
            search,
            open: false,
            focus_generation: 0,
            previous_focus: None,
            root: None,
            index: 0,
            loading: false,
            files: Rc::new(Vec::new()),
            results: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            matcher: composer_complete::matcher(),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }
}

/// Split a finder query into its path filter and an optional `line[:column]`
/// jump target — `main.rs:12`, `main.rs:12:4`. A trailing colon is a
/// separator still being typed, so it drops off too; a segment that is not
/// all digits stays part of the path filter.
fn split_position_suffix(query: &str) -> (&str, Option<usize>, Option<usize>) {
    let query = query.trim_end_matches(':');
    let Some((head, last)) = query.rsplit_once(':') else {
        return (query, None, None);
    };
    if last.is_empty() || !last.bytes().all(|byte| byte.is_ascii_digit()) {
        return (query, None, None);
    }
    if let Some((path, line)) = head.rsplit_once(':')
        && !line.is_empty()
        && line.bytes().all(|byte| byte.is_ascii_digit())
    {
        return (path, line.parse().ok(), last.parse().ok());
    }
    (head, last.parse().ok(), None)
}

fn file_finder_results_height(result_count: usize, show_placeholder: bool) -> f32 {
    let content_height = if show_placeholder {
        EMPTY_RESULTS_HEIGHT
    } else {
        result_count as f32 * RESULT_ROW_HEIGHT
    };
    content_height + RESULTS_BOTTOM_PADDING
}

impl Waku {
    pub(super) fn toggle_file_finder_action(
        &mut self,
        _: &ToggleFileFinder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_finder.open {
            self.close_file_finder(window, cx);
        } else {
            self.open_file_finder(window, cx);
        }
    }

    fn open_file_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The finder's scope is the panel's files root — a session workspace
        // or the terminal's cwd. With neither, there is nothing to search.
        if self.resolve_right_panel_files_root(cx).is_none() {
            return;
        }
        // Land the panel's files slice on that same root first, so a confirm
        // opens the file under the root its listing came from.
        self.sync_right_panel_files_root(cx);
        // One picker at a time; closing first restores the palette's recorded
        // focus so the finder captures the element that was really focused.
        if self.command_palette.is_open() {
            self.close_command_palette(window, cx);
        }

        let open_menus = self
            .menus
            .borrow()
            .values()
            .filter(|menu| menu.is_open())
            .cloned()
            .collect::<Vec<_>>();
        // A focused picker input disappears when its menu closes. Restore to a
        // durable surface instead of remembering that soon-detached handle.
        self.file_finder.previous_focus = if open_menus.is_empty() {
            window.focused(cx)
        } else if self.settings_page.is_some() {
            Some(self.settings_focus.clone())
        } else {
            Some(self.composer_focus(cx))
        };

        self.file_finder.open = true;
        self.file_finder.focus_generation = self.file_finder.focus_generation.wrapping_add(1);
        self.file_finder.selected = 0;
        self.file_finder.scroll.scroll_to_item(0);
        self.file_finder
            .search
            .update(cx, |input, cx| input.clear(cx));
        self.refresh_file_finder_results(cx);
        // Kick the shared index so a cold workspace starts loading; the
        // results refresh again when it lands.
        self.refresh_composer_sources(cx);

        // Closing an open GPUI menu can call its toggle observers back into
        // this entity, so release this action listener's mutable borrow first.
        if !open_menus.is_empty() {
            window.defer(cx, move |window, cx| {
                for menu in open_menus {
                    menu.close(window, cx);
                }
            });
        }

        // The finder is deferred onto GPUI's overlay plane. Wait for that
        // subtree to join the dispatch tree before handing focus to its input.
        let focus = self.file_finder.search.read(cx).focus();
        let weak = cx.entity().downgrade();
        let focus_generation = self.file_finder.focus_generation;
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let mut should_focus = false;
                let _ = weak.update(cx, |this, _| {
                    should_focus = this.file_finder.open
                        && this.file_finder.focus_generation == focus_generation;
                });
                if should_focus {
                    window.focus(&focus, cx);
                }
            });
        });
        cx.notify();
    }

    pub(super) fn close_file_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.file_finder.open {
            return;
        }
        self.file_finder.open = false;
        self.file_finder.focus_generation = self.file_finder.focus_generation.wrapping_add(1);
        if let Some(previous_focus) = self.file_finder.previous_focus.take() {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    pub(super) fn refresh_file_finder_localized_text(&mut self, cx: &mut Context<Self>) {
        self.file_finder.search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.file_finder"), cx);
            input.set_placeholder(tr!("file_finder.placeholder"), cx)
        });
    }

    pub(super) fn file_finder_query_edited(&mut self, cx: &mut Context<Self>) {
        if !self.file_finder.open {
            return;
        }
        self.file_finder.selected = 0;
        self.file_finder.scroll.scroll_to_item(0);
        self.refresh_file_finder_results(cx);
        cx.notify();
    }

    /// Recompute the drawn rows from the files root's listing. Runs on a
    /// keystroke and when the index lands or swaps underneath an open finder —
    /// never from `render`. A root that drifted under an open finder clears
    /// the projection and relists, so results never name another directory's
    /// files.
    pub(super) fn refresh_file_finder_results(&mut self, cx: &mut Context<Self>) {
        if !self.file_finder.open {
            return;
        }
        let root = self.resolve_right_panel_files_root(cx);
        if root != self.file_finder.root {
            self.file_finder.root = root;
            self.file_finder.index = 0;
            self.file_finder.files = Rc::new(Vec::new());
            self.file_finder.results.clear();
            self.file_finder.selected = 0;
            self.file_finder.scroll.scroll_to_item(0);
        }
        if let Some(root) = self.file_finder.root.clone() {
            match self.mention_files.read(&root) {
                Query::Ready(entries) => {
                    self.file_finder.loading = false;
                    let index_ptr = Arc::as_ptr(&entries) as usize;
                    if self.file_finder.index != index_ptr {
                        self.file_finder.index = index_ptr;
                        self.file_finder.files = Rc::new(
                            entries
                                .iter()
                                .filter(|entry| !entry.is_dir)
                                .cloned()
                                .collect(),
                        );
                    }
                }
                Query::Pending => self.file_finder.loading = true,
                Query::Missing(token) => {
                    self.file_finder.loading = true;
                    self.fetch_mention_files(token, root, cx);
                }
            }
        } else {
            self.file_finder.loading = false;
        }
        let query = self.file_finder.search.read(cx).content().to_owned();
        let (path_query, _, _) = split_position_suffix(&query);
        self.file_finder.results = composer_complete::filter_files(
            &self.file_finder.files,
            path_query,
            &mut self.file_finder.matcher,
        );
        self.file_finder.selected = self
            .file_finder
            .selected
            .min(self.file_finder.results.len().saturating_sub(1));
        cx.notify();
    }

    fn move_file_finder_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.file_finder.results.len();
        let Some(next) = next_selection_index(self.file_finder.selected, len, delta) else {
            return;
        };
        self.file_finder.selected = next;
        self.file_finder.scroll.scroll_to_item(next);
        cx.notify();
    }

    fn set_file_finder_selection(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.file_finder.results.len() && self.file_finder.selected != index {
            self.file_finder.selected = index;
            cx.notify();
        }
    }

    /// Open the highlighted row (`None`, the keyboard path) or a clicked one.
    /// The open goes through the transcript-link path — the Files surface
    /// selects the file, or a standalone tab takes over when a dirty editor
    /// would be displaced — and the editor takes focus once it exists.
    fn execute_file_finder_selection(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = index.unwrap_or_else(|| {
            self.file_finder
                .selected
                .min(self.file_finder.results.len().saturating_sub(1))
        });
        let Some(row) = self.file_finder.results.get(index) else {
            return;
        };
        let relative_path = row.item.path.clone();
        let query = self.file_finder.search.read(cx).content().to_owned();
        let (_, line, column) = split_position_suffix(&query);
        self.right_panel_pending_file_focus = Some(PendingFileFocus {
            path: relative_path.clone(),
            position: line.map(|line| (line, column.unwrap_or(1))),
        });
        if let Some(workspace) = self.right_panel_files_root.clone() {
            self.reveal_right_panel_file_in_tree_at_root(
                relative_path.clone(),
                workspace,
                cx,
            );
        } else {
            self.open_right_panel_surface(RightPanelSurface::Files, cx);
        }
        self.open_right_panel_file(relative_path, cx);
        // Restoring the previous focus is harmless: the pending handoff above
        // moves it to the editor a frame later, and stays as the fallback if
        // the editor never materializes.
        self.close_file_finder(window, cx);
    }

    pub(super) fn render_file_finder(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.file_finder.open {
            return None;
        }
        let theme = Theme::current(cx);
        let font = window.text_style().font();
        let viewport_height = f32::from(window.viewport_size().height);
        let top = (viewport_height * 0.09).clamp(48.0, 72.0);
        let card_max_height = (viewport_height - top - 36.0)
            .max(SEARCH_ROW_HEIGHT)
            .min(MAX_CARD_HEIGHT);
        let selected = self
            .file_finder
            .selected
            .min(self.file_finder.results.len().saturating_sub(1));
        let loading = self.file_finder.loading;
        let show_placeholder = self.file_finder.results.is_empty();
        let results_height =
            file_finder_results_height(self.file_finder.results.len(), show_placeholder)
                .min((card_max_height - SEARCH_ROW_HEIGHT).max(0.0));
        let card_height = SEARCH_ROW_HEIGHT + results_height;

        let mut results = div()
            .id("file-finder-results")
            .h(px(results_height))
            .flex_none()
            .overflow_y_scroll()
            .track_scroll(&self.file_finder.scroll)
            .px(px(8.0))
            .pb(px(8.0));

        if show_placeholder {
            let (icon_path, title, hint, spinning) = if loading {
                (
                    "icons/loader-circle.svg",
                    tr!("file_finder.loading"),
                    None,
                    true,
                )
            } else {
                (
                    "icons/search.svg",
                    tr!("file_finder.no_results"),
                    Some(tr!("file_finder.no_results_hint")),
                    false,
                )
            };
            let empty_icon = icon(icon_path, 18.0, theme.text_ghost);
            results = results.child(
                div()
                    .h(px(EMPTY_RESULTS_HEIGHT))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(if spinning {
                        motion::spin(empty_icon)
                    } else {
                        empty_icon.into_any_element()
                    })
                    .child(
                        div()
                            .mt(px(12.0))
                            .text_size(sp(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child(title),
                    )
                    .when_some(hint, |empty, hint| {
                        empty.child(
                            div()
                                .max_w(px(540.0))
                                .mt(px(5.0))
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(hint),
                        )
                    }),
            );
        } else {
            for (index, row) in self.file_finder.results.iter().enumerate() {
                let highlighted = index == selected;
                results = results.child(self.render_file_finder_row(
                    index,
                    row,
                    highlighted,
                    &theme,
                    &font,
                    cx,
                ));
            }
        }

        let card = div()
            .id("file-finder-card")
            .key_context(CARD_CONTEXT)
            .on_action(cx.listener(Self::toggle_file_finder_action))
            .on_action(
                cx.listener(|this, _: &SelectNext, _, cx| this.move_file_finder_selection(1, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                this.move_file_finder_selection(-1, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                this.move_file_finder_selection(isize::MIN, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                this.move_file_finder_selection(isize::MAX, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                this.move_file_finder_selection(PAGE_STEP, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                this.move_file_finder_selection(-PAGE_STEP, cx)
            }))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                this.execute_file_finder_selection(None, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &Dismiss, window, cx| this.close_file_finder(window, cx)),
            )
            .w_full()
            .max_w(px(640.0))
            .h(px(card_height))
            .overflow_hidden()
            .rounded(px(18.0))
            .bg(theme.raised)
            .shadow_xl()
            .relative()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(SEARCH_ROW_HEIGHT))
                    .px(px(19.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .border_b(hairline())
                    .border_color(theme.separator)
                    .text_size(sp(15.5))
                    .text_color(theme.text)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(self.file_finder.search.clone()),
                    ),
            )
            .child(results);

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.26)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.14)
        };
        let layer = div()
            .id("file-finder-layer")
            .absolute()
            .inset_0()
            .occlude()
            .bg(scrim)
            .px(px(24.0))
            .pt(px(top))
            .flex()
            .items_start()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_file_finder(window, cx)),
            )
            .child(motion::modal_enter("file-finder-card-enter", card));
        Some(
            deferred(motion::fade_in("file-finder-layer-enter", layer))
                .with_priority(3)
                .into_any_element(),
        )
    }

    /// A file row: icon, basename, then the dimmed parent directory — the
    /// same split the `@` mention popup draws, with matched characters in
    /// accent. Positions index the full path, so each segment recovers its
    /// own byte ranges.
    fn render_file_finder_row(
        &self,
        index: usize,
        row: &Scored<FileEntry>,
        highlighted: bool,
        theme: &Theme,
        font: &Font,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = &row.item.path;
        let name_start = path.rfind('/').map_or(0, |index| index + 1);
        let name = &path[name_start..];
        let parent = &path[..name_start.saturating_sub(1)];
        let name_char_offset = path[..name_start].chars().count();
        div()
            .id(SharedString::from(format!("file-finder-row-{index}")))
            .h(px(RESULT_ROW_HEIGHT))
            .px(px(11.0))
            .rounded(px(11.0))
            .border(hairline())
            .border_color(if highlighted {
                theme.border_strong
            } else {
                gpui::transparent_black()
            })
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_default()
            .when(highlighted, |row| row.bg(theme.overlay_strong))
            .hover(|row| row.bg(theme.overlay))
            .active(|row| row.opacity(0.82))
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                if *hovering {
                    this.set_file_finder_selection(index, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.execute_file_finder_selection(Some(index), window, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(
                        super::right_panel::file_icon_for_path(path),
                        14.0,
                        theme.text_secondary,
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(px(300.0))
                    .truncate()
                    .text_size(sp(13.0))
                    .child(matched_text(
                        name.to_owned(),
                        highlight_byte_ranges(name, &row.positions, name_char_offset),
                        theme.text,
                        theme.accent,
                        font.clone(),
                    )),
            )
            .when(!parent.is_empty(), |element| {
                element.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(12.5))
                        .child(matched_text(
                            parent.to_owned(),
                            highlight_byte_ranges(parent, &row.positions, 0),
                            theme.text_ghost,
                            theme.accent,
                            font.clone(),
                        )),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::split_position_suffix;

    #[test]
    fn position_suffix_splits_line_and_column_off_the_path() {
        assert_eq!(
            split_position_suffix("src/main.rs"),
            ("src/main.rs", None, None)
        );
        assert_eq!(
            split_position_suffix("src/main.rs:12"),
            ("src/main.rs", Some(12), None)
        );
        assert_eq!(
            split_position_suffix("src/main.rs:12:4"),
            ("src/main.rs", Some(12), Some(4))
        );
        // A colon still being typed is a separator, not part of the filter.
        assert_eq!(
            split_position_suffix("src/main.rs:"),
            ("src/main.rs", None, None)
        );
        assert_eq!(
            split_position_suffix("src/main.rs:12:"),
            ("src/main.rs", Some(12), None)
        );
        // Non-numeric segments stay in the filter, so a path with a colon in
        // it still matches.
        assert_eq!(split_position_suffix("foo:bar"), ("foo:bar", None, None));
        assert_eq!(
            split_position_suffix("foo:bar:12"),
            ("foo:bar", Some(12), None)
        );
        assert_eq!(
            split_position_suffix("foo:12:bar"),
            ("foo:12:bar", None, None)
        );
    }

    /// The modal repaints on every keystroke frame; the index walk and the
    /// fuzzy match must stay out of it — everything the render path shows
    /// comes from the prefetched, pre-filtered snapshot.
    #[test]
    fn the_finder_render_path_does_no_io_or_filtering() {
        let source = include_str!("./file_finder.rs");
        let start = source
            .find("\n    pub(super) fn render_file_finder(")
            .expect("render function must exist");
        let end = source[start..]
            .find("\n#[cfg(test)]")
            .map(|offset| start + offset)
            .expect("test module marker must exist");
        let render = &source[start..end];
        for forbidden in [
            "refresh_file_finder_results(",
            "refresh_composer_sources(",
            "filter_files(",
            "background_executor(",
            "std::fs",
            "Command::new",
            "read_dir",
        ] {
            assert!(
                !render.contains(forbidden),
                "finder render must not call `{forbidden}`"
            );
        }
    }
}
