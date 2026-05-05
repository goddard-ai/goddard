import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { afterEach, beforeEach, expect, test } from "bun:test"

import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"
import type { BackendPrClient } from "../src/ipc/types.ts"
import { db, resetDb } from "../src/persistence/store.ts"

let daemon: DaemonServer | null = null

beforeEach(() => {
  resetDb({ filename: ":memory:" })
})

afterEach(async () => {
  await daemon?.close().catch(() => {})
  daemon = null
  resetDb({ filename: ":memory:" })
})

test("daemon app settings IPC stores one latest record per scope and key", async () => {
  daemon = await startDaemonServer(createBackendClient(), { port: 0 })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const key = "goddard.app.state.v1"
  const primaryScope = {
    scopeKind: "window",
    scopeId: "primary",
  } as const
  const secondaryScope = {
    scopeKind: "window",
    scopeId: "secondary",
  } as const
  const firstRecord = {
    version: 1,
    savedAt: 100,
    value: {
      navigation: {
        selectedNavId: "inbox",
      },
    },
  }
  const secondRecord = {
    version: 1,
    savedAt: 200,
    value: {
      navigation: {
        selectedNavId: "sessions",
      },
    },
  }

  await expect(client.send("appSettings.get", { key, ...primaryScope })).resolves.toEqual({
    setting: null,
  })
  await expect(
    client.send("appSettings.set", {
      key,
      ...primaryScope,
      record: firstRecord,
    }),
  ).resolves.toEqual({
    setting: firstRecord,
  })
  await expect(
    client.send("appSettings.set", {
      key,
      ...primaryScope,
      record: secondRecord,
    }),
  ).resolves.toEqual({
    setting: secondRecord,
  })

  await expect(
    client.send("appSettings.set", {
      key,
      ...secondaryScope,
      record: firstRecord,
    }),
  ).resolves.toEqual({
    setting: firstRecord,
  })

  expect(db.appSettings.findMany({ where: { key } })).toHaveLength(2)
  expect(
    db.appSettings.findMany({ where: { scopeKind: "window", scopeId: "primary" } }),
  ).toHaveLength(1)
  await expect(client.send("appSettings.get", { key, ...primaryScope })).resolves.toEqual({
    setting: secondRecord,
  })
  await expect(
    client.send("appSettings.get", {
      key,
      ...secondaryScope,
    }),
  ).resolves.toEqual({
    setting: firstRecord,
  })
  await expect(client.send("appSettings.delete", { key, ...primaryScope })).resolves.toEqual({
    deleted: true,
  })
  await expect(client.send("appSettings.get", { key, ...primaryScope })).resolves.toEqual({
    setting: null,
  })
  await expect(
    client.send("appSettings.get", {
      key,
      ...secondaryScope,
    }),
  ).resolves.toEqual({
    setting: firstRecord,
  })
})

function createBackendClient() {
  return {
    auth: {
      startDeviceFlow: async () => ({
        deviceCode: "dev_1",
        userCode: "ABCD-1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      }),
      completeDeviceFlow: async () => ({
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      }),
      whoami: async () => ({
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      }),
      logout: async () => {},
    },
    pr: {
      create: async () => ({
        number: 1,
        url: "https://github.com/example/repo/pull/1",
      }),
      reply: async () => ({ success: true }),
    },
  } satisfies BackendPrClient
}
