import * as assert from "node:assert/strict"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { afterEach, test, vi } from "vitest"

vi.mock("../src/daemon/index.ts", () => ({
  runAgent: vi.fn(),
}))

import {
  buildActionSessionParams,
  resolveAction,
  type ResolvedAgentAction,
} from "../src/node/actions.ts"

const originalHome = process.env.HOME

afterEach(() => {
  if (originalHome === undefined) {
    delete process.env.HOME
    return
  }

  process.env.HOME = originalHome
})

test("resolveAction applies local root defaults to prompt-only actions", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-md-"))
  const actionsDir = path.join(tempDir, ".goddard", "actions")

  await fs.mkdir(actionsDir, { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      actions: {
        cwd: "/tmp/local-root",
        systemPrompt: "Use the repository checklist.",
      },
    }),
    "utf-8",
  )
  await fs.writeFile(
    path.join(actionsDir, "review.md"),
    "Review the current diff carefully.\n",
    "utf-8",
  )

  const action = await resolveAction("review", tempDir)

  assert.equal(action.prompt, "Review the current diff carefully.\n")
  assert.deepEqual(action.config, {
    cwd: "/tmp/local-root",
    systemPrompt: "Use the repository checklist.",
  })
})

test("resolveAction merges root defaults with packaged config.json", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-dir-"))
  const actionDir = path.join(tempDir, ".goddard", "actions", "ship-it")

  await fs.mkdir(actionDir, { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      actions: {
        systemPrompt: "Use repository defaults.",
        env: {
          SHARED: "true",
        },
      },
    }),
    "utf-8",
  )
  await fs.writeFile(path.join(actionDir, "prompt.md"), "Ship the change.\n", "utf-8")
  await fs.writeFile(
    path.join(actionDir, "config.json"),
    JSON.stringify({
      cwd: "/tmp/entity-override",
      env: {
        ENTITY: "true",
      },
      mcpServers: [{ name: "filesystem" }],
    }),
    "utf-8",
  )

  const action = await resolveAction("ship-it", tempDir)

  assert.equal(action.prompt, "Ship the change.\n")
  assert.deepEqual(action.config, {
    systemPrompt: "Use repository defaults.",
    cwd: "/tmp/entity-override",
    env: {
      SHARED: "true",
      ENTITY: "true",
    },
    mcpServers: [{ name: "filesystem" }],
  })
})

test("resolveAction applies local root defaults to a globally defined action", async () => {
  const tempHome = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-home-"))
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-cwd-"))
  process.env.HOME = tempHome

  const globalActionDir = path.join(tempHome, ".goddard", "actions", "global")
  await fs.mkdir(globalActionDir, { recursive: true })
  await fs.writeFile(path.join(globalActionDir, "prompt.md"), "Use the global action.\n", "utf-8")
  await fs.writeFile(
    path.join(globalActionDir, "config.json"),
    JSON.stringify({
      systemPrompt: "Use the global defaults.",
    }),
    "utf-8",
  )

  await fs.mkdir(path.join(tempDir, ".goddard"), { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      actions: {
        cwd: "/tmp/local-root",
      },
    }),
    "utf-8",
  )

  const action = await resolveAction("global", tempDir)

  assert.equal(action.prompt, "Use the global action.\n")
  assert.deepEqual(action.config, {
    cwd: "/tmp/local-root",
    systemPrompt: "Use the global defaults.",
  })
})

test("resolveAction rejects frontmatter-based actions", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-frontmatter-"))
  const actionsDir = path.join(tempDir, ".goddard", "actions")

  await fs.mkdir(actionsDir, { recursive: true })
  await fs.writeFile(
    path.join(actionsDir, "review.md"),
    ["---", 'systemPrompt: "legacy"', "---", "Review the current diff carefully."].join("\n"),
    "utf-8",
  )

  await assert.rejects(resolveAction("review", tempDir), /YAML frontmatter.*must move into JSON/)
})

test("buildActionSessionParams lets runtime config override resolved defaults", () => {
  const action: ResolvedAgentAction = {
    prompt: "Review the pull request.",
    config: {
      cwd: "/tmp/action-cwd",
      systemPrompt: "Follow the security checklist.",
    },
    path: "/tmp/review.md",
  }

  const params = buildActionSessionParams(action, {
    cwd: "/tmp/caller-cwd",
    systemPrompt: "Start with repo conventions.",
  })

  assert.equal(params.cwd, "/tmp/caller-cwd")
  assert.equal(params.systemPrompt, "Start with repo conventions.")
})
