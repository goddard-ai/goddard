//! VS Code-style go-to-line for the right panel's file editor, on ctrl-g.
//!
//! A sibling of the find bar: one bar serves whichever file editor is on
//! screen, sitting in normal flow above the scroll region. Typing previews the
//! jump live — the editor's selection and scroll follow the input — and
//! Escape restores the position captured when the bar opened, the way VS
//! Code's go-to-line popover cancels back to the origin. Enter keeps the
//! landing and returns focus to the editor.

use super::*;

/// Bar state. Created on first use and kept for the window's lifetime so the
/// input survives closing the bar; `open` says whether it shows.
pub(super) struct GoToLine {
    pub(super) open: bool,
    /// The file the bar is bound to. A different visible file closes the bar
    /// — its restore snapshot and pending preview mean nothing there.
    pub(super) path: String,
    pub(super) input: Entity<TextInput>,
    /// Scroll offset and selection captured when the bar opened; Escape puts
    /// both back. `None` once the jump is committed.
    restore: Option<(gpui::Point<Pixels>, Range<usize>)>,
    /// The input is non-empty but not a usable `line` or `line:column`.
    invalid: bool,
}

/// Parses `line` or `line:column` into a 1-based target. Column is a
/// character column; a trailing colon alone is tolerated (`"42:"` is line 42).
fn parse_go_to_line_target(input: &str) -> Option<(usize, Option<usize>)> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let (line, column) = match input.split_once(':') {
        Some((line, column)) => (line, column),
        None => (input, ""),
    };
    let line = positive(line)?;
    let column = if column.is_empty() {
        None
    } else {
        Some(positive(column)?)
    };
    Some((line, column))
}

/// A non-empty all-digits string as a positive number — exactly what
/// `strip_file_location` accepts in a `file:line` target. Rejecting a leading
/// `+` keeps it from looking like a relative move.
fn positive(value: &str) -> Option<usize> {
    right_panel::positive_number(value)
        .then(|| value.parse::<usize>().ok())
        .flatten()
}

/// Byte range of a 1-based logical line, excluding its newline. Lines are the
/// editor's own definition — `content.split('\n')` — so the gutter's numbers
/// and the bar agree even when soft wrap turns one line into several rows.
fn line_range_for(content: &str, line: usize) -> Option<Range<usize>> {
    let mut start = 0;
    for (index, text) in content.split('\n').enumerate() {
        if index + 1 == line {
            return Some(start..start + text.len());
        }
        start += text.len() + 1;
    }
    None
}

/// Byte offset of a 1-based character column within one line; past the end
/// clamps to the line's end.
fn column_byte_offset(line_text: &str, column: usize) -> usize {
    line_text
        .char_indices()
        .nth(column - 1)
        .map(|(index, _)| index)
        .unwrap_or(line_text.len())
}

impl Waku {
    pub(super) fn go_to_line_open(&self) -> bool {
        self.go_to_line.as_ref().is_some_and(|goto| goto.open)
    }

