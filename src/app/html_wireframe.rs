//! HTML preview preparation runs after rendering and parses on the background
//! executor. The frame only reads a prepared browser entity or status.

use super::*;
use waku_protocol::wireframe_comments::{WireframeCommentOperation as Op, WireframeCommentThread};
use waku_protocol::wireframe_html::HtmlWireframe;

pub(super) struct Preview {
    pub generation: Uuid,
    pub path: String,
    pub document: Option<HtmlWireframe>,
    pub view: Option<Entity<BrowserView>>,
    pub error: Option<String>,
    pub theme: serde_json::Value,
    pub task: Uuid,
    pub threads: Vec<WireframeCommentThread>,
    pub requests: u64,
    in_flight: bool,
    refresh_pending: bool,
    pub was_visible: bool,
    queued: VecDeque<(Op, Option<String>)>,
}

#[derive(serde::Deserialize)]
struct Envelope {
    instance: Uuid,
    document: String,
    #[serde(flatten)]
    action: Action,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Action {
    Ready,
    Add {
        screen: String,
        anchor: Option<String>,
        body: String,
        key: String,
    },
    Reply {
        thread: Uuid,
        body: String,
        key: String,
    },
    Read {
        thread: Uuid,
        message: Uuid,
    },
    Resolve {
        thread: Uuid,
        resolved: bool,
    },
    Stage {
        thread: Uuid,
    },
}

fn theme_variables(theme: Theme) -> serde_json::Value {
    let color = |c: Hsla| {
        format!(
            "hsla({},{:.3}%,{:.3}%,{})",
            c.h * 360.0,
            c.s * 100.0,
            c.l * 100.0,
            c.a
        )
    };
    serde_json::json!({
        "--wf-canvas": color(theme.canvas),
        "--wf-surface": color(theme.surface),
        "--wf-inset": color(theme.inset),
        "--wf-text": color(theme.text),
        "--wf-secondary": color(theme.text_secondary),
        "--wf-border": color(theme.border_subtle),
        "--wf-info": color(theme.info),
        "--wf-accent": color(theme.accent),
        "--wf-color-scheme": if theme.is_dark { "dark" } else { "light" },
    })
}

/// JSON embedded in a script must not let authored strings close the tag.
fn script_json(value: &serde_json::Value) -> String {
    value
        .to_string()
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn shell(document: &HtmlWireframe, instance: Uuid, theme: &serde_json::Value) -> String {
    let nonce = Uuid::new_v4().simple().to_string();
    let config = script_json(&serde_json::json!({
        "instance": instance,
        "id": document.id(),
        "html": document.html(),
        "screens": document.screens(),
        "base": waku_protocol::wireframe_html::BASE_STYLES,
        "theme": theme,
    }));
    include_str!("html_wireframe/shell.html")
        .replace("{{css}}", include_str!("html_wireframe/shell.css"))
        .replace("{{controller}}", include_str!("html_wireframe/shell.js"))
        .replace("{{nonce}}", &nonce)
        // Authored strings are inserted last so they cannot expand template markers.
        .replace("{{config}}", &config)
}

impl Waku {
    pub(super) fn refresh_html_wireframe_comments(&mut self, cx: &mut Context<Self>) {
        if let Some(preview) = &self.html_wireframe_preview {
            let path = preview.path.clone();
            let generation = preview.generation;
            if self.html_wireframe_visible(&path) {
                self.request_wireframe_comments(path, generation, Op::List { offset: 0 }, None, cx);
            }
        }
    }

    pub(super) fn drain_wireframe_comment_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut refresh = false;
        while let Ok((key, update)) = self.wireframe_comment_events.try_recv() {
            if let Some(preview) = &self.html_wireframe_preview {
                if self.daemons.session_owner(preview.task) == key
                    && update.is_none_or(|(task, document)| {
                        task == preview.task
                            && preview
                                .document
                                .as_ref()
                                .is_some_and(|d| d.id() == document)
                    })
                {
                    refresh = true;
                }
            }
        }
        if refresh {
            self.refresh_html_wireframe_comments(cx);
        }
        refresh
    }
    pub(super) fn html_wireframe_visible(&self, path: &str) -> bool {
        if !self.state.wireframes_experiment_enabled {
            return false;
        }
        let Some(preview) = self
            .html_wireframe_preview
            .as_ref()
            .filter(|p| p.path == path)
        else {
            return false;
        };
        if !preview.task.is_nil() && self.composer_session_id() != Some(preview.task) {
            return false;
        }
        match self.active_right_panel_surface() {
            Some(RightPanelSurface::Plan { session_id, .. }) => self
                .plan_wireframes
                .get(session_id)
                .is_some_and(|p| p.selected && *session_id == preview.task),
            _ => {
                self.visible_right_panel_file_path().as_deref() == Some(path)
                    && self
                        .right_panel_file_editors
                        .get(path)
                        .is_some_and(|editor| !editor.show_source)
            }
        }
    }
    pub(super) fn html_preview_matches(&self, task: Uuid, path: &str) -> bool {
        self.html_wireframe_preview
            .as_ref()
            .is_some_and(|p| p.task == task && p.path == path)
    }

    pub(super) fn render_html_wireframe(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let task = self.composer_session_id().unwrap_or(Uuid::nil());
        let source = if !self.html_preview_matches(task, path) {
            self.right_panel_file_editors
                .get(path)
                .filter(|e| e.text_loaded)
                .map(|e| e.state.read(cx).content().to_owned())
        } else {
            None
        };
        self.render_html_document(path, task, source, window, cx)
    }

    pub(super) fn render_html_document(
        &mut self,
        path: &str,
        task: Uuid,
        source: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        return div().p(px(24.0)).child("HTML wireframe previews are available on macOS and Windows. Switch to source to read this document.");
        let variables = theme_variables(theme);
        if let Some(source) = source {
            let generation = Uuid::new_v4();
            self.html_wireframe_preview = Some(Preview {
                path: path.to_owned(),
                generation,
                task,
                document: None,
                view: None,
                error: None,
                theme: variables.clone(),
                threads: Vec::new(),
                requests: 0,
                in_flight: false,
                refresh_pending: false,
                was_visible: false,
                queued: VecDeque::new(),
            });
            let weak = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.prepare_html_wireframe(source, generation, cx)
                });
            });
        }
        if let Some(preview) = self
            .html_wireframe_preview
            .as_mut()
            .filter(|p| p.task == task && p.path == path)
        {
            if preview.theme != variables {
                preview.theme = variables.clone();
                if let Some(view) = preview.view.clone() {
                    let script = format!(
                        "window.goddardWireframe?.setTheme({});",
                        script_json(&variables)
                    );
                    window.on_next_frame(move |_, cx| view.read(cx).wireframe_script(&script));
                }
            }
            if let Some(view) = &preview.view {
                return div().flex_1().min_h_0().child(view.clone());
            }
            if let Some(error) = &preview.error {
                return div()
                    .p(px(24.0))
                    .text_color(theme.danger)
                    .child(error.clone());
            }
        }
        div()
            .p(px(24.0))
            .text_color(theme.text_secondary)
            .child(tr!("files.loading"))
    }

    fn prepare_html_wireframe(&mut self, source: String, generation: Uuid, cx: &mut Context<Self>) {
        let variables = theme_variables(Theme::current(cx));
        let window_handle = self.window_handle;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    HtmlWireframe::parse(&source)
                        .map(|document| {
                            let html = shell(&document, generation, &variables);
                            (document, html)
                        })
                        .map_err(|e| e.to_string())
                })
                .await;
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let Some(preview) = this.html_wireframe_preview.as_mut() else {
                        return;
                    };
                    if preview.generation != generation {
                        return;
                    }
                    match result {
                        Ok((document, html)) => {
                            let view = cx.new(|cx| BrowserView::new_wireframe(html, window, cx));
                            let subscribed_path = preview.path.clone();
                            cx.subscribe(
                                &view,
                                move |this, _, event: &crate::browser::WireframeEvent, cx| {
                                    this.html_wireframe_event(&subscribed_path, &event.0, cx);
                                },
                            )
                            .detach();
                            preview.document = Some(document);
                            preview.view = Some(view);
                        }
                        Err(error) => preview.error = Some(error),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn html_wireframe_event(&mut self, path: &str, message: &str, cx: &mut Context<Self>) {
        let Ok(envelope) = serde_json::from_str::<Envelope>(message) else {
            return;
        };
        let Some(preview) = self
            .html_wireframe_preview
            .as_ref()
            .filter(|p| p.path == path)
        else {
            return;
        };
        if preview.generation != envelope.instance
            || preview
                .document
                .as_ref()
                .is_none_or(|d| d.id() != envelope.document)
        {
            return;
        }
        if matches!(envelope.action, Action::Stage { .. }) {
            if self.composer_session_id() != Some(preview.task) {
                return;
            }
            if let Action::Stage { thread } = envelope.action {
                if let Some(thread) = preview.threads.iter().find(|t| t.id == thread) {
                    let revision = preview
                        .document
                        .as_ref()
                        .map_or("unknown", |d| d.revision());
                    let text = feedback_snapshot(path, &envelope.document, revision, thread);
                    if !self.composer.read(cx).content(cx).contains(&text) {
                        self.composer
                            .update(cx, |composer, cx| composer.insert_text(&text, cx));
                    }
                    cx.notify();
                }
            }
            return;
        }
        let generation = preview.generation;
        if matches!(envelope.action, Action::Ready) {
            if let Some(view) = &preview.view {
                view.read(cx).wireframe_script(&format!(
                    "window.goddardWireframe?.setTheme({});",
                    script_json(&preview.theme)
                ));
            }
        }
        let (operation, request) = match envelope.action {
            Action::Add {
                screen,
                anchor,
                body,
                key,
            } => (
                Op::Add {
                    screen,
                    anchor,
                    body,
                    key: key.clone(),
                },
                Some(key),
            ),
            Action::Reply { thread, body, key } => (
                Op::Reply {
                    thread,
                    body,
                    key: key.clone(),
                },
                Some(key),
            ),
            Action::Read { thread, message } => (Op::Read { thread, message }, None),
            Action::Resolve { thread, resolved } => (Op::Resolve { thread, resolved }, None),
            Action::Ready => (Op::List { offset: 0 }, None),
            Action::Stage { .. } => return,
        };
        self.request_wireframe_comments(path.to_owned(), generation, operation, request, cx);
    }

    fn request_wireframe_comments(
        &mut self,
        path: String,
        generation: Uuid,
        operation: Op,
        request: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(preview) = self
            .html_wireframe_preview
            .as_mut()
            .filter(|p| p.path == path)
        {
            if preview.in_flight {
                if matches!(operation, Op::List { .. }) {
                    preview.refresh_pending = true;
                } else {
                    preview.queued.push_back((operation, request));
                }
                return;
            }
        }
        let Some(preview) = self
            .html_wireframe_preview
            .as_ref()
            .filter(|p| p.path == path)
        else {
            return;
        };
        let task = preview.task;
        let Some(document) = preview.document.as_ref().map(|d| d.id().to_owned()) else {
            return;
        };
        let revision = preview.document.as_ref().map(|d| d.revision().to_owned());
        let Some(client) = self.daemon_for_session(task).map(|d| d.client()) else {
            if let Some(view) = &preview.view {
                view.read(cx).wireframe_script("window.goddardWireframe?.setError('Select a connected task to save comments.');");
            }
            return;
        };
        let sequence = if let Some(preview) = self
            .html_wireframe_preview
            .as_mut()
            .filter(|p| p.path == path)
        {
            preview.requests += 1;
            preview.requests
        } else {
            return;
        };
        if let Some(preview) = self
            .html_wireframe_preview
            .as_mut()
            .filter(|p| p.path == path)
        {
            preview.in_flight = true;
        }
        cx.spawn(async move |this, cx| {
            let request_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut threads = Vec::new();
                    let mut operation = operation;
                    loop {
                        match client.request(
                            task,
                            Uuid::nil(),
                            waku_client::Command::WireframeComments {
                                path: request_path.clone(),
                                document: Some(document.clone()),
                                revision: revision.clone(),
                                operation,
                            },
                        ) {
                            Ok(waku_client::ResponsePayload::WireframeComments { page }) => {
                                threads.extend(page.threads);
                                match page.next_offset {
                                    Some(offset) => operation = Op::List { offset },
                                    None => return Ok(threads),
                                }
                            }
                            Ok(_) => return Err("unexpected wireframe comment response".to_owned()),
                            Err(error) => return Err(error.to_string()),
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(preview) = this
                    .html_wireframe_preview
                    .as_mut()
                    .filter(|p| p.path == path)
                else {
                    return;
                };
                if preview.generation != generation || preview.requests != sequence {
                    return;
                }
                preview.in_flight = false;
                let queued = preview.queued.pop_front();
                let queued = queued.or_else(|| {
                    if std::mem::take(&mut preview.refresh_pending) {
                        Some((Op::List { offset: 0 }, None))
                    } else {
                        None
                    }
                });
                let Some(view) = &preview.view else {
                    return;
                };
                match result {
                    Ok(threads) => {
                        let script = format!(
                            "window.goddardWireframe?.setThreads({}, {});",
                            script_json(&serde_json::json!(threads)),
                            script_json(&serde_json::json!(request))
                        );
                        preview.threads = threads;
                        view.read(cx).wireframe_script(&script);
                    }
                    Err(error) => view.read(cx).wireframe_script(&format!(
                        "window.goddardWireframe?.setError({});",
                        script_json(&serde_json::json!(error))
                    )),
                }
                if let Some((operation, request)) = queued {
                    this.request_wireframe_comments(
                        path.clone(),
                        generation,
                        operation,
                        request,
                        cx,
                    );
                }
            });
        })
        .detach();
    }
}

