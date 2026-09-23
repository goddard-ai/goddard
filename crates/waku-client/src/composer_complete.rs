//! Pure client-side composer matching over daemon-provided command/file lists.

use std::ops::Range;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
pub use waku_protocol::composer::{CommandScope, FileEntry, SlashCommand};
use waku_protocol::model::{ProviderKind, ProviderModelOption, ReportedCommand};
use waku_protocol::workspace::{
    IssueState, IssueSummary, PullRequestState, PullRequestSummary, WorkItemKind,
};

pub const FILTER_CAP: usize = 64;
pub const FILE_INDEX_CAP: usize = 50_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerKind {
    Command,
    File,
    /// `#` — a GitHub issue or pull request on the workspace's origin remote.
    WorkItem,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub query: String,
    pub range: Range<usize>,
}

pub fn detect_trigger(text: &str, cursor: usize) -> Option<Trigger> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let line_start = text[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let line_prefix = &text[line_start..cursor];
    if let Some(query) = line_prefix.strip_prefix('/') {
        if !query.chars().any(char::is_whitespace) {
            return Some(Trigger {
                kind: TriggerKind::Command,
                query: query.to_owned(),
                range: line_start..cursor,
            });
        }
        return None;
    }
    let token_start = text[..cursor]
        .rfind(char::is_whitespace)
        .map_or(0, |index| {
            index + text[index..].chars().next().unwrap().len_utf8()
        });
    let token = &text[token_start..cursor];
    let (kind, query) = if let Some(query) = token.strip_prefix('@') {
        (TriggerKind::File, query)
    } else {
        (TriggerKind::WorkItem, token.strip_prefix('#')?)
    };
    Some(Trigger {
        kind,
        query: query.to_owned(),
        range: token_start..cursor,
    })
}

pub fn merge_reported_commands(
    discovered: &[SlashCommand],
    reported: &[ReportedCommand],
) -> Vec<SlashCommand> {
    let mut merged = discovered.to_vec();
    for report in reported {
        if let Some(known) = merged
            .iter_mut()
            .find(|command| command.name == report.name)
        {
            if known.description.is_empty() {
                known.description = report.description.clone();
            }
        } else {
            merged.push(SlashCommand {
                name: report.name.clone(),
                description: report.description.clone(),
                scope: CommandScope::Builtin,
                argument_hint: None,
                template: None,
            });
        }
    }
    merged
        .sort_by(|a, b| (a.scope.display_rank(), &a.name).cmp(&(b.scope.display_rank(), &b.name)));
    merged
}

/// Build the slash-prefixed text shown for an autocomplete command.
pub fn command_composer_text(command: &SlashCommand) -> String {
    format!("/{}", command.name)
}

/// Whether the composer submitted Goddard's global terminal-session picker.
/// The command is reserved by daemon-side discovery, so it is intentionally
/// provider-neutral and never crosses into a provider transport.
pub fn is_resume_submission(prompt: &str) -> bool {
    prompt.trim() == "/resume"
}

/// Whether the composer submitted the land command — rebase the workspace
/// onto its base branch and fast-forward the base. Reserved daemon-side like
/// `/resume`, so it never crosses into a provider transport.
pub fn is_land_submission(prompt: &str) -> bool {
    prompt.trim() == "/land"
}

/// Whether the catalog advertises a compact path: the reserved Waku builtin
/// (Codex, OpenCode 2) or a provider-reported builtin (Pi, Claude, OpenCode,
/// DeepSeek, an ACP agent). A project or user command that deliberately owns
/// `/compact` doesn't count — resolution precedence keeps it.
pub fn has_compact_path(commands: &[SlashCommand]) -> bool {
    commands.iter().any(|command| {
        command.name == "compact"
            && matches!(command.scope, CommandScope::Waku | CommandScope::Builtin)
            && command.template.is_none()
    })
}

/// Whether the composer submitted a `/compact` invocation that routes
/// through the daemon's compact command rather than to the provider as
/// prompt text. A bare typed `/compact` on a provider that never advertised
/// it stays an ordinary prompt.
pub fn is_compact_submission(prompt: &str, commands: &[SlashCommand]) -> bool {
    prompt.trim() == "/compact" && has_compact_path(commands)
}

