# Spec Review Rules

These rules are shared by `spec-calibrate` and `spec-align`.

## Required Alignment

- Always read `.agents/skills/spec-editing/SKILL.md` before calibration or alignment work.
- Apply the `spec-editing` purpose, tree shape, writing values, and keep-out rules as the standard for all recommendations and resulting spec edits.
- Treat `spec-review/` as workflow state, not canonical product behavior.

## Review Artifact Shape

- `spec-review/` mirrors reviewed Markdown files from `spec/`.
- Preserve relative paths. For example, `spec/core/runtime-loop.md` is reviewed in `spec-review/core/runtime-loop.md`.
- `spec-review/__tree-structure-review.md` reviews the shape, navigation, naming, hierarchy, and cross-file ownership of the reviewed spec tree.
- Do not copy spec text into review files except for short phrases needed to identify an issue.

Each mirrored review file should use this shape when the sections add signal:

```markdown
# Review: spec/path.md

## Contract Read

- The durable product behavior this file appears to define.
- The boundaries, guarantees, or exclusions it appears to establish.

## Calibration Findings

- Findings ordered by severity or importance.
- Each finding explains the spec risk and the smallest likely correction.
- Each finding includes a human-accepted resolution checkbox:
  - [ ] Go with this: <specific spec change, split, move, deletion, or clarification to apply>

## Coverage Questions

- Product questions that must be answered before the contract is complete enough to guide implementation.

## Neighbor Checks

- Relevant consistency checks against parent, child, sibling, README, glossary, or ADR context.
```

## Finding Bar

- Focus on missing behavior, spec drift, and high-risk ambiguity.
- Include only findings that could cause future implementation divergence, incorrect product behavior, inconsistent specs, or repeated confusion.
- Do not record typos, local phrasing preferences, minor organization nits, or speculative improvements unless they create meaningful contract risk.
- If a review has no material missing behavior, drift, or risky ambiguity, say that directly instead of filling space.
- When the right resolution is uncertain, still recommend the most likely resolution and use the surrounding text to name the uncertainty.
- Do not create open-ended TODOs without a proposed path.

## Tree Structure Review

`spec-review/__tree-structure-review.md` should check:

- Whether `spec/README.md` accurately orients readers to the tree.
- Whether parent specs, child specs, ADRs, and scoped directories have clear ownership boundaries.
- Whether related capabilities are grouped, split, named, and linked in a way that helps future spec edits land in the right place.
- Whether any files appear misplaced, overloaded, orphaned, duplicated, or hidden behind vague names.
- Each structural finding should use the same `- [ ] Go with this:` recommendation convention.

## Human Feedback Contract

- Humans accept the likely resolution by changing `- [ ] Go with this:` to `- [x] Go with this:`.
- Humans may edit recommendation text directly in `spec-review/`; alignment should treat those edits as the accepted source.
- After calibration commits `spec-review/`, human feedback should be discoverable with `git diff -- spec-review/`.
