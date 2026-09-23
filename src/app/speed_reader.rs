//! A quiet, transient RSVP reader for Markdown prose.
//!
//! The source is parsed only after an explicit user action. Prose advances one
//! Unicode word at a time; structured content becomes an inspection stop so
//! code, equations, and tables are never flashed past or silently discarded.

use std::time::Duration;

use super::*;
use crate::md::parser::{Block, InlineRun};
use gpui::InteractiveElement;
use gpui::StatefulInteractiveElement;
use unicode_segmentation::UnicodeSegmentation;

const MIN_WPM: u32 = 100;
const MAX_WPM: u32 = 1200;
const WPM_STEP: u32 = 25;

pub(super) struct SpeedReader {
    title: String,
    beats: Vec<ReadingBeat>,
    index: usize,
    wpm: u32,
    playing: bool,
    generation: u64,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
}

enum ReadingBeat {
    Word { text: String, pause_ms: u16 },
    Inspect { title: String, content: String },
}

impl SpeedReader {
    fn new(
        title: String,
        markdown: &str,
        wpm: u32,
        focus: FocusHandle,
        previous_focus: Option<FocusHandle>,
    ) -> Option<Self> {
        let tree = md::parser::parse(markdown);
        let mut beats = Vec::new();
        for top in tree.blocks {
            append_block(&top.block, &mut beats);
        }
        (!beats.is_empty()).then_some(Self {
            title,
            beats,
            index: 0,
            wpm,
            playing: false,
            generation: 0,
            focus,
            previous_focus,
        })
    }

    fn current_delay(&self) -> Duration {
        let pause_ms = match self.beats.get(self.index) {
            Some(ReadingBeat::Word { pause_ms, .. }) => *pause_ms,
            _ => 0,
        };
        let base_ms = 60_000 / self.wpm.max(MIN_WPM);
        Duration::from_millis((base_ms + u32::from(pause_ms)).into())
    }
}

