//! The shared model picker: one row model, one list builder, one panel
//! body, and one per-picker state. Every surface that picks a model — the
//! composer, the automation editor, and the routing class targets — drives
//! this machinery so the pickers cannot drift apart: a site supplies a
//! [`PickerRowSpec`], renders each [`PickerRow`], and decides what a pick
//! means; the drawn list, the search grammar, the rail, and the keyboard
//! contract all live here.

use super::*;

/// Where the picker's keyboard cursor lands, wrapping at both ends.
///
/// `None` for `current` means the cursor has not moved yet, so `down` opens on
/// the first row and `up` on the last. `None` in the result means the key does
/// not navigate.
pub(super) fn next_picker_highlight(
    current: Option<usize>,
    len: usize,
    key: &str,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match key {
        "down" => Some(current.map_or(0, |index| (index + 1) % len)),
        "up" => Some(current.map_or(len - 1, |index| (index + len - 1) % len)),
        _ => None,
    }
}

/// The row the session's effective selection occupies, when it is listed —
/// the Auto row while the draft is routed, else the row matching its combo.
/// Shared by the scroll reveal and by the keyboard cursor's seed so the
/// filled "current" row and the first arrow press agree on where the
/// selection sits.
pub(super) fn picker_selected_row_index(
    selection: Option<&(ProviderKind, String, Option<String>, bool)>,
    auto_route: bool,
    rows: &[PickerRow],
) -> Option<usize> {
    if auto_route {
        return rows
            .iter()
            .position(|row| matches!(row, PickerRow::Policy(PolicyRowId::Auto)));
    }
    let (provider, model_id, effort, fast) = selection?;
    rows.iter().position(|row| match row {
        PickerRow::Combo(row) => {
            row.provider == *provider
                && row.model.id == *model_id
                && row.effort == *effort
                && row.fast == *fast
        }
        _ => false,
    })
}

/// The picker's whole body when nothing can back a session: no agent CLI
/// found on this machine, and none left switched on.
///
/// A rail holding a lone star above an empty filter field would invite the
/// user to search a list that cannot have rows, so the panel names what is
/// missing and offers the page that fixes it. Its one button also carries the
/// panel's focus, which is what `escape` dispatches up from.
pub(super) fn model_picker_empty_state(
    theme: &Theme,
    focus: &FocusHandle,
    popover: ContextMenuHandle,
    waku: WeakEntity<Waku>,
) -> AnyElement {
    let click_popover = popover.clone();
    let click_waku = waku.clone();
    div()
        .w(px(320.0))
        .rounded(px(16.0))
        .overflow_hidden()
        .border(hairline())
        .border_color(theme.border_subtle)
        .bg(theme.raised)
        .shadow_lg()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(9.0))
        .px(px(24.0))
        .py(px(22.0))
        .child(
            div()
                .w(px(40.0))
                .h(px(40.0))
                .rounded(px(12.0))
                .bg(theme.overlay)
                .flex()
                .items_center()
                .justify_center()
                .child(icon("icons/bot.svg", 19.0, theme.text_tertiary)),
        )
        .child(
            div()
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(tr!("models.no_providers_title")),
        )
        .child(
            div()
                .text_size(sp(12.5))
                .line_height(sp(17.0))
                .text_center()
                .text_color(theme.text_secondary)
                .child(tr!("models.no_providers_description")),
        )
        .child(
            div()
                .id("model-picker-open-providers")
                .track_focus(focus)
                .tab_index(0)
                .tab_stop(true)
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .mt(px(3.0))
                .h(px(28.0))
                .px(px(11.0))
                .rounded(px(9.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .child(icon("icons/settings.svg", 11.0, theme.text_tertiary))
                .child(tr!("models.open_provider_settings"))
                .on_click(move |_, window, cx| {
                    open_settings_page_from_picker(
                        &click_waku,
                        &click_popover,
                        SettingsPage::Providers,
                        window,
                        cx,
                    );
                })
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        open_settings_page_from_picker(
                            &waku,
                            &popover,
                            SettingsPage::Providers,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }
                }),
        )
        .into_any_element()
}

/// Dismiss the picker and land on the given settings page, for both the
/// empty state's click and its keyboard activation. Closing first matters:
/// the picker returns focus to the composer as it closes, which would
/// otherwise pull focus straight back out of the settings view.
pub(super) fn open_settings_page_from_picker(
    waku: &WeakEntity<Waku>,
    popover: &ContextMenuHandle,
    page: SettingsPage,
    window: &mut Window,
    cx: &mut App,
) {
    popover.close(window, cx);
    let _ = waku.update(cx, |this, cx| {
        this.open_settings_action(&OpenSettings, window, cx);
        this.open_settings_page(page, window, cx);
    });
}

/// Whether the provider contributes rows to the merged list at all.
///
/// Installed on this machine and not switched off in the Providers settings.
/// Both of those are settings-level facts the user has already decided, so
/// the provider's rows are absent rather than dimmed — the list offers what
/// could be picked, not a catalog of everything Goddard can speak to. A
/// session locked to a provider switched off afterwards keeps its rows,
/// since the picker is that session's only route to another model.
pub(super) fn picker_lists_provider(
    probes: &[ProviderProbe],
    disabled_providers: &[ProviderKind],
    locked_provider: Option<ProviderKind>,
    remote: bool,
    kind: ProviderKind,
) -> bool {
    // Antigravity's surface is a local PTY running the CLI's TUI — a remote
    // daemon cannot host it, so the tab does not exist there.
    if remote && kind == ProviderKind::Antigravity {
        return false;
    }
    let installed = probes
        .iter()
        .any(|probe| probe.provider == kind && probe.installed);
    let switched_off = disabled_providers.contains(&kind) && locked_provider != Some(kind);
    installed && !switched_off
}

pub(super) fn model_picker_subtitle(provider: ProviderKind, sub_provider: Option<&str>) -> String {
    let provider_name = provider.short_name();
    match sub_provider.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if name.eq_ignore_ascii_case(provider_name) => provider_name.to_owned(),
        Some(name) => format!("{name} · {provider_name}"),
        None => provider_name.to_owned(),
    }
}

