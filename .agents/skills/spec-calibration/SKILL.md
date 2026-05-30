---
name: spec-calibration
description: Thoroughly review the repository `spec/` tree by creating a mirrored `spec-review/` tree whose files evaluate the corresponding spec files for contract clarity, coverage, boundaries, and fit with neighboring specs.
---

# Spec Calibration

Use this skill when the user asks to calibrate, audit, evaluate, or review the quality of specs under `spec/`, especially before or after using `spec-editing`.

## Purpose

Spec calibration checks whether the spec tree is acting as a coherent set of product capability contracts.

It produces review artifacts. It does not edit `spec/` unless the user separately asks for spec editing.

## Output Shape

- Create or update `spec-review/`.
- Mirror every reviewed `spec/` Markdown file with a corresponding Markdown file under `spec-review/`.
- Preserve relative paths. For example, `spec/core/runtime-loop.md` is reviewed in `spec-review/core/runtime-loop.md`.
- Create or update `spec-review/__tree-structure-review.md` to review the shape, navigation, naming, hierarchy, and cross-file ownership of the reviewed spec tree.
- When reviewing the whole spec tree, include every Markdown file under `spec/`, including ADRs unless the user excludes them.
- If the user scopes the calibration to a subtree or file, mirror only that scope.
- Do not copy spec text into the review files except for short phrases needed to identify an issue.

## Review File Shape

Each mirrored spec review file should be thorough, but keep the structure stable and skimmable:

```markdown
# Review: spec/path.md

## Contract Read

- The durable product behavior this file appears to define.
- The boundaries, guarantees, or exclusions it appears to establish.

## Calibration Findings

- Findings ordered by severity or importance.
- Each finding should explain the spec risk and the smallest likely correction.

## Coverage Questions

- Product questions that must be answered before the contract is complete.

## Neighbor Checks

- Relevant consistency checks against parent, child, sibling, README, glossary, or ADR context.
```

Omit empty sections only when they would add no signal. Prefer specific findings over generic praise.

`spec-review/__tree-structure-review.md` should focus on tree-level concerns:

- Whether `spec/README.md` accurately orients readers to the tree.
- Whether parent specs, child specs, ADRs, and scoped directories have clear ownership boundaries.
- Whether related capabilities are grouped, split, named, and linked in a way that helps future spec edits land in the right place.
- Whether any files appear misplaced, overloaded, orphaned, duplicated, or hidden behind vague names.

## Calibration Principles

- Be deliberately thorough. Read enough parent, child, sibling, README, glossary, and ADR context to judge fit, not just grammar.
- Treat specs as durable product contracts, not plans, implementation notes, roadmaps, tickets, or persuasive briefs.
- Identify unclear capabilities, missing boundaries, conflicting terms, duplicated responsibilities, stale rationale, implementation leakage, and places where the tree shape hides important behavior.
- Separate review from repair. Recommend the smallest corrective edit, split, merge, move, or deletion, but leave actual `spec/` changes to `spec-editing`.
- Bias toward contract strength. Call out weak descriptive prose when the intended requirement, guarantee, or prohibition is not explicit.
- Preserve local vocabulary unless terminology itself is inconsistent or misleading.
- Do not invent product decisions to close gaps. Record the calibration question instead.
- Keep review artifacts useful after the implementation changes. Avoid references to transient code state unless the spec explicitly claims implementation reality.

## Workflow

1. Read `spec/README.md` first to understand the intended tree.
2. List the Markdown files in the requested scope with `rg --files spec`.
3. For each spec file, read nearby context needed to judge that file's role: parent specs, child specs, sibling indexes, local `glossary.md`, and relevant ADRs.
4. Create or update `spec-review/__tree-structure-review.md` for the reviewed scope's structure.
5. Create or update each mirrored review file under `spec-review/`.
6. Re-read the reviews for actionable signal. Remove generic commentary, duplicated summaries, and findings that do not point to a concrete spec risk.
7. If the calibration reveals that the requested scope misses related files, either add those files to the review or call out the omitted scope explicitly.

## Pairing With Spec Editing

- If a request includes both calibration and editing, complete the calibration first and stop unless the user clearly asked to continue into edits.
- When moving from calibration to editing, use `spec-editing` and treat `spec-review/` as review input, not as canonical product behavior.
