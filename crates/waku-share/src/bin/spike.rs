//! Spike: two iroh endpoints in one process exchange a file over local
//! addresses (relay disabled), then run the friend-request handshake.
//! Proves both paths end to end.
//!
//!   cargo run -p waku-share --bin spike

use std::io::Write as _;
use std::sync::Arc;

use iroh::{Endpoint, RelayMode, endpoint::presets};
use iroh::protocol::Router;
use tokio::sync::Mutex;
use waku_share::friends::{
    FriendStore, FriendsProtocol, RequestDecision, send_friend_request,
};
use waku_share::identity;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base = std::env::temp_dir().join(format!("waku-share-spike-{}", std::process::id()));
    let send_dir = base.join("send");
    let recv_dir = base.join("recv");
    std::fs::create_dir_all(&send_dir)?;
    std::fs::create_dir_all(&recv_dir)?;

    // ---- part 1: file transfer ------------------------------------------
    let payload = "hello from goddard\n".repeat(100_000); // ~1.9 MB
    let file_path = send_dir.join("hello.txt");
    let mut f = std::fs::File::create(&file_path)?;
    f.write_all(payload.as_bytes())?;
    drop(f);

    let provider =
        waku_share::Provider::start(&file_path, &send_dir.join("blobs"), RelayMode::Disabled)
            .await?;
    let ticket = provider.ticket();
    println!("serving {} as ticket {}", file_path.display(), ticket);

    let hash = waku_share::fetch(&ticket, &recv_dir.join("blobs"), RelayMode::Disabled).await?;
    println!("fetched hash {hash}");

    let out = recv_dir.join("hello.txt");
    waku_share::export_blob(&recv_dir.join("blobs"), hash, &out).await?;

    let got = std::fs::read(&out)?;
    assert_eq!(got, payload.as_bytes(), "received bytes differ");
    println!("verified: {} bytes at {} — OK", got.len(), out.display());
    provider.shutdown().await?;

    // ---- part 2: friend handshake ----------------------------------------
    let alice_dir = base.join("alice");
    let bob_dir = base.join("bob");
    std::fs::create_dir_all(&alice_dir)?;
    std::fs::create_dir_all(&bob_dir)?;

    let alice_key = identity::load_or_create(&alice_dir)?;
    let bob_key = identity::load_or_create(&bob_dir)?;
    println!(
        "alice code: {}\nbob code:   {}",
        identity::friend_code(alice_key.public()),
        identity::friend_code(bob_key.public())
    );

    let alice_store = Arc::new(Mutex::new(FriendStore::load(&alice_dir)?));
    let bob_store = Arc::new(Mutex::new(FriendStore::load(&bob_dir)?));

    // Bob auto-accepts requests addressed to him (stand-in for the UI card).
    let bob_ep = Endpoint::builder(presets::N0)
        .secret_key(bob_key)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    let bob_router = Router::builder(bob_ep)
        .accept(
            waku_share::friends::ALPN_FRIENDS,
            FriendsProtocol::new(
                Arc::new(|_id, name| RequestDecision::Accept {
                    our_name: format!("bob (accepting {name})"),
                }),
                bob_store.clone(),
            ),
        )
        .spawn();

    let alice_ep = Endpoint::builder(presets::N0)
        .secret_key(alice_key)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;

    let bob_addr = bob_router.endpoint().addr();
    let parsed = identity::parse_friend_code(&identity::friend_code(bob_addr.id))?;
    assert_eq!(parsed, bob_addr.id);

    let their_name =
        send_friend_request(&alice_ep, bob_addr, "alice", &alice_store).await?;
    println!("alice friended: {their_name}");

    {
        let alice = alice_store.lock().await;
        assert!(alice.is_friend(&bob_router.endpoint().addr().id));
    }
    {
        let bob = bob_store.lock().await;
        assert!(bob.is_friend(&alice_ep.addr().id));
    }
    println!("both sides recorded the friendship — OK");

    alice_ep.close().await;
    bob_router.shutdown().await?;
    println!("spike done");
    Ok(())
}