/// Whether the provider can return a live session to the base model after a
/// reasoning-effort pick. Both OpenCode majors express effort as a per-model
/// `variant` whose base selection is `default`, so the picker gets an explicit
/// Default row; other providers keep auto-selecting a catalog effort instead.
pub(super) fn supports_reasoning_default_reset(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::OpenCode | ProviderKind::OpenCode2)
}

/// Whether the picker has nothing left to offer, so the composer's trigger
/// and the panel behind it both swap to their empty state.
///
/// `detection_settled` gates the whole answer. Every probe is seeded as "not
/// installed" and detection answers off the UI thread, so a pass that has
/// never completed means "not known yet", never "nothing here" — otherwise
/// the trigger would flash an empty state during every launch.
pub(super) fn picker_has_no_providers(
    probes: &[ProviderProbe],
    disabled_providers: &[ProviderKind],
    locked_provider: Option<ProviderKind>,
    remote: bool,
    detection_settled: bool,
) -> bool {
    detection_settled
        && !ProviderKind::ALL.into_iter().any(|kind| {
            picker_lists_provider(probes, disabled_providers, locked_provider, remote, kind)
        })
}

/// Every picker row's fixed height — the list is uniform, which lets
/// `ListState` size its scrollbar before any row has painted.
pub(super) const MODEL_PICKER_ROW_HEIGHT: Pixels = px(58.0);

/// A rail button's destination in a model picker: the leading policy
/// block, one of the two stored-selection sections, or the first row of a
/// provider's block.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum PickerSection {
    /// Leading stance rows — Auto routing or the unmapped target. They
    /// name no provider, so they cannot borrow a provider's section.
    Policies,
    Favorites,
    Recents,
    Provider(ProviderKind),
}

/// A row's jump section in the merged list: policy rows lead alone, then
/// favorites and recents each form one block ahead of the provider blocks.
/// A provider-default row is not a stored selection — it stays glued to the
/// head of its own provider's block.
pub(super) fn picker_row_section(row: &PickerRow) -> PickerSection {
    match row {
        PickerRow::Policy(_) => PickerSection::Policies,
        PickerRow::ProviderDefault(provider) => PickerSection::Provider(*provider),
        PickerRow::Combo(row) => {
            if row.favorite_rank.is_some() {
                PickerSection::Favorites
            } else if row.recent_rank.is_some() {
                PickerSection::Recents
            } else {
                PickerSection::Provider(row.provider)
            }
        }
    }
}

/// A selection unstarred while the picker is open. The entry leaves
/// `favorite_models` at once — the star empties and the ⌘⌥ chords compact —
/// but `position` parks its row in the favorites block until the picker
/// hides, so re-starring restores the exact slot rather than appending.
#[derive(Clone)]
pub(super) struct PinnedUnfavorite {
    pub favorite: FavoriteModel,
    /// Slot in the merged favorites block — where the entry sat counting
    /// earlier parked entries — so several parked rows keep their relative
    /// order when more than one star comes off in a session.
    pub position: usize,
}

/// One selectable combo in a model picker: a model pinned to a concrete
/// effort and fast-tier choice. Favorites, recents, and the ⌘⌥1–⌘⌥9 chords
/// all address rows, not bare models.
#[derive(Clone)]
pub(super) struct ModelPickerRow {
    pub provider: ProviderKind,
    pub model: ProviderModel,
    /// The effort id the row selects; `None` when the model advertises no
    /// effort options at all.
    pub effort: Option<String>,
    /// Whether the row selects the `fast` service tier.
    pub fast: bool,
    /// Position in `favorite_models` when this exact selection is starred.
    /// Stays `None` on a parked unstar — the row renders unstarred and
    /// claims no chord even though `favorite_rank` still holds its slot.
    pub favorite_index: Option<usize>,
    /// Slot the row occupies in the favorites block: a live favorite's rank
    /// counting the parked entries ahead of it, or the slot a parked unstar
    /// holds until the picker hides. `None` outside the block.
    pub favorite_rank: Option<usize>,
    /// Rank among recently used selections — present only on the fast-variant
    /// that was actually started last, so one of `effort` and `effort-fast`
    /// ever carries it.
    pub recent_rank: Option<usize>,
}

impl ModelPickerRow {
    /// A bare model row — the `Models` granularity's whole payload.
    fn model(provider: ProviderKind, model: ProviderModel) -> Self {
        Self {
            provider,
            model,
            effort: None,
            fast: false,
            favorite_index: None,
            favorite_rank: None,
            recent_rank: None,
        }
    }
}

/// One selectable row in a model picker: a policy stance, a provider's own
/// default, or a concrete combo. Every surface draws this same row model;
/// what a pick *means* stays the site's call.
#[derive(Clone)]
pub(super) enum PickerRow {
    /// A stance that names no concrete model — the composer's Auto route,
    /// the class picker's unmapped target.
    Policy(PolicyRowId),
    /// The provider's own default model — heads its provider block.
    ProviderDefault(ProviderKind),
    /// A concrete provider/model/effort/fast-tier combination.
    Combo(ModelPickerRow),
}

/// Which stance a [`PickerRow::Policy`] row picks. The id carries only the
/// semantics — the mark and copy are the site's render choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PolicyRowId {
    /// Route the first prompt through the eval backend — composer drafts.
    Auto,
    /// Leave the class unmapped: the route keeps whatever was last used.
    NoOverride,
}

impl PolicyRowId {
    /// The stance's filter vocabulary — every whitespace token must land
    /// here for the row to survive a search. Structured keys like
    /// `provider:` never appear in it, so token filters drop policy rows.
    fn searchable(self) -> String {
        match self {
            PolicyRowId::Auto => "auto jev".to_owned(),
            PolicyRowId::NoOverride => tr!("routing.no_override").to_lowercase(),
        }
    }
}

/// The model's effective effort when the user picks it without naming one:
/// its declared default, else the first advertised option.
pub(super) fn model_default_effort(model: &ProviderModel) -> Option<String> {
    model.default_reasoning_effort.clone().or_else(|| {
        model
            .reasoning_efforts
            .first()
            .map(|option| option.id.clone())
    })
}

