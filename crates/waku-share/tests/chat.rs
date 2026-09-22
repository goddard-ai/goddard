// Chat messages over the friends channel — local endpoints, no network.
// A friend's message fires the receiver's chat handler and acks; a
// stranger's is dropped with no reply.
use std::sync::Arc;

use parking_lot::Mutex;

use waku_share::friends::{self, FriendStore, FriendsProtocol, RequestDecision};
use waku_share::{RelayMode, ShareNode};

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

async fn spawn_node(
    dir: &std::path::Path,
    store: Arc<Mutex<FriendStore>>,
    on_chat: friends::ChatHandler,
) -> ShareNode {
    let proto = FriendsProtocol::new(
        accept_all(),
        Arc::new(|_offer| {}),
        Arc::new(|_id, _ticket| {}),
        store,
    )
    .with_chat_handler(on_chat);
    let secret = waku_share::identity::load_or_create(dir).unwrap();
    ShareNode::spawn(dir, secret, RelayMode::Disabled, proto, None)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_delivers_name_and_text_to_friend() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let store_a = friend_store();
    let store_b = friend_store();

    let node_a = spawn_node(
        dir_a.path(),
        store_a.clone(),
        Arc::new(|_id, _name, _text| {}),
    )
    .await;
    let (tx, rx) = std::sync::mpsc::channel();
    let node_b = spawn_node(
        dir_b.path(),
        store_b.clone(),
        Arc::new(move |id, name, text| {
            let _ = tx.send((id, name, text));
        }),
    )
    .await;

    friends::send_friend_request(node_a.endpoint(), node_b.addr(), "alice", &store_a)
        .await
        .unwrap();

    friends::send_chat(
        node_a.endpoint(),
        node_b.addr(),
        "alice",
        "shipping the update tonight",
    )
    .await
    .expect("chat send failed");

    let (from, name, text) = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("chat handler never fired");
    assert_eq!(from, node_a.endpoint().id());
    assert_eq!(name, "alice");
    assert_eq!(text, "shipping the update tonight");

    node_a.shutdown().await.unwrap();
    node_b.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_from_stranger_is_dropped() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let node_a = spawn_node(
        dir_a.path(),
        friend_store(),
        Arc::new(|_id, _name, _text| {}),
    )
    .await;
    let (tx, rx) = std::sync::mpsc::channel();
    let node_b = spawn_node(
        dir_b.path(),
        friend_store(),
        Arc::new(move |id, name, text| {
            let _ = tx.send((id, name, text));
        }),
    )
    .await;

    // No pairing — B closes the stream without acking and never fires
    // the handler, so the send surfaces as an error.
    let result = friends::send_chat(node_a.endpoint(), node_b.addr(), "alice", "hi").await;
    assert!(
        result.is_err(),
        "a stranger's chat should error, got {result:?}"
    );
    assert!(
        rx.recv_timeout(std::time::Duration::from_secs(2)).is_err(),
        "a stranger's chat must not reach the handler"
    );

    node_a.shutdown().await.unwrap();
    node_b.shutdown().await.unwrap();
}
