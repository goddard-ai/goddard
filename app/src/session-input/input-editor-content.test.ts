import { expect, test } from "bun:test"
import {
  $createParagraphNode,
  $createTextNode,
  $getRoot,
  $isElementNode,
  createEditor,
} from "lexical"

import { $createComposerChipNode, ComposerChipNode } from "~/session-chat/composer-chip-node.tsrx"
import {
  $isComposerShellPromptNode,
  ComposerShellPromptNode,
} from "~/session-chat/composer-shell-prompt-node.tsrx"
import {
  deleteSessionInputChipBeforeCaret,
  setSessionInputEditorPrompt,
} from "./input-editor-content.ts"

function buildEditor(deleteChip: () => void) {
  const editor = createEditor({
    nodes: [ComposerChipNode, ComposerShellPromptNode],
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

test("session input prompt rehydrates shell fences into shell prompt nodes", () => {
  const editor = createEditor({
    nodes: [ComposerChipNode, ComposerShellPromptNode],
    onError(error) {
      throw error
    },
  })

  setSessionInputEditorPrompt(editor, [
    {
      type: "text",
      text: "Run these commands:\n```shell\nbun test\nbun run lint\n```\nThen summarize the failures.",
    },
  ])

  editor.getEditorState().read(() => {
    const paragraph = $getRoot().getFirstChild()

    expect($getRoot().getTextContent()).toBe(
      "Run these commands:\n$ bun test\n$ bun run lint\nThen summarize the failures.",
    )

    expect($isElementNode(paragraph)).toBe(true)

    if (!$isElementNode(paragraph)) {
      return
    }

    expect($isComposerShellPromptNode(paragraph.getChildAtIndex(2))).toBe(true)
    expect($isComposerShellPromptNode(paragraph.getChildAtIndex(5))).toBe(true)
  })
})
