//! OAuth 2.x for remote MCP servers: metadata discovery (RFC 8414/9728),
//! dynamic client registration, PKCE, a loopback redirect listener, and
//! refresh. Everything is blocking and runs on dedicated threads — the same
//! posture as the rest of the daemon's network code.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use uuid::Uuid;

use super::catalog::{CatalogEntry, CatalogVariant};
use super::http::{self, CurlJob};
use super::secrets::SecretStore;

const CALLBACK_PATH: &str = "/oauth/callback";
const AUTH_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StoredCredential {
    ApiKey {
        key: String,
    },
    OAuth {
        client_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_secret: Option<String>,
        access_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        /// Unix seconds; absent when the server issued no expiry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<i64>,
        token_endpoint: String,
    },
}

/// The credential currently stored for `integration_id`, ready to send
/// upstream. OAuth tokens are refreshed (and the store updated) when expired.
/// `Ok(None)` means no credential exists yet.
pub fn access_token(
    secrets: &SecretStore,
    integration_id: &str,
) -> anyhow::Result<Option<String>> {
    let Some(raw) = secrets.read(integration_id) else {
        return Ok(None);
    };
    let credential: StoredCredential =
        serde_json::from_str(&raw).context("stored integration credential is unreadable")?;
    match credential {
        StoredCredential::ApiKey { key } => Ok(Some(key)),
        StoredCredential::OAuth {
            client_id,
            client_secret,
            access_token,
            refresh_token,
            expires_at,
            token_endpoint,
        } => {
            let now = chrono::Utc::now().timestamp();
            let expired = expires_at.is_some_and(|at| now + 30 >= at);
            if !expired {
                return Ok(Some(access_token));
            }
            let Some(refresh_token) = refresh_token else {
                return Ok(None);
            };
            let refreshed = exchange_refresh(
                &token_endpoint,
                &client_id,
                client_secret.as_deref(),
                &refresh_token,
            )?;
            let credential = StoredCredential::OAuth {
                client_id,
                client_secret,
                access_token: refreshed.access_token.clone(),
                refresh_token: refreshed.refresh_token.or(Some(refresh_token)),
                expires_at: refreshed
                    .expires_in
                    .map(|seconds| chrono::Utc::now().timestamp() + seconds),
                token_endpoint,
            };
            let encoded = serde_json::to_string(&credential)?;
            secrets.store(integration_id, &encoded)?;
            Ok(Some(refreshed.access_token))
        }
    }
}

/// Run the full browser authorization flow for `entry`/`variant`: discover
/// the authorization server, register a client when possible, open the
/// browser, wait for the loopback callback, and exchange the code.
pub fn authorize(
    secrets: &SecretStore,
    data_dir: &std::path::Path,
    entry: &CatalogEntry,
    variant: &CatalogVariant,
) -> anyhow::Result<StoredCredential> {
    let issuer = url::Url::parse(variant.url)
        .and_then(|url| url.join("/"))
        .context("catalog entry has an invalid URL")?
        .to_string();
    let metadata = discover(&issuer)?;
    let (client_id, client_secret) = match (&entry.oauth_client_id, &metadata.registration_endpoint)
    {
        (Some(id), _) => (id.to_string(), entry.oauth_client_secret.map(str::to_owned)),
        (None, Some(registration)) => register_client(registration)?,
        (None, None) => bail!(
            "{} requires a registered OAuth client; use an API key if the service offers one",
            entry.name
        ),
    };

    let listener =
        TcpListener::bind("127.0.0.1:0").context("could not bind the OAuth callback listener")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");

    let verifier = random_urlsafe(32);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe(16);

    let mut authorize = url::Url::parse(&metadata.authorization_endpoint)
        .context("authorization endpoint is invalid")?;
    {
        let mut query = authorize.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", &issuer);
        if let Some(scopes) = variant.scopes {
            query.append_pair("scope", scopes);
        }
    }
    open_browser(authorize.as_str())?;

    let code = await_callback(&listener, &state)?;
    let token = exchange_code(
        data_dir,
        &metadata.token_endpoint,
        &client_id,
        client_secret.as_deref(),
        &code,
        &redirect_uri,
        &verifier,
        &issuer,
    )?;
    let credential = StoredCredential::OAuth {
        client_id,
        client_secret,
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: token
            .expires_in
            .map(|seconds| chrono::Utc::now().timestamp() + seconds),
        token_endpoint: metadata.token_endpoint,
    };
    let encoded = serde_json::to_string(&credential)?;
    secrets.store(entry.id, &encoded)?;
    Ok(credential)
}

#[derive(Deserialize)]
struct ServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    registration_endpoint: Option<String>,
}

fn discover(issuer: &str) -> anyhow::Result<ServerMetadata> {
    let issuer = issuer.trim_end_matches('/');
    for path in [
        format!("{issuer}/.well-known/oauth-authorization-server"),
        format!("{issuer}/.well-known/openid-configuration"),
    ] {
        let job = CurlJob {
            method: "GET",
            url: &path,
            headers: &[("Accept", "application/json".to_owned())],
            body_file: None,
        };
        let Ok(response) = http::request(&job) else {
            continue;
        };
        if response.status == 200 {
            return serde_json::from_slice(&response.body)
                .context("authorization server metadata is unreadable");
        }
    }
    bail!("{issuer} does not publish OAuth metadata")
}