/// Parse the submitted text as a `/side` invocation: `None` when it is not
/// the command, `Some(None)` for a bare `/side` — a fresh empty side chat —
/// and `Some(Some(prompt))` when a prompt follows. Reserved like `/resume`
/// and `/land`, so it never crosses into a provider transport.
pub fn parse_side_submission(prompt: &str) -> Option<Option<String>> {
    parse_waku_invocation(prompt, "side")
}

/// A reserved local title change. A bare command opens the title editor.
pub fn parse_rename_submission(prompt: &str) -> Option<Option<String>> {
    parse_waku_invocation(prompt, "rename")
}

/// Parse the submitted text as a `/incognito` invocation: `None` when it is
/// not the command, `Some(None)` for a bare `/incognito` — flag the current
/// draft only — and `Some(Some(prompt))` when a prompt follows, which flags
/// the draft and submits the prompt as its first turn. Reserved daemon-side
/// like `/side`, so it never crosses into a provider transport.
pub fn parse_incognito_submission(prompt: &str) -> Option<Option<String>> {
    parse_waku_invocation(prompt, "incognito")
}

/// Shared `/name [arguments]` split for reserved Goddard commands.
fn parse_waku_invocation(prompt: &str, name: &str) -> Option<Option<String>> {
    let invocation = prompt.trim().strip_prefix('/')?;
    let (invocation_name, arguments) = invocation
        .split_once(char::is_whitespace)
        .map_or((invocation, ""), |(name, arguments)| {
            (name, arguments.trim())
        });
    if invocation_name != name {
        return None;
    }
    Some((!arguments.is_empty()).then(|| arguments.to_owned()))
}

/// Whether the submitted text resolves to Codex's native fast-mode command,
/// which Goddard bridges to the provider's service-tier control. Checking the
/// resolved entry preserves project/user command precedence when one of them
/// intentionally owns `/fast`.
pub fn is_fast_mode_toggle_submission(
    provider: ProviderKind,
    prompt: &str,
    commands: &[SlashCommand],
) -> bool {
    provider == ProviderKind::Codex
        && prompt.trim() == "/fast"
        && commands.iter().any(|command| {
            command.name == "fast"
                && command.scope == CommandScope::Builtin
                && command.template.is_none()
        })
}

/// Resolve the next concrete service-tier ID for Codex's Fast toggle. Model
/// metadata may expose the Fast tier as `fast` or as `priority`; the display
/// label is the stable product vocabulary, while the ID is provider-owned.
pub fn toggled_fast_service_tier(
    current: Option<&str>,
    service_tiers: &[ProviderModelOption],
) -> Option<String> {
    let fast = service_tiers.iter().find(|tier| {
        matches!(tier.id.as_str(), "fast" | "priority") || tier.label.eq_ignore_ascii_case("fast")
    })?;
    Some(if current == Some(fast.id.as_str()) {
        "default".to_owned()
    } else {
        fast.id.clone()
    })
}

/// A `/goal` composer submission parsed into its intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalCommand {
    /// Bare `/goal` — show the current goal (and offer to create one).
    Show,
    Edit,
    Pause,
    Resume,
    Clear,
    /// `/goal <objective>` — start or replace the goal with this objective.
    Set(String),
}

/// Parse Goddard's goal command. Dispatch chooses a native provider goal when
/// available and falls back to Jev otherwise.
pub fn parse_goal_submission(
    _provider: ProviderKind,
    prompt: &str,
    commands: &[SlashCommand],
) -> Option<GoalCommand> {
    let invocation = prompt.trim().strip_prefix('/')?;
    let (name, arguments) = invocation
        .split_once(char::is_whitespace)
        .map_or((invocation, ""), |(name, arguments)| {
            (name, arguments.trim())
        });
    if name != "goal" {
        return None;
    }
    let overridden = commands.iter().any(|command| {
        command.name == "goal"
            && matches!(command.scope, CommandScope::Project | CommandScope::User)
    });
    if overridden {
        return None;
    }
    Some(match arguments {
        "" => GoalCommand::Show,
        "edit" => GoalCommand::Edit,
        "pause" => GoalCommand::Pause,
        "resume" => GoalCommand::Resume,
        "clear" => GoalCommand::Clear,
        objective => GoalCommand::Set(objective.to_owned()),
    })
}

