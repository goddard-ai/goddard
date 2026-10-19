# Boss provider and startup-budget contract

No current provider configuration is qualified for the approved pre-compaction
guarantee. Item 2 can consume the fail-closed startup interface below, but must
not activate a successor using historical veto evidence, a receipt or pointer
ownership. A positive arithmetic result alone cannot qualify a provider.

This is Rudolph's item-1 evidence packet for approved revision
`dce71999-e6b3-4b80-8764-ba30fbdd9363`, plan
`922930c1-3fc4-4572-9391-4b7078528ade`. The frozen plan is unchanged.
The report distinguishes source inspection, host observations and missing proof.

## Baselines and observations

Inspected on 2026-10-19. Ivan's historical baseline was `f83cf3f3299d`,
Codex 0.159.0 and Pi 0.85.1. Current assigned checkout and local `dev` began at
`60310e6656295bb83408a924b546978a3de35dbb`. Installed version commands report
Codex **0.162.1** and Pi **0.85.1**. These commands identify PATH installations,
not the Boss's effective runtime.

The running daemon was PID 1831, started Oct 19 at 12:20:38 local time, with
an executable mapped from the QA debug app. The observed executable file digest
was `84dd711d4be6b6fe321211ee46948f7409939acefe72aa81e4fa45e5790e28e7`.
This identifies an on-disk file, not the loaded source revision or an effective
provider configuration. Several running Codex processes use the installed
0.162.1 package; they were not correlated to the Boss. No daemon restart,
provider turn, compaction trigger or private transcript inspection was performed.

`DaemonSettings::default_path` selects the home `.goddard/settings.json`;
`main.rs` opens that store with a legacy task-directory fallback. The current
production file digest was
`17f2aaddd122c6886c7fbb85b70aae2400ff48075dd37b08a2fbfd377123df00`.
Only selected routing fields were emitted; secret fields were excluded.

| File setting | Observed value |
| --- | --- |
| easy | codex / gpt-6-luna / max |
| medium | codex / gpt-6.1-sol / medium |
| hard | codex / gpt-6.1-sol / high |
| saved provider-specific Devin routes | swe-2; medium/high/max efforts |
| rotation disabled / threshold | false / 0.8 |
| binary override | Codex override present |

These are file settings, **not a running-daemon snapshot**. The effective
in-memory routes, Boss session model/effort overrides, executable/config revision,
provider instruction/context overrides, loaded hooks/extensions and launch trust
are unknown. The inspected metadata has address/token fields and no source
revision; build-stat fields are counters. A read-only `boss view` succeeded,
but exposes identity/session pointers rather than effective model or settings. No credentials were emitted or used
to bypass the assigned CLI surface. No restart is needed to preserve this honest
unknown result. A future qualified adapter must expose correlated effective
launch and boundary evidence; installed versions cannot fill these fields.

## Provider matrix

| Configuration | Prevention evidence | Boundaries and telemetry | Verdict |
| --- | --- | --- | --- |
| Codex 0.162.1; easy/medium/hard file routes | Official PreCompact veto candidate; Ivan's 0.159 auto-compaction probe is historical. Current launch code does not install a Boss veto or bypass-hook-trust configuration. Effective loaded controls unknown. | Goddard sees prompts/steers/responses; provider owns internal loops. `thread/tokenUsage/updated` supplies `last.totalTokens` and `modelContextWindow`, independently optional. No finite next-loop growth or startup upper bound established. | Unqualified; startup blocked |
| Pi 0.85.1; any model | Historical source-veto evidence only. Current Pi launcher adds a delegation extension, not a Boss compaction-veto extension. No current trigger probe. | `message_end` assistant usage uses totalTokens or summed input/output/cache counters. Window can arrive separately. Internal continuation is not admitted by Goddard. | Unqualified; startup blocked |
| Devin/swe-2 saved routes | No established prevention contract; saved routes do not prove active selection. | No qualified compaction boundary or coherent context bound. Stable-runtime transport evidence does not supply it. | Unqualified; startup blocked |
| Claude, OpenCode, OhMyPi and all remaining ProviderKind values | No current effective Boss control established. Documentation candidates do not qualify launches. | Required boundary/control and accounting evidence absent. | Unqualified; startup blocked |

