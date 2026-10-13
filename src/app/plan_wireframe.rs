//! Revision-bound companion reads. All transport and parsing run off the UI thread.
use super::*;
use waku_client::boss::{BossOperation, BossResult};
use waku_protocol::wireframe::Wireframe;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Request {
    pub key: waku_client::DaemonKey,
    pub revision: u64,
    pub path: String,
    pub generation: Uuid,
}

pub(super) struct Preview {
    pub request: Request,
    pub pending: bool,
    /// None is verified absence; errors include transport and schema failures.
    pub content: Option<Result<Option<Wireframe>, String>>,
    pub selected: bool,
    pub screen: usize,
}

impl Preview {
    pub fn accepts(&self, request: &Request) -> bool {
        self.pending && self.request == *request
    }

    pub fn finish(&mut self, request: &Request, result: Result<Option<Wireframe>, String>) -> bool {
        if !self.accepts(request) {
            return false;
        }
        self.pending = false;
        self.content = Some(result);
        if let Some(Ok(Some(wireframe))) = &self.content {
            self.screen = self.screen.min(wireframe.screens.len().saturating_sub(1));
        }
        if !self.visible(true) {
            self.selected = false;
        }
        true
    }

    pub fn visible(&self, enabled: bool) -> bool {
        enabled && !matches!(self.content, None | Some(Ok(None)))
    }
}

pub(super) fn companion_path(plan: &str) -> Option<String> {
    plan.strip_suffix(".md")
        .map(|stem| format!("{stem}.wireframe.json"))
}

/// List first so absence is distinct from permissions/transport failures.
/// Recheck after a failed read to handle removal between listing and reading.
pub(super) fn read(
    path: &str,
    mut request: impl FnMut(BossOperation) -> Result<BossResult, String>,
) -> Result<Option<Wireframe>, String> {
    let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    let exists =
        |request: &mut dyn FnMut(BossOperation) -> Result<BossResult, String>| match request(
            BossOperation::ListFiles {
                path: parent.to_owned(),
            },
        )? {
            BossResult::Files { files } => Ok(files
                .iter()
                .any(|file| file.path == path && !file.directory)),
            _ => Err("unexpected companion listing response".to_owned()),
        };
    if !exists(&mut request)? {
        return Ok(None);
    }
    match request(BossOperation::ReadFile {
        path: path.to_owned(),
    }) {
        Ok(BossResult::File { content, .. }) => Wireframe::parse(&content)
            .map(Some)
            .map_err(|error| error.to_string()),
        Ok(_) => Err("unexpected companion read response".to_owned()),
        Err(error) => {
            if exists(&mut request)? {
                Err(error)
            } else {
                Ok(None)
            }
        }
    }
}

