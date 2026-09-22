//! Provider fallback choices used before daemon-side discovery completes.

use crate::model::{ProviderAgentPreset, ProviderKind, ProviderModel, ProviderModelOption};

pub fn fallback_models(provider: ProviderKind) -> Vec<ProviderModel> {
    match provider {
        ProviderKind::Amp => [
            ProviderModel::new("low", tr!("model_option.low")),
            ProviderModel::new("medium", tr!("model_option.medium")).default(),
            ProviderModel::new("high", tr!("model_option.high")),
            ProviderModel::new("ultra", tr!("model_option.ultra")),
        ]
        .into_iter()
        .map(|model| {
            model.service_tiers(
                [ProviderModelOption::new("fast", tr!("model_option.fast"))
                    .description(tr!("model_option.amp_fast_description"))],
                "default",
            )
        })
        .collect(),
        ProviderKind::Codex => [
            ProviderModel::new("gpt-5.6-sol", "GPT-5.6-Sol").default(),
            ProviderModel::new("gpt-5.6-terra", "GPT-5.6-Terra"),
            ProviderModel::new("gpt-5.6-luna", "GPT-5.6-Luna"),
            ProviderModel::new("gpt-5.5", "GPT-5.5"),
            ProviderModel::new("gpt-5.4", "GPT-5.4"),
        ]
        .into_iter()
        .map(|model| {
            model
                .reasoning(
                    reasoning_options(["low", "medium", "high", "xhigh"]),
                    "medium",
                )
                .service_tiers(
                    [ProviderModelOption::new("fast", tr!("model_option.fast"))
                        .description(tr!("model_option.fast_description"))],
                    "default",
                )
        })
        .collect(),
        ProviderKind::Claude => vec![
            claude_long_context(claude_ultracode_model("claude-fable-5", "Claude Fable 5")),
            claude_long_context(claude_ultracode_model("claude-opus-5", "Claude Opus 5")),
            claude_long_context(claude_ultracode_model("claude-opus-4-8", "Claude Opus 4.8")),
            claude_long_context(claude_ultracode_model("claude-opus-4-7", "Claude Opus 4.7")),
            claude_long_context(claude_reasoning_model("claude-opus-4-6", "Claude Opus 4.6")),
            claude_reasoning_model("claude-opus-4-5", "Claude Opus 4.5"),
            claude_long_context(claude_ultracode_model("claude-sonnet-5", "Claude Sonnet 5"))
                .default(),
            claude_long_context(claude_reasoning_model(
                "claude-sonnet-4-6",
                "Claude Sonnet 4.6",
            )),
            ProviderModel::new("claude-haiku-4-5", "Claude Haiku 4.5"),
        ],
        // Copilot's catalog is subscription- and BYOK-route-specific; the SDK's
        // `models.list` is authoritative. `auto` names the runtime's own
        // routing and is valid on every account.
        ProviderKind::Copilot => {
            vec![ProviderModel::new("auto", tr!("model_option.auto")).default()]
        }
        ProviderKind::Cursor => {
            vec![ProviderModel::new("auto", tr!("model_option.auto")).default()]
        }
        // Droid's catalog is account-specific (BYOK routes and open models
        // come and go), so only the Factory-router default stands in before
        // discovery answers; `auto` is valid on every account.
        ProviderKind::Droid => {
            vec![ProviderModel::new("auto", tr!("model_option.auto")).default()]
        }
        ProviderKind::Antigravity
        | ProviderKind::Devin
        | ProviderKind::DeepSeek
        | ProviderKind::Fx
        | ProviderKind::Goose
        | ProviderKind::Grok
        | ProviderKind::Kimi
        | ProviderKind::Muse
        | ProviderKind::OpenCode
        | ProviderKind::OpenCode2
        | ProviderKind::OhMyPi
        | ProviderKind::Pi => Vec::new(),
    }
}

