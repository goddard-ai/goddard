//! HTTPS through the system `curl` — the repo's convention for one-off
//! fetches, so no TLS stack gets pinned for a handful of provider calls.
//! Every request runs `curl -K <file>`: the URL, method, and headers live
//! in a 0600 temp config and the JSON body travels over stdin via
//! `data-binary = "@-"`, so API keys never appear in `ps` output.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdout, Stdio};

use anyhow::{Context as _, bail};
use serde_json::Value;
use uuid::Uuid;

pub(crate) struct Response {
    pub body: Value,
}

/// `body["detail"]`/`["message"]`/`["error"]["message"]`, the shapes
/// provider error bodies take.
fn error_detail(body: &Value) -> Option<String> {
    body.get("detail")
        .or_else(|| body.get("message"))
        .or_else(|| body.pointer("/error/message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// A curl `-K` config rendered to a private temp file. Removed on drop —
/// which for a streamed request happens when the child exits.
struct ConfigFile {
    path: std::path::PathBuf,
}

impl ConfigFile {
    fn write(
        url: &str,
        method: &str,
        headers: &[(String, String)],
        streamed: bool,
        has_body: bool,
    ) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!("goddard-curl-{}", Uuid::new_v4()));
        let mut config = format!(
            "url = \"{}\"\nrequest = \"{}\"\nwrite-out = \"\\n%{{http_code}}\"\nsilent\nshow-error\n",
            url.replace('\\', "\\\\").replace('"', "\\\""),
            method,
        );
        for (name, value) in headers {
            config.push_str(&format!(
                "header = \"{}: {}\"\n",
                name,
                value.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }
        if streamed {
            config.push_str("no-buffer\n");
        } else if has_body {
            // `-d` on a GET would mutate the method — only bodies opt in.
            config.push_str("data-binary = \"@-\"\n");
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        file.write_all(config.as_bytes())?;
        Ok(Self { path })
    }
}

impl Drop for ConfigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `GET`/`POST`/`DELETE` with a JSON body on stdin and a JSON response out.
/// HTTP errors surface the provider's own detail text.
pub(crate) fn request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&Value>,
) -> anyhow::Result<Response> {
    let config = ConfigFile::write(url, method, headers, false, body.is_some())?;
    let mut child = crate::command_env::search_path_command("curl")
        .arg("-K")
        .arg(&config.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to launch curl")?;
    if let Some(stdin) = child.stdin.as_mut() {
        let payload = body.map(Value::to_string).unwrap_or_default();
        stdin.write_all(payload.as_bytes())?;
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().context("curl failed to finish")?;
    if !output.status.success() {
        bail!(
            "curl {method} {url} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // `write-out` appends the status as the last line.
    let (body_text, status_text) = text.rsplit_once('\n').unwrap_or((&text, ""));
    let status: u16 = status_text.trim().parse().unwrap_or(0);
    let body: Value = serde_json::from_str(body_text).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = error_detail(&body);
        bail!(
            "{method} {url} failed with HTTP {status}{}",
            detail.map(|d| format!(" — {d}")).unwrap_or_default()
        );
    }
    Ok(Response { body })
}

/// A live SSE/NDJSON stream — `curl -N` with stdout readable line by line.
/// Killing the child ends the stream; the config file is dropped with it.
pub(crate) struct Stream {
    pub stdout: BufReader<ChildStdout>,
    child: Child,
    _config: ConfigFile,
}

impl Stream {
    /// Next `data:` payload from the stream, or `None` when the remote
    /// closed it.
    pub fn next_event(&mut self) -> Option<String> {
        let mut line = String::new();
        loop {
            line.clear();
            match self.stdout.read_line(&mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    let line = line.trim_end();
                    if let Some(data) = line.strip_prefix("data:") {
                        return Some(data.trim().to_owned());
                    }
                }
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn stream(
    method: &str,
    url: &str,
    headers: &[(String, String)],
) -> anyhow::Result<Stream> {
    let config = ConfigFile::write(url, method, headers, true, false)?;
    let mut child = crate::command_env::search_path_command("curl")
        .arg("-K")
        .arg(&config.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to launch curl")?;
    let stdout = BufReader::new(child.stdout.take().expect("stdout piped"));
    Ok(Stream {
        stdout,
        child,
        _config: config,
    })
}
