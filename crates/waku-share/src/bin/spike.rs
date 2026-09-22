//! Spike: two ShareNodes in one process run the full friend flow over local
//! addresses (relay disabled): handshake → offer → auto-fetch → done.
//!
//!   cargo run -p waku-share --bin spike

use std::io::Write as _;
use std::sync::Arc;

use iroh::RelayMode;
use parking_lot::Mutex;
use waku_share::friends::{
    FriendStore, FriendsProtocol, OfferInfo, RequestDecision, send_friend_request,
};
use waku_share::{ShareNode, Ticket, identity};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base = std::env::temp_dir().join(format!("waku-share-spike-{}", std::process::id()));
    let alice_dir = base.join("alice");
    let bob_dir = base.join("bob");
    for d in [&alice_dir, &bob_dir] {
        std::fs::create_dir_all(d.join("blobs"))?;
        std::fs::create_dir_all(d.join("files"))?;
    }

    let payload = "hello from goddard\n".repeat(100_000); // ~1.9 MB
    let file_path = alice_dir.join("files/hello.txt");
    let mut f = std::fs::File::create(&file_path)?;
    f.write_all(payload.as_bytes())?;
    drop(f);

    let alice_key = identity::load_or_create(&alice_dir)?;
    let bob_key = identity::load_or_create(&bob_dir)?;
    println!(
        "alice code: {}\nbob code:   {}",
        identity::friend_code(alice_key.public()),
        identity::friend_code(bob_key.public())
    );

    let alice_store = Arc::new(Mutex::new(FriendStore::load(&alice_dir)?));
    let bob_store = Arc::new(Mutex::new(FriendStore::load(&bob_dir)?));

    // ---- nodes -----------------------------------------------------------
    // Alice's node: declines inbound friend requests/offers, reports done on a
    // channel so the spike can assert it.
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<String>(4);
    let alice = Arc::new(
        ShareNode::spawn(
            &alice_dir,
            alice_key,
            RelayMode::Disabled,
            FriendsProtocol::new(
                Arc::new(|_id, _name| {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = tx.send(RequestDecision::Decline);
                    rx
                }),
                Arc::new(|_offer| {}),
                Arc::new(move |_id, ticket| {
                    let _ = done_tx.try_send(ticket);
                }),
                alice_store.clone(),
            ),
            None,
        )
        .await?,
    );

    // Bob's node: auto-accepts friend requests (stand-in for the UI card) and
    // auto-fetches offers from friends (no per-transfer accept, per product).
    let bob_ep_holder: Arc<Mutex<Option<Arc<ShareNode>>>> = Arc::new(Mutex::new(None));
    let bob = Arc::new(
        ShareNode::spawn(
            &bob_dir,
            bob_key,
            RelayMode::Disabled,
            FriendsProtocol::new(
                Arc::new(|_id, name| {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = tx.send(RequestDecision::Accept {
                        our_name: format!("bob (accepting {name})"),
                    });
                    rx
                }),
                Arc::new({
                    let bob_ep_holder = bob_ep_holder.clone();
                    let bob_dir = bob_dir.clone();
                    move |offer: OfferInfo| {
                        let bob_ep_holder = bob_ep_holder.clone();
                        let bob_dir = bob_dir.clone();
                        tokio::spawn(async move {
                            let bob = bob_ep_holder.lock().clone().unwrap();
                            let ticket: Ticket = offer.ticket.parse().expect("bad ticket");
                            eprintln!("bob: fetching {}", ticket.hash());
                            let hash = match bob.fetch(&ticket, |_| {}).await {
                                Ok(h) => h,
                                Err(e) => {
                                    eprintln!("bob: fetch failed: {e:?}");
                                    return;
                                }
                            };
                            eprintln!("bob: fetched {hash}");
                            let dest = bob_dir.join("files").join(&offer.file_name);
                            bob.export(ticket.hash_and_format(), &dest)
                                .await
                                .expect("export failed");
                            eprintln!("bob: exported, notifying done");
                            match waku_share::friends::notify_transfer_done(
                                bob.endpoint(),
                                ticket.addr().clone(),
                                &offer.ticket,
                            )
                            .await
                            {
                                Ok(()) => eprintln!("bob: done notified"),
                                Err(e) => eprintln!("bob: done notify failed: {e:?}"),
                            }
                        });
                    }
                }),
                Arc::new(|_id, _ticket| {}),
                bob_store.clone(),
            ),
            None,
        )
        .await?,
    );
    *bob_ep_holder.lock() = Some(bob.clone());

    // ---- friend handshake --------------------------------------------------
    let bob_addr = bob.addr();
    let parsed = identity::parse_friend_code(&identity::friend_code(bob_addr.id))?;
    assert_eq!(parsed, bob_addr.id);

    let their_name = send_friend_request(alice.endpoint(), bob_addr, "alice", &alice_store).await?;
    println!("alice friended: {their_name}");
    assert!(alice_store.lock().is_friend(&bob.addr().id));
    assert!(bob_store.lock().is_friend(&alice.addr().id));
    println!("both sides recorded the friendship — OK");

    // ---- offer → auto-fetch → done ------------------------------------------
    let (ticket, _tag, size) = alice.provide(&file_path).await?;
    assert_eq!(size, payload.len() as u64);
    waku_share::friends::send_offer(
        alice.endpoint(),
        bob.addr(),
        "alice",
        "hello.txt",
        size,
        Some("here's the new mockups".into()),
        &ticket.to_string(),
    )
    .await?;
    println!("bob accepted the offer");

    let done = tokio::time::timeout(std::time::Duration::from_secs(30), done_rx.recv())
        .await?
        .expect("no TransferDone");
    assert_eq!(done, ticket.to_string());
    let received = std::fs::read(bob_dir.join("files/hello.txt"))?;
    assert_eq!(received, payload.as_bytes(), "bob's received bytes differ");
    println!("alice got TransferDone, bob's copy verified — OK");

    // ---- folder offer ------------------------------------------------------
    let bundle = alice_dir.join("files/bundle");
    std::fs::create_dir_all(bundle.join("sub"))?;
    std::fs::write(bundle.join("one.txt"), b"one\n")?;
    std::fs::write(bundle.join("sub/two.txt"), b"two\n")?;
    std::fs::write(bundle.join(".hidden"), b"dot\n")?;
    let (dir_ticket, _dir_tag, dir_size) = alice.provide(&bundle).await?;
    assert_eq!(dir_size, 12);
    waku_share::friends::send_offer(
        alice.endpoint(),
        bob.addr(),
        "alice",
        "bundle",
        dir_size,
        None,
        &dir_ticket.to_string(),
    )
    .await?;
    let done = tokio::time::timeout(std::time::Duration::from_secs(30), done_rx.recv())
        .await?
        .expect("no TransferDone for folder");
    assert_eq!(done, dir_ticket.to_string());
    let received_dir = bob_dir.join("files/bundle");
    assert_eq!(std::fs::read(received_dir.join("one.txt"))?, b"one\n");
    assert_eq!(std::fs::read(received_dir.join("sub/two.txt"))?, b"two\n");
    assert_eq!(std::fs::read(received_dir.join(".hidden"))?, b"dot\n");
    println!("folder tree verified — OK");

    println!("spike done");
    Ok(())
}
