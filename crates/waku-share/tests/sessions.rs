// Session sharing over the friends channel — local endpoints, no network.
// Alice shares a project with session sharing on; Bob lists and watches.
use std::sync::Arc;

use parking_lot::Mutex;
use uuid::Uuid;

use waku_protocol::friends::SharedSessionSummary;
use waku_protocol::model::{AgentSession, ProviderKind, SessionStatus};
use waku_protocol::{SequencedEvent, WireDriverEvent};
use waku_share::friends::{
    self, FriendStore, FriendsMessage, FriendsProtocol, RequestDecision, SessionFeed,
};
use waku_share::{RelayMode, ShareNode};

const ORIGIN: &str = "git@github.com:org/repo.git";

fn accept_all() -> friends::RequestHandler {
    Arc::new(|_id, name| {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = tx.send(RequestDecision::Accept { our_name: name });
        rx
    })
}

fn friend_store() -> Arc<Mutex<FriendStore>> {
    Arc::new(Mutex::new(FriendStore::default()))
}

fn summary(session: &AgentSession) -> SharedSessionSummary {
    SharedSessionSummary {
        session_id: session.id,
        title: session.title.clone(),
        auto_title: session.auto_title.clone(),
        status: session.status,
        created_at: session.created_at,
        last_reply_at: session.last_reply_at,
    }
}

async fn spawn_node(
    dir: &std::path::Path,
    store: Arc<Mutex<FriendStore>>,
    on_list: friends::SessionListHandler,
    on_subscribe: friends::SessionSubscribeHandler,
) -> ShareNode {
    let proto = FriendsProtocol::new(
        accept_all(),
        Arc::new(|_offer| {}),
        Arc::new(|_id, _ticket| {}),
        store,
    )
    .with_session_handlers(on_list, on_subscribe);
    let secret = waku_share::identity::load_or_create(dir).unwrap();
    ShareNode::spawn(dir, secret, RelayMode::Disabled, proto, None)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn session_list_denied_without_authorization() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let store_a = friend_store();
    let store_b = friend_store();

    // B declines every session request — nothing is shared.
    let node_a = spawn_node(
        dir_a.path(),
        store_a.clone(),
        Arc::new(|_peer, _origin| None),
        Arc::new(|_peer, _origin, _session, _resume| None),
    )
    .await;
    let node_b = spawn_node(
        dir_b.path(),
        store_b.clone(),
        Arc::new(|_peer, _origin| None),
        Arc::new(|_peer, _origin, _session, _resume| None),
    )
    .await;

    friends::send_friend_request(node_a.endpoint(), node_b.addr(), "alice", &store_a)
        .await
        .unwrap();

    let result = friends::fetch_session_list(node_a.endpoint(), node_b.addr().id, ORIGIN).await;
    assert!(result.is_err(), "denied list should error, got {result:?}");

    let result = friends::subscribe_session(
        node_a.endpoint(),
        node_b.addr().id,
        ORIGIN,
        Uuid::new_v4(),
        None,
    )
    .await;
    assert!(result.is_err(), "denied subscribe should error");

    node_a.shutdown().await.unwrap();
    node_b.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn session_list_and_live_tail_flow_to_friend() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let store_a = friend_store();
    let store_b = friend_store();

    let session = AgentSession::new(Uuid::new_v4(), ProviderKind::Claude);
    let session_id = session.id;
    let summaries = vec![summary(&session)];
    let (feed_tx, feed_rx) = tokio::sync::mpsc::channel(8);
    let feed_rx = Mutex::new(Some(feed_rx));

    // A is the sharer: lists its session and serves the subscription.
    let node_a = spawn_node(
        dir_a.path(),
        store_a.clone(),
        Arc::new(move |_peer, origin| (origin == ORIGIN).then(|| summaries.clone())),
        Arc::new(move |_peer, _origin, requested, _resume| {
            if requested != session_id {
                return None;
            }
            Some(SessionFeed {
                session: session.clone(),
                events: feed_rx.lock().take()?,
            })
        }),
    )
    .await;
    let node_b = spawn_node(
        dir_b.path(),
        store_b.clone(),
        Arc::new(|_peer, _origin| None),
        Arc::new(|_peer, _origin, _session, _resume| None),
    )
    .await;

    friends::send_friend_request(node_b.endpoint(), node_a.addr(), "bob", &store_b)
        .await
        .unwrap();

    let sessions = friends::fetch_session_list(node_b.endpoint(), node_a.addr().id, ORIGIN).await;
    let sessions = sessions.expect("session list failed");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, session_id);
    assert_eq!(sessions[0].status, SessionStatus::Idle);

    let (snapshot, conn, mut recv) = friends::subscribe_session(
        node_b.endpoint(),
        node_a.addr().id,
        ORIGIN,
        session_id,
        None,
    )
    .await
    .expect("subscribe failed");
    assert_eq!(snapshot.id, session_id);

    // One live event, then revocation — the viewer must see both in order.
    let event = SequencedEvent {
        session_id,
        runtime_id: Uuid::new_v4(),
        epoch: Uuid::new_v4(),
        sequence: 1,
        event: WireDriverEvent::new("turnStarted", serde_json::json!({})),
    };
    feed_tx
        .send(FriendsMessage::SessionEvent {
            event: Box::new(event.clone()),
        })
        .await
        .unwrap();
    feed_tx.send(FriendsMessage::SharingRevoked).await.unwrap();
    drop(feed_tx);

    match friends::read_session_frame(&mut recv).await.unwrap() {
        FriendsMessage::SessionEvent { event: got } => {
            assert_eq!(got.sequence, 1);
            assert_eq!(got.event.kind, "turnStarted");
        }
        other => panic!("expected SessionEvent, got {other:?}"),
    }
    match friends::read_session_frame(&mut recv).await.unwrap() {
        FriendsMessage::SharingRevoked => {}
        other => panic!("expected SharingRevoked, got {other:?}"),
    }

    drop(conn);
    node_a.shutdown().await.unwrap();
    node_b.shutdown().await.unwrap();
}
