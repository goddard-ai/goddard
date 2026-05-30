---
name: implementation-drift-audit
description: Audit source changes against `spec/` contracts, preferring a human-provided baseline commit and otherwise diffing source roots since the last commit that touched `spec/`.
---

# Implementation Drift Audit

Use this skill when the user asks whether implementation work has drifted from `spec/`, whether recent code changes require spec updates, or whether code still matches the canonical product contracts.

## Purpose

Implementation drift audits compare source changes against current spec contracts.

They produce review findings. They do not edit `spec/`, source code, or `spec-review/` unless the user explicitly asks for follow-up changes.

## Required Reading

Before doing any work, read:

- `.agents/skills/spec-editing/SKILL.md`
- `spec/README.md`

## Baseline Commit

Prefer an explicit human-provided baseline commit when one exists.

If no baseline is provided, find the most recent commit that touched `spec/`:

```sh
git log -1 --format=%H -- spec
```

Treat the chosen commit as an audit assumption. Name it in the final report.

## Workflow

1. Resolve the baseline commit from the user's request or `git log -1 --format=%H -- spec`.
2. Identify source roots present in the repository, such as `app/`, `core/`, `features/`, `packages/`, `daemon/`, or other implementation directories.
3. Inspect implementation changes since the baseline with `git diff <baseline>..HEAD -- <src-roots>`.
4. Use the diff to choose the narrowest relevant specs to read. Do not read broad spec areas unless the changed behavior crosses boundaries.
5. Report only material drift risks:
   - implementation adds user-facing or shared behavior not described by spec
   - implementation removes, weakens, or bypasses behavior still required by spec
   - implementation changes ownership, defaults, permissions, lifecycle, persistence, system configuration, SDK behavior, or other product contracts without matching spec movement
   - source changes reveal that an existing spec is ambiguous enough to permit incompatible implementations
6. Do not report mechanical refactors, formatting, tests, internal renames, file movement, or implementation details unless they change product behavior.
7. If a source change is hard to classify from the diff alone, read the smallest surrounding source context needed to decide whether product behavior changed.
8. Stop after the audit report unless the user explicitly asks for spec or code edits.

## Report Shape

Keep the report concise and finding-first:

```markdown
# Implementation Drift Audit

Baseline: <commit> (<reason it was chosen>)

Source diff reviewed:
- <root or path>

## Findings

### P1/P2/P3: <short title>
Changed source:
- <path>

Relevant spec:
- <path>

Drift risk:
<behavioral mismatch or missing contract>

Likely resolution:
- [ ] Update spec to describe ...
- [ ] Or change implementation to preserve ...

## No Material Drift

- <changed area>: <why no product-contract drift was found>
```

Omit empty sections. If no material drift is found, say so directly and list the source areas reviewed.
