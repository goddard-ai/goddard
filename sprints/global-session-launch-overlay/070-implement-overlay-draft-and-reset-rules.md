# 070-implement-overlay-draft-and-reset-rules

## Objective

Implement overlay-specific draft preservation and repeated-launch reset behavior.

## Scope

- Preserve overlay draft in memory only.
- Apply hide-preserve semantics to outside click, Escape, close button, and repeat shortcut.
- Avoid draft persistence across app restart.
- Reset after submit while keeping selected project for rapid successive launches.

## Acceptance Criteria

- Reopening the overlay restores unsent drafts during the same app run.
- Dismiss paths never discard the draft.
- App restart clears overlay draft.
- Submit resets the prompt and transient fields while preserving repeated-launch project context.

