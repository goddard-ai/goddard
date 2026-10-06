//! Configures and verifies the pinned native Whistle engine.

use std::{env, fs, path::PathBuf, process::Command};

use sha2::{Digest, Sha256};

const PIN: &str = include_str!("whistle-engine.pin");

fn pin(key: &str) -> &'static str {
    PIN.lines()
        .filter_map(|line| line.trim().split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
        .expect("whistle-engine.pin missing value")
}

fn main() {
    println!("cargo:rerun-if-changed=whistle-engine.pin");
    println!("cargo:rerun-if-env-changed=WAKU_NEEDLE_LIB_DIR");
    println!("cargo:rustc-check-cfg=cfg(whistle_native)");
    println!("cargo:rustc-check-cfg=cfg(whistle_unavailable)");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos")
        || env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("aarch64")
    {
        println!("cargo:rustc-cfg=whistle_unavailable");
        return;
    }

    println!("cargo:rustc-cfg=whistle_native");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"));
    let archive = env::var_os("WAKU_NEEDLE_LIB_DIR")
        .map(PathBuf::from)
        .map(|dir| dir.join("libneedle.a"))
        .unwrap_or_else(|| out.join("libneedle.a"));
    if !archive.is_file() {
        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}/libneedle.a",
            pin("repo"),
            pin("commit"),
            pin("platform")
        );
        let partial = archive.with_extension("a.partial");
        fs::create_dir_all(archive.parent().expect("archive path has a parent"))
            .expect("create Needle archive directory");
        let status = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-redirs",
                "5",
                "--max-filesize",
                "250000000",
                "--max-time",
                "300",
                "--output",
            ])
            .arg(&partial)
            .arg(&url)
            .status()
            .expect("start curl to fetch pinned Needle archive");
        if !status.success() {
            let _ = fs::remove_file(&partial);
            panic!("failed to fetch pinned Needle archive from {url}");
        }
        fs::rename(partial, &archive).expect("install fetched Needle archive");
    }
    let actual = format!(
        "{:x}",
        Sha256::digest(fs::read(&archive).expect("read Needle archive"))
    );
    assert_eq!(
        actual,
        pin("sha256"),
        "pinned Needle archive SHA-256 mismatch"
    );
    println!(
        "cargo:rustc-link-search=native={}",
        archive.parent().unwrap().display()
    );
    println!("cargo:rustc-link-lib=static=needle");
    println!("cargo:rustc-link-lib=dylib=c++");
}
