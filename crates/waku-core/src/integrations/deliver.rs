//! Provider delivery: sync each file-based provider's MCP config with the
//! integrations the user enabled for it, and produce the launch-time forms
//! (`-c` flags, env config, API calls) for providers Goddard injects at
//! spawn.
//!
//! File writers are additive and idempotent: entries are keyed
//! `goddard_<id>`; sync rewrites exactly that managed set and leaves every
//! unrelated key — and the file itself when unreadable — untouched.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, anyhow};
use serde_json::{Map, Value};
use waku_protocol::DaemonSettings;
use waku_protocol::model::ProviderKind;

use crate::driver::McpServerSpec;

/// Providers whose integration delivery is a config file Goddard rewrites at
/// connect/disconnect time. The rest are launch-injected per session.
const FILE_PROVIDERS: &[ProviderKind] = &[
    ProviderKind::Claude,
    ProviderKind::Copilot,
    ProviderKind::Cursor,
    ProviderKind::Amp,
    ProviderKind::Droid,
    ProviderKind::Devin,
    ProviderKind::Kimi,
    ProviderKind::Fx,
    ProviderKind::Antigravity,
    ProviderKind::OhMyPi,
    ProviderKind::Muse,
    ProviderKind::Grok,
];

const ACP_PROVIDERS: &[ProviderKind] = &[
    ProviderKind::Cursor,
    ProviderKind::Devin,
    ProviderKind::Droid,
    ProviderKind::Fx,
    ProviderKind::Grok,
    ProviderKind::Kimi,
];

pub(super) fn uses_file_sync(provider: ProviderKind) -> bool {
    FILE_PROVIDERS.contains(&provider)
}

pub(crate) fn uses_acp(provider: ProviderKind) -> bool {
    ACP_PROVIDERS.contains(&provider)
}

/// Rewrite every file provider's managed entries from `settings`. Called at
/// startup — the proxy's port is ephemeral, so entries written by an earlier
/// daemon point at a dead listener until this runs — and after
/// connect/disconnect/provider-set/settings changes. With the experiment
/// off, every provider's desired set is empty, which strips leftover
/// `goddard_*` entries. Errors for one provider are logged and do not stop
/// the others — a broken config file should not wedge the pane.
pub fn sync_file_providers(settings: &DaemonSettings, service: &super::IntegrationService) {
    for provider in FILE_PROVIDERS {
        sync_file_provider(*provider, settings, service);
    }
}

pub(super) fn sync_file_provider(
    provider: ProviderKind,
    settings: &DaemonSettings,
    service: &super::IntegrationService,
) {
    let entries = desired_entries(settings, service, provider);
    if let Err(error) = sync_provider(provider, &entries) {
        eprintln!(
            "goddard-mcp: could not sync {} config: {error:#}",
            provider.display_name()
        );
    }
}

/// The `goddard_<id>` → server-config map one provider should carry.
fn desired_entries(
    settings: &DaemonSettings,
    service: &super::IntegrationService,
    provider: ProviderKind,
) -> BTreeMap<String, Value> {
    if !settings.integrations_enabled || service.http_mcp_supported(provider) {
        return BTreeMap::new();
    }
    settings
        .integrations
        .iter()
        .filter(|setting| setting.providers.contains(&provider))
        .map(|setting| {
            let url = service.endpoint_url(&setting.id);
            let token = service.proxy_token();
            let server =
                McpServerSpec::http(super::server_name(&setting.id), url, token.to_owned());
            (server.name().to_owned(), server_entry(provider, &server))
        })
        .collect()
}

/// The server object in one provider's dialect.
fn server_entry(provider: ProviderKind, server: &McpServerSpec) -> Value {
    let Some((_, url, token)) = server.http_parts() else {
        unreachable!("file integration delivery requires an HTTP MCP server")
    };
    let bearer = format!("Bearer {token}");
    match provider {
        ProviderKind::Antigravity => serde_json::json!({
            "serverUrl": url,
            "headers": { "Authorization": bearer },
        }),
        ProviderKind::Fx => serde_json::json!({
            "type": "http",
            "url": url,
            "headers": { "Authorization": bearer },
        }),
        ProviderKind::Kimi => serde_json::json!({
            "url": url,
            "transport": "http",
            "headers": { "Authorization": bearer },
        }),
        ProviderKind::Muse => serde_json::json!({
            "url": url,
            "headers": { "Authorization": bearer },
            "enabled": true,
            "mode": "optional",
        }),
        _ => serde_json::json!({
            "type": "http",
            "url": url,
            "headers": { "Authorization": bearer },
        }),
    }
}

