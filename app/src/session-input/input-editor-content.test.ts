import { expect, test } from "bun:test"
import { $createParagraphNode, $createTextNode, $getRoot, createEditor } from "lexical"

import { $createComposerChipNode, ComposerChipNode } from "~/session-chat/composer-chip-node.tsrx"
import { deleteSessionInputChipBeforeCaret } from "./input-editor-content.ts"

function buildEditor(deleteChip: () => void) {
  const editor = createEditor({
    nodes: [ComposerChipNode],
    onError(error) {
      throw error
    },
  })

  editor.update(deleteChip, { discrete: true })
  return editor
}

test("session input backspace deletion removes a chip before a text caret", () => {
  const editor = buildEditor(() => {
    const paragraph = $createParagraphNode()
    const text = $createTextNode(" after")
    paragraph.append(
      $createComposerChipNode({
        kind: "file",
        label: "input.ts",
        path: "/repo/input.ts",
        uri: "file:///repo/input.ts",
        detail: "./input.ts",
      }),
      text,
    )
    $getRoot().append(paragraph)
    text.select(0, 0)

    expect(deleteSessionInputChipBeforeCaret()).toBe(true)
  })

  editor.getEditorState().read(() => {
    expect($getRoot().getTextContent()).toBe(" after")
  })
})

test("session input backspace deletion removes a chip before an element caret", () => {
  const editor = buildEditor(() => {
    const paragraph = $createParagraphNode()
    paragraph.append(
      $createComposerChipNode({
        kind: "skill",
        label: "review",
        path: "/repo/.agents/skills/review/SKILL.md",
        uri: "file:///repo/.agents/skills/review/SKILL.md",
        detail: "./.agents/skills/review/SKILL.md",
        source: "local",
      }),
    )
    $getRoot().append(paragraph)
    paragraph.select(1, 1)

    expect(deleteSessionInputChipBeforeCaret()).toBe(true)
  })

  editor.getEditorState().read(() => {
    expect($getRoot().getTextContent()).toBe("")
  })
})

test("session input backspace deletion ignores non-adjacent text carets", () => {
  const editor = buildEditor(() => {
    const paragraph = $createParagraphNode()
    const text = $createTextNode(" after")
    paragraph.append(
      $createComposerChipNode({
        kind: "file",
        label: "input.ts",
        path: "/repo/input.ts",
        uri: "file:///repo/input.ts",
        detail: "./input.ts",
      }),
      text,
    )
    $getRoot().append(paragraph)
    text.select(1, 1)

    expect(deleteSessionInputChipBeforeCaret()).toBe(false)
  })

  editor.getEditorState().read(() => {
    expect($getRoot().getTextContent()).toBe("@input.ts after")
  })
})
