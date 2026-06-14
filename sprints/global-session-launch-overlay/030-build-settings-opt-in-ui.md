# 030-build-settings-opt-in-ui

## Objective

Add settings UI that distinguishes the in-app launch shortcut from the global shortcut.

## Scope

- Add a settings section or control for the global launch shortcut.
- Include opt-in toggle, binding display/edit affordance, no-project disabled state, and inline registration error presentation.
- Reuse existing shortcut capture patterns where practical.

## Acceptance Criteria

- Users can see that the global shortcut works outside Goddard.
- Users can opt in only after adding a project.
- `Command+Period` appears as the suggested default.
- Shortcut conflicts show an inline error without silent fallback.

