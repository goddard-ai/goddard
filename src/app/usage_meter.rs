//! The usage meter under the composer: a circular context-window gauge that
//! opens a panel with the session's context occupancy and the account's
//! rate-limit lanes, mirroring Claude Code's `/usage` rows. Context numbers
//! stream in from every provider transport that reports them; plan lanes come
//! from the Claude OAuth endpoint (fetched off-thread in [`crate::usage`]),
//! OpenCode Go's usage endpoint, and Codex's own rate-limit
//! notifications. Frames read only snapshots stored on the entity.

use gpui::relative;

use super::*;
use crate::usage::{PlanUsage, format_tokens, reset_label};

const USAGE_METER_MENU_ID: &str = "usage-meter";

/// Providers with an account-level plan fetcher. Codex additionally refreshes
/// live from its own stream notifications.
pub(super) const PLAN_USAGE_PROVIDERS: [ProviderKind; 4] = [
    ProviderKind::Claude,
    ProviderKind::Codex,
    ProviderKind::OpenCode,
    ProviderKind::Grok,
];

/// Refresh cadences for the plan snapshots. Quota moves only when turns run,
/// so idle refreshes stay rare; a settled turn or a just-opened panel asks
/// sooner. Grok's fetch spawns a probe process, so its idle cadence is wider.
const PLAN_USAGE_REFRESH: Duration = Duration::from_secs(300);
const PLAN_USAGE_REFRESH_GROK: Duration = Duration::from_secs(600);
const PLAN_USAGE_REFRESH_STALE: Duration = Duration::from_secs(30);
const PLAN_USAGE_RETRY: Duration = Duration::from_secs(90);

