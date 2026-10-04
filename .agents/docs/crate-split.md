# Crate boundaries for faster iteration

Recommendation: split `waku-core` incrementally, with the largest eventual
payoff coming from caching provider implementations separately from Boss and
daemon orchestration. Keep the `Waku` application coordinator together for now;
extract reusable UI foundations before attempting per-screen crates.

This is a proposal, not an implementation. Source baseline: detached `dev`
commit `cd37afb3`. The stated workload is roughly eight parallel worktrees
sharing mbx artifacts. Benefits below are estimates of compilation isolation,
not measured wall-clock speedups.

## What actually compiles together

`cargo metadata --no-deps --offline --locked --format-version 1` confirmed these
normal workspace dependency edges (arrows mean “depends on”):

```text
waku (UI library + binaries) -> waku-client, waku-protocol
waku-agent                  -> waku-client, waku-protocol
waku-client                 -> waku-share, waku-protocol
waku-daemon (binaries)       -> waku-core, waku-protocol
waku-core                   -> waku-share, waku-protocol
waku-share                  -> waku-protocol
waku-computer-use           -> external dependencies only
```

The desktop already avoids `waku-core`. A Boss or daemon implementation edit
does not directly invalidate the UI library through Cargo dependencies.
Protocol edits affect both sides; `waku-core` also has a test-only dependency
on `waku-client`. Evidence: root `Cargo.toml:60–61`,
`crates/waku-core/Cargo.toml:45`, `crates/waku-core/Cargo.toml:74–75`, and each
workspace manifest's `[dependencies]` section.

Line counts from this checkout include comments, blanks, and tests:

| Compilation surface | Rust lines | Useful interpretation |
| --- | ---: | --- |
| `waku-core/src` | 126,755 | One library unit for 117 source files |
| `driver/` | 29,470 | About 20,780 before trailing test modules; substantial reusable implementation |
| `daemon.rs` | 13,124 | 8,905 before its trailing tests; composition, dispatch, lifecycle, queues |
| `server.rs` | 6,017 | 2,918 before trailing tests; transport and replay |
| `persistence.rs` | 5,467 | 2,821 before trailing tests |
| `boss*.rs` | 4,385 | Service, eval, context, rotation; about 2,722 before trailing tests |
| Root `src/` | 224,199 | One large UI library plus binary source files |
| `src/app/` | 162,412 | 79 files; 73 contain inherent `impl Waku` blocks |
| `waku-protocol/src` | 18,723 | Already a separate, widely depended-on contract |
| `waku-client/src` | 11,024 | Already separates client proxies from backend implementation |

Sizes are a scoping proxy, not compiler-cost measurements. Ordinary builds
omit `cfg(test)` code, and incremental rustc compilation can reuse work within
a dirty crate. Crate extraction improves the granularity of independently
reusable artifacts; it does not turn every edited module into a cache hit.

No build ran for this investigation. This worktree has no `target/`, and
`mbx analyze` reported no recorded build for the project. Graft used the main
checkout's index, which lacked Boss and memory-engine definitions; its leads
were checked against this worktree. `sem impact` on the `WakuBackend` struct
confirmed automation and auto-prompt dependents, but did not enumerate every
field/type edge. The boundary assessment also uses direct source references.

## How a split helps—and when it does not

Moving a small module into a dependency crate leaves the large consumer dirty
when that dependency changes. The important goal is to shrink the consumer
and keep large sibling implementations reusable. For example:

```text
Today: boss.rs edit -> waku-core library -> daemon executable
Target: boss edit -> waku-boss -> small backend/composition -> daemon executable
        unchanged providers, storage, transport, map, memory reuse artifacts
```

Here “leaf implementation” means a feature whose callers are limited to
composition, not a crate with no dependencies. Foundational dependencies have
the opposite property: changing their shared interfaces can invalidate many
consumers. Re-exports preserve imports during migration but do not block
dependency invalidation.

Debug workspace code uses `opt-level = 1`; `.cargo/config.toml:34` overrides
debug information to `line-tables-only`, and `jobs = 4` limits concurrent Cargo
work. The root `[profile.dev.package."*"]` optimization applies to non-workspace
dependencies, so new workspace crates retain the workspace development profile.
More crates may allow parallel work, but dependency chains and four jobs bound
that benefit. Release thin LTO remains a separate cost.
[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) explain
these overrides and incremental reuse.

