# Nodes and State

Use this file to model custom content, store node metadata, or override built-in node behavior.

## Choose the Representation

- Use `NodeState` when the job is "store serializable metadata on an existing node or on the root".
- Use `TextNode` when the content is inline text with custom behavior or DOM.
- Use `ElementNode` when the content owns children and participates in document structure.
- Use `DecoratorNode` when the content should render an arbitrary component or view.
- Use node replacement when built-in nodes need different behavior but existing transforms and listeners should still match them.

## NodeState First

`NodeState` is the preferred option for ad-hoc metadata because it participates in reconciliation, history, and JSON serialization with far less boilerplate than subclass fields.

```ts
import {
  $createParagraphNode,
  $getRoot,
  $getState,
  $setState,
  createState,
} from 'lexical';

const questionState = createState('question', {
  parse: (value) => (typeof value === 'string' ? value : ''),
});

editor.update(() => {
  const paragraph = $createParagraphNode();
  $setState(paragraph, questionState, 'Ready to publish?');
  $getRoot().append(paragraph);
});

editor.getEditorState().read(() => {
  const node = $getRoot().getFirstChildOrThrow();
  console.log($getState(node, questionState));
});
```

Use `NodeState` for document metadata too; it can live on the `RootNode`.

## Custom TextNode

```ts
import type {
  EditorConfig,
  NodeKey,
  SerializedTextNode,
  Spread,
} from 'lexical';
import {TextNode} from 'lexical';

type SerializedEmojiNode = Spread<
  {
    unifiedID?: string;
  },
  SerializedTextNode
>;

export class EmojiNode extends TextNode {
  __unifiedID: string;

  static getType(): string {
    return 'emoji';
  }

  static clone(node: EmojiNode): EmojiNode {
    return new EmojiNode(node.__unifiedID, node.__key);
  }

  static importJSON(serializedNode: SerializedEmojiNode): EmojiNode {
    return new EmojiNode(serializedNode.unifiedID ?? '').updateFromJSON(
      serializedNode,
    );
  }

  constructor(unifiedID = '', key?: NodeKey) {
    super(unifiedID, key);
    this.__unifiedID = unifiedID;
  }

  createDOM(_config: EditorConfig): HTMLElement {
    const span = document.createElement('span');
    span.className = 'emoji-node';
    span.textContent = this.__text;
    return span;
  }

  exportJSON(): SerializedEmojiNode {
    return {
      ...super.exportJSON(),
      unifiedID: this.__unifiedID,
    };
  }
}
```

Keep constructors zero-argument friendly where possible. If you store direct properties, initialize them unconditionally so collaboration code can sync them reliably.

## Node Replacement

Use node replacement when the job is "make Lexical treat my subclass like the built-in node everywhere".

```ts
const initialConfig = {
  nodes: [
    CustomParagraphNode,
    {
      replace: ParagraphNode,
      with: () => $createCustomParagraphNode(),
      withKlass: CustomParagraphNode,
    },
  ],
};
```

`withKlass` keeps transforms and mutation listeners written for the original class working against the replacement class too.

## Non-Negotiable Rules

- Never subclass or replace `RootNode`.
- Never store `'\n'` in a `TextNode`; use `LineBreakNode`.
- Wrap mutable direct properties behind getters and setters that use `getLatest()` and `getWritable()`.
- Do not assume node keys are stable or serializable.
