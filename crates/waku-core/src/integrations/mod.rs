//! MCP integrations: a bundled catalog of official remote servers, a
//! daemon-hosted local proxy that injects credentials, and the per-provider
//! delivery that points agents at it.

pub mod catalog;
mod http;
mod oauth;
mod proxy;
mod secrets;
mod service;

pub use oauth::StoredCredential;
pub(crate) use service::Inner;
pub use service::{IntegrationService, LaunchIntegration, Upstream};

/// Server name agents see for a delivered integration: `goddard_linear`,
/// `goddard_github`, … The prefix keeps delivered entries distinct from
/// servers the user configured by hand.
pub fn server_name(integration_id: &str) -> String {
    format!("goddard_{integration_id}")
}