/// A stored selection's effective combo: packed aliases (`grok-4.6-xhigh-fast`,
/// `swe-2-high`) written before the catalog folded them resolve to the base
/// model plus the effort and fast tier their suffix carries, matching what
/// `session_model_combo` reports for the same pick. An id that resolves
/// against no advertised base keeps its stored spelling.
pub(super) fn normalize_model_combo(
    probes: &[ProviderProbe],
    provider: ProviderKind,
    model: &str,
    effort: Option<String>,
    fast: bool,
) -> (String, Option<String>, bool) {
    let Some(matched) = probes
        .iter()
        .find(|probe| probe.provider == provider)
        .and_then(|probe| {
            waku_protocol::model_catalog::packed_catalog_model(&probe.models, model, provider)
        })
        .filter(|matched| !matched.suffix.is_empty())
    else {
        return (model.to_owned(), effort, fast);
    };
    (
        matched.model.id.clone(),
        effort.or_else(|| {
            waku_protocol::model_catalog::packed_suffix_reasoning_effort(
                &matched.suffix,
                &matched.model.reasoning_efforts,
            )
        }),
        fast || waku_protocol::model_catalog::packed_suffix_service_tier(
            &matched.suffix,
            &matched.model.service_tiers,
        )
        .is_some(),
    )
}

/// Whether a starred entry marks this row. Favorites saved before rows were
/// combos carry no effort; one claims the model's default-effort row so a
/// bare `{provider, model}` star still lands on something selectable.
pub(super) fn favorite_matches_row(
    favorite: &FavoriteModel,
    provider: ProviderKind,
    model: &str,
    effort: Option<&str>,
    fast: bool,
    default_effort: Option<&str>,
) -> bool {
    if favorite.provider != provider || favorite.model != model || favorite.fast != fast {
        return false;
    }
    match favorite.effort.as_deref() {
        Some(favorite_effort) => Some(favorite_effort) == effort,
        None => effort == default_effort,
    }
}

/// How much of a model each combo row names: the composer's full
/// (effort, tier) matrix, effort rows without tiers for targets that
/// cannot encode one, or one row per model for bare `provider:model`
/// targets.
#[derive(Clone, Copy)]
pub(super) enum PickerGranularity {
    /// Every advertised effort crossed with the standard/fast pair.
    Combos,
    /// Every advertised effort on the standard tier — the routing class
    /// map stores effort but has no service-tier slot.
    Efforts,
    /// One row per model — pickers whose value is a bare provider/model
    /// pair.
    Models,
}

/// Where a model picker's selection lands: the composer session (provider,
/// model, effort, tier), a side chat's own session while its chip opened
/// the picker, or the automation editor's bare provider/model pair.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ModelPickerTarget {
    #[default]
    Composer,
    SideChat(Uuid),
    AutomationEditor,
}

/// The query after a provider rail click. The provider's token replaces any
/// `provider:*` token already present — two provider filters can never both
/// match — and clicking the provider already filtered on drops the token
/// instead. Everything else the user typed is preserved, in place when a
/// token is replaced and appended when none exists.
pub(super) fn picker_provider_query(content: &str, id: &str) -> String {
    let is_provider_token = |token: &str| matches!(token.split_once(':'), Some((key, _)) if key.eq_ignore_ascii_case("provider"));
    let active = content.split_whitespace().any(|token| {
        is_provider_token(token) && token.split_once(':').unwrap().1.eq_ignore_ascii_case(id)
    });
    let mut query = String::new();
    let mut inserted = false;
    for token in content.split_whitespace() {
        if is_provider_token(token) {
            if !active && !inserted {
                inserted = true;
                if !query.is_empty() {
                    query.push(' ');
                }
                query.push_str("provider:");
                query.push_str(id);
            }
            continue;
        }
        if !query.is_empty() {
            query.push(' ');
        }
        query.push_str(token);
    }
    if !active && !inserted {
        if !query.is_empty() {
            query.push(' ');
        }
        query.push_str("provider:");
        query.push_str(id);
    }
    query
}

/// Byte ranges of the *recognized* values in structured query tokens — the
/// `pi` in `provider:pi` — so the search field can wash them and a working
/// token reads differently from a mistyped one. Only the value half paints;
/// the key, unknown keys, and unrecognized values stay plain.
pub(super) fn picker_query_annotations(
    content: &str,
    probes: &[ProviderProbe],
) -> Vec<(Range<usize>, bool)> {
    let mut ranges = Vec::new();
    let mut base = 0;
    let mut rest = content;
    while !rest.is_empty() {
        let skipped = rest.len() - rest.trim_start().len();
        base += skipped;
        rest = &rest[skipped..];
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..len];
        if let Some((key, value)) = token.split_once(':') {
            let recognized = !value.is_empty()
                && if key.eq_ignore_ascii_case("provider") {
                    ProviderKind::ALL
                        .iter()
                        .any(|kind| kind.id().eq_ignore_ascii_case(value))
                } else if key.eq_ignore_ascii_case("effort") {
                    probes
                        .iter()
                        .filter(|probe| probe.installed)
                        .flat_map(|probe| &probe.models)
                        .flat_map(|model| &model.reasoning_efforts)
                        .any(|option| option.id.eq_ignore_ascii_case(value))
                } else {
                    false
                };
            if recognized {
                ranges.push((base + key.len() + 1..base + len, true));
            }
        }
        base += len;
        rest = &rest[len..];
    }
    ranges
}

