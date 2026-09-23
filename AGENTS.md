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
- [.agents/docs/jev.md](.agents/docs/jev.md) — the eval model's plumbing,
  call sites, thresholds, and the spend-gating rules

## Performance

- Performance is a product requirement: nothing a frame can reach may do I/O
  or block — no subprocesses, filesystem walks, network, or synchronous IPC
  from render paths — and per-frame work stays proportional to what is on
  screen. The rules and streaming cadences live in
  [.agents/docs/performance.md](.agents/docs/performance.md).
- Render paths must not call `.update`, `.read`, or `.read_with` on an entity
  already leased by their caller; transcript rows render under a `Waku` lease.

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

## Jev (eval model)

- Jev is TypeSafe's structured decision model: one call posts a shared
  `state` plus typed questions (`Noul`/`Choice`/`Score`) and returns
  calibrated probabilities — never generated text. Jev decides, code
  applies; full rules and call sites in
  [.agents/docs/jev.md](.agents/docs/jev.md).
- Every call goes through the session's daemon (`Command::Evaluate`), runs
  off the UI thread, degrades to the deterministic default on any failure,
  and is logged to `eval-decisions.jsonl` under a `feature` tag.

## QA branch workflow

- `dev` is the QA branch — there is no `qa` branch. Proposed work lands
  on `dev` unreviewed and is promoted to `main` in order once approved.
  History is immutable up to and including the newest `dev` commit
  carrying an approval — approvals are git notes under `refs/notes/qa`
  and bind to commit SHAs, so rewriting them orphans the approvals. The
  unreviewed tail past that commit may be rebased or amended.
- `dev` stays checked out in its own worktree — land by rebasing onto
  `origin/dev` and fast-forwarding `dev` there rather than checking it
  out here.
- Squash-merge feature work so each `dev` commit is one reviewable unit.
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
