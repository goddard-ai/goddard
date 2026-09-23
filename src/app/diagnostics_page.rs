//! The Diagnostics settings page: a merged, newest-first feed of reported
//! app errors and the daemon's forensic logs — error toasts, supervisor
//! restarts, panics. `ensure_diagnostics` reads the files on the
//! background executor; rendering only walks the snapshot it landed, so
//! the page never touches disk in a frame.

use std::rc::Rc;

use super::settings::settings_button;
use super::*;
use crate::diagnostics::{DiagnosticEntry, DiagnosticSource};
use crate::ui::ActivationExt;

/// Uniform feed row height — `ListState` never has to measure.
const DIAGNOSTICS_ROW_HEIGHT: f32 = 42.0;
/// A snapshot older than this rescans when the page is next opened; the
/// refresh button covers anything newer.
const DIAGNOSTICS_RESCAN_AFTER: Duration = Duration::from_secs(15);

impl Waku {
    /// Start a background read of the diagnostics sources unless a
    /// current-enough snapshot already covers this visit. `force` is the
    /// refresh button; a scan already inbound absorbs either ask.
    pub(super) fn ensure_diagnostics(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.diagnostics_scan_pending {
            return;
        }
        let satisfied = self.diagnostics.is_some()
            && self
                .diagnostics_scanned_at
                .is_some_and(|scanned| scanned.elapsed() < DIAGNOSTICS_RESCAN_AFTER);
        if !force && satisfied {
            return;
        }
        self.diagnostics_scan_pending = true;
        cx.spawn(async move |this, cx| {
            let entries = cx
                .background_executor()
                .spawn(async move { crate::diagnostics::load_entries() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.diagnostics_scan_pending = false;
                this.diagnostics = Some(Rc::new(entries));
                this.diagnostics_scanned_at = Some(Instant::now());
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_diagnostics_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let mut page = div()
            .mt(px(15.0))
            .w_full()
            .flex_1()
            .min_h_0()
            .pb(px(32.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .text_size(sp(12.5))
                    .line_height(sp(18.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("settings.diagnostics_description")),
            );

        let mut toolbar = div()
            .mt(px(14.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(settings_button(
                "diagnostics-refresh",
                if self.diagnostics_scan_pending {
                    tr!("diagnostics.loading")
                } else {
                    tr!("diagnostics.refresh")
                },
                !self.diagnostics_scan_pending,
                false,
                true,
                theme,
                cx,
                |this, _, cx| this.ensure_diagnostics(true, cx),
            ));
        if crate::diagnostics::logs_directory().is_some() {
            toolbar = toolbar.child(settings_button(
                "diagnostics-reveal",
                tr!("diagnostics.reveal_logs"),
                true,
                false,
                true,
                theme,
                cx,
                |_, _, cx| {
                    if let Some(dir) = crate::diagnostics::logs_directory() {
                        crate::platform::reveal_in_file_manager(&dir, cx);
                    }
                },
            ));
        }
        page = page.child(toolbar);

        let Some(entries) = &self.diagnostics else {
            return page
                .child(
                    div()
                        .mt(px(12.0))
                        .text_size(sp(13.0))
                        .text_color(theme.text_tertiary)
                        .child(tr!("diagnostics.loading")),
                )
                .into_any_element();
        };
        if entries.is_empty() {
            return page
                .child(
                    div()
                        .mt(px(12.0))
                        .text_size(sp(13.0))
                        .text_color(theme.text_tertiary)
                        .child(tr!("settings.diagnostics_empty")),
                )
                .into_any_element();
        }

        let list_state = self.diagnostics_list.clone();
        if list_state.item_count() != entries.len() {
            list_state.reset_with_uniform_height(entries.len(), px(DIAGNOSTICS_ROW_HEIGHT));
        }
        let entries = entries.clone();
        let entity = cx.entity().downgrade();
        page.child(
            div()
                .mt(px(10.0))
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    list(list_state.clone(), move |index, _window, cx| {
                        let Some(entry) = entries.get(index).cloned() else {
                            return div().into_any_element();
                        };
                        entity
                            .upgrade()
                            .map(|entity| {
                                entity.update(cx, |this, cx| {
                                    this.render_diagnostics_row(index, &entry, cx)
                                })
                            })
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .size_full(),
                )
                .child(scrollbar::vertical(
                    &list_state,
                    &self.diagnostics_scrollbar,
                )),
        )
        .into_any_element()
    }

    /// One feed row: age, source chip, one-line record. Activating it —
    /// click or Enter — copies the raw record for a bug report.
    fn render_diagnostics_row(
        &self,
        index: usize,
        entry: &DiagnosticEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let (source_label, source_color) = match entry.source {
            DiagnosticSource::AppError => (tr!("diagnostics.source_app"), theme.danger),
            DiagnosticSource::DaemonRecovery => (tr!("diagnostics.source_restart"), theme.warning),
            DiagnosticSource::DaemonPanic => (tr!("diagnostics.source_panic"), theme.danger),
        };
        let copy_id = format!("diagnostics-copy-{index}");
        let copied = self.control_was_copied(&copy_id);
        let detail = entry.detail.clone();
        let age = super::sidebar::format_time_ago(unix_time().saturating_sub(entry.at));
        div()
            .id(SharedString::from(format!("diagnostics-row-{index}")))
            .tab_index(0)
            .h(px(DIAGNOSTICS_ROW_HEIGHT))
            .px(px(10.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .tooltip(Tooltip::text(tr!("diagnostics.copy_record")))
            .child(
                div()
                    .w(px(42.0))
                    .flex_none()
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(age),
            )
            .child(
                div()
                    .w(px(52.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(div().size(px(5.0)).rounded(px(3.0)).bg(source_color))
                    .child(
                        div()
                            .text_size(sp(11.0))
                            .text_color(theme.text_secondary)
                            .child(source_label),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .text_color(theme.text)
                    .child(entry.summary.clone()),
            )
            .child(icon(
                if copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                if copied {
                    theme.success
                } else {
                    theme.text_tertiary
                },
            ))
            .on_activation(cx, move |this, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(detail.clone()));
                this.show_control_copied(copy_id.clone(), cx);
            })
            .into_any_element()
    }
}
