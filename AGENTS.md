# AI Agent Instructions

## Feature Implementation Requirements
When adding a feature that grants the user a new ability, it must be added to all of these packages in the same PR:
- `sdk`
- `cmd`
- `app`

## `spec/` Folder Purpose and Guidelines
The `spec/` folder contains the canonical source of truth and theory of mind for the project.
- You must **never** edit the spec (unless explicitly requested by the user).
- You must strictly adhere to its values and not diverge from them.
- If you sense a suggested change will stray from the spec, you must warn the user that a spec change might be needed.

## Commit Message Format
All commits must follow the Conventional Commits standard.
- Required format: `<type>(optional-scope): <description>`
- Common types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `build`, `perf`, `revert`
- Use `!` after type/scope or include a `BREAKING CHANGE:` footer when introducing breaking changes
- Keep the subject concise and imperative (e.g., `fix(daemon): pass GODDARD_AGENT_BIN_DIR to session server`)
- Every commit must include a full description in the commit body covering each meaningful change made by that commit
- The body should use clear bullet points and explain what changed and why (not just restate the subject)
