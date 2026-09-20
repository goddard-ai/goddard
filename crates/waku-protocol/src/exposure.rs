use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The non-loopback listener `setDaemonExposure` asks the daemon to run.
/// Sending `None` unexposes; the daemon's loopback listener — and every
/// session it serves — is never touched.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonExposure {
    /// `0` lets the OS pick; the response reports the bound port.
    pub port: u16,
    /// Exact `http://`/`https://` browser origins permitted through the
    /// exposed listener's WebSocket handshake.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Bearer token clients on the exposed listener authenticate with.
    /// Paired-device tokens work there too; the loopback token does not.
    pub token: String,
}

impl DaemonExposure {
    /// Normalize and validate a requested exposure: the token must be
    /// non-empty and every origin must be an exact browser origin.
    pub fn validated(mut self) -> anyhow::Result<Self> {
        if self.token.trim().is_empty() {
            bail!("daemon authentication token is empty");
        }
        let mut origins = Vec::with_capacity(self.allowed_origins.len());
        for candidate in std::mem::take(&mut self.allowed_origins) {
            let origin = normalize_browser_origin(&candidate)?;
            if !origins.contains(&origin) {
                origins.push(origin);
            }
        }
        self.allowed_origins = origins;
        Ok(self)
    }
}

/// Parse the comma-separated exact browser origins the desktop settings
/// field accepts — the same input the daemon validates on
/// `setDaemonExposure`. Browser Origin headers contain only an HTTP(S)
/// origin, never a path.
pub fn parse_allowed_origins(text: &str) -> anyhow::Result<Vec<String>> {
    let mut origins = Vec::new();
    for candidate in text
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let origin = normalize_browser_origin(candidate)?;
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    Ok(origins)
}

fn normalize_browser_origin(candidate: &str) -> anyhow::Result<String> {
    let url = url::Url::parse(candidate)
        .with_context(|| format!("invalid browser origin {candidate:?}"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        bail!(
            "browser origin {candidate:?} must be an exact http:// or https:// origin without a path"
        );
    }
    let origin = url.origin().ascii_serialization();
    if origin == "null" {
        bail!("browser origin {candidate:?} is not a network origin");
    }
    Ok(origin)
}
