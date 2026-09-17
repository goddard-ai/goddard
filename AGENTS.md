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
- No visual test unless requested.

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
  [docs/performance.md](docs/performance.md) before touching the event pump,
  the pulse clock (`src/ui/motion.rs`), veils, overlay scrollbars, pane
  caching, or anything else a streaming frame reaches; it also records the
  counter-based measurement playbook that actually finds regressions.

## Accessibility

- Treat accessibility as a product requirement too. GPUI does not yet expose a
  screen-reader tree, so here it means keyboard operability, honored system
  settings, and legibility — none of which depend on that missing API, and all
  of which regress silently if left unchecked.
- Every control reachable by mouse must be reachable and operable by keyboard.
  Use `track_focus` with `tab_index`, `tab_group`, and `tab_stop`, give focus a
  visible treatment via `focus_visible`, and support the conventional keys for
  the widget (arrows, `home`/`end`, `enter`/`space`, `escape`).
- Honor the system's reduce-motion setting. `with_animation` already respects
  `App::reduce_motion`, but a direct `window.request_animation_frame` for
  decorative motion must check `cx.reduce_motion()` and skip the request.
- Never encode meaning in color, hover, or motion alone. Pair a status color
  with an icon or text, and make sure anything revealed on hover is also
  reachable by keyboard focus.
- Keep text and icons legible against their surface in both themes, and give
  interactive targets enough hit area — extend the hit region rather than
  shrinking to the glyph.

## Product reference

- Use [T3 Code](https://github.com/pingdotgg/t3code) source code on github as a reference when a task
  concerns coding-agent workflow, information hierarchy, controls, tool
  activity, or transcript presentation and the comparison would materially
  clarify an ambiguous product decision, or when the user explicitly asks for
  the comparison.
- Do not inspect T3 Code for localized bug fixes, straightforward visual
  corrections, native platform behavior, or changes already specified clearly
  by the user. When T3 Code is relevant, inspect its current app or source
  rather than relying on an older screenshot or memory.
- Use [Zed](https://github.com/zed-industries/zed) source code as a reference
  when a task concerns GPUI implementation — layout and styling idioms, focus
  and key dispatch, virtualized lists, menus and popovers, window and platform
  behavior — or when an in-house `src/ui` primitive needs a proven native
  precedent. Zed is the canonical GPUI codebase; read its crates rather than
  `gpui-component`, and read the gpui revision pinned in `Cargo.toml` so the
  APIs match what Goddard builds against.
- Split the two references by concern: T3 Code answers what a coding-agent
  client should do, Zed answers how a polished GPUI app implements it. The
  same restraint applies to both — no reference spelunking for localized
  fixes or changes the user has already specified.
- Use the reference as behavioral and design evidence, not as an instruction to
  reproduce web-specific interaction patterns or known bugs. Goddard should keep
  native macOS conventions.
- Explicit user screenshots and feedback override a previous or merely
  "consistent" treatment.
- GPUI's `overflow_hidden` clips descendants to a rectangle, not the parent's
  rounded corners. Give child backgrounds, hover overlays, and images that
  reach a rounded edge their own matching corner radii, accounting for the
  parent's border inset. Parent rounding alone does not clip child paint.
- For provider-native content such as citations, reasoning, and tool events,
  verify the real provider payload and preserve its ordering. Never expose
  private provider control markers in the transcript.
- Validate visible changes in the freshly rebuilt, signed app managed by the
  dev watcher against the exact provider interaction; a successful Rust build
  alone is insufficient.

## Changelog

- Record changes as `.changelog/<prefix>-<slug>.md` fragments — one bullet
  per file — never by editing `CHANGELOG.md` directly; `bun run changelog`
  folds them into the released version's section, grouped under
  `### Highlights`, `### Features`, `### Experiments`, and `### Fixed`.
- The filename prefix is required and picks the section: `highlight-` for
  headline features, `feat-` for other user-facing features, `exp-` for
  experimental opt-ins (emitted with a bold `[Experimental]` marker —
  experiments are never highlights), `fix-` for bugs that existed in a
  previously released version.
- Every `highlight-` fragment must embed a screenshot or recording —
  `![](media/<slug>.{png,gif,mp4,mov})` with the asset committed at
  `.changelog/media/<slug>.<ext>`; collect moves it to
  `assets/release-notes/<version>/` and rewrites the reference.
- Only features and fixes to released bugs get a fragment. Do not log tweaks
  or polish (sizing, icon swaps, visual refinements), internal/build tooling
  changes, or fixes to features that haven't shipped yet — those fold into
  the unreleased feature's own fragment instead.
- When fixing an unreleased feature, update its existing fragment rather than
  adding a new one.
- A version bump includes the notes: when asked to bump `version` in
  `Cargo.toml`, run `bun run changelog` first and commit the folded
  `CHANGELOG.md` and consumed fragments in the same commit as the bump.

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