/// Every (effort, tier) combination a catalog model expands into: one row per
/// advertised effort — a single row when the model has none — crossed with
/// the standard/fast pair when `with_tiers` and the model offers a fast tier.
fn picker_model_rows(
    provider: ProviderKind,
    model: ProviderModel,
    with_tiers: bool,
) -> Vec<ModelPickerRow> {
    let efforts: Vec<Option<String>> = if model.reasoning_efforts.is_empty() {
        vec![None]
    } else {
        model
            .reasoning_efforts
            .iter()
            .map(|option| Some(option.id.clone()))
            .collect()
    };
    let fast_variants: &[bool] =
        if with_tiers && model.service_tiers.iter().any(|option| option.id == "fast") {
            &[false, true]
        } else {
            &[false]
        };
    let mut rows = Vec::with_capacity(efforts.len() * fast_variants.len());
    for effort in efforts {
        for fast in fast_variants {
            rows.push(ModelPickerRow {
                provider,
                model: model.clone(),
                effort: effort.clone(),
                fast: *fast,
                favorite_index: None,
                favorite_rank: None,
                recent_rank: None,
            });
        }
    }
    rows
}

/// The provider's slot in `ProviderKind::ALL`, which keeps the merged list's
/// fallback ordering stable and consistent with the rest of the app.
pub(super) fn provider_sort_rank(provider: ProviderKind) -> usize {
    ProviderKind::ALL
        .iter()
        .position(|kind| *kind == provider)
        .unwrap_or(usize::MAX)
}

/// A row's slot within its model: the ladder index of its effort, or zero
/// for models that name no effort at all.
fn effort_sort_rank(row: &ModelPickerRow) -> usize {
    row.model
        .reasoning_efforts
        .iter()
        .position(|option| Some(option.id.as_str()) == row.effort.as_deref())
        .unwrap_or(0)
}

/// Whether a combo row survives the filter: every whitespace token either
/// matches a structured field (`provider:`, `effort:`) or appears in the
/// row's searchable text. Unrecognized keys and values simply match nothing.
fn combo_matches_query(row: &ModelPickerRow, normalized_query: &str) -> bool {
    let mut searchable = None;
    normalized_query
        .split_whitespace()
        .all(|token| match token.split_once(':') {
            Some(("provider", value)) => row.provider.id().eq_ignore_ascii_case(value),
            Some(("effort", value)) => row
                .effort
                .as_deref()
                .is_some_and(|effort| effort.eq_ignore_ascii_case(value)),
            _ => searchable
                .get_or_insert_with(|| {
                    format!(
                        "{} {} {} {} {} {}",
                        row.model.name,
                        row.model.id,
                        row.provider.short_name(),
                        row.model.sub_provider.as_deref().unwrap_or(""),
                        row.effort.as_deref().unwrap_or(""),
                        if row.fast { "fast" } else { "" },
                    )
                    .to_ascii_lowercase()
                })
                .contains(token),
        })
}

/// A provider-default row's filter match — the provider's searchable text
/// plus the "default" vocabulary. `provider:` narrows on its provider;
/// `effort:` drops it since a bare provider names no effort.
fn provider_default_matches_query(provider: ProviderKind, normalized_query: &str) -> bool {
    normalized_query
        .split_whitespace()
        .all(|token| match token.split_once(':') {
            Some(("provider", value)) => provider.id().eq_ignore_ascii_case(value),
            Some(("effort", _)) => false,
            _ => format!(
                "{} {} {}",
                provider.id(),
                provider.short_name(),
                tr!("routing.provider_default")
            )
            .to_ascii_lowercase()
            .contains(token),
        })
}

/// Everything [`picker_rows`] needs to build one surface's row list: which
/// policy rows lead, whether provider blocks open with a default row, how
/// much of a model each combo names, and the stored selections favorites
/// and recents decorate.
pub(super) struct PickerRowSpec<'a> {
    /// Policy rows heading the list, in order — `[PolicyRowId::Auto]` for a
    /// routable composer draft, `[PolicyRowId::NoOverride]` on the class
    /// pickers, empty on surfaces with no stance to offer.
    pub leading: &'a [PolicyRowId],
    /// Whether each provider block opens with its provider-default row —
    /// the class picker's `model: None` target. The composer never picks a
    /// bare provider, so it builds none.
    pub provider_defaults: bool,
    /// How much of a model each combo row names.
    pub granularity: PickerGranularity,
    /// Starred selections, in the user's drag order.
    pub favorites: &'a [FavoriteModel],
    /// Selections unstarred while the picker is open — parked rows.
    pub pinned: &'a [PinnedUnfavorite],
    /// Selections a session was actually started with, most recent first.
    pub recents: &'a [RecentModelUse],
    /// Providers switched off in settings — listed only while a session is
    /// locked to one.
    pub disabled_providers: &'a [ProviderKind],
    /// The provider the picker cannot leave, if one is locked in.
    pub locked_provider: Option<ProviderKind>,
    /// The normalized filter text — already trimmed and lowercased.
    pub normalized_query: &'a str,
}

/// The rows the picker lists, in display order: the leading policy rows,
/// then starred combos in their drag order, then combos a session actually
/// ran — most recent first — then provider blocks, each opening with its
/// provider-default row when the spec asks for one and continuing with its
/// combos by model name, effort ladder, and standard-before-fast.
///
/// Shared by the panel body and by `enter`'s handler so a keyboard cursor
/// index always means the same row in both.
pub(super) fn picker_rows(probes: &[ProviderProbe], spec: &PickerRowSpec) -> Vec<PickerRow> {
    let searching = !spec.normalized_query.is_empty();
    // Switched-off providers keep serving the session already locked to
    // them, but offer nothing to new work — including favorites.
    let provider_listed = |provider: ProviderKind| {
        spec.locked_provider.is_none_or(|locked| locked == provider)
            && (!spec.disabled_providers.contains(&provider)
                || spec.locked_provider == Some(provider))
    };
    let mut combos: Vec<ModelPickerRow> = probes
        .iter()
        .filter(|probe| probe.installed)
        .flat_map(|probe| {
            probe
                .models
                .iter()
                .cloned()
                .flat_map(move |model| match spec.granularity {
                    PickerGranularity::Combos => picker_model_rows(probe.provider, model, true),
                    PickerGranularity::Efforts => picker_model_rows(probe.provider, model, false),
                    PickerGranularity::Models => {
                        vec![ModelPickerRow::model(probe.provider, model)]
                    }
                })
        })
        .filter(|row| provider_listed(row.provider))
        .filter(|row| !searching || combo_matches_query(row, spec.normalized_query))
        .collect();
    mark_stored_selections(&mut combos, probes, spec);

    // Leading policy rows: a stance heads the list while its own vocabulary
    // matches every token — a `provider:` filter drops it like any row
    // that names no provider.
    let mut rows: Vec<PickerRow> = spec
        .leading
        .iter()
        .filter(|policy| {
            spec.normalized_query
                .split_whitespace()
                .all(|token| policy.searchable().contains(token))
        })
        .map(|policy| PickerRow::Policy(*policy))
        .collect();

    let mut body: Vec<PickerRow> = Vec::new();
    if spec.provider_defaults {
        body.extend(
            probes
                .iter()
                .filter(|probe| probe.installed)
                .map(|probe| probe.provider)
                .filter(|provider| provider_listed(*provider))
                .filter(|provider| {
                    !searching || provider_default_matches_query(*provider, spec.normalized_query)
                })
                .map(PickerRow::ProviderDefault),
        );
    }
    body.extend(combos.into_iter().map(PickerRow::Combo));
    body.sort_by_key(picker_row_sort_key);
    rows.extend(body);
    rows
}