impl Waku {
    /// Start background fetches of any plan meters whose snapshot is due.
    /// The slow maintenance clock and explicit panel-open requests call this;
    /// guards keep it to one in-flight fetch per provider.
    pub(super) fn maybe_refresh_plan_usage(&mut self, cx: &mut Context<Self>) {
        // Disabling a provider only stops it backing new sessions; a session
        // already locked to it keeps running, and while one is selected its
        // usage panel still owes the account meters. Without this the panel
        // would show its loading skeleton forever: fetchable provider, no
        // snapshot, and no fetch ever allowed to start.
        let selected_provider = self.selected_session().map(|session| session.provider);
        for provider in PLAN_USAGE_PROVIDERS {
            if self.plan_usage_pending.contains(&provider)
                || (!self.provider_enabled(provider) && selected_provider != Some(provider))
            {
                continue;
            }
            let interval = if self.plan_usage_error.contains_key(&provider) {
                PLAN_USAGE_RETRY
            } else if self.plan_usage_stale.contains(&provider) {
                PLAN_USAGE_REFRESH_STALE
            } else if provider == ProviderKind::Grok {
                PLAN_USAGE_REFRESH_GROK
            } else {
                PLAN_USAGE_REFRESH
            };
            if self
                .plan_usage_checked_at
                .get(&provider)
                .is_some_and(|checked| checked.elapsed() < interval)
            {
                continue;
            }
            self.plan_usage_pending.insert(provider);
            let tx = self.plan_usage_tx.clone();
            let event_wake = self.event_wake_tx.clone();
            let claude_version = self
                .provider_versions
                .get(&ProviderKind::Claude)
                .cloned()
                .flatten();
            let binary_override = self.state.provider_binary_overrides.get(&provider).cloned();
            let daemon = self.daemon.client();
            cx.background_executor()
                .spawn(async move {
                    let result = match daemon.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::FetchPlanUsage {
                            provider,
                            binary_override,
                            cli_version: claude_version,
                        },
                    ) {
                        Ok(waku_client::ResponsePayload::PlanUsage { usage }) => Ok(usage),
                        Ok(_) => Err(anyhow::anyhow!(
                            "the daemon returned an invalid plan usage response"
                        )),
                        Err(error) => Err(error),
                    };
                    if tx
                        .send((provider, result.map_err(|error| format!("{error:#}"))))
                        .is_ok()
                    {
                        signal_event_pump(&event_wake);
                    }
                })
                .detach();
        }
    }

    pub(super) fn drain_plan_usage_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok((provider, result)) = self.plan_usage_events.try_recv() {
            self.plan_usage_pending.remove(&provider);
            self.plan_usage_stale.remove(&provider);
            self.plan_usage_checked_at.insert(provider, Instant::now());
            match result {
                Ok(Some(usage)) => {
                    changed |= self.plan_usage.get(&provider) != Some(&usage)
                        || self.plan_usage_error.contains_key(&provider)
                        || self.plan_usage_unconfigured.contains(&provider);
                    self.plan_usage.insert(provider, usage);
                    self.plan_usage_error.remove(&provider);
                    self.plan_usage_unconfigured.remove(&provider);
                }
                Ok(None) => {
                    let had_usage = self.plan_usage.remove(&provider).is_some();
                    let had_error = self.plan_usage_error.remove(&provider).is_some();
                    let newly_unconfigured = self.plan_usage_unconfigured.insert(provider);
                    changed |= had_usage || had_error || newly_unconfigured;
                }
                Err(error) => {
                    let was_unconfigured = self.plan_usage_unconfigured.remove(&provider);
                    changed |=
                        self.plan_usage_error.get(&provider) != Some(&error) || was_unconfigured;
                    // Keep any previous snapshot; stale meters with reset
                    // times still self-correct visually.
                    self.plan_usage_error.insert(provider, error);
                }
            }
        }
        changed
    }

    /// Whether the footer shows the gauge. Always true with a session
    /// selected — an empty ring is the honest "nothing measured yet" state,
    /// and hiding it would make the control feel intermittent.
    pub(super) fn usage_meter_available(&self) -> bool {
        self.selected_session().is_some()
    }

    /// Primary modifier + U: toggle the usage panel as if its footer trigger were clicked.
    pub(super) fn toggle_usage_panel_action(
        &mut self,
        _: &ToggleUsagePanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.is_some() || !self.usage_meter_available() {
            return;
        }
        let menus = self.menus.borrow();
        let Some(handle) = menus.get(USAGE_METER_MENU_ID).cloned() else {
            return;
        };
        // A keyboard toggle produces no mouse-down for another open menu's
        // dismiss-on-down-out to see, so close the rest here.
        let other_open: Vec<_> = menus
            .iter()
            .filter(|(id, other)| id.as_ref() != USAGE_METER_MENU_ID && other.is_open())
            .map(|(_, other)| other.clone())
            .collect();
        drop(menus);
        window.defer(cx, move |window, cx| {
            for menu in other_open {
                menu.close(window, cx);
            }
            crate::ui::menu::toggle_popover(&handle, MenuAlign::AboveRight, window, cx);
        });
    }

    /// The footer's circular context gauge plus its anchored panel. `None`
    /// while there is nothing to show for the selected session's provider.
    pub(super) fn render_usage_meter(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.usage_meter_available() {
            return None;
        }
        let session = self.selected_session()?;
        self.render_usage_meter_for(
            session,
            USAGE_METER_MENU_ID,
            self.composer.read(cx).focus(),
            cx,
        )
    }

    /// Render the same meter for a session that is visible outside the main
    /// selection, such as a side chat. Its menu id and return focus are
    /// supplied by the surface so multiple session panes do not share a
    /// trigger or return focus to the wrong composer.
    pub(super) fn render_usage_meter_for(
        &self,
        session: &AgentSession,
        menu_id: impl Into<SharedString>,
        composer_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let provider = session.provider;
        let context = session.context_usage;
        let theme = Theme::current(cx);
        let plan = self.plan_usage.get(&provider).cloned();
        let error = self.plan_usage_error.get(&provider).cloned();
        // Fetchable but nothing cached yet: the panel shows a skeleton
        // whether the fetch is already in flight or lands on the next tick.
        let plan_loading = plan.is_none()
            && error.is_none()
            && !self.plan_usage_unconfigured.contains(&provider)
            && PLAN_USAGE_PROVIDERS.contains(&provider);

        let weak = cx.entity().downgrade();
        let menu_id = menu_id.into();
        let menu_id_for_lookup = menu_id.clone();
        let session_provider = provider;
        // The meter's popover closure owns its own clone — `weak` moves
        // into the open/close handler above it.
        let panel_waku = weak.clone();
        let handle = self.menu_handle_with(menu_id.clone(), cx, move |open, window, cx| {
            if open {
                let mut card_focus = None;
                let _ = weak.update(cx, |this, cx| {
                    // An opening panel wants fresh numbers; the pump honors
                    // the stale flag on its next tick once the backoff allows.
                    if PLAN_USAGE_PROVIDERS.contains(&session_provider) {
                        this.plan_usage_stale.insert(session_provider);
                    }
                    this.maybe_refresh_plan_usage(cx);
                    card_focus = this
                        .menus
                        .borrow()
                        .get(&menu_id_for_lookup)
                        .map(|handle| handle.focus_handle().clone());
                    cx.notify();
                });
                // The card is deferred, so its focus handle joins the
                // dispatch tree only after the deferred draw — the same
                // two-frame wait the menus use. Focused, the card's menu
                // context is what lets `escape` dismiss it.
                if let Some(focus) = card_focus {
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| window.focus(&focus, cx));
                    });
                }
            } else {
                let _ = weak.update(cx, |_, cx| cx.notify());
                window.focus(&composer_focus, cx);
            }
        });

        // The panel's compact action uses the same eligibility as `/compact`
        // and the palette: a reserved Waku path or a provider-reported one.
        let compact = (provider.supports_compact()
            || crate::composer_complete::has_compact_path(&self.slash_command_index))
        .then(|| (session.id, cx.entity().downgrade()));
        let percent = context.and_then(context_percent);
        let fill = match percent {
            Some(percent) if percent >= 95.0 => theme.danger,
            Some(percent) if percent >= 80.0 => theme.warning,
            _ => theme.gauge,
        };
        let tooltip = match (&error, percent) {
            (Some(error), _) => SharedString::from(tr!("usage.refresh_failed", error = error)),
            (None, Some(percent)) => SharedString::from(tr!(
                "usage.context_used",
                percent = format!("{percent:.0}"),
                shortcut = crate::platform::primary_shortcut("⌘U", "Ctrl+U")
            )),
            (None, None) => SharedString::from(tr!(
                "usage.shortcut",
                shortcut = crate::platform::primary_shortcut("⌘U", "Ctrl+U")
            )),
        };

        let trigger = div()
            .id(menu_id.clone())
            .h(px(20.0))
            .px(px(5.0))
            .rounded(px(5.0))
            .flex()
            .items_center()
            .flex_none()
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .when(handle.is_open(), |element| element.bg(theme.overlay_strong))
            .tooltip(Tooltip::text(tooltip))
            .child(crate::ui::progress_ring(percent, theme.separator, fill));

        Some(popover(
            trigger,
            &handle,
            MenuAlign::AboveRight,
            move |handle, _, cx| {
                usage_panel(
                    handle,
                    provider,
                    context,
                    plan.clone(),
                    error.as_deref(),
                    plan_loading,
                    compact.clone(),
                    panel_waku.clone(),
                    cx,
                )
            },
        ))
    }
}