    fn ensure_go_to_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.go_to_line.is_some() {
            return;
        }
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("input.go_to_line"))
                .placeholder(tr!("input.go_to_line"))
        });
        cx.subscribe(
            &input,
            |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::Edited => this.preview_go_to_line(cx),
                InputEvent::Submit(_) => this.commit_go_to_line(cx),
                InputEvent::Focus => {}
                InputEvent::BackspaceOnEmpty => {}
            },
        )
        .detach();
        self.go_to_line = Some(GoToLine {
            open: false,
            path: String::new(),
            input,
            restore: None,
            invalid: false,
        });
    }

    pub(super) fn open_go_to_line_action(
        &mut self,
        _: &OpenGoToLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The jump targets the file editor's source view. A markdown file in
        // preview mode renders a different scroll surface with no line
        // structure to aim at, so the key falls through there too.
        let preview_active = self.visible_right_panel_file_path().is_some_and(|path| {
            right_panel::file_highlighter_language(&path) == "markdown"
                && self.state.markdown_preview
        });
        if !self.right_panel_visible || preview_active {
            cx.propagate();
            return;
        }
        let Some(path) = self.visible_right_panel_file_path() else {
            cx.propagate();
            return;
        };
        self.open_go_to_line(path, window, cx);
    }

    fn open_go_to_line(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let already_open = self.go_to_line_open();
        self.ensure_go_to_line(window, cx);

        let editor = self
            .right_panel_file_editors
            .get(&path)
            .map(|editor| editor.state.clone());
        // The caret's line pre-fills the input so Enter alone re-lands on it;
        // an empty content (a read still in flight) starts the field blank.
        let current_line = editor.as_ref().and_then(|state| {
            let state = state.read(cx);
            state
                .content()
                .get(..state.cursor())
                .map(|before| before.matches('\n').count() + 1)
        });

        let goto = self
            .go_to_line
            .as_mut()
            .expect("ensure_go_to_line just created it");
        goto.open = true;
        goto.invalid = false;
        if !already_open {
            goto.path = path;
            // The restore point belongs to wherever the editor was before the
            // bar opened; a second ctrl-g must not re-snapshot a preview.
            goto.restore = editor.as_ref().map(|state| {
                (
                    self.right_panel_editor_scroll_handle.offset(),
                    state.read(cx).selected_range(),
                )
            });
        }
        let input = goto.input.clone();
        input.update(cx, |input, cx| {
            input.set_content(
                current_line
                    .map(|line| line.to_string())
                    .unwrap_or_default(),
                cx,
            );
            input.select_all_text(cx);
        });
        window.focus(&input.read(cx).focus(), cx);
        cx.notify();
    }

    /// Escape or the bar's close button. `restore` puts the editor back where
    /// it was when the bar opened (Escape); committing keeps the landing.
    pub(super) fn close_go_to_line(
        &mut self,
        restore: bool,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(goto) = self.go_to_line.as_mut().filter(|goto| goto.open) else {
            return;
        };
        goto.open = false;
        let path = goto.path.clone();
        if restore
            && let Some((offset, selection)) = goto.restore.take()
            && let Some(editor) = self.right_panel_file_editors.get(&path)
        {
            let state = editor.state.clone();
            state.update(cx, |state, cx| {
                // The file may have been reloaded under the open bar; a stale
                // range clamps to the new length and `select_range` rejects a
                // non-boundary rather than panicking.
                let len = state.content().len();
                state.select_range(selection.start.min(len)..selection.end.min(len), cx);
            });
            self.right_panel_editor_scroll_handle.set_offset(offset);
        } else {
            goto.restore = None;
        }
        if focus_editor && let Some(editor) = self.right_panel_file_editors.get(&path) {
            let focus = editor.state.read(cx).focus();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Session switches swap the editor set wholesale; a bar bound to an
    /// outgoing file closes, same as the find bar's reset.
    pub(super) fn reset_go_to_line_for_session(&mut self, _cx: &mut Context<Self>) {
        if let Some(goto) = self.go_to_line.as_mut() {
            goto.open = false;
            goto.restore = None;
        }
    }

    /// Called from the editor's render path each frame. A different file on
    /// screen closes the bar rather than retargeting it: its restore snapshot
    /// describes the file that left, and a live preview in the new file would
    /// be a surprise.
    pub(super) fn sync_go_to_line_target(&mut self, relative_path: &str, _cx: &mut Context<Self>) {
        if self
            .go_to_line
            .as_ref()
            .is_some_and(|goto| goto.open && goto.path != relative_path)
        {
            if let Some(goto) = self.go_to_line.as_mut() {
                goto.open = false;
                goto.restore = None;
            }
        }
    }

    /// Live preview: every edit to the input jumps the editor to the parsed
    /// target, the way VS Code's go-to-line popover scrolls while typing.
    fn preview_go_to_line(&mut self, cx: &mut Context<Self>) {
        let Some(goto) = self.go_to_line.as_mut().filter(|goto| goto.open) else {
            return;
        };
        let input = goto.input.read(cx).content().to_owned();
        let path = goto.path.clone();
        let parsed = parse_go_to_line_target(&input);
        let (line_count, reading) = self
            .right_panel_file_editors
            .get(&path)
            .map(|editor| {
                (
                    editor.state.read(cx).content().split('\n').count().max(1),
                    editor.reading,
                )
            })
            .unwrap_or((0, false));
        // Out-of-range is only knowable once the file has content; while a
        // read is in flight any parsed target previews (and queues) fine.
        let target = parsed.filter(|(line, _)| reading || *line <= line_count);
        let goto = self.go_to_line.as_mut().expect("filtered Some above");
        goto.invalid = !input.trim().is_empty() && target.is_none();
        if let Some((line, column)) = target {
            self.jump_to_line(&path, line, column, cx);
        }
        cx.notify();
    }

    /// Enter commits the previewed position — the jump already happened on the
    /// last `Edited` — so this just closes the bar and returns focus to the
    /// editor. The submit subscription has no `Window`; focus goes through the
    /// stored handle.
    fn commit_go_to_line(&mut self, cx: &mut Context<Self>) {
        let Some(goto) = self.go_to_line.as_mut().filter(|goto| goto.open) else {
            return;
        };
        goto.open = false;
        goto.restore = None;
        if let Some(editor) = self.right_panel_file_editors.get(&goto.path) {
            let focus = editor.state.read(cx).focus();
            let window_handle = self.window_handle;
            let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
        }
        cx.notify();
    }

    /// Selects `line` in `path`'s editor — or drops the caret at `column`
    /// within it — and scrolls it into view. A file still reading waits on the
    /// editor's own `pending_position`, and one whose entity does not exist
    /// yet rides the same `PendingFileFocus` handoff the finder uses.
    pub(super) fn jump_to_line(
        &mut self,
        path: &str,
        line: usize,
        column: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.right_panel_file_editors.get_mut(path) else {
            self.right_panel_pending_file_focus = Some(PendingFileFocus {
                path: path.to_owned(),
                position: Some((line, column.unwrap_or(1))),
            });
            return;
        };
        if editor.reading {
            editor.pending_position = Some((line, column.unwrap_or(1)));
            return;
        }
        let state = editor.state.clone();
        let mut jumped = None;
        state.update(cx, |state, cx| {
            let Some(range) = line_range_for(state.content(), line) else {
                return;
            };
            let range = match column {
                Some(column) => {
                    let at =
                        range.start + column_byte_offset(&state.content()[range.clone()], column);
                    at..at
                }
                None => range,
            };
            jumped = Some(range.start);
            state.select_range(range, cx);
        });
        if let Some(offset) = jumped {
            self.reveal_editor_offset(&state, offset, cx);
        }
    }

    /// The go-to-line bar, or `None` while closed. Same in-flow placement as
    /// the find bar: a row above the scroll container that pushes content down
    /// rather than covering the first lines.
    pub(super) fn render_go_to_line_bar(
        &self,
        line_count: usize,
        reading: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let goto = self.go_to_line.as_ref().filter(|goto| goto.open)?;
        let theme = Theme::current(cx);
        let label: SharedString = if reading {
            tr!("file_finder.loading").into()
        } else if goto.invalid {
            tr!("go_to_line.range", total = line_count).into()
        } else {
            tr!("go_to_line.count", total = line_count).into()
        };
        Some(
            div()
                .id("go-to-line-bar")
                .w_full()
                .flex_none()
                .border_b(hairline())
                .border_color(theme.border)
                .bg(theme.surface)
                .font_family(crate::fonts::current(cx).ui)
                .cursor_default()
                .flex()
                .items_center()
                .gap(px(6.0))
                .py(px(6.0))
                .px(px(10.0))
                .child(icon(
                    "icons/corner-down-right.svg",
                    12.0,
                    theme.text_tertiary,
                ))
                .child(self.find_input_box(
                    "go-to-line-input",
                    &goto.input,
                    160.0,
                    goto.invalid,
                    window,
                    cx,
                ))
                .child(
                    div()
                        .flex_none()
                        .text_size(sp(12.5))
                        .whitespace_nowrap()
                        .text_color(if goto.invalid {
                            theme.danger
                        } else {
                            theme.text_tertiary
                        })
                        .child(label),
                )
                .child(div().flex_1())
                .child(file_search::find_bar_button(
                    "go-to-line-close",
                    "icons/x.svg",
                    tr!("find.close"),
                    true,
                    theme,
                    cx.listener(|this, _, window, cx| {
                        this.close_go_to_line(false, true, window, cx)
                    }),
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_to_line_input_parses_line_and_column() {
        assert_eq!(parse_go_to_line_target("42"), Some((42, None)));
        assert_eq!(parse_go_to_line_target("42:7"), Some((42, Some(7))));
        assert_eq!(parse_go_to_line_target("  42  "), Some((42, None)));
        assert_eq!(parse_go_to_line_target("42:"), Some((42, None)));
        assert_eq!(parse_go_to_line_target(""), None);
        assert_eq!(parse_go_to_line_target("abc"), None);
        assert_eq!(parse_go_to_line_target("0"), None);
        assert_eq!(parse_go_to_line_target("42:0"), None);
        assert_eq!(parse_go_to_line_target("42:x"), None);
        assert_eq!(parse_go_to_line_target("+42"), None);
        assert_eq!(parse_go_to_line_target("1:2:3"), None);
    }

    #[test]
    fn line_range_indexes_logical_lines() {
        let content = "fn main() {\n    let x = 1;\n}\n";
        assert_eq!(line_range_for(content, 1), Some(0..11));
        assert_eq!(line_range_for(content, 2), Some(12..26));
        assert_eq!(line_range_for(content, 3), Some(27..28));
        // A trailing newline is the empty last line.
        assert_eq!(line_range_for(content, 4), Some(29..29));
        assert_eq!(line_range_for(content, 5), None);
        assert_eq!(line_range_for("", 1), Some(0..0));
        assert_eq!(line_range_for("", 2), None);
    }

    #[test]
    fn column_offsets_count_characters_not_bytes() {
        let line = "aé日x";
        assert_eq!(column_byte_offset(line, 1), 0);
        assert_eq!(column_byte_offset(line, 2), 1);
        // 'é' is two bytes, '日' three — column 4 lands on 'x'.
        assert_eq!(column_byte_offset(line, 4), 6);
        assert_eq!(column_byte_offset(line, 99), line.len());
    }
}
