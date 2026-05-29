import { expect, test } from "bun:test"
import { $createParagraphNode, $createTextNode, $getRoot, createEditor } from "lexical"

import { isSessionInputCaretAtPromptEnd } from "./input-selection.ts"

function readPromptEndSelection(build: () => void) {
  const editor = createEditor({
    onError(error) {
      throw error
    },
  })
  let isAtEnd = false

  editor.update(
    () => {
      build()
      isAtEnd = isSessionInputCaretAtPromptEnd()
    },
    { discrete: true },
  )

  return isAtEnd
}

test("session input caret end detection accepts a collapsed caret at the prompt end", () => {
  expect(
    readPromptEndSelection(() => {
      const paragraph = $createParagraphNode()
      const text = $createTextNode("send this")
      paragraph.append(text)
      $getRoot().append(paragraph)
      text.select(9, 9)
    }),
  ).toBe(true)
})

test("session input caret end detection rejects a collapsed caret before trailing text", () => {
  expect(
    readPromptEndSelection(() => {
      const paragraph = $createParagraphNode()
      const text = $createTextNode("split here")
      paragraph.append(text)
      $getRoot().append(paragraph)
      text.select(5, 5)
    }),
  ).toBe(false)
})

test("session input caret end detection rejects a collapsed caret before later blocks", () => {
  expect(
    readPromptEndSelection(() => {
      const firstParagraph = $createParagraphNode()
      const firstText = $createTextNode("first")
      firstParagraph.append(firstText)
      $getRoot().append(firstParagraph)
      $getRoot().append($createParagraphNode())
      firstText.select(5, 5)
    }),
  ).toBe(false)
})
