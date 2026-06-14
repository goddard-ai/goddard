# 090-verify-overlay-flow-end-to-end

## Objective

Add focused tests and manual QA coverage for the complete global overlay flow.

## Scope

- Add unit tests for state, defaults, and registration error handling where possible.
- Add host boundary tests where feasible.
- Document manual QA for native window behavior that automated tests cannot reliably cover.

## Acceptance Criteria

- Existing in-app launch dialog still works.
- Global shortcut opt-in flow works.
- Overlay show/hide, draft preservation, repeated launches, async toasts, retry/edit, and conflict errors are verified.