## Proposed boundaries and expected benefit

The governing heuristic is rate-of-change, not size. Crate boundaries separate
hot from cold in both directions: hot coherent domains (Boss today, memory
engine while under construction) get their own crate so their churn recompiles
little else; large cold code (provider adapters, storage) gets its own crate so
everyone else's churn never recompiles it. Both cases are the same
optimization. When a hot domain stabilizes, its dedicated crate stops earning
its boundary; conversely a dormant mega-module is always worth isolating.


Names below are provisional. Keep contracts small and dependencies directed
toward foundations; avoid a generic “common” crate that eventually contains
all implementations again.

| Boundary | Contents and dependency direction | Estimated benefit / effort |
| --- | --- | --- |
| `waku-memory-engine` | `memory_engine.rs`; protocol eval types, serde, hashing, filesystem | Very cheap. 773 total / 612 pre-test lines stop participating in unrelated core changes. Small immediate saving; a good extraction pilot. |
| `waku-repo-map` | `repo_map.rs`; tree-sitter grammars and `ignore`; evaluation stays in daemon | Cheap. 1,161 / 954 lines and parser calls become independently cacheable. Grammar dependencies are already crates and already cacheable; do not count their full build time as new savings. |
| `waku-boss` | Boss service + eval + rotation; context after replacing storage input with a snapshot | Medium effort, high relevance to the stated Boss edit workload. Roughly 4.4k lines; must remove concrete backend and notifier dependencies first. Separate Rhai eval only if timings justify another crate. |
| `waku-storage` | Persistence, blobs, attachments, migration, generated SQL migrations | Medium effort. Isolates database/schema work and keeps storage reusable during Boss/provider edits. Persistence alone is only 2.8k pre-test lines; bundle related storage ownership rather than making every helper a crate. |
| `waku-driver-api` + initially `waku-providers` | Small event/control/launch contract; provider adapters, session readers, catalog probes and shared harness helpers in implementation crate | Highest likely backend payoff. `driver/` alone is 20.8k pre-test lines, plus session/catalog implementation. Medium-to-high effort because current launch types and catalog helpers point in both directions. |
| Later provider-family crates | Codex, Claude, ACP family, Pi family, other local/cloud adapters; a small factory assembles them | Potentially large savings for provider-specific edits. Delay until one provider crate has a clean API and timings identify expensive/hot families. Keep shared ACP providers together. |
| `waku-server` + small runtime contract | Transport, request pools, replay; `Backend`, sinks, session-stream handles and neutral result types in contract | Medium-to-high effort. 2.9k pre-test transport lines can be reused on daemon edits, but share/scheduler/stats callbacks currently obstruct extraction. |
| `waku-runtime` (or remaining `waku-core`) | `WakuBackend`, lifecycle, orchestration, routing adapters, command dispatch and composition | Last phase. Shrink it by extracting implementations; dividing `daemon.rs` into files alone changes no crate-level cache boundary. |
| `waku-ui-foundation` | GPUI primitives, input, theme, fonts, markdown and selection machinery, with platform hooks supplied from app | Useful second track for UI iterations: app edits can reuse substantial generic/layout/render implementation. Moderate effort; these modules have real internal cycles. |

The two initial leaf extractions remove only about 1.5% of core's total source
(about 1.8% of its 88.8k lines before trailing test modules). They demonstrate
the workflow; they do not solve the main bottleneck. Extracting providers is
the first step likely to materially reduce the amount of unrelated backend
code behind each Boss/daemon edit. No percentage here predicts elapsed time.

### The Boss boundary

`BossService` holds `Weak<WakuBackend>` and `share::TaskNotifier`
(`boss.rs:59–69`). Recovery and settlement call
`WakuBackend::finish_boss_employee` from worker threads (`boss.rs:491–539`).
`WakuBackend` owns the Boss service (`daemon.rs:524`). This is a real cycle.

Replace the reverse edge with a narrowly scoped finish callback or host trait:
input `session_id: Uuid`, output `anyhow::Result<()>`. Install it with weak
backend ownership so it does not create an Arc cycle; keep shutdown off the
driver event-forwarding thread. Move `TaskNotifier` to a neutral runtime
contract, or inject a Boss-owned notification callback.

