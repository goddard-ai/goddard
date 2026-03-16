import { test } from "vitest"
import * as assert from "node:assert/strict"
import * as daemonSchema from "../src/daemon.ts"

test("daemon schema keeps response types as types rather than runtime zod schemas", () => {
  assert.equal("DaemonHealthSchema" in daemonSchema, false)
  assert.equal("SubmitPrDaemonResponseSchema" in daemonSchema, false)
  assert.equal("ReplyPrDaemonResponseSchema" in daemonSchema, false)
})