For **every current configuration**, C is unknown as a verified lower compaction
or equivalent capacity bound; U is unknown as a conservative effective-context
upper bound including startup instructions, attachments and overhead; G is
unbounded by the adapter between daemon-visible calls; M has no defensible finite
error bound. A reported model window is not C, a recent usage count is not U,
and a veto alone does not establish finite G or context-capacity safety. No
numerical safety margin or headroom is claimed. The 0.8 file setting remains an
occupancy heuristic, independent of this contract.

Current source evidence: `rotation_continuity::qualify_provider` previously
promoted version floors; `boss_rotation::boss_provider_qualification` performs
an installed-path version probe. The ownership driver checks pointer, archive
and incident state, not measured budget qualification. Codex thread start/resume
and Pi launch paths provide no Boss prevention setup. The adapter must guard
first request, each next request/tool continuation, resume, replay and autonomous
activity; post-hoc usage or compaction cards cannot substitute for admission.

Official [Codex hooks documentation](https://learn.chatgpt.com/docs/hooks)
describes stopping compaction with `PreCompact` and `continue: false`. This
confirms a candidate mechanism, not current deployment or capacity safety.

## Item-2 integration interface

[`provider_budget.rs`](../../crates/waku-boss/src/provider_budget.rs) exports:

```rust
validate_context_budget(&BudgetBounds) -> Result<u64, BudgetBlocker>
validate_startup_budget(&StartupBudgetBinding,
    Option<&StartupBudgetEvidence>) -> Result<u64, BudgetBlocker>
```

The returned number is positive headroom. All current startup routes return an
error even with synthetic safe numbers. `qualify_provider` now returns
Unqualified for every provider/version; old RuntimeVerified/CodeVerified ledger
records also cannot grant qualification. Their serialized forms remain readable.

The binding contains provider, **effective** runtime version, model, coherent
configuration revision, instruction revision, runtime ID, generation, exact
startup-package digest and a fresh operation ID. Evidence must match every field.
Empty identity/revision fields or nil runtime/operation IDs block. The caller must
obtain authoritative expected values from the actual launched adapter, never
copy them from the evidence itself or infer them from disk. A changed package,
model, policy/configuration, runtime or operation invalidates the result.

Bounds contain optional C/U/G/M in one coherent effective-token unit. Missing
means unknown/unbounded. U must cover the complete staged startup package and
all provider overhead; G must cover growth through the next enforced checkpoint.
The validator rejects unknown terms, zero M, arithmetic overflow and equality
or crossing of C. Numeric validation uses the existing oracle ContextBounds
arithmetic; no guessed startup tokenizer or percentage was introduced.

Item 2 must retain blocked packages durably and withhold submission/activation
when this returns an error. A successful future budget check would still require
independent identity, scope, grants, event-position, effect reconciliation and
transport-receipt validation. It is not an ownership-transfer permission.
Repeated empty-session rotation cannot repair an oversized package. Each
continuation requires new evidence, and the lifecycle must atomically prevent
reuse/races; this stateless API does not enforce single use or ownership.

**Integration boundary:** item 1 provides qualification decisions and the
startup validator. The [item-2 coordinator contract](boss-coordinator-contract.md)
now stages production startup packages and invokes validation before launch;
all current configurations remain blocked. Existing handles and scoped
coordinator CLI actions also fail closed. Activation, provider controls and
lifecycle pauses remain separate work. These source changes do not update a
running daemon or qualify its provider. Do not advertise the full prevention
guarantee from deterministic unit tests.

## Verification and remaining qualification

`goddard-agent resource run --json '{"resources":{"native_builds":1},"purpose":"Boss provider budget contract tests"}' -- mbx test -p waku-boss --lib`
passed **188 tests**, including the new budget tests and existing receipt,
rotation and oracle tests. The final rerun using ProviderKind::ALL also passed all 188 tests.
Tests cover strict equality and one unit on either side; zero/missing terms;
both overflow additions; every binding field; missing evidence; all 17 provider
routes; and historical serialized qualification classifications. Synthetic
fixtures prove guard decisions, not current provider behavior.

No qualified provider operations or rotations were exercised; measured minimum
headroom is unavailable. Compaction signals, native continuation interception,
effective launch controls, complete-context counting/error bounds and live
startup validation remain unqualified. Historical probes were not rerun because
a live trigger would not close the missing capacity/boundary proof. No visual
analysis, daemon restart, push, release, plan modification or outcome closure.

Item 1 supplies a conservative contract that item 2 can implement against. It
supplies **no active qualified configuration**. Boss must decide whether to resume
item 2 with fail-closed startup behavior; neither receipt work nor later lifecycle
work may promote a route without new effective-runtime qualification evidence.
