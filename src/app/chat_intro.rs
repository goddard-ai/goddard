use gpui::{KeyBinding, actions};

use super::*;

actions!(waku_chat_intro, [DismissChatIntro, KeepChatIntroFocus]);

const CONTEXT: &str = "ChatIntro";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", DismissChatIntro, Some(CONTEXT)),
        KeyBinding::new("space", DismissChatIntro, Some(CONTEXT)),
        KeyBinding::new("escape", DismissChatIntro, Some(CONTEXT)),
        KeyBinding::new("tab", KeepChatIntroFocus, Some(CONTEXT)),
        KeyBinding::new("shift-tab", KeepChatIntroFocus, Some(CONTEXT)),
    ]);
}

impl Waku {
    pub(super) fn render_chat_intro(
        &mut self,
        session_visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.state.boss_experiment_enabled
            || (self.state.boss_intro_dismissed && self.state.counsel_intro_dismissed)
        {
            self.chat_intro_focus = None;
            return None;
        }

        let target = self
            .state
            .selected_session
            .filter(|_| session_visible && self.composer_mounted() && !self.big_picture.is_open())
            .and_then(|id| {
                let (key, counsel) = self.managed_chat_clear_target(id)?;
                let eligible = if counsel {
                    !self.state.counsel_intro_dismissed && self.counsel_enabled_on(key)
                } else {
                    !self.state.boss_intro_dismissed
                };
                eligible.then_some((id, key, counsel))
            });
        let Some((id, key, counsel)) = target else {
            self.chat_intro_focus = None;
            return None;
        };

        let focus = match &self.chat_intro_focus {
            Some((active, focus)) if *active == id => focus.clone(),
            _ => {
                let focus = cx.focus_handle();
                self.chat_intro_focus = Some((id, focus.clone()));
                let initial_focus = focus.clone();
                let weak = cx.entity().downgrade();
                window.on_next_frame(move |window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if this.state.selected_session == Some(id)
                            && this
                                .chat_intro_focus
                                .as_ref()
                                .is_some_and(|(_, focus)| *focus == initial_focus)
                        {
                            window.focus(&initial_focus, cx);
                        }
                    });
                });
                focus
            }
        };
        let theme = Theme::current(cx);
        let state = self.boss_ui.states.get(&key)?;
        let (identity, title, description, example) = if counsel {
            (
                state.counsel.identity.as_ref(),
                tr!("counsel.intro_title"),
                tr!("counsel.intro_description"),
                tr!("counsel.intro_example"),
            )
        } else {
            (
                Some(&state.identity),
                tr!("boss.intro_title"),
                tr!("boss.intro_description"),
                tr!("boss.intro_example"),
            )
        };
        let avatar = identity
            .map(|identity| self.boss_avatar(identity, 28.0, cx))
            .unwrap_or_else(|| {
                icon("icons/compass.svg", 28.0, theme.text_secondary).into_any_element()
            });
        let start = tr!("boss.intro_start");
        let button = div()
            .id("chat-intro-start")
            .track_focus(&focus)
            .tab_index(0)
            .px(px(14.0))
            .h(px(38.0))
            .flex()
            .items_center()
            .rounded(px(7.0))
            .bg(theme.text)
            .text_color(theme.canvas)
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .border_2()
            .border_color(theme.text)
            .focus_visible(|style| style.border_color(theme.info))
            .aria_label(start.clone())
            .child(start)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.dismiss_chat_intro(counsel, window, cx);
            }));

        let card = div()
            .id("chat-intro")
            .key_context(CONTEXT)
            .on_action(cx.listener(move |this, _: &DismissChatIntro, window, cx| {
                this.dismiss_chat_intro(counsel, window, cx);
            }))
            .on_action(|_: &KeepChatIntroFocus, _, cx| cx.stop_propagation())
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(378.0))
            .p(px(22.0))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .rounded(px(12.0))
            .bg(theme.composer)
            .border_1()
            .border_color(theme.separator)
            .shadow_xl()
            .text_size(sp(15.0))
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(avatar)
                            .child(
                                div()
                                    .text_size(sp(24.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            ),
                    )
                    .child(div().text_color(theme.text_secondary).child(description)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .p(px(11.0))
                    .rounded(px(8.0))
                    .bg(theme.overlay)
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_secondary)
                            .child(tr!("boss.intro_try")),
                    )
                    .child(example),
            )
            .child(div().flex().justify_end().child(button));

        let layer = div()
            .id("chat-intro-layer")
            .absolute()
            .inset_0()
            .occlude()
            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.37))
            .py(px(16.0))
            .pl(px(self.sidebar_rendered_width + 16.0))
            .pr(px(self.right_panel_rendered_width + 16.0))
            .flex()
            .items_center()
            .justify_center()
            .child(card);
        Some(gpui::deferred(layer).with_priority(3).into_any_element())
    }

    fn dismiss_chat_intro(&mut self, counsel: bool, window: &mut Window, cx: &mut Context<Self>) {
        if counsel {
            self.state.counsel_intro_dismissed = true;
        } else {
            self.state.boss_intro_dismissed = true;
        }
        self.chat_intro_focus = None;
        if let Err(error) = self.store.save_app_state(&self.state) {
            eprintln!("Could not save chat intro dismissal: {error}");
        }
        window.focus(&self.composer_focus(cx), cx);
        cx.notify();
    }
}
