use chrono::Datelike as _;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlanUsage {
    pub plan_label: Option<String>,
    pub windows: Vec<PlanWindow>,
    /// Banked reset credits the account can spend on this plan's windows —
    /// a Codex/ChatGPT promotion, absent for every other provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_credits: Option<PlanResetCredits>,
}

/// Banked promotional rate-limit resets on the ChatGPT account. Spending one
/// clears the 5-hour and weekly windows and moves the weekly anchor to the
/// redemption moment.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlanResetCredits {
    pub available_count: u32,
    /// Earliest expiry among redeemable credits, unix seconds — the use-it-
    /// or-lose-it date the row surfaces next to the count.
    pub next_expires_at: Option<i64>,
}

/// The backend's verdict on a reset-credit redemption, mirrored from the
/// ChatGPT consume endpoint's `code` field.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CodexResetCreditOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlanWindow {
    pub label: String,
    /// The i18n semantic behind `label`, when the daemon composed it from a
    /// known key rather than provider text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_i18n: Option<crate::protocol::WireTranslation>,
    pub percent: f64,
    pub resets_at: Option<i64>,
}

pub fn format_tokens(tokens: u64) -> String {
    if tokens >= 999_500 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub fn reset_label(resets_at: i64, now: i64) -> String {
    let delta = resets_at - now;
    if delta <= 0 {
        return tr!("usage.resets_soon");
    }
    let minutes = (delta + 59) / 60;
    if minutes < 60 {
        return tr!("usage.resets_in_minutes", count = minutes);
    }
    if minutes < 24 * 60 {
        let hours = minutes / 60;
        return match minutes % 60 {
            0 => tr!("usage.resets_in_hours", count = hours),
            remainder => tr!(
                "usage.resets_in_hours_minutes",
                hours = hours,
                minutes = remainder
            ),
        };
    }
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_opt(resets_at, 0) {
        chrono::LocalResult::Single(date) if crate::i18n::uses_east_asian_date_format() => tr!(
            "usage.resets_date",
            date = format!(
                "{}月{}日 {}",
                date.month(),
                date.day(),
                date.format("%H:%M")
            )
        ),
        chrono::LocalResult::Single(date) => tr!(
            "usage.resets_date",
            date = date.format("%a %-I:%M %p").to_string()
        ),
        _ => tr!("usage.resets_soon"),
    }
}

/// A banked credit's expiry as a bare local date — expiries run up to ~30
/// days out, where `reset_label`'s weekday-and-time phrasing reads wrong.
pub fn expiry_label(expires_at: i64) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_opt(expires_at, 0) {
        chrono::LocalResult::Single(date) if crate::i18n::uses_east_asian_date_format() => {
            format!("{}月{}日", date.month(), date.day())
        }
        chrono::LocalResult::Single(date) => date.format("%b %-d").to_string(),
        _ => String::new(),
    }
}
