//! Spike: two iroh endpoints in one process exchange a file over local
//! addresses (relay disabled). Proves the provide/fetch path end to end.
//!
//!   cargo run -p waku-share --bin spike

use std::io::Write as _;

use iroh::RelayMode;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base = std::env::temp_dir().join(format!("waku-share-spike-{}", std::process::id()));
    let send_dir = base.join("send");
    let recv_dir = base.join("recv");
    std::fs::create_dir_all(&send_dir)?;
    std::fs::create_dir_all(&recv_dir)?;

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
    println!(
        "verified: {} bytes at {} — OK",
        got.len(),
        out.display()
    );

    provider.shutdown().await?;
    println!("spike done");
    Ok(())
}
