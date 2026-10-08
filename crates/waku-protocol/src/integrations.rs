//! MCP integrations: the wire + persisted shapes behind the settings pane.
//!
//! Goddard hosts a local proxy in the daemon; agents connect to it with a
//! per-install bearer token and the proxy injects the real upstream
//! credential. The catalog itself lives in `waku-core` — these types carry
//! what clients need to render it and what the settings document persists.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::ProviderKind;

/// How an integration authenticates upstream. `OauthOrApiKey` services accept
/// either path; the connect flow offers both.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum IntegrationAuthKind {
    Oauth,
    ApiKey,
    OauthOrApiKey,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationVariantInfo {
    pub id: String,
    pub label: String,
}

/// One catalog row as a client renders it. `url` stays daemon-side — a
/// client never needs the upstream address.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationInfo {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub auth: IntegrationAuthKind,
    pub variants: Vec<IntegrationVariantInfo>,
}

/// Whether the proxy currently holds a usable credential for the
/// integration. Persisted in settings so every attached client sees the same
/// state; the credential itself lives in the daemon's secret store.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum IntegrationAuthState {
    /// The service needs no authentication.
    Ready,
    /// A valid credential is stored and the proxy will inject it.
    Connected,
    /// No usable credential — the OAuth flow needs to run or an API key is
    /// missing.
    #[default]
    NeedsAuth,
}

/// The persisted user choice for one integration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationSetting {
    pub id: String,
    pub variant_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderKind>,
    #[serde(default)]
    pub auth: IntegrationAuthState,
}

/// Catalog row plus the user's current configuration for it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationSnapshot {
    #[serde(flatten)]
    pub info: IntegrationInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured: Option<IntegrationSetting>,
}

/// How the daemon reaches a user-declared MCP server. `Stdio` servers are
/// handed to each provider as a command to spawn; `Http` servers ride the
/// daemon's local proxy like catalog integrations, with `headers` — auth
/// included — injected upstream so provider configs only see the loopback
/// URL and proxy bearer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "transport", rename_all = "camelCase")]
pub enum McpServerTransport {
    Stdio {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
    },
    /// A remote server speaking streamable HTTP. Header values persist in
    /// the settings document — plaintext like the proxy token — rather than
    /// the daemon's secret store.
    Http {
        url: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, String>,
    },
}

/// A user-declared external MCP server, added on the Integrations settings
/// page or by hand in the settings document. Unlike catalog entries these
/// carry no bundled metadata or OAuth flow; the proxy exposes remote ones
/// under `/mcp/<id>` and persona grants key on `id` the same way.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpServerSetting {
    /// Stable slug: the proxy endpoint segment and the delivered
    /// `goddard_<id>` server name. Lowercase letters, digits, `-` and `_`.
    pub id: String,
    /// Display label on the settings page; `id` renders when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub transport: McpServerTransport,
    /// Providers that receive the server; empty means none, matching
    /// `IntegrationSetting::providers`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderKind>,
}

impl McpServerSetting {
    /// What the settings page shows when `name` was left blank.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}