`boss_eval::run` already accepts a `Fn(BossOperation) -> Result<BossResult>`
dispatcher (`boss_eval.rs:98–102`), making it a clean internal boundary. Keep
authorization and task lifecycle in backend dispatch initially. `operation_lock`
and `BossService::update` are currently `pub(crate)` (`boss.rs:63,1169`): replace
cross-boundary access with explicit operations, not exposed mutable state.

`boss_context::work_context` takes `PersistedState` (`boss_context.rs:69–73`).
Accept project/session snapshots or borrowed slices instead, assembled under
the backend's existing lock. `memory_engine.rs:6–16` uses external/protocol
types, unlike the much more coupled `memory.rs:29–32`. Keep the distillation
orchestrator in runtime initially. `boss_rotation.rs:37–45` reads protocol
settings and has no observed production caller at this revision; put it with
Boss policy, not in its own crate just because it has no callers.

### Providers and platform support

`driver/mod.rs:237` defines a useful control trait, but its start options
embed `agent::AgentLaunchEnv`, computer-use runtime, MCP specs and `ShuruVm`
(`driver/mod.rs:308–365`). The provider factory lives alongside adapters
(`driver/mod.rs:392–438`). `integrations/service.rs:25` imports an MCP spec
from driver implementation; provider code also calls integrations.

Move launch DTOs, event sender/control interfaces and MCP configuration into
the driver contract. Keep token minting/queues in runtime. Move the agent
launch environment DTO out of agent state: `command_env.rs:360` currently
requires it, coupling a broadly used subprocess utility to orchestration.
Platform subprocess/sandbox helpers should form a coherent lower layer;
use prepared launch resources or stable handles where possible. Do not move
secrets or daemon-only launch options into the client wire protocol.

Core `model.rs:5` re-exports protocol models but adds executable/catalog
discovery (`model.rs:9–64`). Shared types should import protocol directly;
probe implementations belong with providers. Keep discovery and session
readers with adapters first: splitting each of them immediately would trade a
large knot for many tiny interconnected crates.

### Storage, share, transport and the daemon knot

Storage depends on blob/attachment helpers and checkpoint capture deadlines
(`persistence.rs:28–37,897,1698,1769,1782,2157`). Move storage helpers together;
pass a deadline/budget explicitly or extract its small shared contract rather
than making storage depend on Git checkpoint orchestration. Preserve deletion
guards, timeouts, migrations and transaction behavior. SQL embedding currently
belongs to `crates/waku-core/build.rs:12–23`; relocate it with storage.

`Backend` already gives server an abstraction, but it references pairing,
share and automation-owned sink types (`server.rs:336–423`). Share returns
`server::SessionStream` (`share.rs:224–227`); stats calls server's pool snapshot
(`server.rs:123`). Move only neutral runtime interfaces, stream handles and
callback aliases down to a contract crate. Keep server/backend integration
tests in a separate harness: server's `WakuBackend` references occur in its
test section, not its production implementation.

The P2P transport is already `waku-share`. Core `share.rs` is daemon
integration, coupled to Git review/sync (`share.rs:1250,1989`); a second sharing
crate is not a cheap win. Likewise, `usage.rs` calls command environment,
sandbox and Git commit logic (`usage.rs:542,549,2040`). First establish those
lower layers, then consider usage/history as one service if change frequency
warrants it. Keep Git panel/review/sync/worktree together initially rather
than splitting their mutual calls arbitrarily.

Automations and auto-prompts hold concrete backend dependencies
(`automations.rs:35–37`, `auto_prompts.rs:19–36`). Keep them with runtime until
their scheduling/dispatch host interfaces are small. The 8.9k production
daemon lines include shared lifecycle and queue invariants; a handler crate
per command would need pervasive access to backend internals. Extract services
with bounded inputs/results rather than exporting the entire backend state.

## Is an app split worth it?

Yes for reusable foundations; not yet for `src/app/` screen crates. Its size
is substantial, but most files extend one foreign-to-be type: `Waku` owns
daemon/client state (`app.rs:2224–2252`), settings is an `impl Waku`
(`app/settings.rs:837`), and rendering includes both inherent methods and
`Render for Waku` (`app/render.rs:17,502`). Moving those inherent impl blocks
to another crate cannot preserve that structure: they must stay with the
type or become separate entities/functions with explicit interfaces.

