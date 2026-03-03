import test from "node:test";
import assert from "node:assert/strict";
import { LOOP_SYSTEM_PROMPT, SPEC_SYSTEM_PROMPT } from "../src/prompts.ts";

test("LOOP_SYSTEM_PROMPT is a non-empty string", () => {
  assert.equal(typeof LOOP_SYSTEM_PROMPT, "string");
  assert.ok(LOOP_SYSTEM_PROMPT.length > 0, "LOOP_SYSTEM_PROMPT must not be empty");
});

test("SPEC_SYSTEM_PROMPT is a non-empty string", () => {
  assert.equal(typeof SPEC_SYSTEM_PROMPT, "string");
  assert.ok(SPEC_SYSTEM_PROMPT.length > 0, "SPEC_SYSTEM_PROMPT must not be empty");
});

test("LOOP_SYSTEM_PROMPT describes autonomous engineer role", () => {
  assert.ok(
    LOOP_SYSTEM_PROMPT.includes("Pi coding agent"),
    "loop prompt must identify the pi coding agent role"
  );
});

test("SPEC_SYSTEM_PROMPT describes intent guardian role", () => {
  assert.ok(
    SPEC_SYSTEM_PROMPT.includes("Intent Guardian"),
    "spec prompt must identify the Intent Guardian role"
  );
});

test("LOOP_SYSTEM_PROMPT and SPEC_SYSTEM_PROMPT are distinct", () => {
  assert.notEqual(
    LOOP_SYSTEM_PROMPT,
    SPEC_SYSTEM_PROMPT,
    "the two prompts must be different content"
  );
});

test("prompts are re-exported from SDK root index", async () => {
  const sdk = await import("../src/index.ts");
  assert.equal(typeof sdk.LOOP_SYSTEM_PROMPT, "string");
  assert.equal(typeof sdk.SPEC_SYSTEM_PROMPT, "string");
  assert.ok(sdk.LOOP_SYSTEM_PROMPT.length > 0);
  assert.ok(sdk.SPEC_SYSTEM_PROMPT.length > 0);
});
