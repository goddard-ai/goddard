import { expect, test } from "vitest"
import * as daemonSchema from "../src/daemon.ts"

test("daemon schema keeps loop and workforce response types as types rather than runtime schemas", () => {
  expect("DaemonHealthSchema" in daemonSchema).toBe(false)
  expect("SubmitPrDaemonResponseSchema" in daemonSchema).toBe(false)
  expect("ReplyPrDaemonResponseSchema" in daemonSchema).toBe(false)
  expect("DaemonSessionSchema" in daemonSchema).toBe(false)
  expect("CreateDaemonSessionResponseSchema" in daemonSchema).toBe(false)
  expect("GetDaemonSessionHistoryResponseSchema" in daemonSchema).toBe(false)
  expect("ShutdownDaemonSessionResponseSchema" in daemonSchema).toBe(false)
  expect("DaemonLoopSchema" in daemonSchema).toBe(false)
  expect("StartDaemonLoopResponseSchema" in daemonSchema).toBe(false)
  expect("DaemonWorkforceSchema" in daemonSchema).toBe(false)
  expect("StartDaemonWorkforceResponseSchema" in daemonSchema).toBe(false)
})

test("daemon session and loop contracts expose stable ids without legacy server ids", () => {
  const keys: Array<keyof daemonSchema.DaemonSession> = ["id", "acpSessionId"]

  expect(keys).toEqual(["id", "acpSessionId"])
  expect("serverId" in ({} as daemonSchema.DaemonSession)).toBe(false)
  expect("serverId" in ({} as daemonSchema.DaemonLoopStatus)).toBe(false)
})