pub fn fallback_agent_presets(provider: ProviderKind) -> Vec<ProviderAgentPreset> {
    if provider != ProviderKind::DeepSeek {
        return Vec::new();
    }
    vec![
        ProviderAgentPreset::new("standard", tr!("agent_preset.standard"))
            .description(tr!("agent_preset.standard_description"))
            .default(),
        ProviderAgentPreset::new("code", tr!("agent_preset.code"))
            .description(tr!("agent_preset.code_description")),
        ProviderAgentPreset::new("minimal", tr!("agent_preset.minimal"))
            .description(tr!("agent_preset.minimal_description")),
        ProviderAgentPreset::new("cordis", tr!("agent_preset.creator"))
            .description(tr!("agent_preset.creator_description")),
    ]
}

fn reasoning_effort_label(effort: &str) -> String {
    match effort {
        "none" => tr!("model_option.none"),
        "minimal" => tr!("model_option.minimal"),
        "low" => tr!("model_option.low"),
        "medium" => tr!("model_option.medium"),
        "high" => tr!("model_option.high"),
        "xhigh" => tr!("model_option.extra_high"),
        "max" => tr!("model_option.max"),
        "ultra" => tr!("model_option.ultra"),
        "ultracode" => tr!("model_option.ultracode"),
        other => other.replace(['-', '_'], " "),
    }
}

fn reasoning_options<const N: usize>(efforts: [&str; N]) -> Vec<ProviderModelOption> {
    efforts
        .into_iter()
        .map(|effort| ProviderModelOption::new(effort, reasoning_effort_label(effort)))
        .collect()
}

/// The exact Grok models the hardcoded reasoning menu is known to cover.
/// `grok models` also lists user-defined custom models, whose effort support
/// is not knowable from the ID, so they get no menu. Discovery prefers the
/// CLI's own cached catalog; this table only stands in when that cache is
/// absent or does not describe the model.
pub fn grok_model_reasoning_efforts(id: &str) -> Option<&'static [&'static str]> {
    match id.to_ascii_lowercase().as_str() {
        "grok-4.5" => Some(&["low", "medium", "high"]),
        "grok-4.6" | "grok-4.7" => Some(&["low", "medium", "high", "xhigh"]),
        _ => None,
    }
}

/// A CLI or session model id resolved against advertised base values.
///
/// Providers often ship public IDs that are a base slug plus a hyphenated
/// suffix (`thinking`, `xhigh`, `fast`). A parameterized picker advertises the
/// base slug, and the unconsumed suffix is applied as separate config options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedModelSelection {
    pub value: String,
    pub suffix: String,
}

/// Catalog entry that matches a stored model id, including exploded aliases
/// such as `grok-4.6-xhigh-fast`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedCatalogMatch<'a> {
    pub model: &'a ProviderModel,
    pub suffix: String,
}

/// CLI-facing aliases for a requested Cursor model id.
///
/// Cursor's CLI spells a few families as `claude-4.6-sonnet-*` while ACP
/// advertises `claude-sonnet-4-6`. `auto` and `default` name the same route.
pub fn cursor_model_aliases(requested: &str) -> Vec<String> {
    let mut aliases = vec![requested.to_owned()];
    if let Some(alias) = requested.strip_prefix("cursor-") {
        aliases.push(alias.to_owned());
    }
    match requested {
        "auto" => aliases.push("default".to_owned()),
        "default" => aliases.push("auto".to_owned()),
        _ => {}
    }
    if let Some(rest) = requested.strip_prefix("claude-")
        && let Some((version, family_and_suffix)) = rest.split_once('-')
    {
        let (family, suffix) = family_and_suffix
            .split_once('-')
            .map_or((family_and_suffix, ""), |(family, suffix)| (family, suffix));
        if matches!(family, "haiku" | "opus" | "sonnet") {
            let mut alias = format!("claude-{family}-{}", version.replace('.', "-"));
            if !suffix.is_empty() {
                alias.push('-');
                alias.push_str(suffix);
            }
            if !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
    }
    aliases
}

/// The spellings a stored id can take for `provider` — its own id for most,
/// plus Cursor's re-spelled families (`claude-4.6-sonnet-*` for
/// `claude-sonnet-4-6`, `cursor-` prefixes, `auto`/`default`).
fn packed_model_aliases(provider: ProviderKind, requested: &str) -> Vec<String> {
    match provider {
        ProviderKind::Cursor => cursor_model_aliases(requested),
        _ => vec![requested.to_owned()],
    }
}

/// Resolves a requested model id against advertised base values.
///
/// Exact matches win. Otherwise the longest advertised value that is a hyphen
/// prefix of an alias is used, and the remainder is the variant suffix.
pub fn resolve_packed_model<'a, I>(
    values: I,
    requested: &str,
    provider: ProviderKind,
) -> Option<PackedModelSelection>
where
    I: IntoIterator<Item = &'a str>,
{
    let values: Vec<&str> = values.into_iter().collect();
    let aliases = packed_model_aliases(provider, requested);
    for alias in &aliases {
        if let Some(value) = values.iter().find(|value| **value == *alias) {
            return Some(PackedModelSelection {
                value: (*value).to_owned(),
                suffix: String::new(),
            });
        }
    }
    aliases
        .iter()
        .flat_map(|alias| {
            values.iter().filter_map(move |value| {
                alias
                    .strip_prefix(*value)
                    .and_then(|suffix| suffix.strip_prefix('-'))
                    .map(|suffix| PackedModelSelection {
                        value: (*value).to_owned(),
                        suffix: suffix.to_owned(),
                    })
            })
        })
        .max_by_key(|selection| selection.value.len())
}

