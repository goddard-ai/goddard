# Crate boundaries for faster iteration

Recommendation: pursue backend and desktop decomposition together, starting
with localization isolation and the hottest app domains rather than using
small backend leaves as a gate. Extract Boss, provider families, storage and
transport; give settings, composer, task/sidebar presentation and right-panel
views independent ownership. Keep protocol contracts narrow. Cover the shared
TypeScript client, mobile and web with package/module boundaries, not Cargo
crates.

This is a proposal, not an implementation. Code baseline: `cd37afb3`; the
corrected churn analysis uses analysis HEAD `c4ba1514` (documentation-only
commits after that baseline). The stated workload is roughly eight parallel
worktrees sharing mbx artifacts. Compilation isolation below is inferred from
dependency boundaries; no wall-clock speedups have been measured.

## Current-tree churn, not deleted history

The last 500 commits ending at `c4ba1514` run from `595f20de` (September 23,
2026) through October 4. Count each path at most once per commit, retaining
only paths present in that HEAD's tree. These are touch counts, not changed
lines, independent edits, or compiler timings. Areas overlap and generated TS
files can inflate touches; merge commits follow ordinary `git log` behavior.

| Current area | Commits touching area | Surviving file touches | Implication |
| --- | ---: | ---: | --- |
| `src/app/` | 312 | 743 | Primary native UI target; do not defer all screens |
| `src/app.rs` | 84 | 84 | Hot coordinator; shrink ownership and composition |
| `crates/waku-core/src/` | 155 | 467 | Backend services and provider isolation |
| `locales/` | 107 | 280 | Data edits currently invalidate Rust consumers |
| `packages/waku-client/` | 65 | 174 | TypeScript package, including generated protocol; no direct Cargo cost |
| `crates/waku-client/` | 63 | 68 | Separate Rust client; persistence proxy dominates (50 touches) |
| `crates/waku-protocol/` | 72 | 116 | Shared dependency with broad invalidation |
| `apps/web/` | 26 | 51 | Existing TS application; isolate domain modules and build checks |
| `apps/mobile/` | 20 | 38 | Existing Expo application; separate shared logic from native UI |

Within app subfiles, sessions has 69 touches; settings and sidebar 58 each;
composer 53; right panel 45; runtime 40; Boss UI 28; transcript view 26.
Within core, daemon has 79; Boss 30; server 23; persistence 18; ACP driver 17;
checkpoint 16; Codex driver 15. Protocol model has 26, protocol dispatch types
21, and Boss contracts 19. These justify domain coverage, while compilation
measurements decide the eventual granularity.

There are **zero current `vendor/` paths**. Historical
`vendor/gpui-component` churn is excluded: deleted code is not a present
compilation target. Reproduce the file counts from this checkout with:

```python
import collections
import subprocess

revision = "c4ba1514"
current = set(subprocess.check_output(
    ["git", "ls-tree", "-r", "--name-only", revision], text=True
).splitlines())
history = subprocess.check_output(
    ["git", "log", "-500", "--format=COMMIT:%H", "--name-only", revision],
    text=True,
)
touches = collections.Counter()
for commit in history.split("COMMIT:")[1:]:
    touches.update(set(commit.splitlines()[1:]) & current)
print(touches.most_common(30))
```

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

The governing heuristic is rate-of-change plus dependency fan-out. Separate
hot coherent domains so an edit leaves unrelated siblings reusable; isolate
large stable implementations so other domains do not keep compiling them.
A stable crate still earns its boundary when other hot consumers reuse it.
Source size alone does not establish value, and a hot foundational contract
can remain expensive after extraction because its consumers are invalidated.

Names below are provisional. “Target” describes a proposed boundary, not an
existing crate. Every current hot area has an owner in this plan.