/// Mark each combo row's favorite index/rank and recent rank against the
/// spec's stored selections. Stored selections may name a packed alias
/// (`grok-4.6-xhigh-fast`) from before the catalog folded aliases into base
/// models — each resolves back to base plus the traits its suffix carries,
/// so stars and recents still land on their combo row.
fn mark_stored_selections(
    rows: &mut [ModelPickerRow],
    probes: &[ProviderProbe],
    spec: &PickerRowSpec,
) {
    let normalize = |favorite: &FavoriteModel| {
        let (model, effort, fast) = normalize_model_combo(
            probes,
            favorite.provider,
            &favorite.model,
            favorite.effort.clone(),
            favorite.fast,
        );
        FavoriteModel {
            provider: favorite.provider,
            model,
            effort,
            fast,
        }
    };
    let normalized_favorites: Vec<FavoriteModel> = spec.favorites.iter().map(&normalize).collect();
    let mut normalized_pinned: Vec<(usize, FavoriteModel)> = spec
        .pinned
        .iter()
        .map(|parked| (parked.position, normalize(&parked.favorite)))
        .collect();
    normalized_pinned.sort_by_key(|(position, _)| *position);
    for row in rows.iter_mut() {
        let default_effort = model_default_effort(&row.model);
        let matches_row = |favorite: &FavoriteModel| {
            favorite_matches_row(
                favorite,
                row.provider,
                &row.model.id,
                row.effort.as_deref(),
                row.fast,
                default_effort.as_deref(),
            )
        };
        row.favorite_index = normalized_favorites.iter().position(&matches_row);
        let parked_position = normalized_pinned
            .iter()
            .find(|(_, favorite)| matches_row(favorite))
            .map(|(position, _)| *position);
        row.favorite_rank = if let Some(index) = row.favorite_index {
            // A live favorite's display slot counts the parked entries
            // holding positions ahead of it — they still occupy real rows.
            let mut rank = index;
            for (position, _) in &normalized_pinned {
                if *position <= rank {
                    rank += 1;
                }
            }
            Some(rank)
        } else {
            parked_position
        };
        row.recent_rank = spec.recents.iter().position(|use_| {
            let (model, effort, fast) = normalize_model_combo(
                probes,
                use_.provider,
                &use_.model,
                use_.effort.clone(),
                use_.fast,
            );
            use_.provider == row.provider
                && model == row.model.id
                && effort == row.effort
                && fast == row.fast
        });
    }
}

/// A row's slot in the merged order: starred combos first in their drag
/// order, then recent combos most-recent-first, then provider blocks —
/// each opening with its provider-default row — then combos by model name,
/// effort ladder, and standard-before-fast. Policy rows are prepended
/// ahead of the sort and never pass through it.
fn picker_row_sort_key(
    row: &PickerRow,
) -> (bool, usize, bool, usize, usize, u8, String, usize, bool) {
    match row {
        PickerRow::Policy(_) => unreachable!("policy rows never enter the body sort"),
        PickerRow::ProviderDefault(provider) => (
            true,
            usize::MAX,
            true,
            usize::MAX,
            provider_sort_rank(*provider),
            0,
            String::new(),
            0,
            false,
        ),
        PickerRow::Combo(row) => (
            row.favorite_rank.is_none(),
            row.favorite_rank.unwrap_or(usize::MAX),
            row.recent_rank.is_none(),
            row.recent_rank.unwrap_or(usize::MAX),
            provider_sort_rank(row.provider),
            1,
            row.model.name.to_lowercase(),
            effort_sort_rank(row),
            row.fast,
        ),
    }
}

/// One picker instance's UI state: the filter field, the virtualized row
/// list, the drawn keyboard cursor, and the no-providers state's focus.
/// Every model-target picker owns one — the shape is the same on every
/// surface, so the pickers cannot drift on behavior.
pub(super) struct PickerState {
    /// The filter field inside the panel; it keeps focus while open.
    pub search: Entity<TextInput>,
    /// The virtualized row list and its overlay scrollbar.
    pub list: ListState,
    pub scrollbar: Rc<ScrollbarState>,
    /// The drawn keyboard cursor — `None` until the keyboard moves, so
    /// `enter` on a fresh panel takes the first row.
    pub highlight: Option<usize>,
    /// Focus for the no-providers state. The panel takes focus on open so
    /// `escape` has a focused descendant to dispatch up from, and normally
    /// that is the filter field — which the empty state does not draw, so
    /// its one button holds focus instead.
    pub empty_focus: FocusHandle,
}

