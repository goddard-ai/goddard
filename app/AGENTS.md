# AI Agent Guidelines

## Repository Context

This is a **Tauri 2.0** desktop application using a **Frontend-Heavy** architecture.

- **Framework**: Preact + TypeScript
- **Styling**: CSS Modules
- **Backend**: No custom Rust code. Use Tauri plugins for OS interaction.

## Development Rules

1.  **Source of Truth**: Always refer to `@spec.md` for behavior and `@plan.md` for implementation details.
2.  **No Rust**: Do not modify Rust code unless absolutely necessary for configuration. Logic should reside in TypeScript.
3.  **Tauri Plugins**: Use the official plugins (`@tauri-apps/plugin-*`) as specified in the plan.
    - HTTP: `plugin-http` (for ClickUp and Jules APIs)
    - Store: `plugin-store` (for persistence)
    - Shell: `plugin-shell` (for Git commands)
    - Dialog: `plugin-dialog` (for file selection)
4.  **Verification**: After creating files or modifying code, verify the changes using `read_file` or `list_files`.
5.  **State Management**:
    - Use Preact Signals to manage UI state, but ensure data is persisted to the Tauri Store.
    - **Global State Mutations**: All global state mutations must be encapsulated in **exported functions** (async or sync) within the declaring module (e.g., `src/state.ts`).
    - **Semantic Actions**: Avoid simple setters (e.g., `setActiveSession`). Use semantic action names (e.g., `startActiveSession`, `saveSettings`, `clearActiveSession`).
    - **No Direct Mutation**: Never mutate `.value` directly in components.
6.  **Component Design**:
    - **Inline Handlers**: Prefer inlining event handlers if they are only called in one place.
7.  **Formatting**: Always run `npm run format` after modifying files to maintain consistent code style.
## File Structure

- `src/`: Frontend Preact code
- `src/services/`: TypeScript services for integrations (ClickUp, Git, Jules, Store)
- `src-tauri/`: Tauri configuration (Rust host)

## Pre-requisites

- Ensure `Node.js` and `Rust` (cargo) environments are understood (though you primarily write TS).
- When running shell commands, use the repository root.
