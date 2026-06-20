import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import {
  createDebug,
  createLogger,
  createLogStore,
  subtractHours,
  toErrorProperties,
} from "../src/index.ts"

let testDir: string | undefined

afterEach(async () => {
  if (testDir) {
    await rm(testDir, { recursive: true, force: true })
    testDir = undefined
  }
})

async function createTestStore(options: { inlineByteLimit?: number } = {}) {
  testDir = await mkdtemp(join(tmpdir(), "goddard-logs-test-"))
  return createLogStore({
    databasePath: join(testDir, "logs.sqlite"),
    inlineByteLimit: options.inlineByteLimit,
  })
}

test("logger writes compact log rows", async () => {
  const store = await createTestStore()
  const logger = createLogger({ scope: "daemon", store, pid: 123 })

  logger.info("daemon.startup", {
    port: 53117,
    nested: { ok: true },
  })

  expect(store.query()).toEqual([
    {
      id: 1,
      at: expect.any(String),
      scope: "daemon",
      level: "info",
      pid: 123,
      message: "daemon.startup",
      properties: {
        port: 53117,
        nested: { ok: true },
      },
    },
  ])

  store.close()
})

test("collapses long strings and large nested values", async () => {
  const store = await createTestStore({ inlineByteLimit: 24 })

  const row = store.append({
    scope: "daemon",
    level: "info",
    message: "ipc.response_sent",
    properties: {
      body: "x".repeat(40),
      items: ["one", "two", "three", "four"],
      object: { text: "y".repeat(40) },
    },
  })

  expect(row.properties.body).toMatch(/^str_[0-9A-HJKMNP-TV-Z]{26}$/)
  expect(row.properties.items).toMatch(/^arr_[0-9A-HJKMNP-TV-Z]{26}$/)
  expect(row.properties.object).toMatch(/^obj_[0-9A-HJKMNP-TV-Z]{26}$/)
  expect(store.expand(row.properties.body as string)?.body).toBe("x".repeat(40))
  expect(store.expand(row.properties.items as string)?.body).toEqual([
    "one",
    "two",
    "three",
    "four",
  ])
  expect(store.expand(row.properties.object as string)?.body).toEqual({ text: "y".repeat(40) })

  store.close()
})

test("redacts secrets before persistence", async () => {
  const store = await createTestStore()

  store.append({
    scope: "app",
    level: "info",
    message: "auth.request",
    properties: {
      token: "secret",
      nested: {
        authorization: "Bearer secret",
      },
      env: {
        OPENAI_API_KEY: "secret",
        PATH: "/bin",
      },
    },
  })

  expect(store.query()[0]?.properties).toEqual({
    token: "[redacted]",
    nested: {
      authorization: "[redacted]",
    },
    env: {
      OPENAI_API_KEY: "[redacted]",
      PATH: "/bin",
    },
  })

  store.close()
})

test("serializes error properties for durable logs", () => {
  const cause = new Error("inner")
  const error = new Error("outer", { cause })

  expect(toErrorProperties(error)).toEqual({
    errorMessage: "outer",
    errorName: "Error",
    errorStack: expect.stringContaining("Error: outer"),
    errorCauseMessage: "inner",
  })
  expect(toErrorProperties("plain failure")).toEqual({
    errorMessage: "plain failure",
    errorName: "string",
  })
})

test("queries by scope, grep, cursor, and property", async () => {
  const store = await createTestStore()

  store.append({ scope: "daemon", level: "info", message: "first", properties: { method: "a" } })
  store.append({ scope: "app", level: "info", message: "needle", properties: { method: "b" } })
  store.append({
    scope: "daemon",
    level: "info",
    message: "third",
    properties: { method: "b", durationMs: 38 },
  })

  expect(store.query({ scope: "daemon" }).map((entry) => entry.message)).toEqual(["first", "third"])
  expect(store.query({ grep: "needle" }).map((entry) => entry.message)).toEqual(["needle"])
  expect(store.query({ properties: { method: "b" } }).map((entry) => entry.message)).toEqual([
    "needle",
    "third",
  ])
  expect(store.query({ properties: { durationMs: "38" } }).map((entry) => entry.message)).toEqual([
    "third",
  ])
  expect(store.query({ regex: "nee(dle)?|third" }).map((entry) => entry.message)).toEqual([
    "needle",
    "third",
  ])
  expect(store.query({ afterId: 1 }).map((entry) => entry.message)).toEqual(["needle", "third"])
  expect(store.query({ beforeId: 3 }).map((entry) => entry.message)).toEqual(["first", "needle"])

  store.close()
})

test("queries by minimum level and debug scope prefix", async () => {
  const store = await createTestStore()
  const sessionDebug = createDebug("session.history", { scope: "daemon", store, pid: 123 })
  const configDebug = createDebug("config.reload", { scope: "daemon", store, pid: 123 })

  sessionDebug("history.normalized", { sessionId: "ses_1" })
  configDebug("config.refreshed")
  store.append({ scope: "daemon", level: "info", message: "daemon.ready" })
  store.append({ scope: "daemon", level: "warn", message: "daemon.slow" })

  expect(store.query({ level: "info" }).map((entry) => entry.message)).toEqual([
    "daemon.ready",
    "daemon.slow",
  ])
  expect(store.query({ level: "debug" }).map((entry) => entry.message)).toEqual([
    "history.normalized",
    "config.refreshed",
    "daemon.ready",
    "daemon.slow",
  ])
  expect(store.query({ debugScope: "session" }).map((entry) => entry.message)).toEqual([
    "history.normalized",
  ])
  expect(store.query({ debugScope: "session" })[0]?.properties).toMatchObject({
    debugScope: "session.history",
    sessionId: "ses_1",
  })

  store.close()
})

test("retention removes old rows and unreferenced collapsed values", async () => {
  const store = await createTestStore({ inlineByteLimit: 8 })
  const old = store.append({
    at: subtractHours(new Date(), 25),
    scope: "daemon",
    level: "info",
    message: "old",
    properties: { payload: "old value that collapses" },
  })
  const recent = store.append({
    scope: "daemon",
    level: "info",
    message: "recent",
    properties: { payload: "recent value that collapses" },
  })

  store.retainSince(subtractHours(new Date(), 24))

  expect(store.query().map((entry) => entry.message)).toEqual(["recent"])
  expect(store.expand(old.properties.payload as string)).toBeNull()
  expect(store.expand(recent.properties.payload as string)?.body).toBe(
    "recent value that collapses",
  )

  store.close()
})