fn sync_provider(provider: ProviderKind, entries: &BTreeMap<String, Value>) -> anyhow::Result<()> {
    match provider {
        ProviderKind::Grok => sync_grok(entries),
        _ => {
            let Some((path, key_path)) = json_target(provider) else {
                return Err(anyhow!("no config target for {}", provider.id()));
            };
            sync_json(&path, &key_path, entries)
        }
    }
}

/// The config file and the key path holding its server map.
fn json_target(provider: ProviderKind) -> Option<(PathBuf, &'static [&'static str])> {
    let home = dirs::home_dir()?;
    let (path, key_path): (PathBuf, &[&'static str]) = match provider {
        ProviderKind::Claude => (home.join(".claude.json"), &["mcpServers"]),
        ProviderKind::Copilot => (home.join(".copilot/mcp-config.json"), &["mcpServers"]),
        ProviderKind::Cursor => (home.join(".cursor/mcp.json"), &["mcpServers"]),
        ProviderKind::Amp => (
            home.join(".config/amp/settings.json"),
            &["amp", "mcpServers"],
        ),
        ProviderKind::Droid => (home.join(".factory/mcp.json"), &["mcpServers"]),
        ProviderKind::Devin => (home.join(".config/devin/mcp_config.json"), &["mcpServers"]),
        ProviderKind::Kimi => (home.join(".kimi/mcp.json"), &["mcpServers"]),
        ProviderKind::Fx => (home.join(".fx/mcp.json"), &["mcp"]),
        ProviderKind::Antigravity => (home.join(".gemini/config/mcp_config.json"), &["mcpServers"]),
        ProviderKind::OhMyPi => (home.join(".omp/agent/mcp.json"), &["mcpServers"]),
        ProviderKind::Muse => (home.join(".muse/settings.json"), &["mcp_servers"]),
        _ => return None,
    };
    Some((path, key_path))
}

/// Merge `entries` into the object at `key_path` in `path`, replacing only
/// `goddard_*` keys. An unreadable or non-object document is left alone —
/// Goddard never clobbers a file it did not write.
fn sync_json(
    path: &PathBuf,
    key_path: &[&str],
    entries: &BTreeMap<String, Value>,
) -> anyhow::Result<()> {
    let mut document: Value = match fs::read(path) {
        // Some CLIs create this file before they have any settings to write.
        // Treat an empty file as a fresh config, while keeping malformed
        // non-empty files protected from accidental overwrite.
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Value::Object(Map::new()),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("{} is not valid JSON; skipping", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };
    let mut node = document
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not a JSON object; skipping", path.display()))?;
    for key in key_path {
        node = node
            .entry((*key).to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| anyhow!("{}.{key} is not an object; skipping", path.display()))?;
    }
    let had_managed = node.keys().any(|name| name.starts_with("goddard_"));
    node.retain(|name, _| !name.starts_with("goddard_"));
    if entries.is_empty() && !had_managed {
        // Nothing managed to add or strip: leave the file — including a
        // missing one — untouched.
        return Ok(());
    }
    for (name, entry) in entries {
        node.insert(name.clone(), entry.clone());
    }
    write_atomic_json(path, &document)
}

/// Grok's config is TOML and Codex-style `[mcp_servers.<name>]` tables.
fn sync_grok(entries: &BTreeMap<String, Value>) -> anyhow::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("home directory is unavailable"))?;
    let path = home.join(".grok/config.toml");
    let mut root: toml::Table = match fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => toml::from_str(&content)
            .with_context(|| format!("{} is not valid TOML; skipping", path.display()))?,
        Ok(_) => toml::Table::new(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };
    let servers = root
        .entry("mcp_servers")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("{}.mcp_servers is not a table; skipping", path.display()))?;
    let had_managed = servers.keys().any(|name| name.starts_with("goddard_"));
    servers.retain(|name, _| !name.starts_with("goddard_"));
    if entries.is_empty() && !had_managed {
        return Ok(());
    }
    for (name, entry) in entries {
        let url = entry
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("integration entry {name} has no url"))?;
        let token = entry
            .pointer("/headers/Authorization")
            .and_then(Value::as_str)
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or_default();
        let mut headers = toml::Table::new();
        headers.insert(
            "Authorization".to_owned(),
            toml::Value::String(format!("Bearer {token}")),
        );
        let mut table = toml::Table::new();
        table.insert("url".to_owned(), toml::Value::String(url.to_owned()));
        table.insert("http_headers".to_owned(), toml::Value::Table(headers));
        table.insert("enabled".to_owned(), toml::Value::Boolean(true));
        servers.insert(name.clone(), toml::Value::Table(table));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, toml::to_string(&root)?)?;
    fs::rename(temporary, &path)?;
    Ok(())
}