impl Waku {
    pub(super) fn open_speed_reader(
        &mut self,
        title: String,
        markdown: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.transcript_control_focus("speed-reader", cx);
        let Some(reader) = SpeedReader::new(
            title,
            &markdown,
            self.state.speed_reader_wpm,
            focus.clone(),
            window.focused(cx),
        ) else {
            return;
        };
        self.speed_reader = Some(reader);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn close_speed_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous_focus = self
            .speed_reader
            .take()
            .and_then(|reader| reader.previous_focus);
        if let Some(previous_focus) = previous_focus {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    fn toggle_speed_reader(&mut self, cx: &mut Context<Self>) {
        let Some(reader) = self.speed_reader.as_mut() else {
            return;
        };
        let restarting = reader.index >= reader.beats.len();
        if restarting {
            reader.index = 0;
        }
        if matches!(
            reader.beats.get(reader.index),
            Some(ReadingBeat::Inspect { .. })
        ) {
            if restarting {
                reader.playing = false;
                reader.generation = reader.generation.wrapping_add(1);
                cx.notify();
            }
            return;
        }
        reader.playing = restarting || !reader.playing;
        reader.generation = reader.generation.wrapping_add(1);
        let playing = reader.playing;
        cx.notify();
        if playing {
            self.schedule_speed_reader_tick(cx);
        }
    }

    fn continue_speed_reader(&mut self, cx: &mut Context<Self>) {
        let Some(reader) = self.speed_reader.as_mut() else {
            return;
        };
        if !matches!(
            reader.beats.get(reader.index),
            Some(ReadingBeat::Inspect { .. })
        ) {
            return;
        }
        reader.index += 1;
        reader.playing = matches!(
            reader.beats.get(reader.index),
            Some(ReadingBeat::Word { .. })
        );
        reader.generation = reader.generation.wrapping_add(1);
        let playing = reader.playing;
        cx.notify();
        if playing {
            self.schedule_speed_reader_tick(cx);
        }
    }

    fn step_speed_reader(&mut self, direction: isize, cx: &mut Context<Self>) {
        let Some(reader) = self.speed_reader.as_mut() else {
            return;
        };
        let last = reader.beats.len();
        reader.index = reader.index.saturating_add_signed(direction).min(last);
        if reader.index >= last
            || matches!(
                reader.beats.get(reader.index),
                Some(ReadingBeat::Inspect { .. })
            )
        {
            reader.playing = false;
        }
        reader.generation = reader.generation.wrapping_add(1);
        let playing = reader.playing;
        cx.notify();
        if playing {
            self.schedule_speed_reader_tick(cx);
        }
    }

    fn adjust_speed_reader(&mut self, direction: i32, cx: &mut Context<Self>) {
        let Some(reader) = self.speed_reader.as_mut() else {
            return;
        };
        let adjusted = (reader.wpm as i32 + direction * WPM_STEP as i32)
            .clamp(MIN_WPM as i32, MAX_WPM as i32) as u32;
        if adjusted == reader.wpm {
            return;
        }
        reader.wpm = adjusted;
        self.state.speed_reader_wpm = adjusted;
        reader.generation = reader.generation.wrapping_add(1);
        let playing = reader.playing;
        self.save();
        cx.notify();
        if playing {
            self.schedule_speed_reader_tick(cx);
        }
    }

    fn speed_reader_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => self.close_speed_reader(window, cx),
            "space" => {
                if self.speed_reader.as_ref().is_some_and(|reader| {
                    matches!(
                        reader.beats.get(reader.index),
                        Some(ReadingBeat::Inspect { .. })
                    )
                }) {
                    self.continue_speed_reader(cx);
                } else {
                    self.toggle_speed_reader(cx);
                }
            }
            "left" => self.step_speed_reader(-1, cx),
            "right" => self.step_speed_reader(1, cx),
            "up" => self.adjust_speed_reader(1, cx),
            "down" => self.adjust_speed_reader(-1, cx),
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
    }

    fn schedule_speed_reader_tick(&self, cx: &mut Context<Self>) {
        let Some(reader) = self.speed_reader.as_ref() else {
            return;
        };
        if !reader.playing
            || !matches!(
                reader.beats.get(reader.index),
                Some(ReadingBeat::Word { .. })
            )
        {
            return;
        }
        let generation = reader.generation;
        let delay = reader.current_delay();
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(delay).await;
            let continue_playback = weak.update(cx, |this, cx| {
                let Some(reader) = this.speed_reader.as_mut() else {
                    return false;
                };
                if !reader.playing || reader.generation != generation {
                    return false;
                }
                reader.index += 1;
                if reader.index >= reader.beats.len()
                    || matches!(
                        reader.beats.get(reader.index),
                        Some(ReadingBeat::Inspect { .. })
                    )
                {
                    reader.playing = false;
                    reader.generation = reader.generation.wrapping_add(1);
                }
                let keep_going = reader.playing;
                cx.notify();
                keep_going
            });
            if matches!(continue_playback, Ok(true)) {
                let _ = weak.update(cx, |this, cx| this.schedule_speed_reader_tick(cx));
            }
        })
        .detach();
    }

    pub(super) fn render_speed_reader_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let reader = self.speed_reader.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let focus = reader.focus.clone();
        let title = reader.title.clone();
        let total = reader.beats.len();
        let index = reader.index;
        let wpm = reader.wpm;
        let playing = reader.playing;
        let progress = if total == 0 {
            0.0
        } else {
            index as f32 / total as f32
        };

        let reading_area: AnyElement = match reader.beats.get(index) {
            Some(ReadingBeat::Word { text, .. }) => div()
                .w_full()
                .h(px(220.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(56.0))
                .line_height(px(72.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(text.clone())
                .into_any_element(),
            Some(ReadingBeat::Inspect { title, content }) => div()
                .w_full()
                .h(px(220.0))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(sp(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child(title.clone()),
                )
                .child(
                    div()
                        .id("speed-reader-inspect-scroll")
                        .max_h(px(172.0))
                        .w_full()
                        .overflow_y_scroll()
                        .rounded(px(10.0))
                        .bg(theme.inset)
                        .px(px(14.0))
                        .py(px(11.0))
                        .text_size(px(14.0))
                        .line_height(px(21.0))
                        .text_color(theme.code_text)
                        .child(content.clone()),
                )
                .into_any_element(),
            None => div()
                .w_full()
                .h(px(220.0))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .text_color(theme.text)
                .child(
                    div()
                        .text_size(px(24.0))
                        .child(tr!("speed_reader.end_title")),
                )
                .child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("speed_reader.end_detail")),
                )
                .into_any_element(),
        };

        let mut position = (index + 1).min(total).to_string();
        if total == 0 {
            position = "0".to_owned();
        }
        let bar_width = (300.0 * progress).clamp(0.0, 300.0);
        let play_label = match reader.beats.get(index) {
            Some(ReadingBeat::Inspect { .. }) => tr!("speed_reader.continue"),
            None => tr!("speed_reader.read_again"),
            Some(ReadingBeat::Word { .. }) if playing => tr!("speed_reader.pause"),
            Some(ReadingBeat::Word { .. }) => tr!("speed_reader.play"),
        };

        let close_focus = self.transcript_control_focus("speed-reader-close", cx);
        let previous_focus = self.transcript_control_focus("speed-reader-previous", cx);
        let play_focus = self.transcript_control_focus("speed-reader-play", cx);
        let next_focus = self.transcript_control_focus("speed-reader-next", cx);
        let slower_focus = self.transcript_control_focus("speed-reader-slower", cx);
        let faster_focus = self.transcript_control_focus("speed-reader-faster", cx);

        Some(
            div()
                .id("speed-reader-overlay")
                .key_context("SpeedReader")
                .track_focus(&focus)
                .tab_index(0)
                .tab_group()
                .tab_stop(false)
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.canvas.opacity(0.96))
                .on_key_down(cx.listener(Self::speed_reader_key_down))
                .child(
                    div()
                        .w_full()
                        .max_w(px(720.0))
                        .mx(px(24.0))
                        .rounded(px(20.0))
                        .border(hairline())
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .px(px(34.0))
                        .pt(px(24.0))
                        .pb(px(22.0))
                        .shadow_lg()
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_start()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(5.0))
                                        .child(
                                            div()
                                                .text_size(sp(11.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.text_tertiary)
                                                .child(tr!("speed_reader.go_fast")),
                                        )
                                        .child(
                                            div()
                                                .max_w(px(560.0))
                                                .truncate()
                                                .text_size(sp(14.0))
                                                .text_color(theme.text_secondary)
                                                .child(title),
                                        ),
                                )
                                .child(reader_button(
                                    "speed-reader-close",
                                    tr!("speed_reader.return_to_text"),
                                    None,
                                    close_focus,
                                    theme,
                                    true,
                                    weak.clone(),
                                    |weak, window, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.close_speed_reader(window, cx);
                                        });
                                    },
                                )),
                        )
                        .child(reading_area)
                        .child(
                            div()
                                .mb(px(20.0))
                                .w_full()
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .w(px(300.0))
                                        .h(px(3.0))
                                        .rounded_full()
                                        .bg(theme.overlay_strong)
                                        .child(
                                            div()
                                                .h_full()
                                                .w(px(bar_width))
                                                .rounded_full()
                                                .bg(theme.accent),
                                        ),
                                )
                                .child(
                                    div()
                                        .min_w(px(70.0))
                                        .text_size(sp(12.0))
                                        .text_color(theme.text_tertiary)
                                        .child(format!("{position} / {total}")),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(reader_button(
                                            "speed-reader-previous",
                                            tr!("speed_reader.previous"),
                                            None,
                                            previous_focus,
                                            theme,
                                            index > 0,
                                            weak.clone(),
                                            |weak, _window, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.step_speed_reader(-1, cx);
                                                });
                                            },
                                        ))
                                        .child(reader_button(
                                            "speed-reader-play",
                                            play_label,
                                            None,
                                            play_focus,
                                            theme,
                                            true,
                                            weak.clone(),
                                            |weak, window, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    let is_inspect = this
                                                        .speed_reader
                                                        .as_ref()
                                                        .is_some_and(|reader| {
                                                            matches!(
                                                                reader.beats.get(reader.index),
                                                                Some(ReadingBeat::Inspect { .. })
                                                            )
                                                        });
                                                    if is_inspect {
                                                        this.continue_speed_reader(cx);
                                                    } else {
                                                        this.toggle_speed_reader(cx);
                                                    }
                                                });
                                                let _ = window;
                                            },
                                        ))
                                        .child(reader_button(
                                            "speed-reader-next",
                                            tr!("speed_reader.next"),
                                            None,
                                            next_focus,
                                            theme,
                                            index < total,
                                            weak.clone(),
                                            |weak, window, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.step_speed_reader(1, cx);
                                                });
                                                let _ = window;
                                            },
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(7.0))
                                        .child(reader_button(
                                            "speed-reader-slower",
                                            "−",
                                            Some(tr!("speed_reader.slower").into()),
                                            slower_focus,
                                            theme,
                                            wpm > MIN_WPM,
                                            weak.clone(),
                                            |weak, window, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.adjust_speed_reader(-1, cx);
                                                });
                                                let _ = window;
                                            },
                                        ))
                                        .child(
                                            div()
                                                .min_w(px(76.0))
                                                .text_center()
                                                .text_size(sp(13.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.text)
                                                .child(tr!("speed_reader.wpm", wpm = wpm)),
                                        )
                                        .child(reader_button(
                                            "speed-reader-faster",
                                            "+",
                                            Some(tr!("speed_reader.faster").into()),
                                            faster_focus,
                                            theme,
                                            wpm < MAX_WPM,
                                            weak,
                                            |weak, window, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.adjust_speed_reader(1, cx);
                                                });
                                                let _ = window;
                                            },
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .mt(px(18.0))
                                .pt(px(12.0))
                                .border_t(hairline())
                                .border_color(theme.border_subtle)
                                .text_size(sp(11.5))
                                .text_color(theme.text_tertiary)
                                .child(tr!("speed_reader.keyboard_hint")),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn reader_button(
    id: &'static str,
    label: impl Into<SharedString>,
    accessibility_label: Option<SharedString>,
    focus: FocusHandle,
    theme: Theme,
    enabled: bool,
    weak: WeakEntity<Waku>,
    action: impl Fn(&WeakEntity<Waku>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let action: Rc<dyn Fn(&WeakEntity<Waku>, &mut Window, &mut App)> = Rc::new(action);
    let click_action = action.clone();
    let key_action = action;
    let click_weak = weak.clone();
    let label = label.into();
    let accessibility_label = accessibility_label.unwrap_or_else(|| label.clone());
    let color = if enabled {
        theme.text_secondary
    } else {
        theme.text_ghost
    };
    let button = div()
        .id(id)
        .track_focus(&focus)
        .tab_index(0)
        .tab_stop(enabled)
        .h(px(34.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_default()
        .text_size(sp(12.5))
        .text_color(color)
        .aria_label(accessibility_label)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(theme.overlay_strong).text_color(theme.text))
        })
        .child(label);
    if enabled {
        button
            .on_click(move |_, window, cx| {
                click_action(&click_weak, window, cx);
                cx.stop_propagation();
            })
            .on_key_down(move |event, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    key_action(&weak, window, cx);
                    cx.stop_propagation();
                    window.prevent_default();
                }
            })
            .into_any_element()
    } else {
        button.into_any_element()
    }
}