/// Fraction of the window in use, capped at 100%. A provider (notably
/// OpenCode) can report a lifetime/cumulative token counter that exceeds the
/// model's window; the context can never be more than fully used, so anything
/// past the window reads as full rather than an impossible percentage.
fn context_percent(usage: ContextUsage) -> Option<f64> {
    usage
        .window
        .filter(|window| *window > 0)
        .map(|window| (usage.tokens as f64 * 100.0 / window as f64).min(100.0))
}

fn usage_panel(
    handle: &ContextMenuHandle,
    provider: ProviderKind,
    context: Option<ContextUsage>,
    plan: Option<PlanUsage>,
    error: Option<&str>,
    plan_loading: bool,
    compact: Option<(Uuid, WeakEntity<Waku>)>,
    waku: WeakEntity<Waku>,
    cx: &App,
) -> AnyElement {
    let theme = Theme::current(cx);
    let now = unix_time() as i64;
    let mut panel = div()
        // Focused on open so the surrounding menu context sees `escape`.
        .track_focus(handle.focus_handle())
        .w(px(320.0))
        .p(px(14.0))
        .rounded(px(12.0))
        .border(hairline())
        .border_color(theme.border_subtle)
        .bg(theme.raised)
        .shadow_lg()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .text_size(sp(12.5));

    // The context row always renders; a session with nothing measured yet
    // reads "0" over an empty track, exactly like the CLI's own panel.
    let usage = context.unwrap_or_default();
    let percent = context_percent(usage);
    let value = match (usage.window, percent) {
        (Some(window), Some(percent)) => format!(
            "{} / {} ({percent:.0}%)",
            format_tokens(usage.tokens.min(window)),
            format_tokens(window)
        ),
        // The transport reports occupancy but not the window size.
        _ => format_tokens(usage.tokens),
    };
    panel = panel.child(
        div()
            .flex()
            .flex_col()
            .gap(px(7.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_color(theme.text)
                            .child(tr!("usage.context_window")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(value)),
                    ),
            )
            .child(meter_bar(&theme, percent.unwrap_or(0.0))),
    );
    if let Some((session_id, weak)) = compact {
        panel = panel.child(compact_row(handle, session_id, weak, &theme, cx));
    }
    if plan.is_some() || error.is_some() || plan_loading {
        panel = panel.child(div().h(hairline()).flex_none().bg(theme.separator));
    }

    if let Some(plan) = plan {
        let header = match &plan.plan_label {
            Some(label) => tr!("usage.plan_limits_named", plan = label),
            None => tr!("usage.plan_limits"),
        };
        let usage_url = match provider {
            ProviderKind::Claude => Some("https://claude.ai/settings/usage"),
            ProviderKind::Codex => Some("https://chatgpt.com/codex/settings/usage"),
            _ => None,
        };
        let header_row = div().flex().items_center().gap(px(6.0)).child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary)
                .child(SharedString::from(header)),
        );
        panel = panel.child(match usage_url {
            Some(url) => header_row
                .id("plan-usage-link")
                .cursor_default()
                .hover(|element| element.opacity(0.8))
                .tooltip(Tooltip::text(tr!("usage.open_account_settings")))
                .on_click(move |_, _, cx| cx.open_url(url))
                .child(icon("icons/arrow-right.svg", 10.0, theme.text_tertiary))
                .into_any_element(),
            None => header_row.into_any_element(),
        });
        for window in &plan.windows {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                // A long lane label gives way — truncated
                                // with an ellipsis — rather than pushing the
                                // reset time and percent past the card edge.
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_color(theme.text)
                                    .child(SharedString::from(
                                        window
                                            .label_i18n
                                            .as_ref()
                                            .map(waku_client::WireTranslation::render)
                                            .unwrap_or_else(|| window.label.clone()),
                                    )),
                            )
                            .children(window.resets_at.map(|resets_at| {
                                div()
                                    .flex_none()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .child(SharedString::from(reset_label(resets_at, now)))
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_secondary)
                                    .child(SharedString::from(format!("{:.0}%", window.percent))),
                            ),
                    )
                    .child(meter_bar(&theme, window.percent)),
            );
        }
        if let Some(credits) = plan
            .reset_credits
            .as_ref()
            .filter(|credits| credits.available_count > 0)
        {
            panel = panel.child(reset_credits_section(handle, credits, waku, &theme, cx));
        }
    } else if plan_loading {
        panel = panel.child(plan_skeleton(&theme));
    } else if let Some(error) = error {
        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(tr!("usage.plan_limits")),
                )
                .child(
                    div()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(SharedString::from(tr!("usage.unavailable", error = error))),
                ),
        );
    }

    panel.into_any_element()
}