fn register_client(endpoint: &str) -> anyhow::Result<(String, Option<String>)> {
    let body = serde_json::json!({
        "client_name": "Goddard",
        "redirect_uris": ["http://127.0.0.1/callback", "http://localhost/callback"],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    // Registration endpoints disagree on whether redirect URIs may be
    // dynamic; the loopback listener binds a random port, so the registered
    // URIs above are placeholders some servers ignore. Servers that enforce
    // exact-match redirect URIs reject the flow at authorize time and the
    // credential simply never lands — the tile stays at NeedsAuth.
    let tmp = std::env::temp_dir().join(format!("goddard-oauth-{}", Uuid::new_v4().simple()));
    std::fs::write(&tmp, serde_json::to_vec(&body)?)?;
    let job = CurlJob {
        method: "POST",
        url: endpoint,
        headers: &[
            ("Content-Type", "application/json".to_owned()),
            ("Accept", "application/json".to_owned()),
        ],
        body_file: Some(&tmp),
    };
    let response = http::request(&job);
    let _ = std::fs::remove_file(&tmp);
    let response = response?;
    if !(200..300).contains(&response.status) {
        bail!(
            "dynamic client registration failed ({})",
            response.status
        );
    }
    #[derive(Deserialize)]
    struct Registration {
        client_id: String,
        client_secret: Option<String>,
    }
    let registration: Registration = serde_json::from_slice(&response.body)
        .context("registration response is unreadable")?;
    Ok((registration.client_id, registration.client_secret))
}

struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

fn token_request(
    data_dir: &std::path::Path,
    endpoint: &str,
    form: &[(&str, &str)],
) -> anyhow::Result<TokenResponse> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form.iter().copied())
        .finish();
    let tmp = http::body_file(data_dir, &format!("oauth-{}", Uuid::new_v4().simple()))?;
    std::fs::write(&tmp, body)?;
    let job = CurlJob {
        method: "POST",
        url: endpoint,
        headers: &[
            (
                "Content-Type",
                "application/x-www-form-urlencoded".to_owned(),
            ),
            ("Accept", "application/json".to_owned()),
        ],
        body_file: Some(&tmp),
    };
    let response = http::request(&job);
    let _ = std::fs::remove_file(&tmp);
    let response = response?;
    if !(200..300).contains(&response.status) {
        bail!(
            "token request failed ({}): {}",
            response.status,
            String::from_utf8_lossy(&response.body)
                .chars()
                .take(200)
                .collect::<String>()
        );
    }
    #[derive(Deserialize)]
    struct Raw {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<i64>,
    }
    let raw: Raw =
        serde_json::from_slice(&response.body).context("token response is unreadable")?;
    Ok(TokenResponse {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token,
        expires_in: raw.expires_in,
    })
}

fn exchange_code(
    data_dir: &std::path::Path,
    endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
    resource: &str,
) -> anyhow::Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
        ("resource", resource),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    token_request(data_dir, endpoint, &form)
}

fn exchange_refresh(
    endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> anyhow::Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    // Refresh happens on proxy requests; use the process temp dir for the
    // tiny form body rather than threading the data dir through.
    token_request(std::path::Path::new("/tmp"), endpoint, &form)
}

fn await_callback(listener: &TcpListener, expected_state: &str) -> anyhow::Result<String> {
    let deadline = Instant::now() + AUTH_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(code) = handle_callback(stream, expected_state)? {
                    return Ok(code);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error).context("OAuth callback listener failed"),
        }
        if Instant::now() > deadline {
            bail!("timed out waiting for the OAuth callback");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn handle_callback(
    mut stream: std::net::TcpStream,
    expected_state: &str,
) -> anyhow::Result<Option<String>> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?)
        .read_line(&mut line)
        .context("could not read the OAuth callback")?;
    // "GET /oauth/callback?code=...&state=... HTTP/1.1"
    let path = line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("malformed OAuth callback request"))?;
    let parsed = url::Url::parse(&format!("http://127.0.0.1{path}"))?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    let (html, result) = if let Some(error) = error {
        (
            "Authorization was denied. You can close this tab and return to Goddard.",
            Err(anyhow!("authorization failed: {error}")),
        )
    } else if state.as_deref() != Some(expected_state) {
        (
            "Authorization state mismatch. You can close this tab and return to Goddard.",
            Err(anyhow!("OAuth callback state mismatch")),
        )
    } else if let Some(code) = code {
        (
            "Connected. You can close this tab and return to Goddard.",
            Ok(Some(code)),
        )
    } else {
        (
            "Missing authorization code. You can close this tab and return to Goddard.",
            Ok(None),
        )
    };
    let body = format!(
        "<!doctype html><title>Goddard</title><body style=\"font-family:system-ui;padding:3rem\">{html}</body>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    result
}

fn random_urlsafe(bytes: usize) -> String {
    let mut entropy = Vec::with_capacity(bytes);
    while entropy.len() < bytes {
        entropy.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    entropy.truncate(bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(entropy)
}

fn open_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("/usr/bin/open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/c", "start", "", url]);
        command
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("could not open the browser for authorization")?;
    Ok(())
}
