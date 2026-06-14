# 020-add-global-launch-shortcut-state

## Objective

Add persistent app state for the opt-in global launch shortcut setting.

## Scope

- Add data shape for enabled/disabled state.
- Store the chosen global binding and suggested default `Command+Period`.
- Represent native registration status and registration errors.
- Gate enabling on at least one project being available.

## Acceptance Criteria

- The shortcut is disabled by default.
- The shortcut cannot be enabled without at least one project.
- The opt-in state and binding persist across restarts.
- Registration conflicts can be represented as inline settings errors.

## Review Report

### Plain-English Summary

This task adds the app state that will drive the global session launch shortcut settings UI and later native registration. The new state starts disabled, suggests `Command+Period`, refuses to enable until at least one project exists, and can represent native registration as unregistered, registered, or unavailable with an inline error message.

The state is persisted through the existing app-state snapshot, but it stays separate from the current editable in-app shortcut keymap. That separation keeps the global shortcut opt-in explicit and avoids making the existing `Launch session` shortcut global by accident.

### How To Verify Without Reading Code

1. Review the state behavior for a new install.
   Expected: the global launch shortcut is disabled and has `Command+Period` ready as the suggested binding.

2. Review the no-project behavior.
   Expected: enabling fails when the app has zero projects, which gives the future settings UI a clean way to keep opt-in unavailable until a project exists.

3. Review conflict behavior.
   Expected: a registration conflict disables the global shortcut state and preserves an error message for inline settings display.

### Agent Verification

- With temporary ignored `node_modules` symlinks to the main checkout, `bun test app/src/global-session-launch/global-shortcut.test.ts app/src/bun/app-state-store.test.ts app/src/bun/global-session-launch-overlay.test.ts` passed.
- `bun test app/src/navigation.test.ts app/src/bun/app-state-store.test.ts` could not be used as a clean verification command because direct `navigation.test.ts` execution hit an existing TSRX loader/export issue unrelated to this task.
- `bun run typecheck` could not be used as a clean verification command in this worktree because the TSRX compiler reported existing parse errors across multiple `.tsrx` files before reaching this task's types.
- Temporary dependency symlinks were removed after verification.

### Approval Questions

- Is this state shape acceptable for the settings UI in task `030`?
- Is it acceptable that registration conflicts set the global shortcut back to disabled while retaining the inline error?
- Is keeping this state separate from the existing in-app shortcut keymap the right product boundary?

### Known Limits

- This task does not add settings UI.
- This task does not register the native global shortcut.
- The no-project gate is represented in state, but users will not see the disabled opt-in copy until task `030`.