| Target boundary | Contents and dependency direction | Expected isolation / effort |
| --- | --- | --- |
| `waku-localization` | Catalog embedding and translation registry; below UI/backend, separate from wire DTOs | Early priority: removes catalog data from protocol and stops repeated embedding. Must also remove root/core locale watches; consumers still rebuild on locale edits. |
| `waku-ui-foundation` | GPUI controls, input, theme, fonts, markdown/selection; host supplies platform hooks | Reuses stable local GPUI-heavy code during app edits; does not save recompiling external GPUI, which is already a dependency. Moderate effort due to cycles. |
| `waku-settings-ui` | Settings view and editors own settings/search state; host supplies snapshots and applies typed edits | Covers 58 settings touches. Moderate-to-high effort: replace inherent `impl Waku`, preserve focus and navigation. |
| `waku-composer-ui` | Composer input/drafts, completion presentation and submission commands | Covers 53 composer touches. Own input/draft UI state; coordinator retains daemon routing and submit lifecycle. |
| `waku-task-ui` + `waku-transcript-ui` | Task/session/sidebar presentation and transcript navigation/rendering; share narrow task snapshots | Covers sessions/sidebar/transcript churn (69/58/26). Related list surfaces can move together; transcript keeps its own bounded view state. High effort because selection and streaming currently live on `Waku`. |
| `waku-right-panel-ui` | File tree/preview, review presentation and tabs | Covers 45 touches. Own panel/tab state; filesystem work stays on workers and host dispatch applies task actions. |
| Small desktop host | `src/app.rs`, app runtime/render wiring, Boss UI integration and daemon supervisor ownership | Covers coordinator/runtime churn (84/40). Extract presentation/services first; retain shared lifecycle invariants. Boss view can follow the same panel contract, rather than another full `Waku` dependency. |
| `waku-boss` | Boss service/eval/rotation/context over snapshots and completion callback | Covers 30 Boss touches. Remove concrete backend/notifier and persisted-state edges. Keep Rhai inside initially. |
| `waku-driver-api` + provider-family crates | Control/event/launch contracts; Codex, ACP family, Pi family and remaining adapters; factory in composition | Current ACP/Codex churn supports family isolation now. Establish one clean family boundary, then apply it to others; do not require a long-lived mega-provider crate first. |
| `waku-storage` | Persistence, blobs, attachments, migrations and SQL embedding | Covers persistence churn and caches storage during Boss/provider edits; preserve transaction and deletion behavior. |
| `waku-server` + runtime contract | Transport/replay/request pools; neutral sinks and session streams below implementations | Covers 23 server touches; remove share/stats callback ownership cycles. |
| `waku-workspace-services` / runtime services | Checkpoint/Git review/sync/worktree; bounded agent lifecycle and scheduling interfaces | Covers checkpoint/daemon/runtime churn. Keep mutually dependent Git services together; extract lifecycle services without exposing the backend mutex/state wholesale. |
| `waku-memory-engine`, `waku-repo-map` | Existing mostly independent implementations | Cheap opportunistic extractions; about 1.5% of core total lines together. Useful alongside larger tracks, not prerequisites for them. |
| Narrow protocol domain contracts | Stable wire primitives plus model, Boss, workspace and settings DTO families; wire envelope remains composition | Covers 72 protocol-touching commits. Move unrelated consumers to direct domain imports; keeping everyone on an umbrella re-export preserves fan-out. High effort: command/event enums inherently depend on many DTOs. |
| Rust client domain boundaries | Persistence/task proxies versus connection/supervision and shared transport | Covers Rust-client persistence hotspot. First separate implementation/modules; a crate helps only when clients can avoid unrelated domains and cross-calls are removed. |
| TS client subpaths + mobile/web feature modules | Existing `@waku/client` exports; generated wire types separate from reducer/presentation/autocomplete; app-specific state/screens remain local | Covers TS/mobile/web churn without Rust crates. Keep direct subpath imports, targeted tests/typechecks and codegen routing; package splitting is warranted only for demonstrated JS build/test isolation. |

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

