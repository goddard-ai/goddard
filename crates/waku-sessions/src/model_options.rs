//! Pure model labels and provider variant options shared with session discovery.

use waku_protocol::model::{ProviderModel, ProviderModelOption};

pub fn reasoning_effort_pair(effort: &str) -> (String, Option<waku_protocol::WireTranslation>) {
    let pair = match effort {
        "none" => localized!("model_option.none"),
        "minimal" => localized!("model_option.minimal"),
        "low" => localized!("model_option.low"),
        "medium" => localized!("model_option.medium"),
        "high" => localized!("model_option.high"),
        "xhigh" => localized!("model_option.extra_high"),
        "max" => localized!("model_option.max"),
        "ultra" => localized!("model_option.ultra"),
        "ultracode" => localized!("model_option.ultracode"),
        other => return (display_name_from_slug(other), None),
    };
    (pair.0, Some(pair.1))
}

/// Attaches a model's OpenCode "variants" as its reasoning-effort ladder.
///
/// OpenCode expresses reasoning effort as a per-model variant whose id is
/// drawn from the same vocabulary Goddard already labels
/// (`none`/`minimal`/`low`/`medium`/`high`/`xhigh`/`max`, plus provider-specific
/// ones such as `thinking`), and the chosen id is sent verbatim as
/// `ModelRef::variant`. Models with no variants keep an empty ladder, which is
/// what hides the control.
///
/// The default is the strongest offered rather than the weakest: a variant
/// list is opt-in per model, so a model that publishes one is a reasoning
/// model and the ladder's top is the reason to pick it.
pub fn with_variant_efforts<'a>(
    model: ProviderModel,
    variants: impl IntoIterator<Item = &'a str>,
) -> ProviderModel {
    const STRENGTH: [&str; 8] = [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ];
    let ids: Vec<String> = variants
        .into_iter()
        .map(str::trim)
        .filter(|variant| !variant.is_empty())
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return model;
    }
    let rank = |id: &str| STRENGTH.iter().position(|known| *known == id);
    // Preserve the provider's own ordering; only the default is ranked, and a
    // ladder of entirely unknown ids falls back to the last one offered.
    let default = ids
        .iter()
        .filter(|id| rank(id).is_some())
        .max_by_key(|id| rank(id))
        .or_else(|| ids.last())
        .cloned();
    let options = ids.iter().map(|id| {
        let (label, i18n) = reasoning_effort_pair(id);
        ProviderModelOption::new(id.clone(), label).with_label_i18n(i18n)
    });
    match default {
        Some(default) => model.reasoning(options, default),
        None => model,
    }
}

pub fn display_name_from_slug(slug: &str) -> String {
    let words = slug
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "gpt" => "GPT".to_owned(),
            "ai" => "AI".to_owned(),
            "xai" => "xAI".to_owned(),
            _ if part
                .chars()
                .all(|char| char.is_ascii_digit() || char == '.') =>
            {
                part.to_owned()
            }
            _ => {
                let mut chars = part.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                })
            }
        })
        .collect::<Vec<_>>();
    if words.first().is_some_and(|word| word == "GPT") {
        words.join("-")
    } else {
        words.join(" ")
    }
}
