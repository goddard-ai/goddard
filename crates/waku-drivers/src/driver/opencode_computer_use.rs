//! Session instructions for the CLI on an adopted OpenCode service.
use crate::opencode_api;
use crate::opencode_service::OpenCodeService;
use std::sync::Arc;

pub(super) const INSTRUCTION_KEY: &str = "goddard-computer-use";

pub(super) fn tool_identity(name: &str) -> Option<(&str, &str)> {
    let (server, tool) = name
        .strip_suffix("_js_reset")
        .map(|server| (server, "js_reset"))
        .or_else(|| name.strip_suffix("_js").map(|server| (server, "js")))?;
    let id = server.strip_prefix("goddard_js_repl_")?;
    (id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some((server, tool))
}

/// Retire only the random, reserved names emitted by the former bridge.
/// User server names and all connected integration registrations are retained.
pub(super) fn cleanup_legacy(service: &Arc<OpenCodeService>, directory: &str, session: &str) {
    let endpoint = service.endpoint();
    let _ = opencode_api::remove_instruction_entry(&endpoint, session, INSTRUCTION_KEY);
    match opencode_api::list_mcp(&endpoint, directory) {
        Ok(servers) => {
            for server in servers {
                if let Some(name) = server["name"]
                    .as_str()
                    .filter(|name| legacy_server_name(name))
                {
                    if let Err(error) = opencode_api::remove_mcp(&endpoint, directory, name) {
                        eprintln!("could not remove legacy Goddard Computer Use server: {error}");
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("could not inspect legacy Goddard Computer Use registrations: {error}")
        }
    }
}

fn legacy_server_name(name: &str) -> bool {
    name.strip_prefix("goddard_js_repl_")
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_recognizes_only_the_old_generated_names() {
        assert!(legacy_server_name(
            "goddard_js_repl_0123456789abcdef0123456789abcdef"
        ));
        for name in [
            "goddard_js_repl",
            "goddard_js_repl_mine",
            "goddard_linear",
            "my_computer_use",
            "goddard_js_repl_0123456789abcdef0123456789abcdef_js",
        ] {
            assert!(!legacy_server_name(name), "{name}");
        }
    }
}