/// The panel's compact action: same guards and driver path as `/compact`,
/// closing the popover before the request goes out so the toast it may
/// raise is not anchored to a dismissed card.
fn compact_row(
    handle: &ContextMenuHandle,
    session_id: Uuid,
    weak: WeakEntity<Waku>,
    theme: &Theme,
    cx: &App,
) -> Stateful<Div> {
    let focus = cx.focus_handle();
    let click_close = handle.clone();
    let click_weak = weak.clone();
    let key_close = handle.clone();
    div()
        .id("usage-compact")
        .track_focus(&focus)
        .tab_index(0)
        .h(px(28.0))
        .w_full()
        .px(px(8.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_default()
        .text_color(theme.text)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .hover(|style| style.bg(theme.overlay_strong))
        .tooltip(Tooltip::text(tr!("commands.compact_description")))
        .child(icon("icons/minimize.svg", 12.0, theme.text_secondary))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .child(tr!("commands.compact_context")),
        )
        .on_click(move |_, window, cx| {
            click_close.close(window, cx);
            let _ = click_weak.update(cx, |waku, cx| waku.compact_session(session_id, cx));
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if !event.keystroke.modifiers.modified()
                && matches!(event.keystroke.key.as_str(), "enter" | "space")
            {
                key_close.close(window, cx);
                let _ = weak.update(cx, |waku, cx| waku.compact_session(session_id, cx));
                cx.stop_propagation();
            }
        })
}

/// The banked-reset bank: a count line with the nearest expiry, then the
/// "Use reset" action. Clicking opens the confirmation dialog — the credit
/// is spent only from there. Mirrors Codex's own usage settings.
fn reset_credits_section(
    handle: &ContextMenuHandle,
    credits: &crate::usage::PlanResetCredits,
    waku: WeakEntity<Waku>,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let count = credits.available_count as usize;
    let mut summary = match count {
        1 => tr!("usage.reset_credit_one"),
        _ => tr!("usage.reset_credit_many", count = count),
    };
    if let Some(expires_at) = credits.next_expires_at {
        summary.push_str(&tr!(
            "usage.reset_credit_expires",
            date = crate::usage::expiry_label(expires_at)
        ));
    }
    let focus = cx.focus_handle();
    let click_close = handle.clone();
    let click_waku = waku.clone();
    let key_close = handle.clone();
    let action_row = div()
        .id("usage-reset-credit")
        .track_focus(&focus)
        .tab_index(0)
        .h(px(28.0))
        .w_full()
        .px(px(8.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_default()
        .text_color(theme.text)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .hover(|style| style.bg(theme.overlay_strong))
        .tooltip(Tooltip::text(tr!("usage.reset_credit_hint")))
        .child(icon("icons/rotate-cw.svg", 12.0, theme.text_secondary))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .child(tr!("usage.use_reset_credit")),
        )
        .on_click(move |_, window, cx| {
            click_close.close(window, cx);
            open_reset_credit_from_panel(&click_waku, window, cx);
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if !event.keystroke.modifiers.modified()
                && matches!(event.keystroke.key.as_str(), "enter" | "space")
            {
                key_close.close(window, cx);
                open_reset_credit_from_panel(&waku, window, cx);
                cx.stop_propagation();
            }
        });
    div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary)
                .child(SharedString::from(summary)),
        )
        .child(action_row)
        .into_any_element()
}

