# Goddard development guidance

## Development runtime

- Assume `bun ./scripts/dev.ts` is already running and owns the current
  `Goddard Debug.app` process. Source changes are rebuilt and signed
  automatically; relaunch the app by typing `a` + enter in the watcher
  terminal (`b` also restarts the daemon). Only run it yourself if not
  already launched.
- During normal development and UI validation, do not run
  `scripts/bundle.sh debug` or start a second watcher. Quitting the app
  leaves the watcher and daemon running; `a` relaunches, `q` or Ctrl-C
  stops everything.
- After an edit, wait for the watcher to finish its successful rebuild,
  relaunch the app via the watcher's `a` command, and validate the fresh
  debug app. Only start or recover the watcher manually when it is
  confirmed unavailable.
- Validate visible changes against the exact provider interaction in that
  rebuilt app — a successful Rust build alone is insufficient.
- No visual test unless requested.

## Topic docs

Read the doc before working in its area:

- [.agents/docs/performance.md](.agents/docs/performance.md) — render paths,
  row builders, streaming, the event pump
- [.agents/docs/accessibility.md](.agents/docs/accessibility.md) — controls,
  focusable surfaces, meaning encoded visually
- [.agents/docs/ui-conventions.md](.agents/docs/ui-conventions.md) — element
  constructors, rounded surfaces, transcript content fidelity
- [.agents/docs/references.md](.agents/docs/references.md) — ambiguous product
  or GPUI decisions: when and how to consult T3 Code and Zed source
- [.agents/docs/changelog.md](.agents/docs/changelog.md) — fragment naming,
  groups, the mobile split

## Performance

- Treat performance as a product requirement, not a follow-up. Goddard is a native
  app competing with web clients, and staying smooth under a long transcript on
  a high-refresh display is the point of being native. Prefer the faster design
  when it costs nothing in clarity, and measure before assuming a cost is fine.
- Never block the UI thread with heavy work. Rendering owns it, so anything a
  frame can reach must already be in memory: no subprocess spawns, no
  filesystem walks, no network, no blocking locks, no synchronous IPC.
- Row builders and measurement paths run for every visible item on every frame.
  Treat I/O reached from `render` as a defect even when it looks cheap, is
  cached after the first hit, or only triggers for some rows — one `git`
  invocation is already several frames of budget.
- Render paths must not call `.update`, `.read`, or `.read_with` on `Waku` or
  any entity that can be leased above them in the same render. Transcript rows
  render under `entity.update` and `WakuPane` content runs inside
  `waku.update`, so a nested update on the same entity aborts the process
  (`double_lease_panic`, no unwind). Helpers that need `Waku` state while
  building elements take `&Waku` or `&mut Context<Waku>` as a parameter; a
  `&WeakEntity<Waku>` parameter signals deferred use only — `on_click`,
  `on_key_down`, `spawn` continuations, menu-item callbacks, and canvas paint
  closures all run outside the lease and may update freely.
- Move the work to `cx.background_executor().spawn`, store the result on the
  entity, and `cx.notify()` when it lands. Render then reads only that store,
  and a miss means "not known yet" and must degrade gracefully.
- Resolve a whole session or collection in one background pass instead of
  probing per item, and guard it with a generation counter so a result from a
  superseded pass cannot overwrite newer state.
- One-shot user actions such as a click or menu command may work synchronously
  when freshness matters more than latency; frames may not.
- Keep per-frame work proportional to what is on screen. Long collections are
  virtualized with `list()`, and a row builder must not rebuild whole-session
  state; hoist that to a cache refreshed once per frame.
- Streaming CPU is governed by two cadences — stream commits at ≤ ~8.3 Hz and
  pulse-clock ticks at ≤ 60 Hz (spinners; other pulses stay at ≤ ~30 Hz) —
  and by what one frame can see. Read
  [.agents/docs/performance.md](.agents/docs/performance.md) before touching the event pump,
  the pulse clock (`src/ui/motion.rs`), veils, overlay scrollbars, pane
  caching, or anything else a streaming frame reaches; it also records the
  counter-based measurement playbook that actually finds regressions.

## Accessibility

- Accessibility is a product requirement too: every mouse-reachable control
  is keyboard-operable with visible focus, reduce-motion is honored, and
  meaning is never carried by color or hover alone. Checklist:
  [.agents/docs/accessibility.md](.agents/docs/accessibility.md).

## Changelog

- Record user-facing changes as `.changelog/<prefix>-<slug>.md` fragments —
  one bullet per file, mobile-only changes under `.changelog/mobile/` —
  and preview the fold with `bun ./scripts/changelog.ts check`. Naming rules
  and group vocabulary:
  [.agents/docs/changelog.md](.agents/docs/changelog.md).

## QA branch workflow

- Proposed work lands on the `qa` branch unreviewed and is promoted to
  `main` in order once approved. History is immutable up to and
  including the newest `qa` commit carrying an approval — approvals bind
  to commit SHAs, so rewriting them orphans the approvals. The
  unreviewed tail past that commit may be rebased or amended.
- Squash-merge feature work so each `qa` commit is one reviewable unit.
- Add one or more `Test-Plan:` trailers to the commit message when a
  human should verify the change. Each trailer is one executable check a
  reviewer can run — e.g. `Test-Plan: send a file to an offline friend;
  the transfer fails with a logged cause`.
- Required for: changes to observable behavior (UI, protocol semantics,
  persisted formats); bug fixes (repro before, verify gone after);
  security-sensitive or destructive paths (auth, credentials, deletion,
  migration); concurrency, timing, and shared-state changes; config or
  build changes that alter what ships.
- Not required for: formatting, lint, and typo/comment/doc-only edits;
  behavior-preserving refactors; test-only changes; changelog fragments;
  build or dependency changes that cannot alter shipped behavior.
- When in doubt, write the test plan — a missing trailer skips human
  review entirely.

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->