Yes: foundations and the hot app domains belong in the plan. Extraction
requires ownership changes because most files extend one foreign-to-be type:
`Waku` owns daemon/client state (`app.rs:2224–2252`), settings is an `impl Waku`
(`app/settings.rs:837`), and rendering includes both inherent methods and
`Render for Waku` (`app/render.rs:17,502`). Moving those inherent impl blocks
to another crate cannot preserve that structure: they must stay with the
type or become separate entities/functions with explicit interfaces.

Moving the entire app module into one crate merely relocates roughly 162k
lines into another dirty unit for every screen edit. Splitting coordinator
state into a universally depended-on crate also makes state changes fan out.
Make independent panel ownership part of the split itself. Settings, composer,
task/sidebar presentation, transcript and right panel should own local view
state and exchange bounded commands/snapshots with `Waku`; start with settings
or right panel, then migrate the more coupled task/composer surfaces. The
coordinator owns daemon routing and cross-panel lifecycle. Avoid handing each
crate `&mut Waku` or a universal shared app-state type: that recreates the knot.

A concrete proposed settings interface is `SettingsSnapshot -> SettingsView`
and `SettingsEdit -> host dispatch`; a task list takes immutable row snapshots
and emits select/archive/open commands. Put each domain's input/output types
beside that domain or in a small contract, not one global app API. Rendering
can return GPUI elements or use child entities; use callbacks/events for host
operations, preserving weak ownership and current UI-thread rules. These are
proposed interfaces, not signatures present at HEAD. Existing settings is an
inherent impl (`settings.rs:837`), as are sidebar, sessions and composer
(`sidebar.rs:1214`, `sessions.rs:332`, `composer.rs:472`).

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

1. Capture representative timings during the next already-needed build and
   preserve the current-tree churn baseline. Start localization ownership and
   watcher/build-identity cleanup; these affect both backend and UI tracks.
2. Run the desktop track immediately: extract UI foundations and a settings or
   right-panel view with explicit ownership. Continue through composer,
   task/sidebar and transcript, shrinking `Waku` and runtime composition.
   Preserve Boss UI behavior within the task/panel contracts.
3. Run the backend track alongside it: extract Boss; establish launch/driver
   contracts and isolate ACP/Codex and other provider families. Memory engine
   and repo map are cheap additions during this work, not gates.
4. Extract storage and transport/runtime contracts; then checkpoint/Git and
   bounded lifecycle/scheduling services. Keep daemon composition small and
   keep ownership of queues, locks and teardown explicit.
5. Narrow protocol dependencies by domain and move translation data out of
   wire contracts. Audit Rust-client persistence versus transport/supervision.
   Migrate actual consumers off umbrella imports so the split earns reuse.
6. Cover the TS client, mobile and web in their existing package boundaries:
   separate generated types from handwritten domains, use direct subpaths,
   isolate feature state/components and scope checks/codegen by affected
   package. Introduce additional packages only where they improve measured
   JS iteration, independently of Cargo outcomes.

Phases 2 and 3 are independent tracks, not a requirement to finish all UI
before backend. Timings refine crate granularity and ordering within each
track; they do not leave current hot areas out of scope.

Every new backend crate needs watcher routing: `scripts/dev.ts:1525–1535`
recognizes only `waku-core`, `waku-daemon` and `waku-agent` as daemon-only
changes. Otherwise it takes the app lane, which builds daemon first and then
app (`scripts/dev.ts:757–803`). Update classification and migration/locale
watch ownership with each extraction. This requirement is part of the split,
not a separate optional cleanup.

### Localization is a current rebuild source

Confirmed: root `build.rs:65–69` recursively watches `src`, `locales` and
`assets` while exporting Git build identity. The watcher includes locales
(`scripts/dev.ts:126–133`), classifies their changes as app work
(`scripts/dev.ts:1523`), and the app lane builds daemon before app. Independently,
core's build script watches the shared locale directory
(`crates/waku-core/build.rs:19–23`). Root `src/lib.rs:3–12` and protocol
`crates/waku-protocol/src/lib.rs:13–22` embed catalog data and explicitly
`include_str!` all three YAML files; core also embeds it (`lib.rs:9`).
Thus locale-only edits are Rust inputs, and the protocol dependency propagates
invalidation beyond the UI even if root source has not changed. This is
confirmed dependency evidence, not a measured duration.

