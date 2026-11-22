use super::*;
use futures::future::{BoxFuture, FutureExt, Shared};
use waku_protocol::transcript::{
    TranscriptWindow, TranscriptWindowRequest, TranscriptWindowTarget,
};

const HOVER_DELAY: Duration = Duration::from_millis(200);
const CACHE_CAPACITY: usize = 3;

type PreloadRead = Shared<BoxFuture<'static, Option<Arc<PreloadedWindow>>>>;

#[derive(Clone)]
pub(super) struct PreloadedWindow {
    window: TranscriptWindow,
    serialized_bytes: usize,
}

struct RunningPreload {
    session_id: Uuid,
    read: PreloadRead,
    claimed: bool,
}

/// Speculation owns data only: it never hydrates a session or attaches a runtime.
#[derive(Default)]
pub(super) struct TranscriptPreloads {
    hovered: Option<Uuid>,
    hover_timer: Option<gpui::Task<()>>,
    running: Option<RunningPreload>,
    cache: VecDeque<Arc<PreloadedWindow>>,
    reads: u64,
    hits: u64,
    unused_bytes: usize,
}

impl TranscriptPreloads {
    pub(super) fn cancel_hover(&mut self) {
        self.hovered = None;
        self.hover_timer = None;
    }

    fn contains(&self, session_id: Uuid) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.session_id == session_id)
            || self.cache.iter().any(|p| p.window.session_id == session_id)
    }

    /// A foreground read joins speculation rather than issuing duplicate I/O.
    pub(super) fn take(&mut self, session_id: Uuid) -> Option<PreloadRead> {
        let read = if let Some(running) = self.running.as_mut()
            && running.session_id == session_id
        {
            running.claimed = true;
            running.read.clone()
        } else {
            let index = self
                .cache
                .iter()
                .position(|p| p.window.session_id == session_id)?;
            futures::future::ready(self.cache.remove(index))
                .boxed()
                .shared()
        };
        self.hits += 1;
        Some(read)
    }

    fn cache(&mut self, preload: Arc<PreloadedWindow>) {
        if self.cache.len() == CACHE_CAPACITY
            && let Some(unused) = self.cache.pop_front()
        {
            self.unused_bytes = self.unused_bytes.saturating_add(unused.serialized_bytes);
        }
        self.cache.push_back(preload);
        self.log_stats();
    }

    pub(super) fn log_stats(&self) {
        eprintln!(
            "transcript preload: reads={} hits={} evicted_unused_bytes={} cached={}",
            self.reads,
            self.hits,
            self.unused_bytes,
            self.cache.len(),
        );
    }
}

/// The same request shape for speculation, cold opens, and revision validation.
pub(super) fn read_latest_window(
    client: &waku_client::DaemonClient,
    session_id: Uuid,
    cached: Option<Arc<PreloadedWindow>>,
) -> anyhow::Result<TranscriptWindow> {
    let reply = client.read_transcript_window(TranscriptWindowRequest {
        session_id,
        target: TranscriptWindowTarget::Latest,
        max_entries: None,
        max_bytes: None,
        known_revision: cached.as_ref().map(|p| p.window.revision),
    })?;
    Ok(resolve_window_reply(session_id, cached, reply)?)
}

fn resolve_window_reply(
    session_id: Uuid,
    cached: Option<Arc<PreloadedWindow>>,
    reply: TranscriptWindow,
) -> std::io::Result<TranscriptWindow> {
    if reply.session_id != session_id || reply.invalid_cursor {
        return Err(std::io::Error::other("invalid latest transcript window"));
    }
    if !reply.unchanged {
        return Ok(reply);
    }
    let Some(cached) = cached.filter(|p| p.window.revision == reply.revision) else {
        return Err(std::io::Error::other(
            "transcript revision has no cached window",
        ));
    };
    let mut window = Arc::unwrap_or_clone(cached).window;
    // These control fields accompany even an unchanged content revision.
    window.queued_messages = reply.queued_messages;
    window.event_cursor = reply.event_cursor;
    Ok(window)
}

