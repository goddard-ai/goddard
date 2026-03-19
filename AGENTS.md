# AI Agent Instructions

## Pre-Alpha State

This repository is currently in an unreleased, pre-alpha state. Because of this, backwards compatibility is not a concern. You can freely introduce breaking changes without needing to add legacy fallback logic or deprecation notices.

## Core/SDK and App Feature Parity

When adding a feature that grants the user a new ability or capability within the Tauri `app`, it is strictly required that this same capability is also fully supported and implemented in the `core/sdk` package.

Both packages must be updated within the same Pull Request (PR) to ensure feature parity and prevent discrepancies between the application and the underlying SDK.

## `spec/` Folder Purpose and Guidelines

The `spec/` folder contains the canonical source of truth and theory of mind for the project.

- You must **never** edit the spec (unless explicitly requested by the user).
- You must strictly adhere to its values and not diverge from them.
- If you sense a suggested change will stray from the spec, you must warn the user that a spec change might be needed.

## Code Style Guidelines

- **Avoid Single-Use Variables:** Do not declare variables that are only referenced once unless they significantly improve readability for complex expressions. Inline them directly into their usage.
- **Explain Types and Interfaces:** Every TypeScript type alias or interface must include a `//` comment explaining its purpose. This reduces type bloat and helps future maintainers understand types that are often declared far from their usage.
- **Describe Module Exports:** Every exported module declaration must include a human-readable `/** ... */` description comment. Do not add JSDoc tag boilerplate like `@param` or `@returns`.

## Git/Rebase Note

- When continuing a rebase in non-interactive terminals, use `GIT_EDITOR=true git rebase --continue` to avoid hanging in an interactive editor.

## Commit Message Format

All commits must follow the Conventional Commits standard.

- Required format: `<type>(optional-scope): <description>`
- Common types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `build`, `perf`, `revert`
- Use `!` after type/scope or include a `BREAKING CHANGE:` footer when introducing breaking changes
- Keep the subject concise and imperative (e.g., `fix(daemon): pass GODDARD_AGENT_BIN_DIR to session server`)
- Prefer atomic commits with a single clear purpose.
- Split policy/docs changes from code or test behavior changes unless they are inseparable.
- Every commit must include a full description in the commit body covering each meaningful change made by that commit
- The body should use clear bullet points and explain what changed and why (not just restate the subject)

## Testing the App Package

We do not want automated tests for our `app/` package, so do not write any.

## Test Suite Expectations

The test suite is maintained by AI agents and should stay lean.

- Do not preserve a test just because it already exists.
- If a test does not protect a meaningful contract, user-visible behavior, regression boundary, or shared mock drift check, remove it.
- Prefer contract-level tests over implementation-detail tests.
- Avoid assertions against informal constants or incidental content fragments. Exact checks for real contract strings, such as error messages or stable literals, are acceptable.
- When replacing a shallow or brittle test, remove the old test instead of keeping both unless both protect distinct behavior.

## `build:types` Script Guidance

Do not add a `build:types` script to packages that are both private and only export source `*.ts` files.
