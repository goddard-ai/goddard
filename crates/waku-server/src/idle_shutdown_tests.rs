use crate::{
    Backend, EventSink, ServerOptions,
    idle_shutdown::{IdleShutdownGate, WorkPermit},
    serve,
};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    net::{TcpListener, TcpStream},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tungstenite::{Message, WebSocket};
use uuid::Uuid;
use waku_protocol::{
    ClientMessage, Command, PROTOCOL_VERSION, Request, ResponsePayload, ServerMessage,
};

#[derive(Default)]
struct Owner {
    gate: Arc<IdleShutdownGate>,
    work: Mutex<HashMap<Uuid, WorkPermit>>,
}
impl Backend for Owner {
    fn handle(
        &self,
        request: Request,
        _: EventSink,
        _: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        match request.command {
            Command::AttachSession => {
                self.work
                    .lock()
                    .insert(request.session_id, self.gate.enter()?);
            }
            Command::Cancel => {
                self.work.lock().remove(&request.session_id);
            }
            _ => {}
        }
        Ok(ResponsePayload::Ack)
    }
    fn prepare_idle_shutdown(&self, owner: Uuid, request: Uuid) -> Option<Uuid> {
        self.gate.prepare(owner, request, || true)
    }
    fn commit_idle_shutdown(&self, owner: Uuid, fence: Uuid, signal: &dyn Fn()) -> bool {
        self.gate.commit(owner, fence, || true, signal)
    }
    fn release_idle_shutdown(&self, owner: Uuid, fence: Uuid) {
        self.gate.release(owner, fence);
    }
    fn disconnect_idle_shutdown(&self, owner: Uuid) {
        self.gate.disconnect(owner);
    }
}
fn send(socket: &mut WebSocket<TcpStream>, message: ClientMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(&message).unwrap().into(),
        ))
        .unwrap();
}
fn read(socket: &mut WebSocket<TcpStream>) -> ServerMessage {
    loop {
        if let Message::Text(text) = socket.read().unwrap() {
            return serde_json::from_str(&text).unwrap();
        }
    }
}
fn connect(address: std::net::SocketAddr) -> WebSocket<TcpStream> {
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (mut socket, _) = tungstenite::client(format!("ws://{address}/v1"), stream).unwrap();
    send(
        &mut socket,
        ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            token: "secret".into(),
            client_id: Uuid::new_v4(),
            resume_from: vec![],
        },
    );
    assert!(matches!(read(&mut socket), ServerMessage::Hello { .. }));
    socket
}
fn prepare(socket: &mut WebSocket<TcpStream>) -> Option<Uuid> {
    send(
        socket,
        ClientMessage::PrepareIdleShutdown {
            request_id: Uuid::new_v4(),
        },
    );
    match read(socket) {
        ServerMessage::IdleShutdown {
            fence,
            committed: false,
            ..
        } => fence,
        other => panic!("{other:?}"),
    }
}

#[test]
fn idle_shutdown_wire_preserves_75_sessions_fences_new_work_and_recovers_disconnect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let backend = Arc::new(Owner::default());
    let server_backend = backend.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            server_backend,
            shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..Default::default()
            },
        )
        .unwrap()
    });
    let client = waku_client::DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let sessions = (0..75).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    for id in &sessions {
        client
            .request(*id, Uuid::nil(), Command::AttachSession)
            .unwrap();
    }
    let mut socket = connect(address);
    assert!(prepare(&mut socket).is_none());
    for id in &sessions[..74] {
        client.request(*id, Uuid::nil(), Command::Cancel).unwrap();
    }
    assert!(prepare(&mut socket).is_none());
    client
        .request(sessions[74], Uuid::nil(), Command::Cancel)
        .unwrap();
    let old = prepare(&mut socket).unwrap();
    assert!(
        client
            .request(Uuid::new_v4(), Uuid::nil(), Command::AttachSession)
            .is_err()
    );
    socket.close(None).unwrap();
    let _ = socket.read(); // closing handshake runs the connection's release guard
    let mut socket = connect(address);
    let token = prepare(&mut socket).unwrap();
    assert_ne!(old, token);
    send(
        &mut socket,
        ClientMessage::CommitIdleShutdown {
            request_id: Uuid::new_v4(),
            fence: old,
        },
    );
    assert!(matches!(
        read(&mut socket),
        ServerMessage::IdleShutdown {
            committed: false,
            ..
        }
    ));
    send(
        &mut socket,
        ClientMessage::CommitIdleShutdown {
            request_id: Uuid::new_v4(),
            fence: token,
        },
    );
    assert!(matches!(
        read(&mut socket),
        ServerMessage::IdleShutdown {
            committed: true,
            ..
        }
    ));
    server.join().unwrap();
    assert!(backend.gate.enter().is_err());
}
