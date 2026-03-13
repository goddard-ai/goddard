# AI Agent Instructions

## Feature Implementation Requirements
When adding a feature that grants the user a new ability, it must be added to all of these packages in the same PR:
- `sdk`
- `app`

## `spec/` Folder Purpose and Guidelines
The `spec/` folder contains the canonical source of truth and theory of mind for the project.
- You must **never** edit the spec (unless explicitly requested by the user).
- You must strictly adhere to its values and not diverge from them.
- If you sense a suggested change will stray from the spec, you must warn the user that a spec change might be needed.