impl Waku {
    pub(super) fn ensure_plan_wireframe(
        &mut self,
        key: waku_client::DaemonKey,
        session_id: Uuid,
        plan_file: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.state.wireframes_experiment_enabled {
            self.plan_wireframes.clear();
            return;
        }
        let Some(path) = companion_path(plan_file) else {
            self.plan_wireframes.remove(&session_id);
            return;
        };
        let revision = self
            .boss_ui
            .states
            .get(&key)
            .map_or(0, |state| state.revision);
        if self
            .plan_wireframes
            .get(&session_id)
            .is_some_and(|preview| {
                preview.request.key == key
                    && preview.request.revision == revision
                    && preview.request.path == path
            })
        {
            return;
        }
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            return;
        };
        let previous = self
            .plan_wireframes
            .remove(&session_id)
            .filter(|preview| preview.request.key == key && preview.request.path == path);
        let selected = previous.as_ref().is_some_and(|preview| preview.selected);
        let screen = previous.as_ref().map_or(0, |preview| preview.screen);
        let request = Request {
            key,
            revision,
            path,
            generation: Uuid::new_v4(),
        };
        self.plan_wireframes.insert(
            session_id,
            Preview {
                request: request.clone(),
                pending: true,
                content: previous.and_then(|preview| preview.content),
                selected,
                screen,
            },
        );
        cx.spawn(async move |waku, cx| {
            let path = request.path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    read(&path, |operation| {
                        match client.request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::Boss { operation },
                        ) {
                            Ok(waku_client::ResponsePayload::Boss { result }) => Ok(result),
                            Ok(_) => Err("unexpected companion response".to_owned()),
                            Err(error) => Err(error.to_string()),
                        }
                    })
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if !waku.state.wireframes_experiment_enabled
                    || waku
                        .boss_ui
                        .states
                        .get(&key)
                        .map_or(0, |state| state.revision)
                        != revision
                {
                    return;
                }
                let Some(preview) = waku.plan_wireframes.get_mut(&session_id) else {
                    return;
                };
                let was_selected = preview.selected;
                if !preview.finish(&request, result) {
                    return;
                }
                if was_selected
                    && !preview.selected
                    && waku.right_panel_live_owner == RightPanelOwner::Session(session_id)
                {
                    waku.right_panel_pending_focus = Some(waku.right_panel_focus.clone());
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn plan_wireframe_visible(&self, session_id: Uuid) -> bool {
        self.plan_wireframes
            .get(&session_id)
            .is_some_and(|preview| preview.visible(self.state.wireframes_experiment_enabled))
    }

    #[track_caller]
    pub(super) fn plan_wireframe_header(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Div {
        use crate::ui::ActivationExt;
        let theme = Theme::current(cx);
        let selected = self
            .plan_wireframes
            .get(&session_id)
            .is_some_and(|preview| preview.selected);
        let mut header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .p(px(8.0));
        for (id, label, value) in [
            ("plan-document-view", tr!("boss.plan_document"), false),
            ("plan-wireframe-view", tr!("boss.plan_wireframe"), true),
        ] {
            let focus = self.transcript_control_focus(id, cx);
            header = header.child(
                div()
                    .id(id)
                    .track_focus(&focus)
                    .tab_index(0)
                    .px(px(10.0))
                    .py(px(5.0))
                    .rounded(px(8.0))
                    .text_size(sp(12.0))
                    .text_color(theme.text)
                    .bg(if selected == value {
                        theme.overlay
                    } else {
                        theme.surface
                    })
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(label)
                    .when(selected == value, |element| {
                        element.child(icon("icons/check.svg", 12.0, theme.text))
                    })
                    .on_activation(cx, move |this, _, cx| {
                        if let Some(preview) = this.plan_wireframes.get_mut(&session_id) {
                            preview.selected = value;
                        }
                        cx.notify();
                    }),
            );
        }
        if self
            .plan_wireframes
            .get(&session_id)
            .is_some_and(|preview| preview.pending)
        {
            header = header.child(
                div()
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("boss.plan_wireframe_updating")),
            );
        }
        header
    }

    #[track_caller]
    pub(super) fn render_plan_wireframe(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Div {
        use crate::ui::ActivationExt;
        let theme = Theme::current(cx);
        let preview = self.plan_wireframes.get(&session_id);
        let mut body = div().flex_1().min_h_0().min_w_0().flex().flex_col();
        match preview.and_then(|preview| preview.content.as_ref()) {
            Some(Ok(Some(wireframe))) if !wireframe.screens.is_empty() => {
                let index = preview
                    .map_or(0, |preview| preview.screen)
                    .min(wireframe.screens.len() - 1);
                let screen = &wireframe.screens[index];
                let screen_name = screen.name.clone();
                let screen_element = wireframe::wireframe_screen_element(screen, &theme);
                let count = wireframe.screens.len();
                let mut picker = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .p(px(8.0));
                // Only one screen is rendered; navigation stays bounded even for large documents.
                for (id, label, next) in [
                    (
                        "plan-wireframe-previous",
                        tr!("boss.plan_wireframe_previous"),
                        index.saturating_sub(1),
                    ),
                    (
                        "plan-wireframe-next",
                        tr!("boss.plan_wireframe_next"),
                        (index + 1).min(count - 1),
                    ),
                ] {
                    let focus = self.transcript_control_focus(id, cx);
                    picker = picker.child(
                        div()
                            .id(id)
                            .track_focus(&focus)
                            .tab_index(0)
                            .px(px(8.0))
                            .py(px(5.0))
                            .rounded(px(8.0))
                            .text_size(sp(12.0))
                            .text_color(theme.text)
                            .focus_visible(|style| style.bg(theme.focus_highlight()))
                            .child(label)
                            .on_activation(cx, move |this, _, cx| {
                                if let Some(preview) = this.plan_wireframes.get_mut(&session_id) {
                                    preview.screen = next;
                                }
                                this.plan_wireframe_scroll_handle
                                    .set_offset(point(px(0.0), px(0.0)));
                                cx.notify();
                            }),
                    );
                }
                picker = picker.child(
                    div()
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(format!("{} · {}/{}", screen_name, index + 1, count)),
                );
                let focus = self.transcript_control_focus("plan-wireframe-screen", cx);
                let scroll = self.plan_wireframe_scroll_handle.clone();
                body = body.child(picker).child(
                    div()
                        .id("plan-wireframe-screen")
                        .track_focus(&focus)
                        .tab_index(0)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.modifiers.modified() {
                                return;
                            }
                            let mut offset = scroll.offset();
                            match event.keystroke.key.as_str() {
                                "left" => offset.x += px(40.0),
                                "right" => offset.x -= px(40.0),
                                "up" => offset.y += px(40.0),
                                "down" => offset.y -= px(40.0),
                                "pageup" => offset.y += px(400.0),
                                "pagedown" => offset.y -= px(400.0),
                                "home" => offset = point(px(0.0), px(0.0)),
                                "end" => offset.y = -scroll.max_offset().y,
                                _ => return,
                            }
                            scroll.set_offset(offset);
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .overflow_scroll()
                        .track_scroll(&self.plan_wireframe_scroll_handle)
                        .p(px(16.0))
                        .child(screen_element),
                );
            }
            content => {
                let (message, color) = match content {
                    Some(Err(error)) => (error.clone(), theme.danger),
                    Some(Ok(Some(_))) => (tr!("files.wireframe_empty"), theme.text_tertiary),
                    Some(Ok(None)) => (tr!("boss.plan_wireframe_absent"), theme.text_tertiary),
                    None => (tr!("files.loading"), theme.text_tertiary),
                };
                body = body
                    .items_center()
                    .justify_center()
                    .p(px(24.0))
                    .text_size(sp(12.5))
                    .text_color(color)
                    .child(message);
            }
        }
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_client::boss::BossFile;

    const PATH: &str = "plans/ui.wireframe.json";

    fn listing(present: bool) -> BossResult {
        BossResult::Files {
            files: if present {
                vec![BossFile {
                    path: PATH.into(),
                    directory: false,
                }]
            } else {
                vec![]
            },
        }
    }

    fn read_content(content: &str) -> Result<Option<Wireframe>, String> {
        read(PATH, |op| match op {
            BossOperation::ListFiles { path } => {
                assert_eq!(path, "plans");
                Ok(listing(true))
            }
            BossOperation::ReadFile { path } => {
                assert_eq!(path, PATH);
                Ok(BossResult::File {
                    path,
                    content: content.into(),
                })
            }
            _ => unreachable!(),
        })
    }

    #[test]
    fn companion_contract_valid_empty_invalid_and_unsupported() {
        assert_eq!(
            companion_path("plans/nested/ui.md").as_deref(),
            Some("plans/nested/ui.wireframe.json")
        );
        assert_eq!(companion_path("plans/ui.json"), None);
        let fixture = include_str!(
            "../../crates/waku-protocol/tests/fixtures/boss-brain-redesign.wireframe.json"
        );
        assert!(!read_content(fixture).unwrap().unwrap().screens.is_empty());
        assert!(
            read_content(r#"{"version":1,"name":"Empty","screens":[]}"#)
                .unwrap()
                .unwrap()
                .screens
                .is_empty()
        );
        assert!(read_content("").is_err());
        assert!(read_content("{broken").is_err());
        assert!(
            read_content(r#"{"version":99,"name":"Future","screens":[]}"#)
                .unwrap_err()
                .contains("99")
        );
        assert!(read_content(r#"{"version":1,"name":"Unknown","screens":[{"name":"s","width":20,"height":20,"root":{"type":"unknown"}}]}"#).unwrap_err().contains("unknown"));
    }

    #[test]
    fn absent_and_removed_companions_are_optional_but_read_errors_are_visible() {
        assert!(read(PATH, |_| Ok(listing(false))).unwrap().is_none());
        let mut replies =
            [Ok(listing(true)), Err("removed".into()), Ok(listing(false))].into_iter();
        assert!(read(PATH, |_| replies.next().unwrap()).unwrap().is_none());
        let mut replies = [
            Ok(listing(true)),
            Err("permission denied".into()),
            Ok(listing(true)),
        ]
        .into_iter();
        assert_eq!(
            read(PATH, |_| replies.next().unwrap()).unwrap_err(),
            "permission denied"
        );
        assert_eq!(
            read(PATH, |_| Err("offline".into())).unwrap_err(),
            "offline"
        );
    }

    #[test]
    fn stale_revision_host_document_and_generation_cannot_land() {
        let request = Request {
            key: waku_client::DaemonKey::Local,
            revision: 1,
            path: PATH.into(),
            generation: Uuid::new_v4(),
        };
        let mut preview = Preview {
            request: request.clone(),
            pending: true,
            content: None,
            selected: false,
            screen: 0,
        };
        assert!(preview.accepts(&request));
        let mut stale = request.clone();
        stale.revision = 0;
        assert!(!preview.accepts(&stale));
        stale = request.clone();
        stale.key = waku_client::DaemonKey::Remote(Uuid::new_v4());
        assert!(!preview.accepts(&stale));
        stale = request.clone();
        stale.path = "plans/other.wireframe.json".into();
        assert!(!preview.accepts(&stale));
        stale = request.clone();
        stale.generation = Uuid::new_v4();
        assert!(!preview.accepts(&stale));
        // A newer removal wins; an older valid response must never resurrect the view.
        let older = request.clone();
        preview.request.revision = 2;
        let latest = preview.request.clone();
        assert!(preview.finish(&latest, Ok(None)));
        assert!(!preview.finish(&older, Ok(Some(Wireframe::default()))));
        assert!(matches!(preview.content, Some(Ok(None))));
        assert!(!preview.accepts(&latest));

        // Retargeting away and back creates a new generation even at the same revision.
        preview.pending = true;
        preview.request.generation = Uuid::new_v4();
        assert!(!preview.finish(&latest, Err("old read error".into())));
        assert!(matches!(preview.content, Some(Ok(None))));
    }

    #[test]
    fn experiment_off_hides_every_state_without_modifying_content() {
        let mut preview = Preview {
            request: Request {
                key: waku_client::DaemonKey::Local,
                revision: 1,
                path: PATH.into(),
                generation: Uuid::new_v4(),
            },
            pending: false,
            content: None,
            selected: true,
            screen: 0,
        };
        for content in [
            None,
            Some(Ok(None)),
            Some(Err("invalid".into())),
            Some(Ok(Some(Wireframe {
                version: 1,
                name: "Saved".into(),
                screens: vec![],
            }))),
        ] {
            preview.content = content;
            assert!(!preview.visible(false));
            assert_eq!(
                preview.visible(true),
                !matches!(preview.content, None | Some(Ok(None)))
            );
        }
        assert_eq!(
            preview
                .content
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .name,
            "Saved"
        );
    }
}
