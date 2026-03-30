use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
use tauri::{
    async_runtime::JoinHandle,
    plugin::{Builder, TauriPlugin},
    AppHandle, Emitter, Manager, Runtime, State,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

const STREAM_EVENT: &str = "ipc://message";

#[derive(Default)]
struct SubscriptionStore {
    next_id: AtomicU64,
    tasks: Mutex<HashMap<String, JoinHandle<()>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StreamMessage {
    subscription_id: String,
    socket_path: String,
    name: String,
    payload: serde_json::Value,
}

#[tauri::command]
async fn send(
    socket_path: String,
    name: String,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|error| error.to_string())?;

    let body = serde_json::json!({ "name": name, "payload": payload }).to_string();
    let request = format!(
        "POST / HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| error.to_string())?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .map_err(|error| error.to_string())?;

    let mut sections = response.splitn(2, "\r\n\r\n");
    let headers = sections.next().unwrap_or_default();
    let body = sections.next().unwrap_or_default();
    let status_line = headers.lines().next().unwrap_or_default();

    if !status_line.contains(" 200 ") {
        return Err(if body.is_empty() {
            format!("IPC request failed: {status_line}")
        } else {
            body.to_string()
        });
    }

    serde_json::from_str(body).map_err(|error| error.to_string())
}

#[tauri::command]
async fn subscribe<R: Runtime>(
    app: AppHandle<R>,
    subscriptions: State<'_, SubscriptionStore>,
    socket_path: String,
    name: String,
    subscription: Option<serde_json::Value>,
) -> Result<String, String> {
    let subscription_id = subscriptions
        .next_id
        .fetch_add(1, Ordering::Relaxed)
        .to_string();

    let task_subscription_id = subscription_id.clone();
    let task_socket_path = socket_path.clone();
    let task_name = name.clone();
    let task_subscription = subscription.clone();

    let task = tauri::async_runtime::spawn(async move {
        let mut stream = match UnixStream::connect(&task_socket_path).await {
            Ok(stream) => stream,
            Err(_) => return,
        };

        let query = if let Some(subscription) = task_subscription {
            match serde_json::to_string(&subscription) {
                Ok(raw_subscription) => format!(
                    "/stream?name={}&subscription={}",
                    urlencoding::encode(&task_name),
                    urlencoding::encode(&raw_subscription)
                ),
                Err(_) => return,
            }
        } else {
            format!("/stream?name={}", urlencoding::encode(&task_name))
        };

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            query
        );

        if stream.write_all(request.as_bytes()).await.is_err() {
            return;
        }

        let mut reader = BufReader::new(stream);
        let mut header_line = String::new();
        loop {
            header_line.clear();
            match reader.read_line(&mut header_line).await {
                Ok(0) | Err(_) => return,
                Ok(_) if header_line == "\r\n" => break,
                Ok(_) => {}
            }
        }

        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let payload = line.trim();
                    if payload.is_empty() {
                        continue;
                    }

                    let parsed = match serde_json::from_str::<serde_json::Value>(payload) {
                        Ok(parsed) => parsed,
                        Err(_) => continue,
                    };

                    let message_name = parsed
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let message_payload = parsed
                        .get("payload")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);

                    let _ = app.emit(
                        STREAM_EVENT,
                        StreamMessage {
                            subscription_id: task_subscription_id.clone(),
                            socket_path: task_socket_path.clone(),
                            name: message_name.to_string(),
                            payload: message_payload,
                        },
                    );
                }
            }
        }
    });

    subscriptions
        .tasks
        .lock()
        .expect("subscription store lock poisoned")
        .insert(subscription_id.clone(), task);

    Ok(subscription_id)
}

#[tauri::command]
async fn unsubscribe(
    subscriptions: State<'_, SubscriptionStore>,
    subscription_id: String,
) -> Result<(), String> {
    let task = subscriptions
        .tasks
        .lock()
        .expect("subscription store lock poisoned")
        .remove(&subscription_id);
    if let Some(task) = task {
        task.abort();
    }

    Ok(())
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("ipc")
        .invoke_handler(tauri::generate_handler![send, subscribe, unsubscribe])
        .setup(|app, _api| {
            app.manage(SubscriptionStore::default());
            Ok(())
        })
        .build()
}