/// Open the redemption confirmation, then land focus on its confirm row —
/// the dialog joins the dispatch tree on the deferred draw, so like the
/// other modals focus waits two frames.
fn open_reset_credit_from_panel(weak: &WeakEntity<Waku>, window: &mut Window, cx: &mut App) {
    let focus = weak
        .update(cx, |waku, cx| waku.open_reset_credit_dialog(cx))
        .ok();
    if let Some(focus) = focus {
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
    }
}

/// Placeholder for the plan section while its first fetch is in flight:
/// a header bar and two quota rows, pulsing gently. `with_animation` honors
/// the system's reduce-motion setting on its own.
fn plan_skeleton(theme: &Theme) -> AnyElement {
    let theme = *theme;
    let bar = move |width: f32| {
        div()
            .h(px(9.0))
            .w(px(width))
            .flex_none()
            .rounded(px(4.5))
            .bg(theme.overlay_strong)
    };
    let row = move |label_width: f32, value_width: f32| {
        div()
            .flex()
            .flex_col()
            .gap(px(9.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(bar(label_width))
                    .child(div().flex_1())
                    .child(bar(value_width)),
            )
            .child(
                div()
                    .h(px(3.0))
                    .w_full()
                    .flex_none()
                    .rounded_full()
                    .bg(theme.overlay_strong),
            )
    };
    motion::pulse(Duration::from_millis(1400), move |phase| {
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(bar(132.0))
            .child(row(96.0, 64.0))
            .child(row(120.0, 64.0))
            .opacity(pulsating_between(0.45, 0.9)(phase))
            .into_any_element()
    })
    .every(2)
    .into_any_element()
}

/// A quota bar: full-width track, fill proportional to `percent`. A lane in
/// use keeps a visible sliver even under one percent.
fn meter_bar(theme: &Theme, percent: f64) -> Div {
    let fraction = (percent / 100.0).clamp(0.0, 1.0) as f32;
    let fraction = if fraction > 0.0 {
        fraction.max(0.015)
    } else {
        0.0
    };
    let fill = if percent >= 95.0 {
        theme.danger
    } else if percent >= 80.0 {
        theme.warning
    } else {
        theme.gauge
    };
    div()
        .h(px(3.0))
        .w_full()
        .flex_none()
        .rounded_full()
        .bg(theme.overlay_strong)
        .child(div().h_full().w(relative(fraction)).rounded_full().bg(fill))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_percent_caps_at_full_when_provider_over_reports() {
        // OpenCode reports lifetime/cumulative session counters that can run
        // far past the model's window; the meter must read "full", never an
        // impossible percentage like 472%.
        let usage = ContextUsage {
            tokens: 4_718_002,
            window: Some(1_000_000),
        };
        assert_eq!(context_percent(usage), Some(100.0));
    }

    #[test]
    fn context_percent_reports_partial_usage_verbatim() {
        let usage = ContextUsage {
            tokens: 250_000,
            window: Some(1_000_000),
        };
        assert_eq!(context_percent(usage), Some(25.0));
    }

    #[test]
    fn context_percent_is_none_without_a_positive_window() {
        assert_eq!(
            context_percent(ContextUsage {
                tokens: 4_718_002,
                window: None,
            }),
            None
        );
        assert_eq!(
            context_percent(ContextUsage {
                tokens: 4_718_002,
                window: Some(0),
            }),
            None
        );
    }
}