pub fn packed_catalog_model<'a>(
    models: &'a [ProviderModel],
    requested: &str,
    provider: ProviderKind,
) -> Option<PackedCatalogMatch<'a>> {
    let selection = resolve_packed_model(
        models.iter().map(|model| model.id.as_str()),
        requested,
        provider,
    )?;
    let model = models.iter().find(|model| model.id == selection.value)?;
    Some(PackedCatalogMatch {
        model,
        suffix: selection.suffix,
    })
}

/// `extra-high` and `xhigh` spell the same rung. Goddard stores the latter so
/// the picker label and Codex-style ladder stay one vocabulary.
pub fn normalize_reasoning_effort(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    match normalized.as_str() {
        "extra-high" | "xhigh" => "xhigh".to_owned(),
        _ => normalized,
    }
}

pub fn packed_suffix_has(suffix: &str, value: &str) -> bool {
    suffix.split('-').any(|part| part == value)
}

pub fn packed_suffix_reasoning_effort(
    suffix: &str,
    efforts: &[ProviderModelOption],
) -> Option<String> {
    if suffix.is_empty() || efforts.is_empty() {
        return None;
    }
    if (suffix.contains("extra-high") || packed_suffix_has(suffix, "xhigh"))
        && efforts.iter().any(|option| option.id == "xhigh")
    {
        return Some("xhigh".to_owned());
    }
    efforts.iter().find_map(|option| {
        (packed_suffix_has(suffix, &option.id)
            || (option.id == "xhigh" && suffix.contains("extra-high")))
        .then(|| option.id.clone())
    })
}

pub fn packed_suffix_service_tier(suffix: &str, tiers: &[ProviderModelOption]) -> Option<String> {
    (packed_suffix_has(suffix, "fast") && tiers.iter().any(|option| option.id == "fast"))
        .then(|| "fast".to_owned())
}

fn claude_reasoning_model(id: &str, name: &str) -> ProviderModel {
    ProviderModel::new(id, name).reasoning(
        reasoning_options(["low", "medium", "high", "xhigh", "max"]),
        "high",
    )
}

/// Mirrors the daemon catalog: `ultracode` resolves to xhigh plus standing
/// dynamic-workflow orchestration, so it is only offered on xhigh-capable
/// models.
fn claude_ultracode_model(id: &str, name: &str) -> ProviderModel {
    ProviderModel::new(id, name).reasoning(
        reasoning_options(["low", "medium", "high", "xhigh", "max", "ultracode"]),
        "high",
    )
}

