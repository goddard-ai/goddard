//! Daemon-owned Whistle speech recognition.
//!
//! Needle keeps one process-global model and is not thread-safe. All calls
//! therefore pass through `ENGINE`; PCM is bounded at the protocol edge.

use std::{
    ffi::{CStr, CString, c_char},
    fs,
    path::PathBuf,
    process::Command,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, anyhow, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use waku_protocol::WhistleWord;

const MODEL_URL: &str = "https://huggingface.co/Cactus-Compute/whistle/resolve/d3ea19e0fe4f99fa7dfb9afa63070b1c6eacaff1/whistle.cact";
const MODEL_SHA256: &str = "b6e02f048568ac5d01a2042556c658061e699acbc0aa2a1439f52f3d461dffeb";
const MODEL_SIZE: u64 = 16_919_407;
const MAX_SAMPLES: usize = 16_000 * 30;
const OUTPUT_CAPACITY: usize = 1 << 20;

static ENGINE: OnceLock<Mutex<bool>> = OnceLock::new();
static MODEL_DOWNLOAD: Mutex<()> = Mutex::new(());

#[cfg(whistle_native)]
unsafe extern "C" {
    fn needle_load(cact: *const u8, n: u64) -> i32;
    fn needle_last_error() -> *const c_char;
    fn needle_transcribe(
        pcm: *const f32,
        samples: i32,
        language: *const c_char,
        keywords: *const c_char,
        word_timestamps: i32,
        out: *mut c_char,
        out_capacity: i32,
    ) -> i32;
}

#[derive(Deserialize)]
struct NativeResult {
    #[serde(default)]
    text: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    words: Vec<WhistleWord>,
}

pub fn model_path() -> PathBuf {
    let root = if cfg!(debug_assertions) {
        crate::persistence::StateStore::default_path().with_file_name("whistle")
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(crate::identity::HOME_DIRECTORY_NAME)
            .join("whistle")
    };
    root.join("whistle.cact")
}

pub fn status() -> (bool, bool) {
    (cfg!(whistle_native), valid_model(&model_path()))
}

pub fn download_model() -> anyhow::Result<()> {
    ensure!(
        cfg!(whistle_native),
        "Whistle dictation is unavailable on this build target"
    );
    let path = model_path();
    let _download = MODEL_DOWNLOAD
        .lock()
        .map_err(|_| anyhow!("Whistle model download lock is poisoned"))?;
    if valid_model(&path) {
        return Ok(());
    }
    let parent = path.parent().context("Whistle model path has no parent")?;
    fs::create_dir_all(parent).context("creating Whistle model directory")?;
    let partial = path.with_extension("cact.part");
    let mut curl = Command::new("curl");
    if let Some(search_path) = crate::command_env::executable_search_path() {
        curl.env("PATH", search_path);
    }
    let status = curl
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
            "20000000",
            "--max-time",
            "120",
            "--output",
        ])
        .arg(&partial)
        .arg(MODEL_URL)
        .status()
        .context("starting Whistle model download")?;
    if !status.success() {
        let _ = fs::remove_file(&partial);
        bail!("Whistle model download failed");
    }
    if !valid_model(&partial) {
        let _ = fs::remove_file(&partial);
        bail!("Whistle model download failed SHA-256 verification");
    }
    fs::rename(&partial, &path).context("installing Whistle model")?;
    Ok(())
}

pub fn transcribe(
    pcm: Vec<i16>,
    language: Option<String>,
    keywords: Option<String>,
) -> anyhow::Result<(String, String, Vec<WhistleWord>)> {
    ensure!(
        cfg!(whistle_native),
        "Whistle dictation is unavailable on this build target"
    );
    ensure!(
        !pcm.is_empty() && pcm.len() <= MAX_SAMPLES,
        "Whistle audio must be between 1 sample and 30 seconds"
    );
    let path = model_path();
    ensure!(valid_model(&path), "Whistle model is not downloaded yet");

    #[cfg(whistle_native)]
    {
        let lock = ENGINE.get_or_init(|| Mutex::new(false));
        let mut loaded = lock
            .lock()
            .map_err(|_| anyhow!("Whistle engine lock is poisoned"))?;
        if !*loaded {
            let weights = fs::read(path).context("reading Whistle model")?;
            // SAFETY: the model bytes remain live for the call; Needle copies the
            // model into its process-global engine on successful load.
            let result = unsafe { needle_load(weights.as_ptr(), weights.len() as u64) };
            if result != 0 {
                bail!("loading Whistle model: {}", native_error());
            }
            *loaded = true;
        }
        let pcm: Vec<f32> = pcm
            .into_iter()
            .map(|sample| sample as f32 / 32768.0)
            .collect();
        let language = language
            .filter(|value| {
                matches!(
                    value.as_str(),
                    "en" | "de" | "fr" | "es" | "it" | "nl" | "pl"
                )
            })
            .map(|value| CString::new(value).expect("language codes contain no NUL"));
        let keywords = keywords
            .filter(|value| !value.trim().is_empty())
            .map(|value| CString::new(value.replace('\0', "")).expect("NUL bytes removed"));
        let mut output = vec![0_i8; OUTPUT_CAPACITY];
        // SAFETY: all pointers reference live contiguous buffers for the call;
        // the output allocation is writable and its capacity fits i32.
        let result = unsafe {
            needle_transcribe(
                pcm.as_ptr(),
                pcm.len() as i32,
                language
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                keywords
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                1,
                output.as_mut_ptr(),
                output.len() as i32,
            )
        };
        if result < 0 {
            bail!("Whistle transcription failed: {}", native_error());
        }
        // SAFETY: Needle writes a NUL-terminated JSON string to the supplied buffer
        // on success, as specified by needle.h.
        let json = unsafe { CStr::from_ptr(output.as_ptr()) }.to_string_lossy();
        let decoded: NativeResult =
            serde_json::from_str(&json).context("decoding Whistle result")?;
        Ok((decoded.text, decoded.language, decoded.words))
    }
    #[cfg(not(whistle_native))]
    unreachable!("target support was checked above")
}

fn valid_model(path: &std::path::Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.len() != MODEL_SIZE {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    format!("{:x}", Sha256::digest(bytes)) == MODEL_SHA256
}

#[cfg(whistle_native)]
fn native_error() -> String {
    // SAFETY: Needle owns this NUL-terminated process-global error until its next call.
    unsafe {
        let error = needle_last_error();
        if error.is_null() {
            "unknown native error".to_owned()
        } else {
            CStr::from_ptr(error).to_string_lossy().into_owned()
        }
    }
}

#[cfg(not(whistle_native))]
fn native_error() -> String {
    "native engine unavailable".to_owned()
}
