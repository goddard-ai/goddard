import { test } from "vitest"
import * as assert from "node:assert/strict"
import * as daemonSchema from "../src/daemon.ts"

test("daemon schema keeps response types as types rather than runtime zod schemas", () => {
  assert.equal("DaemonHealthSchema" in daemonSchema, false)
  assert.equal("SubmitPrDaemonResponseSchema" in daemonSchema, false)
  assert.equal("ReplyPrDaemonResponseSchema" in daemonSchema, false)
  assert.equal("DaemonSessionSchema" in daemonSchema, false)
  assert.equal("CreateDaemonSessionResponseSchema" in daemonSchema, false)
  assert.equal("GetDaemonSessionHistoryResponseSchema" in daemonSchema, false)
  assert.equal("ShutdownDaemonSessionResponseSchema" in daemonSchema, false)
})

test.todo("daemon session contracts expose id and acpId and do not require serverId")