fn feedback_snapshot(
    path: &str,
    document: &str,
    revision: &str,
    thread: &WireframeCommentThread,
) -> String {
    let mut text = format!(
        "\nWireframe feedback: {path}\nDocument: {document}\nPreview revision: {revision}\nScreen: {}\nAnchor: {} ({})\nThread: {}\nCreation revision: {}\n",
        thread.screen,
        thread.anchor.as_deref().unwrap_or("screen"),
        thread.label,
        thread.id,
        thread.revision
    );
    for message in &thread.messages {
        text.push_str(&format!(
            "\n{} (message {}):\n",
            message.author_label, message.id
        ));
        for line in message.body.lines() {
            text.push_str("> ");
            text.push_str(line);
            text.push('\n');
        }
    }
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_keeps_both_revisions_and_quotes_literal_discussion() {
        let thread: WireframeCommentThread = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "screen": "tasks", "anchor": "sidebar", "label": "Task navigation",
            "revision": "creation-revision", "resolved": false, "unread": false,
            "messages": [{ "id": Uuid::new_v4(), "authorId": "user", "authorLabel": "User", "createdAt": 1,
                "body": "Keep this visible\n<literal markup>" }]
        })).unwrap();
        let snapshot = feedback_snapshot(
            "design.wireframe.html",
            "task-review",
            "displayed-revision",
            &thread,
        );
        assert!(snapshot.contains("Preview revision: displayed-revision"));
        assert!(snapshot.contains("Creation revision: creation-revision"));
        assert!(snapshot.contains("> Keep this visible\n> <literal markup>\n"));
        assert_eq!(thread.messages.len(), 1);
    }

    #[test]
    fn authored_text_cannot_escape_trusted_script_or_frame() {
        let document = HtmlWireframe::parse(include_str!(
            "../../crates/waku-protocol/tests/fixtures/tasks.wireframe.html"
        ))
        .unwrap();
        let preview = shell(
            &document,
            Uuid::new_v4(),
            &serde_json::json!({"--wf-text": "black"}),
        );
        assert!(preview.contains("sandbox=\"allow-same-origin\""));
        assert!(!preview.contains("allow-scripts"));
        assert_eq!(preview.matches("</script>").count(), 1);
        assert_eq!(
            script_json(&serde_json::json!("</script><script>bad()")),
            "\"\\u003c/script>\\u003cscript>bad()\""
        );
    }
}
