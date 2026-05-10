# Serialization and HTML

Use this file to persist Lexical state, round-trip content through JSON, or customize HTML import and export.

## JSON Round-Trip

```ts
const serializedState = JSON.stringify(editor.getEditorState().toJSON());

const parsedState = editor.parseEditorState(serializedState);
editor.setEditorState(parsedState);
```

Use `setEditorState(...)` for explicit rehydration, resets, or imports. Do not call it on every editor update.

## Custom Node JSON Pattern

Favor `updateFromJSON(...)` inside `importJSON(...)` so subclasses can re-use base logic.

```ts
static importJSON(serializedNode: SerializedHeadingNode): HeadingNode {
  return $createHeadingNode().updateFromJSON(serializedNode);
}

updateFromJSON(
  serializedNode: LexicalUpdateJSON<SerializedHeadingNode>,
): this {
  return super.updateFromJSON(serializedNode).setTag(serializedNode.tag);
}
```

Keep new serialized fields optional when evolving existing node types. If the JSON layout becomes incompatible, prefer a new node `type` over silently reinterpreting old payloads.

## HTML Import and Export

Use node-level `importDOM()` and `exportDOM()` when the behavior belongs to a specific node:

```ts
class MentionNode extends TextNode {
  static importDOM(): DOMConversionMap {
    return {
      span: (domNode: HTMLElement) => {
        if (!domNode.dataset.mention) {
          return null;
        }

        return {
          conversion: () => ({
            node: $createMentionNode(domNode.dataset.mention ?? ''),
          }),
          priority: 2,
        };
      },
    };
  }

  exportDOM(): DOMExportOutput {
    const element = document.createElement('span');
    element.dataset.mention = this.getTextContent();
    element.textContent = this.getTextContent();
    return {element};
  }
}
```

Use the editor-level `html` config when the goal is "apply one import or export policy across many nodes without subclassing each node".

## Full-Fidelity HTML Styling

For rich-text HTML fidelity, replace the base `TextNode` with an extended text node that preserves inline style data during `importDOM()`. That is the standard recipe when imported HTML must keep styles such as `color`, `font-family`, or `text-decoration`.

## Versioning Rules

- Avoid breaking or reinterpreting existing fields in place.
- Add new fields as optional whenever possible.
- Avoid depending on a flat `version` field alone across subclass chains; base-class changes do not compose well there.
