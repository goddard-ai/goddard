# Proposal: Replace Umami with PostHog

> Move Goddard's release-only desktop analytics to PostHog while preserving the existing event vocabulary, anonymous identity, opt-out control, and non-blocking delivery guarantees.

Date: 2026-09-24
Status: Proposed

## Recommendation

Adopt PostHog Cloud and the official [`posthog-rs`](https://github.com/PostHog/posthog-rs) crate behind the existing `Analytics` façade. Keep the current 26-event product vocabulary and persisted installation ID. Send only personless, coarse product events:

- set `$process_person_profile` to `false` on every event;
- disable GeoIP enrichment;
- do not enable autocapture, session replay, feature flags, groups, or identification;
- preserve the existing anonymous-analytics toggle and default-on behavior;
- keep analytics release-only, build-configured, bounded, and best-effort.

Use PostHog Cloud EU by default for a privacy-forward global desktop product, while keeping the ingestion host build-time configurable so a US project can be selected if that better matches the product's legal or operational requirements. PostHog recommends Cloud over self-hosting and offers separate US and EU Cloud regions. See its [FAQ](https://github.com/PostHog/posthog.com/blob/master/contents/faq.mdx) and [privacy guidance](https://github.com/PostHog/posthog.com/blob/master/contents/docs/privacy/gdpr-compliance.mdx).

## What exists today

The integration is isolated in [`src/analytics.rs`](../src/analytics.rs):

| Concern | Current behavior |
| --- | --- |
| Event surface | 26 typed events, converted to stable dotted names such as `app.launched`, `provider.turn.sent`, and `daemon.recovery` |
| Identity | Random installation-scoped UUID persisted in app state, unrelated to accounts, projects, paths, or session content |
| Privacy | No prompts, project names or paths, provider output, or provider-account identity |
| UI boundary | `track` uses a bounded 128-item `try_send`; no network work occurs on the caller's path |
| Delivery | One `waku-analytics` worker, a five-second request timeout, and one best-effort send per event |
| Consent | `analytics_enabled` is persisted, defaults to `true`, and disabling it also drops events already waiting in the worker |
| Build scope | Debug builds never enable analytics; release builds require compile-time endpoint and website-ID configuration |

The main construction point is [`src/app.rs`](../src/app.rs), while the setting is rendered and persisted separately. [`docs/daemon-diagnostics.md`](daemon-diagnostics.md) currently names Umami in its diagnostic event reference and should be updated as part of the cutover.

## Research findings

### PostHog fits the transport boundary

PostHog's public capture API accepts events from any language that can make an HTTP request and authenticates ingestion with a project token. The correct public ingestion domains are region-specific, for example `https://eu.i.posthog.com` or `https://us.i.posthog.com`; the [API overview](https://github.com/PostHog/posthog.com/blob/master/contents/docs/api/index.mdx) documents the endpoint and region rules.

The official Rust SDK currently provides a background transport with batching, retries, a bounded queue, `flush`, `shutdown`, and a `before_send` hook. Its async `capture` method is fire-and-forget and does not wait for network delivery. That matches the app's best-effort analytics contract better than adding a hand-written HTTP client.

### PostHog's identity defaults are not safe to inherit blindly

PostHog distinguishes an event's `distinct_id` from whether it creates a person profile. The SDK's anonymous-event helper generates a new ID itself, which is not appropriate here because Goddard already has a stable installation ID. The migration should construct each event with the persisted ID and explicitly set `$process_person_profile: false`.

The SDK also defaults GeoIP enrichment on. Set `disable_geoip: true` explicitly. Do not call `identify`, send `$set` properties, or add groups. This preserves the current promise that analytics is anonymous usage data rather than account telemetry.

### Cost is unlikely to be the deciding constraint, but it needs a guard

PostHog currently includes 1 million analytics events per month in its free allowance. The free plan stops ingesting additional events after the allowance; pay-as-you-go can charge above it, with per-product billing limits available. Set the project's analytics billing limit before production rollout and add a monthly event-volume check. The current event schema is small enough that the likely cost driver is installation count, not payload size.

### Historical migration is not required

Umami and PostHog do not share an event schema or identity model. Do not backfill old Umami data into PostHog for this change. Keep the Umami project read-only for historical comparison, mark the PostHog cutover date in the dashboard, and recreate only the active product views in PostHog.

## Proposed design

Keep the caller-facing API and event enum unchanged. Replace only the transport and provider-specific conversion:

```text
app event sites
      │
      ▼
Analytics::track ── bounded try_send ──► waku-analytics worker
                                             │
                                             ▼
                                  posthog-rs Event + privacy gates
                                             │
                                             ▼
                                  PostHog regional ingestion API
```

The first implementation should retain the outer Goddard queue. The SDK may use its own worker internally, but keeping the existing queue preserves the current UI-thread guarantee and makes the migration easy to test. Configure the SDK's `before_send` hook to read the same atomic enabled flag so events already handed to the SDK are dropped if the user opts out before delivery. Flush and shut down the SDK worker when the Goddard worker exits.

The PostHog event contract should be:

| Field | Value |
| --- | --- |
| `event` | Existing dotted event name, unchanged |
| `distinct_id` | Existing persisted `analytics_id`, unchanged |
| `properties` | Existing event fields plus `clientType`, `version`, `platform`, `arch`, `build`, and `language` |
| `$process_person_profile` | `false` on every event |
| GeoIP | Disabled at the SDK/client level |
| Identity/profile fields | None; no `identify`, `$set`, email, name, path, prompt, or provider output |

The static Umami context (`goddardai.org`, `/desktop`, title, and device label) does not add useful product signal in PostHog. Keep the meaningful coarse fields as explicit properties and omit the redundant static URL/hostname fields.

## Configuration and rollout

Replace the Umami-specific build inputs with:

```text
GODDARD_POSTHOG_API_KEY   PostHog project token for capture
GODDARD_POSTHOG_HOST      Regional ingestion host, e.g. https://eu.i.posthog.com
```

The project token is used only for the public capture API; no personal or project-secret API key belongs in the desktop binary. Pass these values through [`scripts/release.ts`](../scripts/release.ts) and each release job in [`.github/workflows/release.yml`](../.github/workflows/release.yml). Remove `GODDARD_ANALYTICS_ENDPOINT`, `GODDARD_ANALYTICS_WEBSITE_ID`, and the old Umami dependency after the first PostHog-backed release is verified.

Roll out in this order:

1. Create a separate PostHog project for release validation, choose its region, set a zero or tightly bounded billing limit, and confirm that captured events are personless.
2. Add the PostHog adapter behind `Analytics`; do not change call sites or event names.
3. Test a release build against a local HTTP fixture or PostHog validation project. Confirm event names, properties, installation identity, no-person processing, and opt-out behavior.
4. Update release secrets and build scripts for all platforms, then ship one release with the Umami project retained read-only.
5. Recreate the useful Umami charts in PostHog and remove the old dependency, configuration, and documentation references once the release is confirmed.

## Acceptance criteria

- Every existing analytics call site still compiles without a caller-facing API change.
- A captured event has the expected PostHog name, stable `distinct_id`, existing product properties, and the common version/platform fields.
- A captured event has `$process_person_profile: false`; no person profile, GeoIP property, prompt, path, project name, provider output, or account identity is sent.
- Toggling analytics off prevents both new events and events waiting in either queue from reaching the network.
- Debug builds make no PostHog requests. Release builds with missing configuration remain analytics-disabled and still start normally.
- Queue-full, timeout, rejected-request, SDK initialization, and shutdown failures cannot block or fail app startup, rendering, or user actions.
- The release pipeline embeds the selected region and project token without exposing any personal or project-secret API key.
- `docs/daemon-diagnostics.md` refers to PostHog rather than Umami, and the Umami crate and release variables are gone after cutover.

## Implementation checklist

- [ ] Replace `rust-umami` with a pinned compatible `posthog-rs` release in `Cargo.toml` and `Cargo.lock`.
- [ ] Add the PostHog adapter and privacy gates in `src/analytics.rs`; retain the existing typed events and tests.
- [ ] Add serialization tests for representative lifecycle, turn, and daemon events.
- [ ] Add a mock-ingestion test covering opt-out-before-delivery and missing release configuration.
- [ ] Update [`scripts/release.ts`](../scripts/release.ts) and all release workflow jobs.
- [ ] Recreate dashboards and record the cutover date in the PostHog project.
- [ ] Update [`docs/daemon-diagnostics.md`](daemon-diagnostics.md), remove Umami references, and run the repository's normal Rust checks.

## Decision needed before implementation

Approve PostHog Cloud as the managed backend and choose the production region. The proposal defaults to EU Cloud because the product already promises privacy-conscious anonymous analytics; use US Cloud instead if the product's data-controller or operational requirements call for it. The code should keep this choice in `GODDARD_POSTHOG_HOST`, so changing regions does not require another analytics implementation.

### Sources

- [PostHog API overview](https://github.com/PostHog/posthog.com/blob/master/contents/docs/api/index.mdx)
- [PostHog Rust SDK](https://github.com/PostHog/posthog-rs)
- [Anonymous vs. identified events](https://github.com/PostHog/posthog.com/blob/master/contents/docs/data/anonymous-vs-identified-events.mdx)
- [PostHog pricing](https://posthog.com/pricing)
- [PostHog privacy and GDPR guidance](https://github.com/PostHog/posthog.com/blob/master/contents/docs/privacy/gdpr-compliance.mdx)