impl Waku {
    pub(super) fn set_transcript_preload_hover(
        &mut self,
        session_id: Uuid,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if !hovered {
            if self.transcript_preloads.hovered == Some(session_id) {
                self.transcript_preloads.cancel_hover();
            }
            return;
        }
        self.transcript_preloads.cancel_hover();
        self.transcript_preloads.hovered = Some(session_id);
        let scroll_offset = self.sidebar_list_state.scroll_px_offset_for_scrollbar();
        self.transcript_preloads.hover_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.transcript_preloads.hovered == Some(session_id)
                    && this.sidebar_list_state.scroll_px_offset_for_scrollbar() == scroll_offset
                {
                    this.start_transcript_preload(session_id, cx);
                }
            });
        }));
    }

    fn start_transcript_preload(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.transcript_preloads.running.is_some()
            || self.transcript_preloads.contains(session_id)
            || self.session_hydrations.contains(session_id)
            || self.state.selected_session == Some(session_id)
            || !self.state.sessions.iter().any(|session| {
                session.id == session_id
                    && (!session.detail_loaded || self.stale_task_details.contains(&session_id))
            })
        {
            return;
        }
        // Looking up an existing connection is safe; hover must never start one.
        let Some(daemon) = self.daemon_for_session(session_id) else {
            return;
        };
        let client = daemon.client();
        if !client.transcript_windows_available() {
            return;
        }
        let read = cx
            .background_executor()
            .spawn(async move {
                // No legacy full-detail fallback for speculative work.
                let window = read_latest_window(&client, session_id, None).ok()?;
                let serialized_bytes = serde_json::to_vec(&window).ok()?.len();
                Some(Arc::new(PreloadedWindow {
                    window,
                    serialized_bytes,
                }))
            })
            .boxed()
            .shared();
        self.transcript_preloads.reads += 1;
        self.transcript_preloads.running = Some(RunningPreload {
            session_id,
            read: read.clone(),
            claimed: false,
        });
        cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |this, _| {
                let Some(running) = this.transcript_preloads.running.take() else {
                    return;
                };
                if !running.claimed
                    && this.state.sessions.iter().any(|s| {
                        s.id == session_id
                            && (!s.detail_loaded || this.stale_task_details.contains(&session_id))
                    })
                    && let Some(preload) = result
                {
                    this.transcript_preloads.cache(preload);
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(session_id: Uuid, revision: u64) -> TranscriptWindow {
        serde_json::from_value(serde_json::json!({
            "sessionId": session_id,
            "revision": revision,
            "entries": [],
            "messageCount": 7,
            "entryCount": 7,
        }))
        .unwrap()
    }

    fn preload(session_id: Uuid) -> Arc<PreloadedWindow> {
        Arc::new(PreloadedWindow {
            window: window(session_id, 1),
            serialized_bytes: 100,
        })
    }

    #[test]
    fn transcript_preload_cache_evicts_unused_data_and_consumes_hits() {
        let ids = [
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        let mut preloads = TranscriptPreloads::default();
        for id in ids {
            preloads.cache(preload(id));
        }
        assert_eq!(preloads.cache.len(), CACHE_CAPACITY);
        assert!(!preloads.contains(ids[0]));
        assert_eq!(preloads.unused_bytes, 100);
        let hit = futures::executor::block_on(preloads.take(ids[1]).unwrap()).unwrap();
        assert_eq!(hit.window.session_id, ids[1]);
        assert!(!preloads.contains(ids[1]));
        assert_eq!(preloads.hits, 1);
    }

    #[test]
    fn transcript_preload_foreground_joins_the_same_in_flight_read() {
        let id = Uuid::new_v4();
        let (sender, receiver) = futures::channel::oneshot::channel();
        let read = async move { receiver.await.ok() }.boxed().shared();
        let mut preloads = TranscriptPreloads {
            running: Some(RunningPreload {
                session_id: id,
                read: read.clone(),
                claimed: false,
            }),
            ..Default::default()
        };
        let foreground = preloads.take(id).unwrap();
        assert!(preloads.take(Uuid::new_v4()).is_none());
        assert!(preloads.running.as_ref().unwrap().claimed);
        sender.send(preload(id)).ok().unwrap();
        let (hover, click) = futures::executor::block_on(futures::future::join(read, foreground));
        assert!(Arc::ptr_eq(&hover.unwrap(), &click.unwrap()));
    }

    #[test]
    fn transcript_preload_revision_check_preserves_content_and_refreshes_controls() {
        let id = Uuid::new_v4();
        let cached = preload(id);
        let mut reply = window(id, 1);
        reply.unchanged = true;
        reply.message_count = 0;
        reply.event_cursor = Some(RuntimeEventCursor {
            runtime_id: Uuid::new_v4(),
            epoch: Uuid::new_v4(),
            sequence: 5,
        });
        let cursor = reply.event_cursor;
        let resolved = resolve_window_reply(id, Some(cached.clone()), reply).unwrap();
        assert_eq!(resolved.message_count, 7);
        assert_eq!(resolved.event_cursor, cursor);
        assert!(!resolved.unchanged);

        let mut newer = window(id, 2);
        newer.message_count = 9;
        let resolved = resolve_window_reply(id, Some(cached), newer).unwrap();
        assert_eq!(resolved.revision, 2);
        assert_eq!(resolved.message_count, 9);

        let mut invalid = window(id, 1);
        invalid.unchanged = true;
        assert!(resolve_window_reply(id, None, invalid.clone()).is_err());
        invalid.revision = 2;
        assert!(resolve_window_reply(id, Some(preload(id)), invalid).is_err());
        assert!(resolve_window_reply(Uuid::new_v4(), None, window(id, 1)).is_err());
    }
}
