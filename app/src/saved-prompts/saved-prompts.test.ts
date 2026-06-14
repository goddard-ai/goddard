import { expect, test } from "bun:test"

import { SavedPrompts } from "./saved-prompts.ts"

test("saved prompts complete the most recently saved matching prefix", () => {
  const savedPrompts = new SavedPrompts()

  savedPrompts.save("Fix the broken settings layout.")
  savedPrompts.save("Fix the broken session input completion.")

  expect(savedPrompts.findCompletion("Fix the broken")?.text).toBe(
    "Fix the broken session input completion.",
  )
})

test("saved prompts toggle by normalized text", () => {
  const savedPrompts = new SavedPrompts()

  savedPrompts.toggle("  Summarize this transcript.  ")
  expect(savedPrompts.isSaved("Summarize this transcript.")).toBe(true)

  savedPrompts.toggle("Summarize this transcript.")
  expect(savedPrompts.promptList).toEqual([])
})
