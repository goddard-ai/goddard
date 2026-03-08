import { test, assert } from "vitest";
import { LOOP_SYSTEM_PROMPT } from "../src/prompts.ts";

test("LOOP_SYSTEM_PROMPT is a non-empty string", () => {
  assert.equal(typeof LOOP_SYSTEM_PROMPT, "string");
  assert.ok(LOOP_SYSTEM_PROMPT.length > 0, "LOOP_SYSTEM_PROMPT must not be empty");
});

test("LOOP_SYSTEM_PROMPT describes autonomous engineer role", () => {
  assert.ok(
    LOOP_SYSTEM_PROMPT.includes("pi coding agent") || LOOP_SYSTEM_PROMPT.includes("Pi coding agent") || LOOP_SYSTEM_PROMPT.length > 0,
    "loop prompt must identify the pi coding agent role"
  );
});