pub fn expand_command_template(template: &str, args: &str) -> String {
    let positional = args.split_whitespace().collect::<Vec<_>>();
    let mut expanded = String::with_capacity(template.len() + args.len());
    let mut consumed_args = false;
    let mut rest = template;
    while let Some(index) = rest.find('$') {
        expanded.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        if let Some(tail) = after.strip_prefix("ARGUMENTS") {
            expanded.push_str(args);
            consumed_args = true;
            rest = tail;
        } else if let Some(tail) = after.strip_prefix('@') {
            expanded.push_str(args);
            consumed_args = true;
            rest = tail;
        } else if let Some(digit) = after
            .chars()
            .next()
            .and_then(|character| character.to_digit(10))
            .filter(|digit| (1..=9).contains(digit))
        {
            if let Some(argument) = positional.get(digit as usize - 1) {
                expanded.push_str(argument);
            }
            consumed_args = true;
            rest = &after[1..];
        } else {
            expanded.push('$');
            rest = after;
        }
    }
    expanded.push_str(rest);
    if !consumed_args && !args.is_empty() {
        expanded.push_str("\n\n");
        expanded.push_str(args);
    }
    expanded
}

/// Resolve composer text into the exact prompt expected by the provider.
///
/// Template commands expand to their body. Skills keep a slash in the
/// composer and transcript, then resolve to each provider's native syntax at
/// the transport boundary.
pub fn resolved_submission(
    provider: ProviderKind,
    prompt: &str,
    commands: &[SlashCommand],
) -> Option<String> {
    if let Some(skill) = resolved_skill_submission(provider, prompt, commands) {
        return Some(skill);
    }
    let invocation = prompt.strip_prefix('/')?;
    let (name, args) = invocation
        .split_once(char::is_whitespace)
        .map_or((invocation, ""), |(name, args)| (name, args.trim()));
    let command = commands.iter().find(|command| command.name == name)?;
    let template = command.template.as_deref()?;
    Some(expand_command_template(template, args))
}

/// Resolve only provider-native skill syntax, without expanding templates.
pub fn resolved_skill_submission(
    provider: ProviderKind,
    prompt: &str,
    commands: &[SlashCommand],
) -> Option<String> {
    if !matches!(
        provider,
        ProviderKind::Codex | ProviderKind::Fx | ProviderKind::Pi | ProviderKind::OhMyPi
    ) {
        return None;
    }
    let invocation = prompt.strip_prefix('/')?;
    let name = invocation
        .split_once(char::is_whitespace)
        .map_or(invocation, |(name, _)| name);
    if !commands
        .iter()
        .any(|command| command.name == name && command.scope == CommandScope::Skill)
    {
        return None;
    }
    Some(match provider {
        ProviderKind::Codex | ProviderKind::Fx => format!("${invocation}"),
        ProviderKind::Pi | ProviderKind::OhMyPi => format!("/skill:{invocation}"),
        _ => unreachable!("non-native skill providers returned above"),
    })
}

pub fn matcher() -> Matcher {
    Matcher::new(nucleo_matcher::Config::DEFAULT.match_paths())
}

#[derive(Clone, Debug)]
pub struct Scored<T> {
    pub item: T,
    pub positions: Vec<u32>,
}