Moving the entire app module into one crate merely relocates roughly 162k
lines into another dirty unit for every screen edit. Splitting coordinator
state into a universally depended-on crate also makes state changes fan out.
Independent panels are a later architecture project, warranted only when a
panel can own its state and exchange bounded commands/snapshots with `Waku`.

A foundation crate can amortize unchanged GPUI-heavy code during app edits.
However, `input.rs:6–8` depends on markdown highlighting and UI menus, UI
shortcuts/text fields depend on input, and markdown rendering depends on
input/theme/UI. Extract that cluster together first. Remove platform coupling
from theme activation (`theme.rs:1590–1613`); supply appearance/accessibility
hooks from the host. A smaller pure markdown parser/highlighter crate is also
possible, but excludes most GPUI rendering cost and needs measurements before
adding another permanent unit.

Preserve existing render leases and off-thread work boundaries from
[performance guidance](performance.md). A crate split is not a reason to add
synchronous calls or shared-state reads to rendering.

## Phase order and integration requirements

1. Establish a timing baseline during the next already-needed build. Extract
   memory engine and repo map; verify re-exports and their existing tests.
2. Break Boss's backend callback and storage-snapshot edges; extract Boss as
   one crate. Keep Rhai eval and dormant rotation policy inside it initially.
3. Establish driver contracts and platform launch support; extract providers
   as one implementation crate. This is the strongest likely cache win for
   Boss/daemon work. Split individual families only with measured justification.
4. Extract coherent storage; then neutral runtime interfaces and server. Leave
   share/Git/usage groups together until their lower dependencies are clean.
5. Keep backend composition small; extract more daemon services only when
   doing so does not require exposing its lock-protected internals.
6. Run the UI foundation extraction as a separate measured effort; defer
   screen/state decomposition.

Every new backend crate needs watcher routing: `scripts/dev.ts:1525–1535`
recognizes only `waku-core`, `waku-daemon` and `waku-agent` as daemon-only
changes. Otherwise it takes the app lane, which builds daemon first and then
app (`scripts/dev.ts:757–803`). Update classification and migration/locale
watch ownership with each extraction. This requirement is part of the split,
not a separate optional cleanup.

Keep checkout identity out of extracted reusable libraries. Root
`build.rs:52–69` emits commit SHA/dirty state and watches all root sources;
`crates/waku-daemon/build.rs:9–31` emits daemon identity. Retain build identity in
small executable/composition units and pass it inward where needed. Copying
those build scripts into new libraries would make otherwise identical
worktree code vary with commit identity. Core's locale macros
(`crates/waku-core/src/lib.rs:9–43`) also need an explicit shared localization
strategy rather than embedding the whole catalog in every new crate.

## Measurement and disk cost

For each representative edit—Boss service, daemon dispatch, one provider,
one app screen—record warm same-worktree latency and reuse across another
worktree with identical unaffected sources. Compare the pre/post dirty units,
per-unit time, critical path, and final linking/bundling time. Capture
`--timings` on the normal watcher build rather than launching a second build;
read `mbx analyze` and `mbx explain --last` afterward. Keep toolchain, profile,
target, features and flags identical. Timing reports explain waiting and
codegen costs: [Cargo timings](https://doc.rust-lang.org/cargo/reference/timings.html).

Acceptance: Boss/daemon edits reuse provider, storage and server artifacts;
UI screen edits reuse UI foundation artifacts; the median end-to-end iteration
improves without a material regression for shared-contract edits. Measure
several edits before expanding the crate count. Keep the same protocol and
persisted formats; use existing provider/lifecycle/storage integration tests
to catch boundary mistakes.

mbx deduplication makes additional crates less concerning across eight
worktrees, but does not make them free. Each distinct source/dependency/profile
variant still needs metadata, rlibs and possibly incremental state; worktree
outputs and executable links also consume space. Smaller hot variants may
reduce duplicated churn, while additional units increase fixed overhead.
Do not estimate bytes from source lines. Compare mbx storage statistics and
managed target/incremental footprints over the same workload and retention
window. Start with a handful of coherent crates, not one per module.
