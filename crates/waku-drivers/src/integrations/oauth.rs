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
pub fn access_token(secrets: &SecretStore, integration_id: &str) -> anyhow::Result<Option<String>> {
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
    let (metadata, resource) = discover(variant.url)?;
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
    // `localhost` rather than `127.0.0.1`: some providers (Supabase) reject
    // loopback-IP redirect URIs while accepting the `localhost` hostname.
    let redirect_uri = format!("http://localhost:{port}{CALLBACK_PATH}");

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
            .append_pair("resource", &resource);
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
        &resource,
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

#[derive(Deserialize)]
struct ResourceMetadata {
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    authorization_servers: Vec<String>,
}

/// Resolve the authorization server for `server_url` per RFC 9728: the
/// resource's own metadata names the servers that issue its tokens — which
/// is not always the same host (Supabase and Atlassian delegate elsewhere).
/// Falls back to metadata at the server's origin. Returns the AS metadata
/// and the RFC 8707 `resource` value to request a token for.
fn discover(server_url: &str) -> anyhow::Result<(ServerMetadata, String)> {
    let server = url::Url::parse(server_url).context("catalog entry has an invalid URL")?;
    for url in protected_resource_urls(&server) {
        let Ok(document) = get_json::<ResourceMetadata>(&url) else {
            continue;
        };
        for issuer in &document.authorization_servers {
            if let Ok(metadata) = discover_as(issuer) {
                let resource = document
                    .resource
                    .clone()
                    .unwrap_or_else(|| server_url.to_owned());
                return Ok((metadata, resource));
            }
        }
    }
    let issuer = server
        .join("/")
        .context("catalog entry has an invalid URL")?
        .to_string();
    Ok((discover_as(&issuer)?, server_url.to_owned()))
}

/// Candidate `/.well-known/oauth-protected-resource` documents: path-aware
/// first per RFC 9728, then the origin root.
fn protected_resource_urls(server: &url::Url) -> Vec<String> {
    let origin = server.origin().ascii_serialization();
    let path = if server.path() == "/" {
        ""
    } else {
        server.path()
    };
    let mut urls = vec![format!(
        "{origin}/.well-known/oauth-protected-resource{path}"
    )];
    if !path.is_empty() {
        urls.push(format!("{origin}/.well-known/oauth-protected-resource"));
    }
    urls
}

fn discover_as(issuer: &str) -> anyhow::Result<ServerMetadata> {
    for url in metadata_urls(issuer) {
        if let Ok(metadata) = get_json(&url) {
            return Ok(metadata);
        }
    }
    bail!("{issuer} does not publish OAuth metadata")
}

/// Candidate authorization-server metadata documents for `issuer`. RFC 8414
/// puts `.well-known` between the host and the issuer path; some servers
/// append it after the path instead.
fn metadata_urls(issuer: &str) -> Vec<String> {
    let issuer = issuer.trim_end_matches('/');
    let mut urls = Vec::new();
    for well_known in ["oauth-authorization-server", "openid-configuration"] {
        if let Ok(url) = url::Url::parse(issuer) {
            let origin = url.origin().ascii_serialization();
            let path = url.path().trim_end_matches('/');
            let candidate = format!("{origin}/.well-known/{well_known}{path}");
            if !urls.contains(&candidate) {
                urls.push(candidate);
            }
        }
        let candidate = format!("{issuer}/.well-known/{well_known}");
        if !urls.contains(&candidate) {
            urls.push(candidate);
        }
    }
    urls
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> anyhow::Result<T> {
    let job = CurlJob {
        method: "GET",
        url,
        headers: &[("Accept", "application/json".to_owned())],
        body_file: None,
        follow: true,
    };
    let response = http::request(&job)?;
    if response.status != 200 {
        bail!("{url} returned {}", response.status);
    }
    serde_json::from_slice(&response.body).context("OAuth metadata is unreadable")
}

fn register_client(endpoint: &str) -> anyhow::Result<(String, Option<String>)> {
    let body = serde_json::json!({
        "client_name": "Goddard",
        "redirect_uris": [
            format!("http://127.0.0.1{CALLBACK_PATH}"),
            format!("http://localhost{CALLBACK_PATH}"),
        ],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    // Providers that enforce redirect URIs follow RFC 8252: any loopback
    // port is accepted, but the path must match a registered URI exactly —
    // so these must carry CALLBACK_PATH, not a placeholder.
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
        follow: false,
    };
    let response = http::request(&job);
    let _ = std::fs::remove_file(&tmp);
    let response = response?;
    if !(200..300).contains(&response.status) {
        bail!("dynamic client registration failed ({})", response.status);
    }
    #[derive(Deserialize)]
    struct Registration {
        client_id: String,
        client_secret: Option<String>,
    }
    let registration: Registration =
        serde_json::from_slice(&response.body).context("registration response is unreadable")?;
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
        follow: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_resource_urls_are_path_aware_then_root() {
        let server = url::Url::parse("https://mcp.atlassian.com/v2/mcp").unwrap();
        assert_eq!(
            protected_resource_urls(&server),
            [
                "https://mcp.atlassian.com/.well-known/oauth-protected-resource/v2/mcp",
                "https://mcp.atlassian.com/.well-known/oauth-protected-resource",
            ]
        );
    }

    #[test]
    fn protected_resource_urls_drops_the_root_slash() {
        let server = url::Url::parse("https://mcp.stripe.com").unwrap();
        assert_eq!(
            protected_resource_urls(&server),
            ["https://mcp.stripe.com/.well-known/oauth-protected-resource"]
        );
    }

    #[test]
    fn metadata_urls_cover_both_well_known_placements() {
        assert_eq!(
            metadata_urls("https://auth.monday.com/mcp"),
            [
                "https://auth.monday.com/.well-known/oauth-authorization-server/mcp",
                "https://auth.monday.com/mcp/.well-known/oauth-authorization-server",
                "https://auth.monday.com/.well-known/openid-configuration/mcp",
                "https://auth.monday.com/mcp/.well-known/openid-configuration",
            ]
        );
    }
}
