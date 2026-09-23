# Changelog

## Write for the reader

Write for a developer who uses Goddard but knows nothing about the
implementation or the work that led to the change. Assume they are competent
and busy, but missing context. Each entry should help them understand what
changed for them.

- Start with the action they can take or the problem they will stop encountering.
- Name the relevant screen, control, or situation so they can recognize it.
- Explain unfamiliar concepts briefly when they are necessary.
- Include defaults, opt-in requirements, and limitations when they affect use.
- Keep implementation details only when they help the reader make a decision.
- Describe the finished behavior. Omit development history, internal identifiers,
  and explanations of how the code broke.
- Prefer one short sentence. Add a second when the reader needs context.

For example, replace “Fixed a crash caused by registering a file link's context
menu while the transcript was locked for rendering” with “Fixed a crash when a
task's activity included a clickable file name.”

Before submitting, ask: **Could someone who never saw the task understand this
entry, recognize when it matters, and know where to try it?** If not, rewrite it.
Check that simpler wording still describes the actual behavior without adding
unsupported promises.

## Fragment and release rules

- Record changes as `.changelog/<prefix>-<slug>.md` fragments — one bullet
  per file — never by editing `CHANGELOG.md` directly; `bun run changelog`
  folds them into the released version's section, grouped under
  `### Highlights`, `### Features`, `### Experiments`, and `### Fixed`.
  A change only a mobile-app user would notice uses `.changelog/mobile/`
  instead — same naming rules — and folds into `CHANGELOG.mobile.md`;
  `CHANGELOG.md` feeds the desktop updater prompt, so mobile-only notes
  must not land in it. The split keys on the affected surface, not the
  touched code — a daemon fix only mobile clients hit belongs to mobile.
- The filename prefix is required and picks the section: `highlight-` for
  headline features, `feat-` for other user-facing features, `exp-` for
  experimental opt-ins (emitted with a bold `[Experimental]` marker —
  experiments are never highlights), `fix-` for bugs that existed in a
  previously released version.
- A second filename segment tags the change's topic group —
  `.changelog/<prefix>-<group>-<slug>.md` — and collect nests grouped
  bullets under a `- **Group**` parent inside their `###` section, in a
  fixed product-surface-first order. The vocabulary: `sessions`,
  `sidebar`, `composer`, `providers`, `git`, `transcript`, `panels`,
  `terminals`, `keyboard`, `navigation`, `appearance`, `permissions`,
  `settings`, `friends`, `ssh`, `platform`. Pick the group the change is
  about (`git` covers worktrees and the Git panel, `providers` covers the
  model picker, `ssh` covers remote daemon pairing); when none fits, omit
  the segment rather than stretching one. A group tagged by only one
  fragment in a release folds back into the flat tail, so tagging is
  always safe. `highlight-` fragments are never grouped — everything
  after the prefix is their media slug.
- Preview how fragments will group with `bun ./scripts/changelog.ts check`
  before folding; an unrecognized group token lands the bullet in the
  flat tail, so check is how a mistyped group gets caught. Before picking
  a tag, skim the pending fragment names — a group needs two entries, so
  a fragment tagged differently from the sibling it belongs with (e.g.
  remote-pairing work filed outside `ssh`) strands both in the flat tail.
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
