---
name: lexical
description: Work on Lexical editors in JavaScript or React, including `lexical`, `@lexical/react`, custom nodes, commands, transforms, listeners, selections, serialization, NodeState, and extensions.
---

# Lexical

Use this skill to make Lexical changes without re-deriving its editor-state model, plugin lifecycle, or node rules.

## Default Stance

- Treat `EditorState` as the source of truth, not the DOM.
- Read or mutate nodes only inside `editor.update(...)`, `editorState.read(...)`, or command and listener callbacks that already run in lexical context.
- Prefer node transforms over scheduling `editor.update(...)` from an update listener.
- Prefer `NodeState` for serializable metadata; subclass or replace nodes only when behavior, DOM output, or serialization truly changes.
- Keep React integrations uncontrolled. Serialize snapshots outward instead of continuously piping state back into the editor.
- Keep extension definitions stable at module scope when using the Extensions API.

## Start Here

- Identify the active Lexical surface first: `createEditor`, `LexicalComposer`, or `LexicalExtensionComposer`.
- Identify the task shape before editing: setup, plugin and command work, node modeling, persistence, or extension migration.
- Read exactly one focused reference first, then expand only if the task crosses boundaries:
  - [setup-and-react.md](./references/setup-and-react.md): bootstrap editors, theme them, mount React composers, save and load state.
  - [plugins-and-commands.md](./references/plugins-and-commands.md): register commands, listeners, transforms, selections, DOM events, and update tags.
  - [nodes-and-state.md](./references/nodes-and-state.md): choose between `NodeState`, custom nodes, and node replacement.
  - [serialization-and-html.md](./references/serialization-and-html.md): persist JSON, customize import and export, and round-trip HTML safely.
  - [extensions.md](./references/extensions.md): use `defineExtension`, migrate from legacy composers, and package React features as extensions.

## Workflow

1. Classify the request.
   - Bootstrapping an editor or composer: start with setup.
   - Formatting or keyboard behavior: start with plugins and commands.
   - New content type or custom rendering: start with nodes and state.
   - Save and load, import and export, or clipboard fidelity: start with serialization.
   - `defineExtension`, `LexicalExtensionComposer`, or migration work: start with extensions.
2. Establish the configuration boundary.
   - Check registered nodes, theme, namespace, error handling, and editor mode.
   - Check whether required packages or plugins are already wired in.
   - Check whether the editor is plain text, rich text, headless, collaborative, or extension-based.
3. Apply Lexical invariants before changing code.
   - Keep `$` helpers inside lexical context.
   - Avoid nesting `editor.update(...)` inside `editor.read(...)`.
   - Return cleanup functions from listeners, commands, and plugins.
   - Do not assume `initialConfig.editorState` will react to later prop changes.
4. Implement the smallest correct fix.
   - Register nodes before inserting or deserializing them.
   - Use transforms for normalization and content rewriting.
   - Use update tags when history, collaboration, focus, or scroll behavior should differ.
   - Prefer `mergeRegister(...)` when combining multiple disposers.
5. Validate the actual failure mode.
   - Confirm node registration and serializer coverage.
   - Confirm React plugins mount under the correct composer and clean up on unmount.
   - Confirm selection and focus behavior when updates or commands run.
   - Confirm persistence by round-tripping through `toJSON()`, `parseEditorState(...)`, or the relevant HTML conversion.

## Output Rules

- Produce code changes, not Lexical commentary alone.
- Preserve the existing integration style unless the task explicitly asks for migration.
- Keep examples in TypeScript or TSX unless the surrounding code is plain JavaScript.
- Re-check for duplicate Lexical builds or misplaced composer context when React behavior looks impossible.
