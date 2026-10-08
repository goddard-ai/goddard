use std::{collections::BTreeMap, path::PathBuf};

use serde_json::{Map, Value, json};

/// The borrowed pieces of a [`McpServerSpec::Stdio`].
pub(crate) type StdioSpec<'a> = (
    &'a str,
    &'a PathBuf,
    &'a [String],
    &'a BTreeMap<String, String>,
);

/// A provider-neutral MCP server description. Provider drivers own the
/// translation from this transport shape to the provider's configuration.
#[derive(Clone)]
pub enum McpServerSpec {
    /// User-declared servers launched as provider subprocesses; connected
    /// catalog integrations use HTTP.
    Stdio {
        name: String,
        command: PathBuf,
        args: Vec<String>,
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
        args: Vec<String>,
        env: BTreeMap<String, String>,
    ) -> Self {
        Self::Stdio {
            name: name.into(),
            command: command.into(),
            args,
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

    pub(crate) fn stdio_parts(&self) -> Option<StdioSpec<'_>> {
        match self {
            Self::Stdio {
                name,
                command,
                args,
                env,
            } => Some((name, command, args, env)),
            Self::Http { .. } => None,
        }
    }

    pub(crate) fn stdio_config_value(&self) -> Option<Value> {
        let (_, command, args, env) = self.stdio_parts()?;
        Some(json!({
            "command": command,
            "args": args,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_stdio_and_http_integrations_keep_their_launch_config() {
        let servers = [
            McpServerSpec::stdio(
                "user_server",
                "/bin/custom-server",
                vec!["--serve".to_owned()],
                [("CUSTOM_ENV".into(), "value".into())].into(),
            ),
            McpServerSpec::http(
                "goddard_linear",
                "https://integration.invalid/mcp",
                "test-token",
            ),
        ];
        let config = config_map(&servers, "http");
        assert_eq!(config["user_server"]["command"], "/bin/custom-server");
        assert_eq!(config["user_server"]["args"][0], "--serve");
        assert_eq!(config["user_server"]["env"]["CUSTOM_ENV"], "value");
        assert_eq!(
            config["goddard_linear"]["url"],
            "https://integration.invalid/mcp"
        );
        let yaml = crate::integrations::deliver::deepseek_overlay_yaml(&servers);
        assert!(yaml.contains("id: user_server"));
        assert!(yaml.contains("transport: stdio"));
        assert!(yaml.contains("id: goddard_linear"));
        assert!(!yaml.contains("goddard-computer-use"));
    }
}