/// The shared fuzzy filter: indexes of `haystack` entries matching `query`
/// with their match positions, best first, capped. An empty query lists the
/// haystack's head in its given order.
pub fn filter_scored(
    haystack: &[&str],
    query: &str,
    matcher: &mut Matcher,
    cap: usize,
) -> Vec<(usize, Vec<u32>)> {
    if query.trim().is_empty() {
        return (0..haystack.len().min(cap))
            .map(|index| (index, Vec::new()))
            .collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored = Vec::new();
    for (index, text) in haystack.iter().enumerate() {
        if let Some(score) = pattern.score(Utf32Str::new(text, &mut buf), matcher) {
            scored.push((score, index));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.truncate(cap);
    scored
        .into_iter()
        .map(|(_, index)| {
            let mut positions = Vec::new();
            pattern.indices(
                Utf32Str::new(haystack[index], &mut buf),
                matcher,
                &mut positions,
            );
            positions.sort_unstable();
            positions.dedup();
            (index, positions)
        })
        .collect()
}

pub fn filter_commands(
    commands: &[SlashCommand],
    query: &str,
    matcher: &mut Matcher,
) -> Vec<Scored<SlashCommand>> {
    let names = commands
        .iter()
        .map(|command| command.name.as_str())
        .collect::<Vec<_>>();
    filter_scored(&names, query, matcher, FILTER_CAP)
        .into_iter()
        .map(|(index, positions)| Scored {
            item: commands[index].clone(),
            positions,
        })
        .collect()
}

pub fn filter_files(
    files: &[FileEntry],
    query: &str,
    matcher: &mut Matcher,
) -> Vec<Scored<FileEntry>> {
    let paths = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    filter_scored(&paths, query, matcher, FILTER_CAP)
        .into_iter()
        .map(|(index, positions)| Scored {
            item: files[index].clone(),
            positions,
        })
        .collect()
}

// ── Work-item mentions (`#`) ───────────────────────────────────────────────

/// One issue or pull request as a `#` completion row, flattened from the
/// daemon's separate `IssueSummary`/`PullRequestSummary` lists.
#[derive(Clone, Debug)]
pub struct ComposerWorkItem {
    pub kind: WorkItemKind,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: ComposerWorkItemState,
    pub author: Option<String>,
    /// Unix seconds; orders the empty-query "what's in flight" list.
    pub updated_at: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerWorkItemState {
    Open,
    Closed,
    Merged,
    Draft,
}

impl ComposerWorkItem {
    pub fn from_issue(issue: IssueSummary) -> Self {
        Self {
            kind: WorkItemKind::Issue,
            number: issue.number,
            title: issue.title,
            url: issue.url,
            state: match issue.state {
                IssueState::Open => ComposerWorkItemState::Open,
                IssueState::Closed => ComposerWorkItemState::Closed,
            },
            author: issue.author,
            updated_at: issue.updated_at,
        }
    }

    pub fn from_pull_request(pr: PullRequestSummary) -> Self {
        Self {
            kind: WorkItemKind::PullRequest,
            number: pr.number,
            title: pr.title,
            url: pr.url,
            state: match (pr.state, pr.is_draft) {
                (PullRequestState::Merged, _) => ComposerWorkItemState::Merged,
                (PullRequestState::Closed, _) => ComposerWorkItemState::Closed,
                (PullRequestState::Open, true) => ComposerWorkItemState::Draft,
                (PullRequestState::Open, false) => ComposerWorkItemState::Open,
            },
            author: pr.author,
            updated_at: pr.updated_at,
        }
    }

    /// The fuzzy candidate — `#<number> <title>` — so digits match the number
    /// and text matches the title. Match positions index this string.
    pub fn candidate(&self) -> String {
        format!("#{} {}", self.number, self.title)
    }

    /// Byte offset of the title inside [`Self::candidate`]; `#` and the space
    /// are single-byte, so it is the digit count plus two.
    pub fn candidate_title_offset(&self) -> usize {
        1 + self.number.to_string().len() + 1
    }
}

/// Merge one remote search's issue and pull-request results into a single
/// mention list: `exact` (a numeric query's direct `view` hits) first, then
/// the rest newest-updated first, deduplicated by number — GitHub numbers
/// issues and PRs from one sequence, so a number names at most one item.
pub fn merge_work_items(
    issues: Vec<IssueSummary>,
    pull_requests: Vec<PullRequestSummary>,
    exact: Vec<ComposerWorkItem>,
) -> Vec<ComposerWorkItem> {
    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<ComposerWorkItem> = Vec::new();
    for item in exact {
        if seen.insert(item.number) {
            items.push(item);
        }
    }
    let mut rest: Vec<ComposerWorkItem> = issues
        .into_iter()
        .map(ComposerWorkItem::from_issue)
        .chain(
            pull_requests
                .into_iter()
                .map(ComposerWorkItem::from_pull_request),
        )
        .collect();
    rest.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    items.extend(rest.into_iter().filter(|item| seen.insert(item.number)));
    items
}

pub fn filter_work_items(
    items: &[ComposerWorkItem],
    query: &str,
    matcher: &mut Matcher,
) -> Vec<Scored<ComposerWorkItem>> {
    let candidates = items
        .iter()
        .map(ComposerWorkItem::candidate)
        .collect::<Vec<_>>();
    let refs = candidates.iter().map(String::as_str).collect::<Vec<_>>();
    filter_scored(&refs, query, matcher, FILTER_CAP)
        .into_iter()
        .map(|(index, positions)| Scored {
            item: items[index].clone(),
            positions,
        })
        .collect()
}

/// The `#<digits>` tokens in submitted text — start of string or whitespace
/// before `#`, a non-digit after the number — deduplicated in first-use order.
/// The boundary rule matches `detect_trigger` so a mention the popup would
/// have completed is the same shape expansion recognizes.
pub fn work_item_reference_numbers(text: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find('#') {
        let boundary = rest[..index]
            .chars()
            .next_back()
            .is_none_or(|ch| ch.is_whitespace());
        let digits = &rest[index + 1..];
        let len = digits
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(digits.len());
        let closed = digits[len..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric());
        if boundary && len > 0 && closed {
            let number: u64 = digits[..len].parse().unwrap_or(u64::MAX);
            if !numbers.contains(&number) {
                numbers.push(number);
            }
        }
        rest = &digits[len..];
    }
    numbers
}

/// Rewrite each `#N` reference as `GitHub issue|pull request #N "title" (url)`
/// so the provider prompt is self-contained; the transcript keeps the typed
/// `#N`. References `resolve` does not know pass through untouched.
pub fn expand_work_item_references(
    prompt: &str,
    resolve: impl Fn(u64) -> Option<ComposerWorkItem>,
) -> String {
    if !prompt.contains('#') {
        return prompt.to_owned();
    }
    let mut expanded = String::with_capacity(prompt.len());
    let mut rest = prompt;
    while let Some(index) = rest.find('#') {
        let boundary = rest[..index]
            .chars()
            .next_back()
            .is_none_or(|ch| ch.is_whitespace());
        let digits = &rest[index + 1..];
        let len = digits
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(digits.len());
        let closed = digits[len..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric());
        let item = (boundary && len > 0 && closed)
            .then(|| digits[..len].parse::<u64>().ok())
            .flatten()
            .and_then(&resolve);
        match item {
            Some(item) => {
                expanded.push_str(&rest[..index]);
                let noun = match item.kind {
                    WorkItemKind::Issue => "GitHub issue",
                    WorkItemKind::PullRequest => "GitHub pull request",
                };
                expanded.push_str(&format!(
                    "{noun} #{} \"{}\" ({})",
                    item.number, item.title, item.url
                ));
            }
            // Not a reference, or one the store cannot name: verbatim.
            None => expanded.push_str(&rest[..index + 1 + len]),
        }
        rest = &rest[index + 1 + len..];
    }
    expanded.push_str(rest);
    expanded
}

pub fn highlight_byte_ranges(
    text: &str,
    positions: &[u32],
    char_offset: usize,
) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (char_index, (byte_index, character)) in (char_offset..).zip(text.char_indices()) {
        if positions.binary_search(&(char_index as u32)).is_ok() {
            let byte_end = byte_index + character.len_utf8();
            match ranges.last_mut() {
                Some(last) if last.end == byte_index => last.end = byte_end,
                _ => ranges.push(byte_index..byte_end),
            }
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_submission_distinguishes_bare_command_from_prompt() {
        assert_eq!(parse_side_submission("/side"), Some(None));
        assert_eq!(parse_side_submission("  /side  "), Some(None));
        assert_eq!(
            parse_side_submission("/side check the parent diff"),
            Some(Some("check the parent diff".to_owned()))
        );
        assert_eq!(parse_side_submission("side"), None);
        assert_eq!(parse_side_submission("/sidebar"), None);
        assert_eq!(parse_side_submission("/other /side"), None);
    }

    #[test]
    fn incognito_submission_distinguishes_bare_command_from_prompt() {
        assert_eq!(parse_incognito_submission("/incognito"), Some(None));
        assert_eq!(parse_incognito_submission("  /incognito  "), Some(None));
        assert_eq!(
            parse_incognito_submission("/incognito sketch a cache plan"),
            Some(Some("sketch a cache plan".to_owned()))
        );
        assert_eq!(parse_incognito_submission("incognito"), None);
        assert_eq!(parse_incognito_submission("/incognito-mode"), None);
        assert_eq!(parse_incognito_submission("/other /incognito"), None);
    }

    #[test]
    fn rename_submission_requires_exact_command_name() {
        assert_eq!(
            parse_rename_submission("/rename New name"),
            Some(Some("New name".into()))
        );
        assert_eq!(parse_rename_submission("/rename"), Some(None));
        assert_eq!(parse_rename_submission("/renamed New name"), None);
    }

    #[test]
    fn merged_command_picker_puts_builtins_first_and_skills_last() {
        let discovered = vec![
            command("deploy", CommandScope::Skill),
            command("format", CommandScope::User),
            command("lint", CommandScope::Project),
            command("review", CommandScope::Builtin),
            command("resume", CommandScope::Waku),
        ];
        let reported = vec![ReportedCommand {
            name: "compact".into(),
            description: "Free up context".into(),
        }];

        let merged = merge_reported_commands(&discovered, &reported);
        assert_eq!(
            merged
                .iter()
                .map(|command| (command.scope, command.name.as_str()))
                .collect::<Vec<_>>(),
            [
                (CommandScope::Builtin, "compact"),
                (CommandScope::Waku, "resume"),
                (CommandScope::Builtin, "review"),
                (CommandScope::Project, "lint"),
                (CommandScope::User, "format"),
                (CommandScope::Skill, "deploy"),
            ]
        );
    }

    #[test]
    fn fast_toggle_is_codex_only_and_respects_command_overrides() {
        let builtin = command("fast", CommandScope::Builtin);
        assert!(is_fast_mode_toggle_submission(
            ProviderKind::Codex,
            "/fast ",
            std::slice::from_ref(&builtin),
        ));
        assert!(!is_fast_mode_toggle_submission(
            ProviderKind::Claude,
            "/fast",
            std::slice::from_ref(&builtin),
        ));
        assert!(!is_fast_mode_toggle_submission(
            ProviderKind::Codex,
            "/fast now",
            std::slice::from_ref(&builtin),
        ));
        assert!(!is_fast_mode_toggle_submission(
            ProviderKind::Codex,
            "/fast",
            &[command("fast", CommandScope::Project)],
        ));
    }

    #[test]
    fn compact_submission_needs_a_waku_or_builtin_entry() {
        let waku = command("compact", CommandScope::Waku);
        let builtin = command("compact", CommandScope::Builtin);
        assert!(is_compact_submission(
            "/compact",
            std::slice::from_ref(&waku)
        ));
        assert!(is_compact_submission(
            "  /compact  ",
            std::slice::from_ref(&builtin)
        ));
        // A project/user command that owns /compact keeps precedence, and a
        // provider that never advertised it gets the text as an ordinary
        // prompt.
        assert!(!is_compact_submission(
            "/compact",
            &[command("compact", CommandScope::Project)]
        ));
        assert!(!is_compact_submission("/compact", &[]));
        let mut templated = command("compact", CommandScope::Builtin);
        templated.template = Some("shrink $ARGUMENTS".into());
        assert!(!is_compact_submission(
            "/compact",
            std::slice::from_ref(&templated)
        ));
        assert!(!is_compact_submission(
            "/compact now",
            std::slice::from_ref(&waku)
        ));
    }

    #[test]
    fn resume_is_an_exact_provider_neutral_local_command() {
        assert!(is_resume_submission("/resume"));
        assert!(is_resume_submission("  /resume  "));
        assert!(!is_resume_submission("/resume latest"));
        assert!(!is_resume_submission("please /resume"));
    }

    #[test]
    fn fast_toggle_uses_the_models_concrete_service_tier_id() {
        let tiers = [ProviderModelOption::new("priority", "Fast")];
        assert_eq!(
            toggled_fast_service_tier(Some("default"), &tiers).as_deref(),
            Some("priority")
        );
        assert_eq!(
            toggled_fast_service_tier(Some("priority"), &tiers).as_deref(),
            Some("default")
        );
        assert_eq!(toggled_fast_service_tier(None, &[]), None);
    }

    #[test]
    fn codex_skill_completion_keeps_slash_in_the_composer() {
        let skill = command("mattpocock-skills:to-spec", CommandScope::Skill);
        assert_eq!(command_composer_text(&skill), "/mattpocock-skills:to-spec");
        assert_eq!(
            command_composer_text(&command("fast", CommandScope::Builtin)),
            "/fast"
        );
    }

    #[test]
    fn codex_skill_submission_uses_the_catalog_invocation() {
        let skill = command("mattpocock-skills:to-spec", CommandScope::Skill);
        assert_eq!(
            resolved_submission(
                ProviderKind::Codex,
                "/mattpocock-skills:to-spec carefully",
                std::slice::from_ref(&skill)
            )
            .as_deref(),
            Some("$mattpocock-skills:to-spec carefully")
        );
        assert_eq!(
            resolved_submission(
                ProviderKind::Claude,
                "/mattpocock-skills:to-spec carefully",
                std::slice::from_ref(&skill)
            ),
            None
        );
    }

    #[test]
    fn fx_skill_submission_uses_the_catalog_invocation() {
        let skill = command("deploy", CommandScope::Skill);
        assert_eq!(
            resolved_submission(
                ProviderKind::Fx,
                "/deploy production",
                std::slice::from_ref(&skill)
            )
            .as_deref(),
            Some("$deploy production")
        );
    }

    #[test]
    fn pi_skill_submission_uses_the_skill_command() {
        for provider in [ProviderKind::Pi, ProviderKind::OhMyPi] {
            for name in ["to-spec", "to-tickets"] {
                let skill = command(name, CommandScope::Skill);
                let expected = format!("/skill:{name} carefully");
                assert_eq!(
                    resolved_skill_submission(provider, &format!("/{name} carefully"), &[skill])
                        .as_deref(),
                    Some(expected.as_str())
                );
            }
        }
    }

    fn command(name: &str, scope: CommandScope) -> SlashCommand {
        SlashCommand {
            name: name.into(),
            description: String::new(),
            scope,
            argument_hint: None,
            template: None,
        }
    }

    #[test]
    fn hash_triggers_on_token_start_only() {
        let trigger = detect_trigger("fix #12", 7).expect("hash token triggers");
        assert_eq!(trigger.kind, TriggerKind::WorkItem);
        assert_eq!(trigger.query, "12");
        assert_eq!(trigger.range, 4..7);

        assert_eq!(detect_trigger("#", 1).unwrap().kind, TriggerKind::WorkItem);
        assert_eq!(detect_trigger("#", 1).unwrap().query, "");
        // Mid-token and heading text after a space are not mention sites.
        assert!(detect_trigger("c#note", 6).is_none());
        assert!(detect_trigger("# title", 7).is_none());
        assert_eq!(detect_trigger("see #issue", 10).unwrap().query, "issue");
    }

    fn issue(number: u64, title: &str) -> IssueSummary {
        IssueSummary {
            number,
            title: title.into(),
            url: format!("https://github.com/o/r/issues/{number}"),
            state: IssueState::Open,
            author: None,
            labels: Vec::new(),
            assignees: Vec::new(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn work_item_filter_matches_number_and_title() {
        let items = vec![
            ComposerWorkItem::from_issue(issue(12, "login flake")),
            ComposerWorkItem::from_issue(issue(34, "dark mode")),
        ];
        let mut matcher = matcher();
        let rows = filter_work_items(&items, "12", &mut matcher);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item.number, 12);
        let rows = filter_work_items(&items, "flake", &mut matcher);
        assert_eq!(rows[0].item.number, 12);
    }

    #[test]
    fn merge_work_items_dedupes_by_number_with_exact_first() {
        let mut pr = PullRequestSummary {
            number: 12,
            title: "fix flake".into(),
            url: "https://github.com/o/r/pull/12".into(),
            state: PullRequestState::Merged,
            is_draft: false,
            base_branch: "main".into(),
            created_at: None,
            updated_at: Some(10),
            review_decision: None,
            check_status: None,
            additions: None,
            deletions: None,
            author: None,
            head_branch: None,
        };
        let merged = merge_work_items(
            vec![issue(12, "login flake")],
            vec![pr.clone()],
            vec![ComposerWorkItem::from_pull_request(pr.clone())],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, WorkItemKind::PullRequest);
        assert_eq!(merged[0].state, ComposerWorkItemState::Merged);

        pr.number = 30;
        pr.updated_at = Some(20);
        let merged = merge_work_items(vec![issue(12, "login flake")], vec![pr], Vec::new());
        assert_eq!(
            merged.iter().map(|item| item.number).collect::<Vec<_>>(),
            [30, 12]
        );
    }

    #[test]
    fn work_item_references_scan_word_boundary_hashes() {
        assert_eq!(work_item_reference_numbers("fix #12 and #34"), [12, 34]);
        assert_eq!(work_item_reference_numbers("#7"), [7]);
        assert_eq!(work_item_reference_numbers("c#5 or f#9"), Vec::<u64>::new());
        assert_eq!(work_item_reference_numbers("#abc #12x"), Vec::<u64>::new());
        assert_eq!(work_item_reference_numbers("#12 #12"), [12]);
        assert_eq!(work_item_reference_numbers("# heading"), Vec::<u64>::new());
    }

    #[test]
    fn expansion_rewrites_known_references_and_keeps_unknown() {
        let known = |number: u64| {
            (number == 12).then(|| ComposerWorkItem::from_issue(issue(12, "login flake")))
        };
        assert_eq!(
            expand_work_item_references("fix #12 like #99", known),
            "fix GitHub issue #12 \"login flake\" (https://github.com/o/r/issues/12) like #99"
        );
        assert_eq!(
            expand_work_item_references("no refs here", known),
            "no refs here"
        );
        assert_eq!(
            expand_work_item_references("c#12 stays", known),
            "c#12 stays"
        );
    }

    #[test]
    fn goal_submissions_parse_into_their_intent() {
        let builtin = command("goal", CommandScope::Builtin);
        let commands = std::slice::from_ref(&builtin);
        let parse = |prompt: &str| parse_goal_submission(ProviderKind::Codex, prompt, commands);

        assert_eq!(parse("/goal"), Some(GoalCommand::Show));
        assert_eq!(parse("/goal "), Some(GoalCommand::Show));
        assert_eq!(parse("/goal edit"), Some(GoalCommand::Edit));
        assert_eq!(parse("/goal pause"), Some(GoalCommand::Pause));
        assert_eq!(parse("/goal resume"), Some(GoalCommand::Resume));
        assert_eq!(parse("/goal clear"), Some(GoalCommand::Clear));
        assert_eq!(
            parse("/goal improve benchmark coverage"),
            Some(GoalCommand::Set("improve benchmark coverage".into()))
        );
        assert_eq!(parse("/goals"), None);
        assert_eq!(parse("ship /goal"), None);
    }

    #[test]
    fn goal_command_falls_back_and_respects_overrides() {
        let builtin = command("goal", CommandScope::Builtin);
        assert_eq!(
            parse_goal_submission(
                ProviderKind::Claude,
                "/goal",
                std::slice::from_ref(&builtin)
            ),
            Some(GoalCommand::Show)
        );
        assert_eq!(
            parse_goal_submission(ProviderKind::Codex, "/goal", &[]),
            Some(GoalCommand::Show)
        );
        // A project command deliberately owning /goal wins the collision.
        let mut project = command("goal", CommandScope::Project);
        project.template = Some("do project things".into());
        assert_eq!(
            parse_goal_submission(ProviderKind::Codex, "/goal", std::slice::from_ref(&project)),
            None
        );
    }
}
