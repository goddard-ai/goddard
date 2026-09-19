//! HTTPS through the system `curl`, matching `usage.rs`'s approach: no TLS
//! stack links into waku-core. Headers — including injected credentials —
//! travel through the stdin config file so secrets never appear in argv.
//! Bodies travel through a caller-written temp file for the same reason.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context as _, anyhow, bail};

#[cfg(target_os = "windows")]
const CURL_PATH: &str = r"C:\Windows\System32\curl.exe";
#[cfg(not(target_os = "windows"))]
const CURL_PATH: &str = "/usr/bin/curl";

pub struct CurlJob<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: &'a [(&'a str, String)],
    /// Request body, already written to a file by the caller.
    pub body_file: Option<&'a Path>,
}

/// Spawn curl for `job` with stdin config written. The child's stdout is the
/// full `curl -i` response (status line, headers, body) — stream it or
/// collect it with [`collect`]. Callers that stream pass
/// `Stdio::null()` for stderr; `collect` callers keep it piped for errors.
pub fn spawn_with(job: &CurlJob<'_>, stderr: Stdio) -> anyhow::Result<Child> {
    let mut config = String::new();
    config.push_str("silent\nshow-error\nno-buffer\nhttp1.1\ninclude\n");
    config.push_str(&format!("request = {}\n", config_quote(job.method)));
    config.push_str(&format!("url = {}\n", config_quote(job.url)));
    for (name, value) in job.headers {
        config.push_str(&format!(
            "header = {}\n",
            config_quote(&format!("{name}: {value}"))
        ));
    }
    if let Some(body) = job.body_file {
        config.push_str(&format!(
            "data-binary = \"@{}\"\n",
            body.display().to_string().replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    let mut child = Command::new(CURL_PATH)
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .context("could not run curl")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("curl stdin unavailable"))?
        .write_all(config.as_bytes())
        .context("could not write curl config")?;
    Ok(child)
}

pub struct CurlResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Collect a finished child's output into a parsed response.
/// `wait_with_output` drains both pipes concurrently, so a chatty stderr
/// cannot deadlock a long response body.
pub fn collect(child: Child) -> anyhow::Result<CurlResponse> {
    let output = child.wait_with_output().context("curl did not finish")?;
    if !output.status.success() {
        bail!(
            "curl failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_response(&output.stdout)
}

/// One-shot request helper.
pub fn request(job: &CurlJob<'_>) -> anyhow::Result<CurlResponse> {
    collect(spawn_with(job, Stdio::piped())?)
}

/// Parse `curl -i` output, skipping interim 1xx blocks.
fn parse_response(raw: &[u8]) -> anyhow::Result<CurlResponse> {
    let mut rest = raw;
    loop {
        let split = rest
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| anyhow!("curl output had no header terminator"))?;
        let (head, body) = rest.split_at(split);
        let body = &body[4..];
        let head = String::from_utf8_lossy(head);
        let mut lines = head.lines();
        let status_line = lines.next().unwrap_or_default();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| anyhow!("could not parse curl status line: {status_line}"))?;
        if (100..200).contains(&status) {
            rest = body;
            continue;
        }
        return Ok(CurlResponse {
            status,
            body: body.to_vec(),
        });
    }
}

fn config_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], " ");
    format!("\"{escaped}\"")
}

/// A scratch file under the daemon's private data dir for request bodies.
pub fn body_file(dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let dir = dir.join("tmp");
    crate::fs_ext::create_private_dir_all(&dir)?;
    let path = dir.join(name);
    Ok(path)
}
