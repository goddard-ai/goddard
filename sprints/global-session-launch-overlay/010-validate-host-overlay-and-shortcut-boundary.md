# 010-validate-host-overlay-and-shortcut-boundary

## Objective

Establish the native host boundary for global shortcut registration, active-display overlay sizing, transparent window show/hide, and webview messaging.

## Scope

- Validate Bun/Electrobun host capabilities for native/global shortcut registration.
- Validate active-display detection and transparent overlay window lifecycle.
- Define a narrow service/module boundary that later tasks can use.
- Add minimal test seams needed for later tasks.

## Acceptance Criteria

- The implementation path is clear for registering/unregistering a global shortcut.
- The implementation path is clear for reporting registration failure.
- The implementation path is clear for showing/hiding a separate transparent overlay window.
- The implementation path is clear for dispatching overlay events without UI importing host APIs.

## Review Report

### Plain-English Summary

This task adds the native host boundary that later sprint tasks will use for the global session launch overlay. It confirms the implementation shape against Electrobun 1.18.1 APIs: global shortcuts return success/failure, the active display can be resolved from the cursor position, transparent `BrowserWindow`s can be created hidden and shown later, and overlay show/hide can be coordinated without importing host APIs into UI code.

The change is intentionally not user-visible yet. It creates a pure coordinator with injectable drivers plus a thin Electrobun adapter, so settings and overlay UI can build on a reviewed contract in later tasks.

### How To Verify Without Reading Code

1. Open the sprint diff for task `010`.
   Expected: the review shows only a host-boundary module and tests, with no settings UI or visible launcher behavior yet.

2. Check that the product plan remains unchanged by this task.
   Expected: task `010` only establishes the implementation path; it does not revise the previously approved UX decisions.

### Agent Verification

- `bun test app/src/bun/global-session-launch-overlay.test.ts` passed.
- Verified from the locally cached Electrobun 1.18.1 package that `GlobalShortcut`, `Screen`, transparent `BrowserWindow`, `hide`, `show`, `setFrame`, and `setAlwaysOnTop` APIs exist.
- Attempted `bun install --frozen-lockfile`; Bun refused because it wanted lockfile changes, so no dependency files were modified.
- Attempted `bunx prettier`; it could not run because the repo's `@prettier/plugin-oxc` dependency is not installed in this worktree. Formatting was kept consistent manually.

### Approval Questions

- Is this host boundary acceptable as the contract for later settings and overlay-window tasks?
- Is the active-display rule acceptable as implemented: use the display containing the cursor, with primary-display fallback?
- Is it acceptable that shortcut registration failure is normalized to an `unavailable` result for later inline settings errors?

### Known Limits

- The overlay is not wired into the app yet; no user can enable or invoke it from this task alone.
- The concrete overlay URL is still supplied by the future app entrypoint task.
- Native visual behavior still needs manual QA once the overlay window has real UI content.

