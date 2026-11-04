//! Bounded live process output for viewers that join after output was emitted.
use std::collections::HashMap;
use waku_protocol::WireDriverEvent;
use waku_protocol::model::{
    BackgroundWorkEvent, BackgroundWorkItem, BackgroundWorkKey, BackgroundWorkKind,
    BackgroundWorkStatus,
};

const MAX_ITEMS: usize = 32;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// The desktop has one card of each kind. Keep those level signals even if
/// their original events have rolled out of the bounded replay journal.
#[derive(Default)]
pub(crate) struct TaskAttention {
    permission: Option<WireDriverEvent>,
    question: Option<WireDriverEvent>,
    daemon_request: Option<WireDriverEvent>,
}

impl TaskAttention {
    pub(crate) fn apply(&mut self, event: &WireDriverEvent) {
        let request_id = event
            .payload
            .get("requestId")
            .and_then(serde_json::Value::as_str);
        match event.kind.as_str() {
            "permission" if request_id.is_some_and(waku_protocol::is_daemon_request) => {
                self.daemon_request = Some(event.clone())
            }
            "permission" => self.permission = Some(event.clone()),
            "userInputRequested" => self.question = Some(event.clone()),
            "requestSettled" => {
                if let Some(id) = request_id {
                    self.resolve(id);
                }
            }
            "turnFinished" | "turnCancelled" => {
                self.permission = None;
                self.question = None;
            }
            _ => {}
        }
    }

    pub(crate) fn resolve(&mut self, request_id: &str) {
        for slot in [
            &mut self.permission,
            &mut self.question,
            &mut self.daemon_request,
        ] {
            if slot.as_ref().is_some_and(|event| {
                event
                    .payload
                    .get("requestId")
                    .and_then(serde_json::Value::as_str)
                    == Some(request_id)
            }) {
                *slot = None;
            }
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<WireDriverEvent> {
        [&self.permission, &self.question, &self.daemon_request]
            .into_iter()
            .filter_map(|event| event.clone())
            .collect()
    }
}

#[derive(Default)]
pub(crate) struct TaskOutput {
    items: HashMap<BackgroundWorkKey, BackgroundWorkItem>,
}

impl TaskOutput {
    pub(crate) fn apply(&mut self, event: &WireDriverEvent) {
        if event.kind != "backgroundWork" {
            return;
        }
        let Ok(event) = serde_json::from_value::<BackgroundWorkEvent>(event.payload.clone()) else {
            return;
        };
        match event {
            BackgroundWorkEvent::Upsert(item) => self.upsert(item),
            BackgroundWorkEvent::ReconcileProcesses { items } => self.reconcile(items, false),
            BackgroundWorkEvent::ReconcileLive { items } => self.reconcile(items, true),
            BackgroundWorkEvent::OutputDelta { key, delta } => {
                if let Some(item) = self.items.get_mut(&key) {
                    item.output.get_or_insert_with(String::new).push_str(&delta);
                    bound_output(item);
                }
            }
            BackgroundWorkEvent::StopRequested(key) => {
                if let Some(item) = self.items.get_mut(&key) {
                    item.status = BackgroundWorkStatus::Stopping;
                }
            }
            BackgroundWorkEvent::StopFailed {
                key,
                message,
                message_i18n,
            } => {
                if let Some(item) = self
                    .items
                    .get_mut(&key)
                    .filter(|item| item.status.is_live())
                {
                    item.status = if key.kind == BackgroundWorkKind::Monitor {
                        BackgroundWorkStatus::Monitoring
                    } else {
                        BackgroundWorkStatus::Running
                    };
                    item.detail = Some(message);
                    if let Some(i18n) = message_i18n {
                        item.detail = Some(i18n.render());
                    }
                }
            }
        }
    }

    fn reconcile(&mut self, items: Vec<BackgroundWorkItem>, all: bool) {
        for previous in self.items.values_mut() {
            if previous.status.is_live()
                && (all || previous.key.kind == BackgroundWorkKind::Process)
                && !items.iter().any(|item| item.key == previous.key)
            {
                previous.status = BackgroundWorkStatus::Lost;
                previous.can_stop = false;
            }
        }
        for item in items {
            self.upsert(item);
        }
    }

    fn upsert(&mut self, mut item: BackgroundWorkItem) {
        if item.output.is_none()
            && let Some(previous) = self.items.get(&item.key)
        {
            item.output.clone_from(&previous.output);
            item.output_truncated |= previous.output_truncated;
        }
        bound_output(&mut item);
        if !self.items.contains_key(&item.key) && self.items.len() == MAX_ITEMS {
            if let Some(oldest) = self
                .items
                .values()
                .min_by_key(|item| item.updated_at_ms)
                .map(|item| item.key.clone())
            {
                self.items.remove(&oldest);
            }
        }
        self.items.insert(item.key.clone(), item);
    }

    pub(crate) fn snapshot(&self) -> Vec<BackgroundWorkItem> {
        let mut items: Vec<_> = self.items.values().cloned().collect();
        items.sort_by_key(|item| item.started_at_ms);
        items
    }
}

fn bound_output(item: &mut BackgroundWorkItem) {
    let Some(output) = &mut item.output else {
        return;
    };
    if output.len() > MAX_OUTPUT_BYTES {
        // Keep headroom so a full log does not shift its buffer on every token.
        let mut start = output.len() - MAX_OUTPUT_BYTES / 2;
        while !output.is_char_boundary(start) {
            start += 1;
        }
        output.drain(..start);
        item.output_truncated = true;
    }
}

/// Background status remains global, but hidden viewers do not need log text.
pub(crate) fn without_output(mut event: WireDriverEvent) -> WireDriverEvent {
    if event.kind == "backgroundWork" {
        if event.payload.get("output").is_some() {
            event.payload["output"] = serde_json::Value::Null;
        }
        if let Some(items) = event
            .payload
            .get_mut("items")
            .and_then(|items| items.as_array_mut())
        {
            for item in items {
                item["output"] = serde_json::Value::Null;
            }
        }
    }
    event
}

/// Completion snapshots are a one-shot delivery, never replay-journal data.
pub(crate) fn without_settlement_snapshot(event: &WireDriverEvent) -> WireDriverEvent {
    let payload = match event.payload.as_object() {
        Some(payload) if payload.contains_key("taskSnapshot") => serde_json::Value::Object(
            payload
                .iter()
                .filter(|(key, _)| key.as_str() != "taskSnapshot")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
        _ => event.payload.clone(),
    };
    WireDriverEvent::new(event.kind.clone(), payload)
}