Move embedding to one localization crate. Keep `WireTranslation` key/argument
DTOs in protocol; make catalog translation a caller concern, avoiding a
protocol-to-catalog dependency. Update parameterized `tr!`/`localized!` paths,
locale activation and fallback behavior together: moving only the macro or
adding another re-export leaves embedding/fan-out intact. A compile-time
catalog crate still invalidates its consumers; runtime-loaded catalogs would
avoid that compilation but add packaging/loading requirements and are a
separate decision. Do not promise locale-free rebuilds from a crate alone.

Move Git identity into small executable/host units as well. Root build identity
currently applies to the whole `waku` package, including its library;
`crates/waku-daemon/build.rs` exports daemon identity. Remove broad locale
watches from units that no longer own catalog data, while retaining watches
that actually describe their inputs. Do not copy identity scripts into reusable
libraries: code identical across worktrees should not differ by checkout SHA.

### GPUI comes from a Git dependency, not vendor code

Root `Cargo.toml:32–47` declares `gpui`, `gpui_platform` and `reqwest_client`
from `https://github.com/goddard-ai/zed`, branch `main`. `Cargo.lock:3514–3516`
pins GPUI to `f4512933a12601f54b8cd880d9df50b080dc0bc2`;
`Cargo.lock:3712–3714` pins the platform crate to the same revision. There is
no root `gpui-component` dependency or current vendor tree. The actual local
coupling is `src/ui`, `src/input`, `src/md`, `src/theme` and app rendering;
`input.rs:6–20` directly imports markdown highlighting, UI menus/scrollbars,
GPUI and theme. The foundation crate must break or move that cluster together.

External GPUI already has its own reusable artifact. Splitting our UI isolates
our GPUI-dependent implementation, not GPUI's existing compilation. Share one
Git revision/features policy across new manifests. Keep platform initialization
and native integrations in the host; preserve `runtime_shaders` and platform
feature configuration (`Cargo.toml:38–45`). Keep `test-support` test-only
(`Cargo.toml:83–91`): enabling it on shipped dependencies changes frame behavior.
A GPUI revision/feature change can still invalidate all GPUI consumers.

## Measurement and disk cost

For each representative edit—Boss service, daemon dispatch, one provider,
one app screen, locale data, protocol types, TS client, mobile and web—record
warm same-worktree latency and reuse across another worktree with identical
unaffected sources. Compare the pre/post dirty units,
per-unit time, critical path, and final linking/bundling time. Capture
`--timings` on the normal watcher build rather than launching a second build;
read `mbx analyze` and `mbx explain --last` afterward. Keep toolchain, profile,
target, features and flags identical. Timing reports explain waiting and
codegen costs: [Cargo timings](https://doc.rust-lang.org/cargo/reference/timings.html).
For TS/mobile/web, time their existing package check/test and normal development
refresh separately; Cargo timings do not measure JavaScript work.

Acceptance: Boss/daemon edits reuse provider, storage and server artifacts;
UI domain edits reuse foundations and unrelated panels; locale edits no longer
invalidate wire-only protocol consumers; TS/mobile/web checks remain scoped to
affected packages. The median end-to-end iteration improves without a material regression for shared-contract edits. Measure
several edits at each domain boundary before choosing finer subdivisions. Keep
the same protocol and persisted formats; use existing provider/lifecycle/storage integration tests
to catch boundary mistakes.

mbx deduplication makes additional crates less concerning across eight
worktrees, but does not make them free. Each distinct source/dependency/profile
variant still needs metadata, rlibs and possibly incremental state; worktree
outputs and executable links also consume space. Smaller hot variants may
reduce duplicated churn, while additional units increase fixed overhead.
Do not estimate bytes from source lines. Compare mbx storage statistics and
managed target/incremental footprints over the same workload and retention
window. Use coherent domain crates; cover the hot areas without making one
crate per helper or generated file.