impl PickerState {
    /// A picker's full UI state — every surface's search field is the same
    /// control: escape clears, and the label and placeholder match.
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("input.search_models"))
                .placeholder(tr!("input.search_models"))
        });
        Self {
            search,
            list: ListState::new(0, ListAlignment::Top, px(512.0))
                .with_uniform_item_height(MODEL_PICKER_ROW_HEIGHT),
            scrollbar: ScrollbarState::new(),
            highlight: None,
            empty_focus: cx.focus_handle(),
        }
    }

    /// Keep the virtualized list's item count in step with the rows a
    /// scroll is about to target — `reset` drops the scroll position, so it
    /// only runs when the total changed.
    pub(super) fn sync_list(&self, count: usize) {
        if self.list.item_count() != count {
            self.list
                .reset_with_uniform_height(count, MODEL_PICKER_ROW_HEIGHT);
        }
    }

    /// Bring `index` into view — the reveal every surface issues for its
    /// current selection on open and on a cleared query. Without a row it
    /// falls back to the top, so a scroll offset from an earlier open never
    /// leaks into a fresh list.
    pub(super) fn reveal(&self, index: usize, count: usize) {
        self.sync_list(count);
        self.list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: Pixels::ZERO,
        });
    }

    /// Move the drawn cursor one step, wrapping at both ends — seeded from
    /// `seed` when the keyboard has not moved yet, so the first arrow walks
    /// from the current selection's row rather than jumping to an end.
    /// Returns the new cursor when the key moved it.
    pub(super) fn move_highlight(
        &mut self,
        seed: Option<usize>,
        len: usize,
        key: &str,
    ) -> Option<usize> {
        let current = self.highlight.filter(|index| *index < len).or(seed);
        let next = next_picker_highlight(current, len, key)?;
        self.highlight = Some(next);
        self.list.scroll_to_reveal_item(next);
        Some(next)
    }

    /// A rail button's jump: drop the filter and bring the section's first
    /// row into view, leaving the keyboard cursor on it so the next arrow
    /// moves from there. Clearing the query routes back through the search
    /// subscription's selection reveal, so the section scroll issued after
    /// it is the request that lands.
    pub(super) fn scroll_to_section(
        &mut self,
        rows: &[PickerRow],
        section: PickerSection,
        cx: &mut Context<Waku>,
    ) {
        self.search.update(cx, |search, cx| search.clear(cx));
        let first = rows
            .iter()
            .position(|row| picker_row_section(row) == section);
        self.highlight = first;
        if let Some(index) = first {
            self.reveal(index, rows.len());
        }
        cx.notify();
    }

    /// Step the rail to the adjacent section, wrapping at both ends.
    /// `tab`/`shift-tab` land here from under the focused filter field, the
    /// same route the arrows take. A live query filters across all
    /// sections, so cycling waits until the field is cleared.
    pub(super) fn cycle_section(
        &mut self,
        rows: &[PickerRow],
        seed_row: usize,
        key: &str,
        cx: &mut Context<Waku>,
    ) {
        if !self.search.read(cx).content().trim().is_empty() {
            return;
        }
        let mut sections = Vec::new();
        let mut previous = None;
        for (index, row) in rows.iter().enumerate() {
            let section = picker_row_section(row);
            if previous != Some(section) {
                sections.push(index);
                previous = Some(section);
            }
        }
        if sections.is_empty() {
            return;
        }
        // The section the keyboard cursor sits in — seeded from the site's
        // current-selection row the way the reveal lands, so the first tab
        // steps relative to the selection rather than an end.
        let current_row = self.highlight.unwrap_or(seed_row);
        let current_section = sections
            .iter()
            .rposition(|start| *start <= current_row)
            .unwrap_or(0);
        let Some(next) = next_picker_highlight(Some(current_section), sections.len(), key) else {
            return;
        };
        self.highlight = Some(sections[next]);
        self.reveal(sections[next], rows.len());
        cx.notify();
    }

    /// A provider rail button's filter: write a `provider:<id>` token into
    /// the query, replacing the provider token already there, or removing
    /// it when the same provider is clicked again. Other tokens and free
    /// text survive, so the rail composes with a hand-typed query. The edit
    /// emits through the search subscription, which pins the cursor to the
    /// first match the same way a keystroke would.
    pub(super) fn toggle_provider(&self, provider: ProviderKind, cx: &mut Context<Waku>) {
        let content = self.search.read(cx).content().to_owned();
        let query = picker_provider_query(&content, provider.id());
        self.search
            .update(cx, |search, cx| search.set_content(query, cx));
    }
}

/// The composer/editor picker's shared state — one [`PickerState`] serves
/// both targets since the panel is only ever open for one.
pub(super) fn model_picker_state(this: &mut Waku) -> &mut PickerState {
    &mut this.model_picker
}

/// The class-target picker's shared state — one [`PickerState`] serves all
/// three class menus; only one can be open at a time.
pub(super) fn route_class_picker_state(this: &mut Waku) -> &mut PickerState {
    &mut this.route_class_picker
}

/// The search subscription's shared behavior on `Edited`: wash the
/// recognized values in structured tokens — `provider:pi`'s `pi` — so a
/// working filter reads differently from a mistyped one, then either pin
/// the cursor to the first match (so `enter` has a visible target) or, on a
/// cleared query, return to the opening state: nothing highlighted, the
/// site's selection row back in view.
pub(super) fn picker_search_edited(
    picker: &mut PickerState,
    probes: &[ProviderProbe],
    rows: &[PickerRow],
    seed: Option<usize>,
    cx: &mut Context<Waku>,
) {
    let annotations = picker_query_annotations(picker.search.read(cx).content(), probes);
    picker.search.update(cx, |search, cx| {
        search.set_annotation_ranges(annotations, cx);
    });
    if picker.search.read(cx).content().trim().is_empty() {
        picker.highlight = None;
        picker.reveal(seed.unwrap_or(0), rows.len());
    } else {
        picker.highlight = Some(0);
        picker.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: Pixels::ZERO,
        });
    }
    cx.notify();
}

/// A policy row's mark, title, and subtitle — the same two-line shape combo
/// rows draw, so the stance rows read alike on every surface.
pub(super) fn policy_row_parts(id: PolicyRowId, theme: &Theme) -> (AnyElement, String, String) {
    match id {
        PolicyRowId::Auto => (
            icon("icons/provider-typesafe.svg", 12.0, theme.text_tertiary).into_any_element(),
            tr!("models.auto"),
            "Jev".to_owned(),
        ),
        PolicyRowId::NoOverride => (
            icon("icons/sparkle.svg", 14.0, theme.accent.opacity(0.9)).into_any_element(),
            tr!("routing.no_override"),
            tr!("routing.no_override_description"),
        ),
    }
}

