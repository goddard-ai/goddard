//! Mascot avatars — raster logos from ipaslogo.com, shipped on the
//! `icons/ipaslogo` orphan branch of the app repository. The first Mascots
//! render shallow-clones that branch into the app's data directory, then
//! each seed hashes onto a stable file. Everything here runs on the avatar
//! queue's background jobs or a dedicated prefetch thread — never a frame.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, Once, OnceLock};

use anyhow::Context as _;
use waku_protocol::identity::DATA_DIRECTORY_NAME;

/// The public repo plus the orphan branch carrying `manifest.json` and
/// `webp/` — one 512px display rendition per logo scraped from
/// cdn.ipaslogo.com.
const PACK_REPO: &str = "https://github.com/goddard-ai/goddard";
const PACK_BRANCH: &str = "icons/ipaslogo";

/// The clone once `ensure_pack` has produced it. `None` is both the
/// initial state and the state after a failed clone, so a transient
/// failure (offline, missing git) retries on the next avatar job rather
/// than latching.
static PACK: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Sorted `webp/` entries, cached for the session so every render doesn't
/// re-walk the pack directory.
static FILES: OnceLock<Vec<String>> = OnceLock::new();

fn pack_dir() -> Option<PathBuf> {
    // Mirror `uses_development_data_dir` in waku-client: debug builds honor
    // GODDARD_DATA_DIR, release builds GODDARD_DEV_DATA_DIR.
    let variable = if cfg!(debug_assertions) {
        "GODDARD_DATA_DIR"
    } else {
        "GODDARD_DEV_DATA_DIR"
    };
    let base = std::env::var_os(variable)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|dir| dir.join(DATA_DIRECTORY_NAME)))?;
    Some(base.join("avatar-packs").join("ipaslogo"))
}

/// The local pack, cloning it on first use. Concurrent callers serialize
/// on `PACK` so only one `git clone` runs; a render job that arrives while
/// a prefetch is mid-clone just waits out the lock.
fn ensure_pack() -> anyhow::Result<PathBuf> {
    let mut pack = PACK.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(dir) = pack.as_ref() {
        return Ok(dir.clone());
    }
    let dir = pack_dir()
        .ok_or_else(|| anyhow::anyhow!("no data directory for the mascot avatar pack"))?;
    if dir.join("manifest.json").is_file() {
        *pack = Some(dir.clone());
        return Ok(dir);
    }
    let parent = dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("mascot pack path {} has no parent", dir.display()))?;
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".ipaslogo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let status = Command::new("git")
        .args([
            "clone",
            "--quiet",
            "--depth",
            "1",
            "--branch",
            PACK_BRANCH,
            PACK_REPO,
        ])
        .arg(&staging)
        .status()
        .context("running git clone for the mascot avatar pack")?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        anyhow::bail!("git clone of {PACK_REPO} branch {PACK_BRANCH} exited with {status}");
    }
    // A sibling process may have completed the same clone first; keep its
    // result rather than erroring on the rename.
    if std::fs::rename(&staging, &dir).is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    *pack = Some(dir.clone());
    Ok(dir)
}

fn mascot_files(dir: &Path) -> anyhow::Result<&'static [String]> {
    if let Some(files) = FILES.get() {
        return Ok(files);
    }
    let mut files: Vec<String> = std::fs::read_dir(dir.join("webp"))?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".webp"))
        .collect();
    files.sort_unstable();
    anyhow::ensure!(!files.is_empty(), "mascot avatar pack has no webp files");
    Ok(FILES.get_or_init(|| files))
}

/// FNV-1a over the seed, picking a stable mascot per identity.
fn mascot_index(seed: &str, len: usize) -> usize {
    let mut hash = 2166136261_u32;
    for byte in seed.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
    }
    hash as usize % len
}

/// The seed's mascot raster at `2 * bucket` device pixels — the size the
/// SVG path produces through `avatar_scale`, so cached rasters stay
/// comparable across styles.
pub(crate) fn avatar_image(seed: &str, bucket: u32) -> anyhow::Result<Arc<gpui::RenderImage>> {
    let dir = ensure_pack()?;
    let files = mascot_files(&dir)?;
    let path = dir
        .join("webp")
        .join(&files[mascot_index(seed, files.len())]);
    let bytes = std::fs::read(&path)?;
    let decoded = image::load_from_memory(&bytes)
        .with_context(|| format!("decoding {}", path.display()))?
        .to_rgba8();
    let target = (bucket * 2).max(1);
    let frame = if decoded.width() > target {
        image::imageops::resize(
            &decoded,
            target,
            target,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };
    Ok(Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
        frame,
    )])))
}

/// Stable mascot pick per seed — verified by the module test so the same
/// identity keeps the same face across launches.
#[cfg(test)]
mod tests {
    #[test]
    fn mascot_index_is_deterministic_and_in_range() {
        for (seed, len) in [("test", 3448), ("", 1), ("😀", 97), ("boss", 3448)] {
            let pick = super::mascot_index(seed, len);
            assert!(pick < len, "{seed:?} picked {pick} of {len}");
            assert_eq!(pick, super::mascot_index(seed, len), "{seed:?} unstable");
        }
        assert_ne!(
            super::mascot_index("test", 3448),
            super::mascot_index("another", 3448),
        );
    }
}

/// Kicks the shallow clone on a dedicated thread so the first Mascots
/// render usually finds the pack already local. Idempotent; render jobs
/// reach `ensure_pack` directly and wait out an in-flight clone on the
/// same mutex.
pub(crate) fn prefetch() {
    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::spawn(|| {
            if let Err(error) = ensure_pack() {
                eprintln!("Goddard: mascot avatar pack clone failed: {error:#}");
            }
        });
    });
}
