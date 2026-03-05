import test from "node:test";
import assert from "node:assert/strict";
import { SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "../src/prompts.ts";
import * as sdk from "../src/index.ts";

test("SPEC_SYSTEM_PROMPT describes intent guardian role", () => {
  assert.ok(
    SPEC_SYSTEM_PROMPT.includes("Intent Guardian"),
    "spec prompt must identify the intent guardian role"
  );
  assert.ok(
    SPEC_SYSTEM_PROMPT.includes("spec/"),
    "spec prompt must reference the spec directory"
  );
});

test("PROPOSE_SYSTEM_PROMPT describes feature proposer role", () => {
  assert.ok(
    PROPOSE_SYSTEM_PROMPT.includes("Feature Proposer"),
    "propose prompt must identify the feature proposer role"
  );
  assert.ok(
    PROPOSE_SYSTEM_PROMPT.includes("spec/proposals/"),
    "propose prompt must reference the proposals directory"
  );
});

test("prompts are exported via index", () => {
  assert.equal(typeof sdk.SPEC_SYSTEM_PROMPT, "string");
  assert.equal(typeof sdk.PROPOSE_SYSTEM_PROMPT, "string");

  assert.ok(sdk.SPEC_SYSTEM_PROMPT.length > 0);
  assert.ok(sdk.PROPOSE_SYSTEM_PROMPT.length > 0);
});