/// A picker row's two-line body: the title line — bold name plus whatever
/// trailing chips the site adds — over the mark-and-subtitle detail line.
/// Shared so a row reads identically on every surface the panel serves.
pub(super) fn model_picker_row_body(
    title: String,
    title_trailing: impl IntoIterator<Item = AnyElement>,
    mark: AnyElement,
    subtitle: String,
    theme: &Theme,
) -> Div {
    div()
        .min_w_0()
        .flex_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(SharedString::from(title)),
                )
                .children(title_trailing),
        )
        .child(
            div()
                .mt(px(4.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(mark)
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(SharedString::from(subtitle)),
                ),
        )
}

/// A button on the model-picker panel's left rail: a mark that runs the
/// activation the call site attached — a section jump or a query filter.
/// `active` paints the wash that mirrors a live filter token.
pub(super) struct ModelPickerRailItem {
    pub id: SharedString,
    pub mark: AnyElement,
    pub active: bool,
    pub on_activate: Rc<dyn Fn(&mut Waku, &mut Context<Waku>)>,
}

/// A rail button that jumps a section: clear the query and bring the
/// section's first row into view. `rows` rebuilds the site's unfiltered
/// list — which sections exist is answered from the unfiltered list, not
/// from what a live search happens to leave.
pub(super) fn picker_section_rail_item(
    id: impl Into<SharedString>,
    mark: AnyElement,
    section: PickerSection,
    rows: impl Fn(&Waku) -> Vec<PickerRow> + 'static,
    picker: fn(&mut Waku) -> &mut PickerState,
) -> ModelPickerRailItem {
    ModelPickerRailItem {
        id: id.into(),
        mark,
        active: false,
        on_activate: Rc::new(move |this, cx| {
            let rows = rows(this);
            picker(this).scroll_to_section(&rows, section, cx);
        }),
    }
}

/// A provider rail button: toggles the query's `provider:<id>` token — the
/// button's `active` wash mirrors the token's presence.
pub(super) fn picker_provider_rail_item(
    provider: ProviderKind,
    mark: AnyElement,
    active: bool,
    picker: fn(&mut Waku) -> &mut PickerState,
) -> ModelPickerRailItem {
    ModelPickerRailItem {
        id: SharedString::from(format!("model-rail-{}", provider.id())),
        mark,
        active,
        on_activate: Rc::new(move |this, cx| {
            picker(this).toggle_provider(provider, cx);
        }),
    }
}

/// Everything the shared model-picker panel needs from a call site: the rows
/// in display order, the state it draws, and what a pick does. The panel owns
/// the frame, rail, filter field, virtualized list, scrollbar, and the
/// arrow/tab/enter contract; the site owns row content and selection
/// semantics.
pub(super) struct ModelPickerPanel {
    /// Rows in display order — the same list `enter`'s handler indexes, so a
    /// keyboard cursor always means the same row in both.
    pub rows: Rc<Vec<PickerRow>>,
    /// The filter field inside the panel; it keeps focus while open.
    pub search: Entity<TextInput>,
    /// The virtualized row list and its overlay scrollbar.
    pub list_state: ListState,
    pub scrollbar_state: Rc<ScrollbarState>,
    /// The drawn keyboard cursor, when it has moved.
    pub highlight: Option<usize>,
    /// What an empty list says — the site picks the wording.
    pub empty_label: SharedString,
    /// Rail buttons above the divider — the site's leading sections.
    pub rail_sections: Vec<ModelPickerRailItem>,
    /// Provider buttons below the divider.
    pub rail_providers: Vec<ModelPickerRailItem>,
    /// A row's full element — build it on [`model_picker_row_shell`] so every
    /// surface's rows share chrome.
    pub render_row: Rc<
        dyn Fn(usize, &PickerRow, bool, &ContextMenuHandle, &mut Window, &mut App) -> AnyElement,
    >,
    /// Arrow keys — move the drawn cursor and scroll its row into view.
    pub on_move: Rc<dyn Fn(&mut Waku, &str, &[PickerRow], &mut Context<Waku>)>,
    /// `enter` — take the highlighted row, defaulting to the first.
    pub on_confirm: Rc<dyn Fn(&mut Waku, &[PickerRow], &mut Context<Waku>)>,
    /// Tab / shift-tab section cycling, when the site has sections.
    pub on_cycle_section: Option<Rc<dyn Fn(&mut Waku, &str, &mut Context<Waku>)>>,
}

