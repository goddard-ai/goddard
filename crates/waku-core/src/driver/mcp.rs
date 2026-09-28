use std::{collections::BTreeMap, path::PathBuf};

use serde_json::{Map, Value, json};

/// A provider-neutral MCP server description. Provider drivers own the
/// translation from this transport shape to the provider's configuration.
#[derive(Clone)]
pub(crate) enum McpServerSpec {
    Stdio {
        name: String,
        command: PathBuf,
        env: BTreeMap<String, String>,
    },
    Http {
        name: String,
        url: String,
        token: String,
    },
}

impl McpServerSpec {
    pub(crate) fn stdio(
        name: impl Into<String>,
        command: impl Into<PathBuf>,
        env: BTreeMap<String, String>,
    ) -> Self {
        Self::Stdio {
            name: name.into(),
            command: command.into(),
            env,
        }
    }

    pub(crate) fn http(
        name: impl Into<String>,
        url: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self::Http {
            name: name.into(),
            url: url.into(),
            token: token.into(),
        }
    }

    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Stdio { name, .. } | Self::Http { name, .. } => name,
        }
    }

    pub(crate) fn http_parts(&self) -> Option<(&str, &str, &str)> {
        match self {
            Self::Http { name, url, token } => Some((name, url, token)),
            Self::Stdio { .. } => None,
        }
    }

    pub(crate) fn stdio_parts(&self) -> Option<(&str, &PathBuf, &BTreeMap<String, String>)> {
        match self {
            Self::Stdio { name, command, env } => Some((name, command, env)),
            Self::Http { .. } => None,
        }
    }

    pub(crate) fn stdio_config_value(&self) -> Option<Value> {
        let (_, command, env) = self.stdio_parts()?;
        Some(json!({
            "command": command,
            "args": [],
            "env": env,
        }))
    }

    pub(crate) fn http_config_value(&self, server_type: &str) -> Option<Value> {
        let (_, url, token) = self.http_parts()?;
        Some(json!({
            "type": server_type,
            "url": url,
            "headers": { "Authorization": format!("Bearer {token}") },
        }))
    }

    pub(crate) fn config_value(&self, http_type: &str) -> Value {
        match self {
            Self::Stdio { .. } => self
                .stdio_config_value()
                .expect("stdio spec has a stdio config value"),
            Self::Http { .. } => self
                .http_config_value(http_type)
                .expect("HTTP spec has an HTTP config value"),
        }
    }
}

pub(crate) fn config_map(servers: &[McpServerSpec], http_type: &str) -> Map<String, Value> {
    servers
        .iter()
        .map(|server| (server.name().to_owned(), server.config_value(http_type)))
        .collect()
}