fn append_block(block: &Block, beats: &mut Vec<ReadingBeat>) {
    match block {
        Block::Paragraph { runs } | Block::Heading { runs, .. } => {
            append_runs(runs, true, beats);
        }
        Block::CodeBlock { language, code } => {
            let title = language
                .as_deref()
                .filter(|language| !language.is_empty())
                .map(|language| tr!("speed_reader.code_language", language = language))
                .unwrap_or_else(|| tr!("speed_reader.code"));
            beats.push(ReadingBeat::Inspect {
                title,
                content: code.clone(),
            });
        }
        Block::DisplayMath { latex } => beats.push(ReadingBeat::Inspect {
            title: tr!("speed_reader.equation"),
            content: latex.clone(),
        }),
        Block::Image { alt, .. } => beats.push(ReadingBeat::Inspect {
            title: tr!("speed_reader.image"),
            content: if alt.trim().is_empty() {
                tr!("speed_reader.no_image_description")
            } else {
                alt.clone()
            },
        }),
        Block::BlockQuote { children } => {
            append_words(&tr!("speed_reader.quote"), false, beats);
            for child in children {
                append_block(child, beats);
            }
        }
        Block::List { items, .. } => {
            for item in items {
                append_list_item(item, beats);
            }
        }
        Block::Table { header, rows, .. } => {
            let mut lines = Vec::with_capacity(rows.len() + 1);
            lines.push(table_row(header));
            lines.extend(rows.iter().map(|row| table_row(row)));
            beats.push(ReadingBeat::Inspect {
                title: tr!("speed_reader.table"),
                content: lines.join("\n"),
            });
        }
        Block::Rule => beats.push(ReadingBeat::Inspect {
            title: tr!("speed_reader.section_break"),
            content: tr!("speed_reader.continue_when_ready"),
        }),
    }
}