/// The model picker's whole panel body: the rail, the filter field, a
/// virtualized row list with its overlay scrollbar, and the keyboard
/// contract — the one component every model-target picker draws so the
/// surfaces cannot drift apart.
pub(super) fn model_picker_panel(
    spec: ModelPickerPanel,
    popover: &ContextMenuHandle,
    theme: &Theme,
    weak: &WeakEntity<Waku>,
) -> AnyElement {
    let ModelPickerPanel {
        rows,
        search,
        list_state,
        scrollbar_state,
        highlight,
        empty_label,
        rail_sections,
        rail_providers,
        render_row,
        on_move,
        on_confirm,
        on_cycle_section,
    } = spec;

    let rail_button = |item: ModelPickerRailItem| {
        let activate_weak = weak.clone();
        let on_activate = item.on_activate;
        div()
            .id(item.id)
            .w(px(38.0))
            .h(px(38.0))
            .rounded(px(9.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .when(item.active, |element| element.bg(theme.overlay))
            .hover(|element| element.bg(theme.overlay))
            .child(item.mark)
            .on_click(move |_, _, cx| {
                let on_activate = on_activate.clone();
                let _ = activate_weak.update(cx, |this, cx| on_activate(this, cx));
            })
    };
    let mut rail = div()
        .w(px(50.0))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.0))
        .p(px(5.0))
        .rounded_tl(px(15.0))
        .rounded_bl(px(15.0))
        .bg(theme.canvas)
        .border_r(hairline())
        .border_color(theme.separator);
    let separated = !rail_sections.is_empty();
    for item in rail_sections {
        rail = rail.child(rail_button(item));
    }
    if separated {
        rail = rail.child(
            div()
                .w(px(34.0))
                .h(hairline())
                .my(px(3.0))
                .bg(theme.separator),
        );
    }
    for item in rail_providers {
        rail = rail.child(rail_button(item));
    }

    let search_input = div()
        .h(px(52.0))
        .px(px(12.0))
        .pt(px(10.0))
        .pb(px(8.0))
        .flex_none()
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .h(px(34.0))
                .px(px(10.0))
                .rounded(px(11.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(icon("icons/search.svg", 15.0, theme.text_secondary))
                .child(div().flex_1().min_w_0().child(search.clone())),
        );

    // The horizontal padding is the rows' transparent side gutter — `list`
    // only honors vertical padding on its items, so it lives on the
    // container to keep row fills clear of the panel edge.
    let mut list_element = div().id("model-picker-list").size_full().px(px(4.0));
    if rows.is_empty() {
        list_element = list_element.p(px(9.0)).child(
            div()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(sp(12.5))
                .text_color(theme.text_ghost)
                .child(empty_label),
        );
    } else {
        // The list is virtualized and every row is one height, so the item
        // count resyncs here and before every scroll — `reset` drops the
        // scroll position, which is also what a changed total should mean.
        if list_state.item_count() != rows.len() {
            list_state.reset_with_uniform_height(rows.len(), MODEL_PICKER_ROW_HEIGHT);
        }
        let list_rows = rows.clone();
        let list_popover = popover.clone();
        list_element = list_element.child(
            list(list_state.clone(), move |row_index, window, cx| {
                let Some(row) = list_rows.get(row_index) else {
                    return div().into_any_element();
                };
                render_row(
                    row_index,
                    row,
                    highlight == Some(row_index),
                    &list_popover,
                    window,
                    cx,
                )
            })
            .size_full()
            .p(px(9.0)),
        );
    }

    let down_rows = rows.clone();
    let up_rows = rows;
    let confirm_rows = down_rows.clone();
    let down_weak = weak.clone();
    let up_weak = weak.clone();
    let confirm_weak = weak.clone();
    let confirm_popover = popover.clone();
    let on_move_down = on_move.clone();
    let mut panel = div()
        .w(px(460.0))
        .h(px(390.0))
        .rounded(px(16.0))
        .overflow_hidden()
        .border(hairline())
        .border_color(theme.border_subtle)
        .bg(theme.surface)
        .shadow_lg()
        .flex()
        // The filter field keeps focus and the selected row is only drawn,
        // never focused — the same split Zed's picker uses. These arrive as
        // actions bound to `Menu > TextInput`, which is the only way to
        // claim a key out from under a focused text field.
        .on_action(move |_: &SelectNextEntry, _, cx| {
            let on_move = on_move_down.clone();
            let rows = down_rows.clone();
            let _ = down_weak.update(cx, |this, cx| on_move(this, "down", &rows, cx));
        })
        .on_action(move |_: &SelectPreviousEntry, _, cx| {
            let rows = up_rows.clone();
            let _ = up_weak.update(cx, |this, cx| on_move(this, "up", &rows, cx));
        })
        .on_action(move |_: &ConfirmEntry, window, cx| {
            let rows = confirm_rows.clone();
            let _ = confirm_weak.update(cx, |this, cx| on_confirm(this, &rows, cx));
            confirm_popover.close(window, cx);
            window.refresh();
        });
    if let Some(on_cycle_section) = on_cycle_section {
        let down_weak = weak.clone();
        let up_weak = weak.clone();
        let cycle_down = on_cycle_section.clone();
        panel = panel
            .on_action(move |_: &SelectNextTab, _, cx| {
                let cycle = cycle_down.clone();
                let _ = down_weak.update(cx, |this, cx| cycle(this, "down", cx));
            })
            .on_action(move |_: &SelectPreviousTab, _, cx| {
                let _ = up_weak.update(cx, |this, cx| on_cycle_section(this, "up", cx));
            });
    }
    panel
        .child(rail)
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .rounded_tr(px(15.0))
                .rounded_br(px(15.0))
                .bg(theme.surface)
                .child(search_input)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(list_element)
                        .child(scrollbar::vertical(&list_state, &scrollbar_state)),
                ),
        )
        .into_any_element()
}

/// Every picker row's chrome: fixed height, hit area, and the
/// selected/highlighted/hover fills — shared so a row reads identically on
/// every surface the panel serves.
#[track_caller]
pub(super) fn model_picker_row_shell(
    id: impl Into<gpui::ElementId>,
    selected: bool,
    highlighted: bool,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        // A flex item laid out as its list's root shrink-wraps, so the row
        // must stretch itself; the slot's padding keeps this fill clear of
        // the panel edge.
        .w_full()
        .h(MODEL_PICKER_ROW_HEIGHT)
        .px(px(12.0))
        .rounded(px(11.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_default()
        .when(selected, |element| element.bg(theme.overlay_strong))
        .hover(|element| element.bg(theme.overlay))
        .active(|element| element.opacity(0.85))
        // The keyboard cursor reads as an accent tint rather than a ring, so
        // it stays legible on the current row's already-filled surface.
        .when(highlighted, |element| element.bg(theme.focus_highlight()))
}

/// A starred model selection dragged within the picker to reorder the
/// favorites section. `index` is its position in `state.favorite_models`.
#[derive(Clone)]
pub(super) struct FavoriteModelDrag {
    pub index: usize,
    pub label: SharedString,
}

/// The view GPUI drags under the cursor for a [`FavoriteModelDrag`]: the
/// starred row's model name as a chip.
pub(super) struct FavoriteModelDragView {
    pub label: SharedString,
}

impl gpui::Render for FavoriteModelDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div()
            .h(px(24.0))
            .pl(px(6.0))
            .pr(px(10.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.composer)
            .flex()
            .items_center()
            .gap(px(5.0))
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .child(icon("icons/star-filled.svg", 11.0, theme.favorite))
            .child(self.label.clone())
    }
}
