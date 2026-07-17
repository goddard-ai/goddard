import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import { UserConfigErrorCodes } from "@goddard-ai/schema/daemon-ipc"
import { afterEach, expect, test } from "bun:test"

import { createUserConfigService } from "../src/user-config.ts"

const originalHome = process.env.HOME
let homeDir: string | undefined

afterEach(async () => {
  if (homeDir) {
    await rm(homeDir, { force: true, recursive: true })
    homeDir = undefined
  }

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

test("user config service exposes the editable schema and preserves unrelated fields", async () => {
  await useTempHome()
  await writeUserConfig({
    agents: { default: "pi-acp" },
    security: { pullRequests: { submit: "deny" } },
  })
  const service = createUserConfigService()

  const current = await service.get()
  expect(current.document).toEqual({
    agents: { default: "pi-acp" },
    security: { pullRequests: { submit: "deny" } },
  })
  expect(current.schema).toMatchObject({
    type: "object",
    properties: {
      daemon: expect.any(Object),
      agents: expect.any(Object),
    },
  })
  expect((current.schema.properties as Record<string, unknown>).$schema).toBeUndefined()

  const [agentUpdate, daemonUpdate] = await Promise.all([
    service.update({ operation: "set", path: "/agents/default", value: "codex-acp" }),
    service.update({ operation: "set", path: "/daemon/port", value: 51_999 }),
  ])

  expect(agentUpdate.restartRequired).toBe(false)
  expect(daemonUpdate).toMatchObject({
    document: {
      agents: { default: "codex-acp" },
      daemon: { port: 51_999 },
      security: { pullRequests: { submit: "deny" } },
    },
    restartRequired: true,
  })

  const persisted = await readPersistedConfig()
  expect(persisted).toEqual({
    $schema:
      "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json",
    agents: { default: "codex-acp" },
    daemon: { port: 51_999 },
    security: { pullRequests: { submit: "deny" } },
  })

  await expect(service.update({ operation: "remove", path: "/security" })).resolves.toMatchObject({
    document: {
      agents: { default: "codex-acp" },
      daemon: { port: 51_999 },
    },
    restartRequired: false,
  })
})

test("user config service rejects an invalid result without replacing the file", async () => {
  await useTempHome()
  await writeUserConfig({ agents: { default: "pi-acp" } })
  const before = await readFile(getGlobalConfigPath(), "utf8")
  const service = createUserConfigService()

  await expect(
    service.update({ operation: "set", path: "/daemon/port", value: 70_000 }),
  ).rejects.toMatchObject({
    code: UserConfigErrorCodes.InvalidDocument,
    details: { paths: ["/daemon/port"] },
  })

  await expect(readFile(getGlobalConfigPath(), "utf8")).resolves.toBe(before)
})

async function useTempHome() {
  homeDir = await mkdtemp(join(tmpdir(), "goddard-user-config-"))
  process.env.HOME = homeDir
}

async function writeUserConfig(document: Record<string, unknown>) {
  const configPath = getGlobalConfigPath()
  await mkdir(dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify({ $schema: "https://example.com/old-schema.json", ...document }, null, 2)}\n`,
    "utf8",
  )
}

async function readPersistedConfig() {
  return JSON.parse(await readFile(getGlobalConfigPath(), "utf8")) as Record<string, unknown>
}
