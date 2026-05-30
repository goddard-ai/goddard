---
name: spec-align
description: Apply human-reviewed `spec-review/` feedback to align the repository `spec/` tree, then remove `spec-review/` as completed workflow state.
---

# Spec Align

Use this skill when the user says to proceed after reviewing `spec-review/`, or otherwise asks to apply accepted spec review feedback to `spec/`.

## Purpose

Spec alignment consumes human-reviewed `spec-review/` feedback, updates `spec/` to match the accepted decisions, and removes the review workflow state.

## Required Reading

Before doing any work, read:

- `.agents/skills/spec-editing/SKILL.md`
- `.agents/skills/spec-review-rules.md`

## Workflow

1. Read the required files.
2. Inspect `git diff -- spec-review/` to find human edits after the calibration commit.
3. Read the relevant `spec-review/` files, including checked boxes and any human-edited recommendation text.
4. Apply checked `Go with this` recommendations and human-edited feedback to `spec/`. If a review file has no checked boxes but was edited by the human, treat the edits as feedback to interpret conservatively.
5. Use `spec-editing` to keep the resulting spec changes concise, durable, and free of implementation details.
6. Re-read the changed `spec/` files against the accepted feedback.
7. Remove the review workflow state with `git rm -r spec-review`.
8. Commit the aligned `spec/` changes and `spec-review/` removal together.

If accepted feedback is ambiguous or conflicts with `spec-editing`, call out the blocker instead of guessing.