fn append_list_item(item: &crate::md::parser::ListItem, beats: &mut Vec<ReadingBeat>) {
    match item.task {
        Some(true) => append_words(&tr!("speed_reader.checked"), false, beats),
        Some(false) => append_words(&tr!("speed_reader.unchecked"), false, beats),
        None => {}
    }
    for block in &item.blocks {
        append_block(block, beats);
    }
}

fn append_runs(runs: &[InlineRun], paragraph_end: bool, beats: &mut Vec<ReadingBeat>) {
    let mut prose = String::new();
    for run in runs {
        if run.style.code || run.style.math {
            append_words(&prose, false, beats);
            prose.clear();
            if !run.text.trim().is_empty() {
                beats.push(ReadingBeat::Inspect {
                    title: if run.style.math {
                        tr!("speed_reader.inline_equation")
                    } else {
                        tr!("speed_reader.inline_code")
                    },
                    content: run.text.clone(),
                });
            }
        } else {
            prose.push_str(&run.text);
        }
    }
    append_words(&prose, paragraph_end, beats);
}

fn append_words(text: &str, paragraph_end: bool, beats: &mut Vec<ReadingBeat>) {
    let ranges = text.unicode_word_indices().collect::<Vec<_>>();
    if ranges.is_empty() {
        if let Some(ReadingBeat::Word { text: previous, .. }) = beats.last_mut() {
            previous.push_str(text.trim());
        }
        return;
    }
    let prefix_end = ranges[0].0;
    for (index, (start, _word)) in ranges.iter().enumerate() {
        let end = ranges.get(index + 1).map_or(text.len(), |(next, _)| *next);
        let leading = if index == 0 {
            text[..prefix_end].trim_start()
        } else {
            ""
        };
        let word = format!("{leading}{}", text[*start..end].trim_end())
            .trim()
            .to_owned();
        if word.is_empty() {
            continue;
        }
        let last = index + 1 == ranges.len();
        let pause_ms = if last && paragraph_end {
            350
        } else {
            match word
                .trim_end_matches(['\"', '\'', ')', ']', '}', '”', '’'])
                .chars()
                .last()
            {
                Some('.' | '?' | '!' | '。' | '？' | '！') => 180,
                Some(',' | ';' | ':' | '、' | '；' | '：') => 80,
                _ => 0,
            }
        };
        beats.push(ReadingBeat::Word {
            text: word,
            pause_ms,
        });
    }
}

fn table_row(row: &[Vec<InlineRun>]) -> String {
    row.iter()
        .map(|cell| cell.iter().map(|run| run.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("  |  ")
}
