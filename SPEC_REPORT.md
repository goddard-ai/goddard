# SPEC Verification Report

## Scope Reviewed
Current uncommitted changes in:
- `spec/README.md`
- `spec/app.md`
- `spec/cli.md`
- `spec/cli/interactive.md`
- `spec/cli/loop.md`
- `spec/core.md`
- `spec/core/architecture.md`
- `spec/core/data-flows.md`
- `spec/daemon.md`

## Standards Check Performed
- Read the active repo agent instructions and spec-guardian rules.
- Confirmed repository version in `package.json` is `0.1.0` (`v0.x`), so direct intent shifts are allowed without a deprecation cycle.
- Reviewed the edited spec files for:
  - tree structure and parent/child clarity
  - anti-bloat / anti-minutiae compliance
  - consistency with the app-first + SDK-first product direction
  - obvious cross-file contradictions introduced by the edits

## What Looks Good
- The edited files are conceptually grouped around one coherent product shift: Goddard is now described as an SDK-first platform with a Tauri desktop workspace as the primary human-facing surface.
- The CLI subtree was simplified into tombstones instead of preserving outdated command mechanics.
- The changes reduce implementation-heavy details in the CLI specs and move intent back toward supported product surfaces.
- Parent/child relationships remain navigable through the existing "Encapsulated Sub-Specs" lists.

## Suspect Items

### 1. Root spec file naming still conflicts with the declared spec-tree contract
**Severity:** High

The repo-level spec-guardian instructions say the root node must be `spec/manifest.md`, but this repository still uses `spec/README.md` as the top-level entry point.

**Why this is suspect:**
- It creates ambiguity about the canonical traversal root.
- It means the current spec tree is not fully aligned with the documented structural contract.
- The current changes continue that pattern rather than resolving it.

**Recommendation:**
Decide whether the real contract is `spec/README.md` or `spec/manifest.md`, then normalize the repo and instructions to one root convention.

### 2. `spec/core/architecture.md` still leans on repository/package paths as identifiers
**Severity:** Medium

The architecture spec names components as directory-like paths such as `backend/`, `github-app/`, `sdk/`, `app/`, and `daemon/`.

**Why this is suspect:**
- The spec-guardian rules explicitly warn against file-path-oriented specification language.
- These names may be durable product modules today, but they are also implementation structure, so the spec is somewhat coupled to repository layout.

**Recommendation:**
Consider reframing those sections around enduring product components (control plane, GitHub integration, SDK, desktop workspace, background runtime) and only use filesystem names when truly necessary for navigation.

### 3. Operability language is only partially harmonized after the CLI removal shift
**Severity:** Low

The updated root and CLI docs clearly remove the CLI as a supported surface, but the broader runtime story still references external process-supervisor concepts elsewhere in the spec tree.

**Why this is suspect:**
- This is not necessarily wrong; background automation can still be supervised externally.
- But the product narrative now centers the desktop app more strongly, so the remaining supervisor-oriented language should be reviewed for consistency.

**Recommendation:**
Do a follow-up consistency pass across the remaining runtime specs to confirm which automation hosts are first-class, which are secondary, and how that hierarchy should be described.

## Commit Readiness Assessment
These changes are still reasonably commit-ready as spec-only documentation updates if they are split atomically:
1. app/core/daemon/root intent realignment
2. CLI removal tombstones
3. verification report

The items above are concerns worth recording, but they do not appear to be accidental contradictions severe enough to block committing the current spec edits.
