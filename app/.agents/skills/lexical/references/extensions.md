# Extensions

Use this file when the codebase uses `defineExtension`, `buildEditorFromExtensions`, or `LexicalExtensionComposer`, or when the task is to migrate from legacy composer and plugin wiring.

## Default Stance

- Treat Lexical Extensions as a distinct integration style, not a small variation on legacy React plugins.
- Keep extension objects stable. Define them at module scope unless there is a strong reason not to.
- Put reusable feature wiring in dependencies instead of scattering config and registration in separate files.
- Preserve legacy React plugins when the task is a minimal migration; move them into extensions only when that reduces duplication or confusion.

Lexical documents this API as experimental, with the extension model introduced in v0.36.1.

## Root Extension

```ts
export const appExtension = defineExtension({
  name: '@example/editor',
  namespace: 'example-editor',
  nodes: () => [QuoteNode, HeadingNode],
  theme,
  $initialEditorState,
  dependencies: [RichTextExtension],
});
```

Use the root extension to set one-time editor properties such as `namespace`, `theme`, `onError`, `editable`, and `$initialEditorState`.

## React Migration

Before:

```tsx
<LexicalComposer initialConfig={initialConfig}>
  <RichTextPlugin
    contentEditable={<ContentEditable />}
    ErrorBoundary={LexicalErrorBoundary}
  />
</LexicalComposer>
```

After:

```tsx
<LexicalExtensionComposer extension={appExtension}>
  <RichTextPlugin
    contentEditable={<ContentEditable />}
    ErrorBoundary={LexicalErrorBoundary}
  />
</LexicalExtensionComposer>
```

For a minimal migration, keep the legacy React plugins as children first and move them into extension dependencies later.

## React Decorator Extensions

Package a legacy React plugin as an extension decorator when it only needs to render a component or run an effect:

```ts
export const LogEditorExtension = defineExtension({
  name: '@example/log-editor',
  dependencies: [
    configExtension(ReactExtension, {
      decorators: [<LogEditorPlugin />],
    }),
  ],
});
```

If the caller must choose where the UI renders, prefer an output component instead of an automatic decorator.

## Dependency Rules

- Use `dependencies` for direct, required extension references.
- Use `peerDependencies` for optional, name-based relationships.
- Use `conflictsWith` only when two extensions truly cannot coexist, such as plain-text and rich-text modes.

## Phase Boundaries

- Put default configuration in `config`.
- Use `build` to expose runtime output, often from `namedSignals(config)`.
- Put command, listener, and registration side effects in `register`.
- Use `afterRegistration` for work that must wait until all registrations are done, such as root-element setup that should happen after initial state application.
