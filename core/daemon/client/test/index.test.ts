import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import {
  createDaemonIpcClient,
  createDaemonIpcClientFromEnv,
  resolveDaemonUrl,
} from "../src/node/index.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME

afterEach(async () => {
  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("createDaemonIpcClient passes the explicit daemon URL to the injected factory", () => {
  const ipcHook = () => {}
  const calls: Array<{ daemonUrl: string; ipcHook: unknown }> = []
  const client = createDaemonIpcClient({
    daemonUrl: "http://127.0.0.1:49827/",
    ipcHook,
    createClient: ({ daemonUrl, ipcHook }) => {
      calls.push({ daemonUrl, ipcHook })
      return { kind: "custom" as const, daemonUrl }
    },
  })

  expect(client).toEqual({
    kind: "custom",
    daemonUrl: "http://127.0.0.1:49827/",
  })
  expect(calls).toEqual([{ daemonUrl: "http://127.0.0.1:49827/", ipcHook }])
})

test("createDaemonIpcClientFromEnv passes the resolved daemon URL to the injected factory", () => {
  const ipcHook = () => {}
  const calls: Array<{ daemonUrl: string; ipcHook: unknown }> = []
  const result = createDaemonIpcClientFromEnv({
    env: {
      GODDARD_DAEMON_URL: "http://127.0.0.1:49829/",
    },
    ipcHook,
    createClient: ({ daemonUrl, ipcHook }) => {
      calls.push({ daemonUrl, ipcHook })
      return { kind: "custom" as const, daemonUrl }
    },
  })

  expect(result.daemonUrl).toBe("http://127.0.0.1:49829/")
  expect(result.client).toEqual({
    kind: "custom",
    daemonUrl: "http://127.0.0.1:49829/",
  })
  expect(calls).toEqual([{ daemonUrl: "http://127.0.0.1:49829/", ipcHook }])
})

test("resolveDaemonUrl can derive the daemon URL from an explicit daemon port", () => {
  expect(
    resolveDaemonUrl({
      GODDARD_DAEMON_PORT: "41234",
    }),
  ).toBe("http://127.0.0.1:41234/")
})

test("resolveDaemonUrl falls back to the configured global daemon port", async () => {
  const homeDir = await useTempHome()

  const configDir = join(homeDir, ".goddard")
  await mkdir(configDir, { recursive: true })
  await writeFile(
    join(configDir, "config.json"),
    `${JSON.stringify({ daemon: { port: 41235 } }, null, 2)}\n`,
    "utf8",
  )

  expect(resolveDaemonUrl({})).toBe("http://127.0.0.1:41235/")
})

test("resolveDaemonUrl falls back to the default daemon port", async () => {
  await useTempHome()
  expect(resolveDaemonUrl({})).toBe("http://127.0.0.1:49827/")
})

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-daemon-client-home-"))
  process.env.HOME = homeDir
  cleanup.push(() => rm(homeDir, { recursive: true, force: true }))
  return homeDir
}
