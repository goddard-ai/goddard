import { test, expect } from "vitest"
import { SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "../src/prompts.ts"
import * as sdk from "../src/index.ts"

test("SPEC_SYSTEM_PROMPT describes intent guardian role", () => {
  expect(SPEC_SYSTEM_PROMPT).toContain("Intent Guardian")
  expect(SPEC_SYSTEM_PROMPT).toContain("spec/")
})

test("PROPOSE_SYSTEM_PROMPT describes feature proposer role", () => {
  expect(PROPOSE_SYSTEM_PROMPT).toContain("Feature Proposer")
  expect(PROPOSE_SYSTEM_PROMPT).toContain("spec/proposals/")
})

test("prompts are exported via index", () => {
  expect(typeof sdk.SPEC_SYSTEM_PROMPT).toBe("string")
  expect(typeof sdk.PROPOSE_SYSTEM_PROMPT).toBe("string")

  expect(sdk.SPEC_SYSTEM_PROMPT.length).toBeGreaterThan(0)
  expect(sdk.PROPOSE_SYSTEM_PROMPT.length).toBeGreaterThan(0)
})
