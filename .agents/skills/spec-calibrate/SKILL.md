---
name: spec-calibrate
description: Create and commit mirrored `spec-review/` artifacts for `spec/`, focusing on missing behavior, spec drift, and high-risk ambiguity with checkbox-backed likely resolutions for human review.
---

# Spec Calibrate

Use this skill when the user asks to calibrate, audit, evaluate, or review specs under `spec/`.

## Purpose

Spec calibration produces review artifacts for human decision-making. It does not edit `spec/`.

## Required Reading

Before doing any work, read:

- `.agents/skills/spec-editing/SKILL.md`
- `.agents/skills/spec-review-rules.md`

## Workflow

1. Read the required files.
2. Read `spec/README.md` first to understand the intended tree.
3. List the Markdown files in the requested scope with `rg --files spec`.
4. For each spec file, read nearby context needed to judge that file's role: parent specs, child specs, sibling indexes, local `glossary.md`, and relevant ADRs.
5. Create or update `spec-review/__tree-structure-review.md` for the reviewed scope's structure.
6. Create or update each mirrored review file under `spec-review/`.
7. Re-read the reviews against `.agents/skills/spec-review-rules.md`. Remove generic commentary, duplicated summaries, low-impact issues, and findings that do not point to missing behavior, spec drift, high-risk ambiguity, or another concrete spec risk.
8. If the calibration reveals that the requested scope misses related files, either add those files to the review or call out the omitted scope explicitly.
9. Commit only the generated `spec-review/` changes so later human feedback is easy to find with `git diff -- spec-review/`.

Stop after committing `spec-review/` unless the user explicitly changes the task.
