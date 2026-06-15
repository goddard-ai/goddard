import { expect, test } from "bun:test"
import { $createParagraphNode, $createTextNode, $getRoot, createEditor } from "lexical"

import {
  captureFocusedSessionInputPrompt,
  registerSessionInputPromptFocus,
} from "./prompt-focus.ts"

async function flushMicrotasks() {
  await Promise.resolve()
  await Promise.resolve()
}

test("session input prompt focus restore preserves a DOM caret inside the editor", async () => {
  const editor = createEditor({
    onError(error) {
      throw error
    },
  })
  const rootElement = document.createElement("div")
  const nextButton = document.createElement("button")

  rootElement.contentEditable = "true"
  rootElement.setAttribute("aria-label", "Prompt")
  document.body.append(rootElement, nextButton)
  editor.setRootElement(rootElement)
  editor.update(
    () => {
      const paragraph = $createParagraphNode()
      paragraph.append($createTextNode("abcxyz"))
      $getRoot().append(paragraph)
    },
    { discrete: true },
  )

  const textNode = document.createTreeWalker(rootElement, NodeFilter.SHOW_TEXT).nextNode()
  const range = document.createRange()
  const unregister = registerSessionInputPromptFocus(editor)

  expect(textNode?.textContent).toBe("abcxyz")
  rootElement.focus()
  range.setStart(textNode as Text, 3)
  range.collapse(true)
  window.getSelection()?.removeAllRanges()
  window.getSelection()?.addRange(range)

  const restorePromptFocus = captureFocusedSessionInputPrompt()

  nextButton.focus()
  restorePromptFocus?.()
  await flushMicrotasks()

  expect(window.getSelection()?.anchorNode?.textContent).toBe("abcxyz")
  expect(window.getSelection()?.anchorOffset).toBe(3)

  unregister()
  editor.setRootElement(null)
  rootElement.remove()
  nextButton.remove()
})
