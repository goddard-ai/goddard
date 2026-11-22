use super::*;
use crate::input::InputEvent;
use crate::ui::{ActivationExt, icon_button, text_field::TextField, tooltip::Tooltip};
use alacritty_terminal::event::VoidListener;
use gpui::StatefulInteractiveElement;

const MATCH_LIMIT: usize = 20_000;

pub(super) struct TerminalSearch {
    input: Entity<TextInput>,
    controls: [FocusHandle; 3],
    job: Option<Task<()>>,
    revision: Option<u64>,
    matches: Vec<Match>,
    limited: bool,
    current: Option<usize>,
    steps: isize,
    reveal: bool,
}

impl TerminalSearch {
    pub(super) fn focused(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus().is_focused(window)
            || self.controls.iter().any(|focus| focus.is_focused(window))
    }

    pub(super) fn highlights(&self, revision: u64) -> Option<(&[Match], Option<&Match>)> {
        (self.revision == Some(revision)).then(|| {
            (
                self.matches.as_slice(),
                self.current.map(|index| &self.matches[index]),
            )
        })
    }

    fn advance(&mut self) {
        self.current = if self.matches.is_empty() {
            None
        } else {
            let base = self
                .current
                .unwrap_or(self.matches.len() - 1)
                .min(self.matches.len() - 1);
            Some((base as isize + self.steps).rem_euclid(self.matches.len() as isize) as usize)
        };
        self.steps = 0;
    }
}

impl TerminalView {
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() {
            return;
        }
        self.command_bar = None;
        let search = self.search.get_or_insert_with(|| {
            let input = cx.new(|cx| {
                TextInput::new(window, cx)
                    .tab_index(0)
                    .accessibility_label(tr!("input.find"))
                    .placeholder(tr!("input.find"))
            });
            cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Edited) {
                    this.edit_search(cx);
                }
            })
            .detach();
            TerminalSearch {
                input,
                controls: std::array::from_fn(|_| cx.focus_handle().tab_index(0).tab_stop(true)),
                job: None,
                revision: None,
                matches: Vec::new(),
                limited: false,
                current: None,
                steps: 0,
                reveal: false,
            }
        });
        search
            .input
            .update(cx, |input, cx| input.select_all_text(cx));
        window.focus(&search.input.read(cx).focus(), cx);
        cx.notify();
    }

    pub(super) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    fn edit_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &mut self.search else {
            return;
        };
        search.job = None;
        search.revision = None;
        search.matches.clear();
        search.current = None;
        search.limited = false;
        search.steps = 0;
        search.reveal = true;
        self.refresh_search(cx);
        cx.notify();
    }

    pub(super) fn navigate_search(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let (Some(session), Some(search)) = (&self.session, &mut self.search) else {
            cx.propagate();
            return;
        };
        if search.input.read(cx).content().is_empty() {
            return;
        }
        search.steps += if backwards { -1 } else { 1 };
        search.reveal = true;
        let mut term = session.term.lock();
        if search.revision == Some(session.grid_revision.load(Ordering::Acquire)) {
            search.advance();
            reveal_match(search, &mut term);
        }
        drop(term);
        self.refresh_search(cx);
        cx.notify();
    }

    pub(super) fn refresh_search(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Some(search)) = (&self.session, &mut self.search) else {
            return;
        };
        let query = search.input.read(cx).content();
        if query.is_empty()
            || search.job.is_some()
            || search.revision == Some(session.grid_revision.load(Ordering::Acquire))
        {
            return;
        }
        let query = query.to_owned();
        let term = session.term.clone();
        let grid_revision = session.grid_revision.clone();
        search.job = Some(cx.spawn(async move |view, cx| {
            // Coalesce input edits and batches of PTY output. Copy the grid
            // under its lock, then search without blocking the PTY or paint.
            cx.background_executor()
                .timer(Duration::from_millis(90))
                .await;
            let (revision, matches, limited) = cx
                .background_executor()
                .spawn(async move {
                    let (revision, grid) = {
                        let term = term.lock();
                        (grid_revision.load(Ordering::Acquire), term.grid().clone())
                    };
                    let dimensions = TerminalDimensions {
                        columns: grid.columns(),
                        rows: grid.screen_lines(),
                    };
                    let mut snapshot = Term::new(Config::default(), &dimensions, VoidListener);
                    *snapshot.grid_mut() = grid;
                    let (matches, limited) = find_matches(&snapshot, &query);
                    (revision, matches, limited)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                let (Some(session), Some(search)) = (&view.session, &mut view.search) else {
                    return;
                };
                search.job = None;
                let mut term = session.term.lock();
                if session.grid_revision.load(Ordering::Acquire) == revision {
                    search.revision = Some(revision);
                    search.matches = matches;
                    search.limited = limited;
                    search.advance();
                    reveal_match(search, &mut term);
                    cx.notify();
                }
            });
        }));
    }

    #[track_caller]
    pub(super) fn search_bar(&self, theme: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let count = if search.input.read(cx).content().is_empty() {
            String::new()
        } else if search.revision.is_none() {
            tr!("find.searching")
        } else if let Some(current) = search.current {
            let suffix = if search.limited { "+" } else { "" };
            tr!(
                "find.result_count",
                current = current + 1,
                total = format!("{}{suffix}", search.matches.len())
            )
        } else {
            tr!("find.no_results")
        };
        let [previous, next, close] = &search.controls;
        Some(
            div()
                .id("terminal-find")
                .key_context("TerminalFind")
                .tab_group()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.0))
                .px(px(8.0))
                .py(px(4.0))
                .border_b(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.raised)
                .child(
                    TextField::new("terminal-find-input", search.input.clone())
                        .flex_1()
                        .min_w_0(),
                )
                .child(
                    div()
                        .text_size(sp(11.0))
                        .text_color(theme.text_secondary)
                        .child(count),
                )
                .child(
                    icon_button("terminal-find-previous", "icons/arrow-up.svg", theme)
                        .track_focus(previous)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .tooltip(|window, cx| {
                            Tooltip::new(tr!("find.previous_match")).build(window, cx)
                        })
                        .on_activation(cx, |this, _, cx| this.navigate_search(true, cx)),
                )
                .child(
                    icon_button("terminal-find-next", "icons/arrow-down.svg", theme)
                        .track_focus(next)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .tooltip(|window, cx| {
                            Tooltip::new(tr!("find.next_match")).build(window, cx)
                        })
                        .on_activation(cx, |this, _, cx| this.navigate_search(false, cx)),
                )
                .child(
                    icon_button("terminal-find-close", "icons/x.svg", theme)
                        .track_focus(close)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .tooltip(|window, cx| Tooltip::new(tr!("find.close")).build(window, cx))
                        .on_activation(cx, Self::close_search),
                )
                .into_any_element(),
        )
    }
}

