//! The bundled catalog of official remote MCP servers.
//!
//! Entries are hand-reviewed and ship with the app; only services reachable
//! through dynamic client registration or a pasted API key belong here until
//! a pre-registered Goddard OAuth client exists for them.

use waku_protocol::integrations::{IntegrationAuthKind, IntegrationInfo, IntegrationVariantInfo};

pub struct CatalogVariant {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    /// OAuth scope override for this variant; `None` lets the server decide.
    pub scopes: Option<&'static str>,
}

pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub auth: IntegrationAuthKind,
    pub variants: &'static [CatalogVariant],
    /// A pre-registered Goddard OAuth client for services without dynamic
    /// client registration. Empty until apps are registered per service.
    pub oauth_client_id: Option<&'static str>,
    pub oauth_client_secret: Option<&'static str>,
}

const fn variant(id: &'static str, label: &'static str, url: &'static str) -> CatalogVariant {
    CatalogVariant {
        id,
        label,
        url,
        scopes: None,
    }
}

const fn entry(
    id: &'static str,
    name: &'static str,
    summary: &'static str,
    auth: IntegrationAuthKind,
    variants: &'static [CatalogVariant],
) -> CatalogEntry {
    CatalogEntry {
        id,
        name,
        summary,
        auth,
        variants,
        oauth_client_id: None,
        oauth_client_secret: None,
    }
}

static CATALOG: &[CatalogEntry] = &[
    entry(
        "linear",
        "Linear",
        "Issues, projects, and comments",
        IntegrationAuthKind::Oauth,
        &[
            CatalogVariant {
                scopes: None,
                ..variant("read-write", "Read & write", "https://mcp.linear.app/mcp")
            },
            CatalogVariant {
                scopes: Some("read"),
                ..variant(
                    "read-only",
                    "Read-only",
                    "https://mcp.linear.app/mcp/readonly",
                )
            },
        ],
    ),
    entry(
        "github",
        "GitHub",
        "Repositories, issues, pull requests, and code search",
        IntegrationAuthKind::OauthOrApiKey,
        &[variant(
            "default",
            "Default",
            "https://api.githubcopilot.com/mcp/",
        )],
    ),
    entry(
        "notion",
        "Notion",
        "Search, read, and edit pages and databases",
        IntegrationAuthKind::Oauth,
        &[variant("default", "Default", "https://mcp.notion.com/mcp")],
    ),
    entry(
        "sentry",
        "Sentry",
        "Issues, traces, and release health",
        IntegrationAuthKind::Oauth,
        &[variant("default", "Default", "https://mcp.sentry.dev/mcp")],
    ),
    entry(
        "figma",
        "Figma",
        "Design context, components, and code Connect",
        IntegrationAuthKind::Oauth,
        &[variant("default", "Default", "https://mcp.figma.com/mcp")],
    ),
    entry(
        "stripe",
        "Stripe",
        "Payments, customers, and subscription data",
        IntegrationAuthKind::OauthOrApiKey,
        &[variant("default", "Default", "https://mcp.stripe.com")],
    ),
    entry(
        "supabase",
        "Supabase",
        "Projects, tables, edge functions, and logs",
        IntegrationAuthKind::OauthOrApiKey,
        &[
            variant("default", "Read & write", "https://mcp.supabase.com/mcp"),
            variant(
                "read-only",
                "Read-only",
                "https://mcp.supabase.com/mcp?read_only=true",
            ),
        ],
    ),
    entry(
        "vercel",
        "Vercel",
        "Projects, deployments, and logs",
        IntegrationAuthKind::Oauth,
        &[variant("default", "Default", "https://mcp.vercel.com")],
    ),
    entry(
        "atlassian",
        "Atlassian",
        "Jira, Confluence, and Compass via Rovo",
        IntegrationAuthKind::Oauth,
        &[variant(
            "default",
            "Default",
            "https://mcp.atlassian.com/v2/mcp",
        )],
    ),
    entry(
        "monday",
        "monday.com",
        "Boards, items, and updates",
        IntegrationAuthKind::OauthOrApiKey,
        &[variant("default", "Default", "https://mcp.monday.com/mcp")],
    ),
];

pub fn catalog() -> &'static [CatalogEntry] {
    CATALOG
}

pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

impl CatalogEntry {
    pub fn variant(&self, variant_id: &str) -> Option<&'static CatalogVariant> {
        self.variants
            .iter()
            .find(|variant| variant.id == variant_id)
    }

    pub fn default_variant(&self) -> &'static CatalogVariant {
        &self.variants[0]
    }

    pub fn info(&self) -> IntegrationInfo {
        IntegrationInfo {
            id: self.id.to_owned(),
            name: self.name.to_owned(),
            summary: self.summary.to_owned(),
            auth: self.auth,
            variants: self
                .variants
                .iter()
                .map(|variant| IntegrationVariantInfo {
                    id: variant.id.to_owned(),
                    label: variant.label.to_owned(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique_valid_and_url_backed() {
        let mut ids = std::collections::HashSet::new();
        for entry in catalog() {
            assert!(ids.insert(entry.id), "duplicate id {}", entry.id);
            assert!(
                entry
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} is not a valid server-name fragment",
                entry.id
            );
            assert!(!entry.variants.is_empty(), "{} has no variants", entry.id);
            for variant in entry.variants {
                assert!(
                    url::Url::parse(variant.url).is_ok(),
                    "{} variant {} has an invalid URL",
                    entry.id,
                    variant.id
                );
            }
        }
    }
}
