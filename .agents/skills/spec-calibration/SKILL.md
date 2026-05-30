---
name: spec-calibration
description: Thoroughly review and later align the repository `spec/` tree by using committed mirrored `spec-review/` files focused on missing behavior, spec drift, and high-risk ambiguity.
---

# Spec Calibration

Use this skill when the user asks to calibrate, audit, evaluate, review, or proceed with feedback for specs under `spec/`.

## Purpose

Spec calibration checks whether the spec tree is acting as a coherent set of product capability contracts. It is deliberately opinionated about spec quality and always aligns itself with `spec-editing`.

It has two phases:

- Calibration: produce and commit `spec-review/` artifacts for human review.
- Proceed: after the human reviews `spec-review/` and says "proceed", align `spec/` to the accepted feedback and remove `spec-review/`.

Do not edit `spec/` during calibration. During proceed, edit `spec/` using the accepted `spec-review/` feedback and the `spec-editing` values.

## Required Alignment

- Always read `.agents/skills/spec-editing/SKILL.md` before calibration or proceed work.
- Apply the `spec-editing` purpose, tree shape, writing values, and keep-out rules as the standard for all recommendations and resulting spec edits.
- Treat `spec-review/` as review workflow state, not canonical product behavior.

## Output Shape

- Create or update `spec-review/`.
- Mirror every reviewed `spec/` Markdown file with a corresponding Markdown file under `spec-review/`.
- Preserve relative paths. For example, `spec/core/runtime-loop.md` is reviewed in `spec-review/core/runtime-loop.md`.
- Create or update `spec-review/__tree-structure-review.md` to review the shape, navigation, naming, hierarchy, and cross-file ownership of the reviewed spec tree.
- When reviewing the whole spec tree, include every Markdown file under `spec/`, including ADRs unless the user excludes them.
- If the user scopes the calibration to a subtree or file, mirror only that scope.
- Do not copy spec text into the review files except for short phrases needed to identify an issue.
- After creating or updating `spec-review/` during calibration, commit the `spec-review/` changes before ending the turn unless the user explicitly says not to or a safe scoped commit is not possible.

## Review File Shape

Each mirrored spec review file should be thorough, but high-signal. Focus on missing behavior, spec drift, and high-risk ambiguity rather than trivial wording, formatting, or preference issues.

```markdown
# Review: spec/path.md

## Contract Read

- The durable product behavior this file appears to define.
- The boundaries, guarantees, or exclusions it appears to establish.

## Calibration Findings

- Findings ordered by severity or importance.
- Each finding should explain the spec risk and the smallest likely correction.
- Include only findings that could cause future implementation divergence, incorrect product behavior, inconsistent specs, or repeated confusion.
- Always include the most likely resolution with a checkbox the human can accept:
  `- [ ] Go with this: <specific spec change, split, move, deletion, or clarification to apply>`

## Coverage Questions

- Product questions that must be answered before the contract is complete enough to guide implementation.

## Neighbor Checks

- Relevant consistency checks against parent, child, sibling, README, glossary, or ADR context.
```

Omit empty sections only when they would add no signal. Prefer specific findings over generic praise. If a review has no material missing behavior, drift, or risky ambiguity, say that directly instead of filling space.

When the right resolution is uncertain, still recommend the most likely resolution and use the surrounding text to name the uncertainty. Do not create open-ended TODOs without a proposed path.

`spec-review/__tree-structure-review.md` should focus on tree-level concerns:

- Whether `spec/README.md` accurately orients readers to the tree.
- Whether parent specs, child specs, ADRs, and scoped directories have clear ownership boundaries.
- Whether related capabilities are grouped, split, named, and linked in a way that helps future spec edits land in the right place.
- Whether any files appear misplaced, overloaded, orphaned, duplicated, or hidden behind vague names.
- Use the same `- [ ] Go with this:` recommendation convention for each structural finding.

## Calibration Principles

- Be deliberately thorough. Read enough parent, child, sibling, README, glossary, and ADR context to judge fit, not just grammar.
- Apply a high finding bar. Do not record typos, local phrasing preferences, minor organization nits, or speculative improvements unless they create meaningful contract risk.
- Treat specs as durable product contracts, not plans, implementation notes, roadmaps, tickets, or persuasive briefs.
- Identify missing behavior, spec drift, and high-risk ambiguity, including unclear capabilities, missing boundaries, conflicting terms, duplicated responsibilities, stale rationale, implementation leakage, and places where the tree shape hides important behavior.
- Separate review from repair. Recommend the smallest corrective edit, split, merge, move, or deletion, but leave actual `spec/` changes to `spec-editing`.
- Bias toward contract strength. Call out weak descriptive prose when the intended requirement, guarantee, or prohibition is not explicit.
- Preserve local vocabulary unless terminology itself is inconsistent or misleading.
- Do not invent product decisions to close gaps. Record the calibration question instead.
- Keep review artifacts useful after the implementation changes. Avoid references to transient code state unless the spec explicitly claims implementation reality.

## Workflow

### Calibration

1. Read `.agents/skills/spec-editing/SKILL.md`.
2. Read `spec/README.md` first to understand the intended tree.
3. List the Markdown files in the requested scope with `rg --files spec`.
4. For each spec file, read nearby context needed to judge that file's role: parent specs, child specs, sibling indexes, local `glossary.md`, and relevant ADRs.
5. Create or update `spec-review/__tree-structure-review.md` for the reviewed scope's structure.
6. Create or update each mirrored review file under `spec-review/`.
7. Re-read the reviews for actionable signal. Remove generic commentary, duplicated summaries, low-impact issues, and findings that do not point to missing behavior, spec drift, high-risk ambiguity, or another concrete spec risk.
8. If the calibration reveals that the requested scope misses related files, either add those files to the review or call out the omitted scope explicitly.
9. Commit only the generated `spec-review/` changes so later human feedback is easy to find with `git diff -- spec-review/`.

### Proceed

1. Read `.agents/skills/spec-editing/SKILL.md`.
2. Inspect `git diff -- spec-review/` to find human edits after the calibration commit.
3. Read the relevant `spec-review/` files, including checked boxes and any human-edited recommendation text.
4. Apply checked `Go with this` recommendations and human-edited feedback to `spec/`. If a review file has no checked boxes but was edited by the human, treat the edits as feedback to interpret conservatively.
5. Use `spec-editing` to keep the resulting spec changes concise, durable, and free of implementation details.
6. Re-read the changed `spec/` files against the accepted feedback.
7. Remove the review workflow state with `git rm -r spec-review`.
8. Commit the aligned `spec/` changes and `spec-review/` removal together.

## Pairing With Spec Editing

- If a request includes calibration and editing without a human-reviewed `spec-review/` pass, complete calibration first and stop after committing the review artifacts.
- When the user says "proceed" after reviewing `spec-review/`, use `spec-editing` and the accepted review feedback to align `spec/`.
