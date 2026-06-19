import { $createListItemNode, $createListNode, ListItemNode, ListNode } from "@lexical/list"
import { $createParagraphNode, $createTextNode, $getRoot, createEditor } from "lexical"
import { expect, test } from "vitest"

import { isSessionInputCaretAtPromptEnd, isSessionInputCaretInsideList } from "./input-selection.ts"

function readPromptSelection<T>(build: () => void, read: () => T): T {
  const editor = createEditor({
    nodes: [ListNode, ListItemNode],
    onError(error) {
      throw error
    },
  })
  let result: T | undefined

  editor.update(
    () => {
      build()
      result = read()
    },
    { discrete: true },
  )

  if (result === undefined) {
    throw new Error("Expected prompt selection read to run.")
  }

  return result
}

function readPromptEndSelection(build: () => void) {
  return readPromptSelection(build, isSessionInputCaretAtPromptEnd)
}

function readPromptListSelection(build: () => void) {
  return readPromptSelection(build, isSessionInputCaretInsideList)
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

test("session input list detection accepts a collapsed caret inside a list item", () => {
  expect(
    readPromptListSelection(() => {
      const list = $createListNode("bullet")
      const item = $createListItemNode()
      const text = $createTextNode("item")
      item.append(text)
      list.append(item)
      $getRoot().append(list)
      text.select(4, 4)
    }),
  ).toBe(true)
})

test("session input list detection rejects a collapsed caret outside a list", () => {
  expect(
    readPromptListSelection(() => {
      const paragraph = $createParagraphNode()
      const text = $createTextNode("plain")
      paragraph.append(text)
      $getRoot().append(paragraph)
      text.select(5, 5)
    }),
  ).toBe(false)
})