/// Mirrors the daemon catalog: the 1M window is opt-in behind a `[1m]` model-id
/// suffix the CLI refuses on its older models.
fn claude_long_context(model: ProviderModel) -> ProviderModel {
    model.context_windows(
        [
            ProviderModelOption::new("200k", tr!("model_option.context_200k")),
            ProviderModelOption::new("1m", tr!("model_option.context_1m")),
        ],
        "200k",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_reasoning_menu_covers_only_exact_builtins() {
        assert_eq!(
            grok_model_reasoning_efforts("grok-4.5"),
            Some(&["low", "medium", "high"][..])
        );
        assert_eq!(
            grok_model_reasoning_efforts("grok-4.6"),
            Some(&["low", "medium", "high", "xhigh"][..])
        );
        assert_eq!(
            grok_model_reasoning_efforts("grok-4.7"),
            Some(&["low", "medium", "high", "xhigh"][..])
        );
        // Custom models and unknown spellings get no menu.
        assert_eq!(grok_model_reasoning_efforts("grok-build"), None);
        assert_eq!(grok_model_reasoning_efforts("my-custom-test"), None);
        assert_eq!(grok_model_reasoning_efforts("grok-4-6"), None);
    }

    #[test]
    fn grok_fallback_catalog_is_empty() {
        // A fabricated fallback would offer a model the CLI rejects, so
        // discovery is authoritative and the pre-discovery picker is empty.
        assert!(fallback_models(ProviderKind::Grok).is_empty());
    }

    #[test]
    fn cursor_aliases_resolve_cli_and_acp_spellings() {
        assert_eq!(
            resolve_packed_model(
                ["default", "grok-4.6", "composer-2.5"],
                "auto",
                ProviderKind::Cursor,
            ),
            Some(PackedModelSelection {
                value: "default".into(),
                suffix: String::new(),
            })
        );
        assert_eq!(
            resolve_packed_model(["auto", "grok-4.6"], "default", ProviderKind::Cursor)
                .map(|selection| selection.value),
            Some("auto".into())
        );
        assert_eq!(
            resolve_packed_model(
                ["default", "grok-4.6", "composer-2.5", "claude-sonnet-4-6"],
                "cursor-grok-4.6-xhigh-fast",
                ProviderKind::Cursor,
            ),
            Some(PackedModelSelection {
                value: "grok-4.6".into(),
                suffix: "xhigh-fast".into(),
            })
        );
        assert_eq!(
            resolve_packed_model(
                ["default", "claude-sonnet-4-6"],
                "claude-4.6-sonnet-medium-thinking",
                ProviderKind::Cursor,
            ),
            Some(PackedModelSelection {
                value: "claude-sonnet-4-6".into(),
                suffix: "medium-thinking".into(),
            })
        );
    }

    #[test]
    fn packed_aliases_resolve_without_cursor_spellings_for_other_providers() {
        // A non-Cursor provider resolves only its own spelling — no
        // `cursor-` prefix, family re-spelling, or auto/default swap.
        assert_eq!(
            resolve_packed_model(
                ["grok-4.6", "composer-2.5"],
                "grok-4.6-xhigh",
                ProviderKind::Grok
            ),
            Some(PackedModelSelection {
                value: "grok-4.6".into(),
                suffix: "xhigh".into(),
            })
        );
        assert_eq!(
            resolve_packed_model(["default", "grok-4.6"], "auto", ProviderKind::Grok),
            None
        );
    }

    #[test]
    fn packed_suffix_traits_prefer_extra_high_over_high() {
        let efforts = [
            ProviderModelOption::new("low", "Low"),
            ProviderModelOption::new("high", "High"),
            ProviderModelOption::new("xhigh", "Extra High"),
        ];
        let tiers = [ProviderModelOption::new("fast", "Fast")];
        assert_eq!(
            packed_suffix_reasoning_effort("thinking-extra-high-fast", &efforts).as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            packed_suffix_service_tier("thinking-extra-high-fast", &tiers).as_deref(),
            Some("fast")
        );
        assert_eq!(packed_suffix_service_tier("thinking-high", &tiers), None);
    }

    #[test]
    fn packed_catalog_match_keeps_exploded_ids_when_they_are_catalogued() {
        let models = [
            ProviderModel::new("auto", "Auto").default(),
            ProviderModel::new("claude-opus-5-thinking-high", "Opus 5 Thinking"),
        ];
        let matched =
            packed_catalog_model(&models, "claude-opus-5-thinking-high", ProviderKind::Cursor)
                .unwrap();
        assert_eq!(matched.model.id, "claude-opus-5-thinking-high");
        assert!(matched.suffix.is_empty());
    }
}