pub(super) fn write_atomic_json(path: &PathBuf, document: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(document)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

/// Codex launch flags for one integration: the URL and an env-var-referenced
/// bearer, so the token stays out of argv.
pub(crate) fn codex_config_args(server: &McpServerSpec) -> Vec<String> {
    let mut args = Vec::new();
    if let Some((name, command, env)) = server.stdio_parts() {
        args.extend([
            "-c".to_owned(),
            format!(
                "mcp_servers.{name}.command={}",
                toml_string(&command.display().to_string())
            ),
            "-c".to_owned(),
            format!("mcp_servers.{name}.args=[]"),
        ]);
        for (key, value) in env {
            args.extend([
                "-c".to_owned(),
                format!("mcp_servers.{name}.env.{key}={}", toml_string(value)),
            ]);
        }
    } else if let Some((name, url, _)) = server.http_parts() {
        args.extend([
            "-c".to_owned(),
            format!("mcp_servers.{name}.url={url}"),
            "-c".to_owned(),
            format!("mcp_servers.{name}.bearer_token_env_var=GODDARD_MCP_PROXY_TOKEN"),
        ]);
    }
    args
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

/// OpenCode's `mcp` map entries for a set of launch MCP servers.
pub(crate) fn opencode_config_entries(servers: &[McpServerSpec]) -> Map<String, Value> {
    let mut mcp = Map::new();
    for server in servers {
        let value = if let Some((_, command, env)) = server.stdio_parts() {
            serde_json::json!({
                "type": "local",
                "command": [command.display().to_string()],
                "enabled": true,
                "environment": env,
            })
        } else if let Some((_, url, token)) = server.http_parts() {
            serde_json::json!({
                "type": "remote",
                "url": url,
                "enabled": true,
                "headers": { "Authorization": format!("Bearer {token}") },
            })
        } else {
            continue;
        };
        mcp.insert(server.name().to_owned(), value);
    }
    mcp
}

/// One DeepSeek Cordis overlay row per integration — `dsh web --patch` takes
/// the generated file at launch.
pub(crate) fn deepseek_overlay_yaml(servers: &[McpServerSpec]) -> String {
    let mut yaml = String::new();
    for server in servers {
        if let Some((name, url, token)) = server.http_parts() {
            yaml.push_str(&format!(
                "- id: {name}\n  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n    serverName: {name}\n    transport: streamable-http\n    url: {}\n    headers:\n      Authorization: {}\n",
                serde_json::to_string(url).expect("URL is valid JSON"),
                serde_json::to_string(&format!("Bearer {token}"))
                    .expect("authorization header is valid JSON"),
            ));
        } else if let Some((name, command, env)) = server.stdio_parts() {
            let id = if name == crate::driver::MCP_SERVER_NAME {
                "goddard-computer-use"
            } else {
                name
            };
            yaml.push_str(&format!(
                "- id: {id}\n  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n    serverName: {name}\n    transport: stdio\n    command: {}\n    args: []\n    env:\n",
                serde_json::to_string(&command.display().to_string())
                    .expect("command path is valid JSON"),
            ));
            for (key, value) in env {
                yaml.push_str(&format!(
                    "      {key}: {}\n",
                    serde_json::to_string(value).expect("environment value is valid JSON"),
                ));
            }
            if name == crate::driver::MCP_SERVER_NAME {
                yaml.push_str("    toolCallTimeoutMs: 300000\n");
            }
        }
    }
    yaml
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("goddard-deliver-{name}-{}", Uuid::new_v4()))
    }

    fn entries() -> BTreeMap<String, Value> {
        BTreeMap::from([(
            "goddard_linear".to_owned(),
            serde_json::json!({
                "type": "http",
                "url": "http://127.0.0.1:9999/mcp/linear",
                "headers": { "Authorization": "Bearer gmi-test" },
            }),
        )])
    }

    #[test]
    fn sync_json_writes_entries_into_an_empty_file() {
        let path = temp_path("empty.json");
        sync_json(&path, &["mcpServers"], &entries()).unwrap();
        let document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            document["mcpServers"]["goddard_linear"]["url"],
            "http://127.0.0.1:9999/mcp/linear"
        );
        fs::remove_file(path).ok();
    }

    #[test]
    fn sync_json_preserves_unrelated_keys_and_replaces_managed_ones() {
        let path = temp_path("merge.json");
        fs::write(
            &path,
            r#"{
                "theme": "dark",
                "mcpServers": {
                    "mine": { "url": "https://example.com" },
                    "goddard_stale": { "url": "http://127.0.0.1:1/mcp/stale" }
                }
            }"#,
        )
        .unwrap();
        sync_json(&path, &["mcpServers"], &entries()).unwrap();
        let document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(document["theme"], "dark");
        assert_eq!(document["mcpServers"]["mine"]["url"], "https://example.com");
        assert!(document["mcpServers"].get("goddard_stale").is_none());
        assert!(document["mcpServers"].get("goddard_linear").is_some());
        fs::remove_file(path).ok();
    }

    #[test]
    fn sync_json_refuses_a_non_object_document() {
        let path = temp_path("broken.json");
        fs::write(&path, "[1, 2, 3]").unwrap();
        assert!(sync_json(&path, &["mcpServers"], &entries()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[1, 2, 3]");
        fs::remove_file(path).ok();
    }

    #[test]
    fn sync_json_with_no_entries_only_clears_managed_keys() {
        let path = temp_path("clear.json");
        fs::write(
            &path,
            r#"{"mcpServers": {"goddard_linear": {}, "mine": {"url": "x"}}}"#,
        )
        .unwrap();
        sync_json(&path, &["mcpServers"], &BTreeMap::new()).unwrap();
        let document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(document["mcpServers"].get("goddard_linear").is_none());
        assert!(document["mcpServers"].get("mine").is_some());
        fs::remove_file(path).ok();
    }

    #[test]
    fn sync_json_with_no_entries_does_not_create_a_missing_file() {
        let path = temp_path("nocreate.json");
        sync_json(&path, &["mcpServers"], &BTreeMap::new()).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn sync_json_with_no_entries_leaves_an_unmanaged_file_untouched() {
        let path = temp_path("untouched.json");
        let raw = r#"{"mcpServers": {"mine": {"url": "x"}}}"#;
        fs::write(&path, raw).unwrap();
        sync_json(&path, &["mcpServers"], &BTreeMap::new()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        fs::remove_file(path).ok();
    }

    #[test]
    fn codex_args_keep_the_token_out_of_argv() {
        let server =
            McpServerSpec::http("goddard_linear", "http://127.0.0.1:9/mcp/linear", "secret");
        let args = codex_config_args(&server);
        assert!(args.iter().any(|arg| arg.contains("bearer_token_env_var")));
        assert!(!args.iter().any(|arg| arg.contains("secret")));
    }

    #[test]
    fn deepseek_overlay_is_one_plugin_row_per_integration() {
        let servers = vec![McpServerSpec::http(
            "goddard_linear",
            "http://127.0.0.1:9/mcp/linear",
            "tok",
        )];
        let yaml = deepseek_overlay_yaml(&servers);
        assert!(yaml.contains("@deepseek-ai/dsh-mcp-client"));
        assert!(yaml.contains("transport: streamable-http"));
        assert!(yaml.contains("Bearer tok"));
    }
}