fn reveal_match<T: EventListener>(search: &mut TerminalSearch, term: &mut Term<T>) {
    if std::mem::take(&mut search.reveal)
        && let Some(index) = search.current
    {
        let matched = &search.matches[index];
        term.scroll_to_point(*matched.end());
        term.scroll_to_point(*matched.start());
    }
}

fn find_matches<T>(term: &Term<T>, query: &str) -> (Vec<Match>, bool) {
    let Ok(mut regex) = RegexSearch::new(&format!("(?i:{})", regex::escape(query))) else {
        return (Vec::new(), false);
    };
    let mut origin = Some(TerminalPoint::new(
        term.bottommost_line(),
        term.last_column(),
    ));
    let oldest = TerminalPoint::new(term.topmost_line(), Column(0));
    let mut matches: Vec<_> = std::iter::from_fn(|| {
        let matched = term.regex_search_left(&mut regex, origin?, oldest)?;
        let start = term.expand_wide(*matched.start(), Direction::Left);
        origin = (start > oldest).then(|| start.sub(term, Boundary::Grid, 1));
        Some(matched)
    })
    .take(MATCH_LIMIT + 1)
    .collect();
    let limited = matches.len() > MATCH_LIMIT;
    matches.truncate(MATCH_LIMIT);
    matches.reverse();
    (matches, limited)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::Processor;

    #[test]
    fn finds_literal_matches_across_soft_wraps_in_scrollback() {
        let dimensions = TerminalDimensions {
            columns: 8,
            rows: 3,
        };
        let mut term = Term::new(Config::default(), &dimensions, VoidListener);
        let mut processor: Processor = Processor::new();
        processor.advance(&mut term, b"123456AB.CD\r\nABxCD\r\nAB.\r\nCD\r\nAB.CD");
        let (matches, limited) = find_matches(&term, "ab.cd");

        assert!(!limited);
        assert_eq!(matches.len(), 2);
        assert!(matches[0].start().line < Line(0));
        assert_eq!(matches[0].end().line, matches[0].start().line + 1);
        assert_eq!(matches[1].start().line, term.bottommost_line());

        processor.advance(&mut term, b"\r\naaaaaa");
        let (matches, _) = find_matches(&term, "aa");
        assert_eq!(matches.len(), 3);
    }

    #[cfg(unix)]
    #[gpui::test]
    fn terminal_find_shortcuts_focus_reveal_and_close(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::input::init(cx);
            crate::bind_keys(cx);
        });
        let launch = TerminalLaunch::Program {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "printf 'needle\\n'; seq 100; printf 'needle done\\n'; read -r unused".into(),
            ],
        };
        let (view, cx) =
            cx.add_window_view(|_, cx| TerminalView::with_launch(std::env::temp_dir(), launch, cx));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.run_until_parked();
            let ready = view.read_with(cx, |view, _| {
                view.output_tail(1).iter().any(|line| line == "needle done")
            });
            if ready {
                break;
            }
            assert!(Instant::now() < deadline, "terminal output did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        view.update_in(cx, |view, window, cx| window.focus(&view.focus_handle, cx));
        cx.run_until_parked();

        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        });
        cx.simulate_input("needle");
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let search = view.search.as_ref().expect("Cmd+F opens terminal find");
            assert_eq!(search.matches.len(), 2);
            assert_eq!(search.current, Some(1));
        });

        cx.simulate_keystrokes("shift-enter");
        view.read_with(cx, |view, _| {
            let search = view.search.as_ref().unwrap();
            assert_eq!(search.current, Some(0));
            let term = view.session.as_ref().unwrap().term.lock();
            let offset = term.grid().display_offset() as i32;
            assert_eq!(search.matches[0].start().line, Line(-offset));
        });

        cx.simulate_keystrokes("enter");
        view.read_with(cx, |view, _| {
            assert_eq!(view.search.as_ref().unwrap().current, Some(1));
        });

        cx.simulate_keystrokes("escape");
        view.update_in(cx, |view, window, _| {
            assert!(view.search.is_none());
            assert!(view.focus_handle.is_focused(window));
        });
    }
}
