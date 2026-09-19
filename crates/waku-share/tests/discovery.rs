// E2E diagnostics for the friend transport: two endpoints (or share
// nodes) discovering and reaching each other over n0's public DNS +
// relays. Ignored — they need network access and are for manual runs:
// `cargo test -p waku-share --test discovery -- --ignored --nocapture`.
use iroh::endpoint::presets;
use iroh::Endpoint;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use waku_share::friends::{FriendStore, FriendsProtocol, RequestDecision};
use waku_share::{RelayMode, ShareNode};

const ALPN: &[u8] = b"waku-share/discovery-test/0";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs network access to n0 discovery + relays"]
async fn two_endpoints_discover_over_n0() {
    let a = Endpoint::builder(presets::N0).bind().await.unwrap();
    let b = Endpoint::builder(presets::N0).bind().await.unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(30), a.online())
        .await
        .expect("A never went online");
    tokio::time::timeout(std::time::Duration::from_secs(30), b.online())
        .await
        .expect("B never went online");

    eprintln!("A id={} addr={:?}", a.id(), a.addr());
    eprintln!("B id={} addr={:?}", b.id(), b.addr());

    let b_id = b.id();
    let accept = tokio::spawn(async move {
        let incoming = b.accept().await.expect("B saw no incoming connection");
        let conn = incoming.await.expect("B handshake failed");
        let (_send, _recv) = conn.accept_bi().await.unwrap();
        eprintln!("B accepted connection");
    });

    let mut last_err = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut connected = false;
    while std::time::Instant::now() < deadline {
        match a.connect(b_id, ALPN).await {
            Ok(conn) => {
                let _ = conn.open_bi().await;
                connected = true;
                break;
            }
            Err(error) => {
                last_err = Some(format!("{error:#}"));
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
    assert!(connected, "A never reached B: {:?}", last_err);
    accept.await.unwrap();
    a.close().await;
}

// Mirrors the daemon path exactly: two ShareNodes with the friends
// protocol installed, a friend request A -> B that B auto-accepts.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs network access to n0 discovery + relays"]
async fn share_nodes_friend_request_over_n0() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let store_a = Arc::new(Mutex::new(FriendStore::default()));
    let store_b = Arc::new(Mutex::new(FriendStore::default()));

    let proto_a = FriendsProtocol::new(
        Arc::new(|_peer, _name| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = tx.send(RequestDecision::Decline);
            rx
        }),
        Arc::new(|_offer| {}),
        Arc::new(|_peer, _ticket| {}),
        store_a.clone(),
    );
    let proto_b = FriendsProtocol::new(
        Arc::new(|_peer, _name| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = tx.send(RequestDecision::Accept {
                our_name: "B".to_string(),
            });
            rx
        }),
        Arc::new(|_offer| {}),
        Arc::new(|_peer, _ticket| {}),
        store_b.clone(),
    );

    let secret_a = waku_share::identity::load_or_create(dir_a.path()).unwrap();
    let secret_b = waku_share::identity::load_or_create(dir_b.path()).unwrap();

    let node_a = ShareNode::spawn(dir_a.path(), secret_a, RelayMode::Default, proto_a)
        .await
        .unwrap();
    let node_b = ShareNode::spawn(dir_b.path(), secret_b, RelayMode::Default, proto_b)
        .await
        .unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(30), node_b.wait_online())
        .await
        .expect("B never went online");
    let b_id = node_b.endpoint().id();
    eprintln!("B id={b_id} addr={:?}", node_b.addr());

    // The daemon does not await online() before dialing — the endpoint
    // connects to its relay lazily in the background.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut last_err = None;
    let mut peer_name = None;
    while std::time::Instant::now() < deadline {
        match waku_share::friends::send_friend_request(
            node_a.endpoint(),
            b_id,
            "A",
            &store_a,
        )
        .await
        {
            Ok(name) => {
                peer_name = Some(name);
                break;
            }
            Err(error) => {
                last_err = Some(format!("{error:#}"));
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
    assert_eq!(peer_name.as_deref(), Some("B"), "request failed: {last_err:?}");
    assert!(store_b.lock().is_friend(&node_a.endpoint().id()));
    node_a.shutdown().await.unwrap();
    node_b.shutdown().await.unwrap();
}

// Same as above, but each ShareNode lives on a dedicated thread with a
// current_thread runtime — the exact shape the daemon's share worker uses.
#[test]
#[ignore = "needs network access to n0 discovery + relays"]
fn share_nodes_on_current_thread_runtimes() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let path_a = dir_a.path().to_path_buf();
    let path_b = dir_b.path().to_path_buf();

    fn spawn_node(dir: PathBuf, accept: bool) -> Endpoint {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let store = Arc::new(Mutex::new(FriendStore::default()));
                let proto = FriendsProtocol::new(
                    Arc::new(move |_peer, _name| {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        let _ = tx.send(if accept {
                            RequestDecision::Accept {
                                our_name: "B".to_string(),
                            }
                        } else {
                            RequestDecision::Decline
                        });
                        rx
                    }),
                    Arc::new(|_offer| {}),
                    Arc::new(|_peer, _ticket| {}),
                    store,
                );
                let secret = waku_share::identity::load_or_create(&dir).unwrap();
                let node = ShareNode::spawn(&dir, secret, RelayMode::Default, proto)
                    .await
                    .unwrap();
                tx.send(node.endpoint().clone()).unwrap();
                // Park the runtime so endpoint background tasks keep running.
                std::future::pending::<()>().await;
            });
        });
        rx.recv().unwrap()
    }

    let ep_a = spawn_node(path_a, false);
    let ep_b = spawn_node(path_b, true);
    let b_id = ep_b.id();

    // Drive A's connect from a caller-side runtime; the endpoint's own
    // tasks live on its dedicated current_thread runtime.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let mut last_err;
        loop {
            match waku_share::friends::send_friend_request(
                &ep_a,
                b_id,
                "A",
                &Arc::new(Mutex::new(FriendStore::default())),
            )
            .await
            {
                Ok(name) => {
                    eprintln!("accepted by {name}");
                    return;
                }
                Err(error) => {
                    last_err = Some(format!("{error:#}"));
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
            }
            if std::time::Instant::now() > deadline {
                panic!("A never reached B: {last_err:?}");
            }
        }
    });
}
