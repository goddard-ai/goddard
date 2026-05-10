# Setup and React

Use this file to bootstrap a Lexical editor, wire it into React, theme it, and persist state without turning the editor into a controlled component.

## Core Rules

- Create one editor surface at a time: vanilla `createEditor`, legacy React `LexicalComposer`, or `LexicalExtensionComposer`.
- Register the nodes a feature needs before the feature runs.
- Treat `initialConfig.editorState` as one-time initialization, not a live prop.
- Save serialized state outward with listeners or `OnChangePlugin`; only call `setEditorState(...)` for explicit rehydration.

## Vanilla Editor

```ts
import {
  $createParagraphNode,
  $createTextNode,
  $getRoot,
  createEditor,
} from 'lexical';

const editor = createEditor({
  namespace: 'MyEditor',
  onError(error) {
    throw error;
  },
  theme: {
    paragraph: 'editor-paragraph',
  },
});

editor.setRootElement(document.getElementById('editor'));

editor.update(() => {
  const root = $getRoot();
  const paragraph = $createParagraphNode();
  paragraph.append($createTextNode('Hello from Lexical'));
  root.append(paragraph);
});
```

Remember that the core `lexical` package does not wire typing, deletion, or rich-text behavior by itself. Add the relevant helpers or register commands manually.

## React Composer

```tsx
import {LexicalComposer} from '@lexical/react/LexicalComposer';
import {RichTextPlugin} from '@lexical/react/LexicalRichTextPlugin';
import {ContentEditable} from '@lexical/react/LexicalContentEditable';
import {HistoryPlugin} from '@lexical/react/LexicalHistoryPlugin';
import {OnChangePlugin} from '@lexical/react/LexicalOnChangePlugin';
import {LexicalErrorBoundary} from '@lexical/react/LexicalErrorBoundary';

const initialConfig = {
  namespace: 'MyEditor',
  theme,
  onError(error: Error) {
    throw error;
  },
};

export function Editor() {
  return (
    <LexicalComposer initialConfig={initialConfig}>
      <RichTextPlugin
        contentEditable={<ContentEditable className="editor-input" />}
        placeholder={<div className="editor-placeholder">Write something</div>}
        ErrorBoundary={LexicalErrorBoundary}
      />
      <HistoryPlugin />
      <OnChangePlugin
        onChange={(editorState) => {
          saveDraft(JSON.stringify(editorState.toJSON()));
        }}
      />
    </LexicalComposer>
  );
}
```

## Load and Save State

Use `initialConfig.editorState` only for first render initialization. Rehydrate an existing editor explicitly:

```ts
const nextState = editor.parseEditorState(serializedState);
editor.setEditorState(nextState);
```

Use that pattern for explicit loads, resets, or server-driven rehydration. Do not feed every keystroke back through `setEditorState(...)`.

## Theme and Mode

- Pass a `theme` object that maps node roles to CSS classes.
- Set read-only mode with `editable: false` in config or later with `editor.setEditable(false)`.
- Use `registerEditableListener(...)` when toolbar state or UI affordances depend on editability.

## React-Specific Failure Modes

- Keep `initialConfig` stable; `LexicalComposer` only uses it when the editor is created.
- Call `useLexicalComposerContext()` only below the matching composer from the same Lexical build.
- Use `EditorRefPlugin` if code outside the composer tree needs the editor instance.
- Make effect-based plugins idempotent in React Strict Mode and return clean disposers.
