# Plugins and Commands

Use this file to add keyboard behavior, toolbar actions, node normalization, DOM event wiring, or selection-aware features.

## Pick the Right Mechanism

- Use `registerCommand(...)` for key handling, toolbar actions, and feature-level intents.
- Use `registerNodeTransform(...)` for content normalization that should happen in the same update.
- Use `registerUpdateListener(...)` to observe committed state, not to schedule another routine update.
- Use `registerMutationListener(...)` to react to node lifecycle changes.
- Use `registerRootListener(...)` to bind native DOM events to the contenteditable root.

## Custom Command

```ts
import {
  $insertNodes,
  COMMAND_PRIORITY_EDITOR,
  type LexicalCommand,
  createCommand,
} from 'lexical';

export const INSERT_BADGE_COMMAND: LexicalCommand<string> = createCommand();

export function registerBadgeCommand(editor: LexicalEditor): () => void {
  return editor.registerCommand(
    INSERT_BADGE_COMMAND,
    (label) => {
      $insertNodes([$createBadgeNode(label)]);
      return true;
    },
    COMMAND_PRIORITY_EDITOR,
  );
}
```

Command callbacks already run inside lexical context. Use `$` helpers directly there instead of nesting `editor.update(...)`.

## Transform Instead of Waterfall Updates

```ts
import {TextNode} from 'lexical';

function textNodeTransform(node: TextNode): void {
  if (!node.isSimpleText() || node.hasFormat('code')) {
    return;
  }

  const text = node.getTextContent();
  const match = findEmoji(text);
  if (match == null) {
    return;
  }

  const [, targetNode] = node.splitText(
    match.position,
    match.position + match.shortcode.length,
  );

  targetNode.replace($createEmojiNode(match.unifiedID));
}

export function registerEmoji(editor: LexicalEditor): () => void {
  return editor.registerNodeTransform(TextNode, textNodeTransform);
}
```

Transforms run before DOM reconciliation, so they avoid the extra render that an update-listener-plus-update pattern would cause.

## Selection and Focus

```ts
import {
  $addUpdateTag,
  $getSelection,
  $isRangeSelection,
  COMMAND_PRIORITY_LOW,
  SELECTION_CHANGE_COMMAND,
  SKIP_DOM_SELECTION_TAG,
} from 'lexical';

editor.registerCommand(
  SELECTION_CHANGE_COMMAND,
  () => {
    const selection = $getSelection();
    if ($isRangeSelection(selection)) {
      console.log(selection.getTextContent());
    }
    return false;
  },
  COMMAND_PRIORITY_LOW,
);

editor.update(() => {
  $addUpdateTag(SKIP_DOM_SELECTION_TAG);
  // Mutate editor state here without syncing DOM selection.
});
```

Use `SKIP_DOM_SELECTION_TAG` when a command or update must not steal focus.

## DOM Events on the Root

```ts
editor.registerRootListener((rootElement, prevRootElement) => {
  prevRootElement?.removeEventListener('paste', handlePaste);
  rootElement?.addEventListener('paste', handlePaste);
});
```

Use this for native events tied to the editor root. Clean up the previous root every time.

## Combining Disposers

```ts
import {mergeRegister} from '@lexical/utils';

useEffect(() => {
  return mergeRegister(
    registerBadgeCommand(editor),
    editor.registerUpdateListener(({editorState}) => {
      editorState.read(() => {
        setIsEmpty($getRoot().getTextContent() === '');
      });
    }),
  );
}, [editor]);
```

## Update Tags

Use built-in tag constants instead of string literals when a change should affect history, collaboration, scroll, or selection behavior:

```ts
import {HISTORY_PUSH_TAG, PASTE_TAG} from 'lexical';

editor.update(
  () => {
    // Apply pasted content.
  },
  {tag: [HISTORY_PUSH_TAG, PASTE_TAG]},
);
```
