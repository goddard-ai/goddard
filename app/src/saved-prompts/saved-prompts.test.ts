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

test("saved prompts automatically save a prompt after the third repeat in seven days", () => {
  const savedPrompts = new SavedPrompts()
  const baseTime = Date.UTC(2026, 0, 1)

  savedPrompts.recordSubmission("  Explain the failure.  ", baseTime)
  savedPrompts.recordSubmission("Explain the failure.", baseTime + 1)
  expect(savedPrompts.isSaved("Explain the failure.")).toBe(false)

  savedPrompts.recordSubmission("Explain the failure.", baseTime + 2)
  expect(savedPrompts.promptList.map((prompt) => prompt.text)).toEqual(["Explain the failure."])
})

test("saved prompts ignore repeats older than seven days", () => {
  const savedPrompts = new SavedPrompts()
  const baseTime = Date.UTC(2026, 0, 1)
  const eightDays = 8 * 24 * 60 * 60 * 1000

  savedPrompts.recordSubmission("Review this diff.", baseTime)
  savedPrompts.recordSubmission("Review this diff.", baseTime + 1)
  savedPrompts.recordSubmission("Review this diff.", baseTime + eightDays)
  expect(savedPrompts.isSaved("Review this diff.")).toBe(false)

  savedPrompts.recordSubmission("Review this diff.", baseTime + eightDays + 1)
  savedPrompts.recordSubmission("Review this diff.", baseTime + eightDays + 2)
  expect(savedPrompts.isSaved("Review this diff.")).toBe(true)
})
