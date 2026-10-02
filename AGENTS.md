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

## Build cache (mbx)

Rust builds on this machine go through mbx (Mr Boxington), which shares
compiled artifacts across checkouts and worktrees into
`~/dev/Library/Caches/mbx`.

- Prefer `mbx <cargo command>` (`mbx build`, `mbx check`, `mbx clippy`,
  `mbx test`) — it works regardless of PATH. Plain `cargo` routes through
  mbx only when `~/Library/Application Support/mbx/bin` precedes rustup on
  PATH (`command -v cargo` to check); the dev watcher may run unwrapped
  depending on where it was launched.
- `target/` may be a symlink into the managed cache. Don't delete it or
  `cargo clean` to reclaim disk — use `mbx gc` or `mbx clean`. Never put
  `cargo clean` in a watch loop.
- `check`/`clippy` get their own `target/check/` lane and can run beside a
  `cargo build` watcher without waiting on its lock.
- Restored artifacts can carry another checkout's absolute paths in debug
  info. For path-sensitive debugging:
  `CARGO_TARGET_DIR=target/dbg MBX_DISABLE=1 cargo build`.
- `mbx explain --last` shows why compilations hit, missed, or bypassed.

## Topic docs

Read the doc before working in its area:

- [.agents/docs/testing.md](.agents/docs/testing.md) — choosing, maintaining,
  and verifying tests
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

## Documentation audiences

- Write `README.md`, `WIKI.md`, and `docs/` for human readers. This also
  applies recursively to local documentation linked from those pages:
  explain the reader's task, prerequisites, steps, and observable result.
- Keep implementation notes, agent instructions, investigations, and internal
  proposals in `.agents/docs/`. Preserve useful technical detail there rather
  than mixing it into user guides.
- Contributor and release guides may explain technical procedures for human
  maintainers. Keep their links within human-facing documentation and verify
  relative paths and heading anchors after moving pages.

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

For code questions, unfamiliar areas, or change scoping, use Graft before broad
text searches or source reads. Choose the command that fits the task:

- `graft map` gives a token-budgeted repo orientation with directory clusters,
  hubs, and hotspots. Start here when the relevant area is not yet clear.
- `graft ask "<terms or specific question>" --source` is an optional first-pass
  lookup when you have concrete symbols, paths, or distinctive terms. Its
  ranked matches can be noisy, so inspect the returned source spans rather than
  treating the top hit as an answer. It shows an eight-line crux by default;
  add `--full` for whole definitions and `--in <scope>/` to narrow a monorepo.
- `graft callers <symbol>` gives exact caller edges. Add `--direction out` for
  callees or `--depth N` (or `all`) to walk relationships transitively.
- `graft grep "<literal or regex>"` finds every match in indexed files,
  grouped by enclosing symbol. Use it for exhaustive text searches; fall back
  to `rg` only for files Graft does not index.
- `graft skeleton <file>` lists every definition's signature and span for a
  quick API-surface overview.
- `graft blast` shows what depends on changed lines; use it to scope the
  impact of a diff.
- Browse `graft/INDEX.md` when you want to follow the linked system notes
  directly.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0); use `graft check` to verify freshness.
<!-- graft:end -->
