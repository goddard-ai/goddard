import * as assert from "node:assert/strict"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { afterEach, test, vi } from "vitest"

vi.mock(
  "../src/daemon/index.ts",
  async (importOriginal): Promise<typeof import("../src/daemon/index.ts")> => ({
    ...(await importOriginal<typeof import("../src/daemon/index.ts")>()),
    runAgent: vi.fn(),
  }),
)

vi.mock("@goddard-ai/config", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@goddard-ai/config")>()
  return {
    ...actual,
    resolveDefaultAgent: vi.fn().mockResolvedValue("pi-acp"),
  }
})

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
  process.env.HOME = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-home-"))
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-md-"))
  const actionsDir = path.join(tempDir, ".goddard", "actions")

  await fs.mkdir(actionsDir, { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      actions: {
        session: {
          env: {
            ROOT: "true",
          },
        },
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
    session: {
      agent: "pi-acp",
      env: {
        ROOT: "true",
      },
    },
  })
})

test("resolveAction merges root defaults with packaged config.json", async () => {
  process.env.HOME = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-home-"))
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-dir-"))
  const actionDir = path.join(tempDir, ".goddard", "actions", "ship-it")

  await fs.mkdir(actionDir, { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      actions: {
        session: {
          env: {
            SHARED: "true",
          },
        },
      },
    }),
    "utf-8",
  )
  await fs.writeFile(path.join(actionDir, "prompt.md"), "Ship the change.\n", "utf-8")
  await fs.writeFile(
    path.join(actionDir, "config.json"),
    JSON.stringify({
      session: {
        env: {
          ENTITY: "true",
        },
        mcpServers: [{ name: "filesystem" }],
      },
    }),
    "utf-8",
  )

  const action = await resolveAction("ship-it", tempDir)

  assert.equal(action.prompt, "Ship the change.\n")
  assert.deepEqual(action.config, {
    session: {
      agent: "pi-acp",
      env: {
        ENTITY: "true",
      },
      mcpServers: [{ name: "filesystem" }],
    },
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
      session: {
        env: {
          GLOBAL: "true",
        },
      },
    }),
    "utf-8",
  )

  await fs.mkdir(path.join(tempDir, ".goddard"), { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      actions: {
        session: {
          env: {
            LOCAL: "true",
          },
        },
      },
    }),
    "utf-8",
  )

  const action = await resolveAction("global", tempDir)

  assert.equal(action.prompt, "Use the global action.\n")
  assert.deepEqual(action.config, {
    session: {
      agent: "pi-acp",
      env: {
        GLOBAL: "true",
      },
    },
  })
})

test("resolveAction rejects frontmatter-based actions", async () => {
  process.env.HOME = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-home-"))
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
      session: {
        env: {
          ACTION: "true",
        },
      },
    },
    path: "/tmp/review.md",
  }

  const params = buildActionSessionParams(action, {
    cwd: "/tmp/caller-cwd",
    systemPrompt: "Start with repo conventions.",
  })

  assert.equal(params.cwd, "/tmp/caller-cwd")
  assert.equal(params.systemPrompt, "Start with repo conventions.")
  assert.deepEqual(params.env, {
    ACTION: "true",
  })
})
