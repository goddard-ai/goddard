import * as assert from "node:assert/strict"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { afterEach, test, vi } from "vitest"

vi.mock("@goddard-ai/session", () => ({
  runAgent: vi.fn(),
}))

import { dedent } from "radashi"
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

test("resolveAction strips frontmatter from markdown actions", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-md-"))
  const actionsDir = path.join(tempDir, ".goddard", "actions")

  await fs.mkdir(actionsDir, { recursive: true })
  await fs.writeFile(
    path.join(actionsDir, "review.md"),
    dedent`
      ---
      oneShot: false
      agent:
        type: binary
        cmd: custom-agent
      ---
      Review the current diff carefully.
    `,
    "utf-8",
  )

  const action = await resolveAction("review", tempDir)

  assert.equal(action.prompt, "Review the current diff carefully.")
  assert.deepEqual(action.config, {
    oneShot: false,
    agent: {
      type: "binary",
      cmd: "custom-agent",
    },
  })
})

test("resolveAction loads folder actions and merges prompt frontmatter with config.json", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-dir-"))
  const actionDir = path.join(tempDir, ".goddard", "actions", "ship-it")

  await fs.mkdir(actionDir, { recursive: true })
  await fs.writeFile(
    path.join(actionDir, "prompt.md"),
    dedent`
      ---
      oneShot: true
      systemPrompt: "Start with the checklist."
      ---
      Ship the change.
    `,
    "utf-8",
  )
  await fs.writeFile(
    path.join(actionDir, "config.json"),
    JSON.stringify({
      oneShot: false,
      cwd: "/tmp/override",
      mcpServers: [{ name: "filesystem" }],
    }),
    "utf-8",
  )

  const action = await resolveAction("ship-it", tempDir)

  assert.equal(action.prompt, "Ship the change.")
  assert.deepEqual(action.config, {
    oneShot: false,
    systemPrompt: "Start with the checklist.",
    cwd: "/tmp/override",
    mcpServers: [{ name: "filesystem" }],
  })
})

test("resolveAction falls back to the global action directory", async () => {
  const tempHome = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-home-"))
  process.env.HOME = tempHome

  const actionDir = path.join(tempHome, ".goddard", "actions")
  await fs.mkdir(actionDir, { recursive: true })
  await fs.writeFile(path.join(actionDir, "global.md"), "Use the global action.\n", "utf-8")

  const action = await resolveAction("global", await fs.mkdtemp(path.join(os.tmpdir(), "cwd-")))
  assert.equal(action.prompt, "Use the global action.\n")
})

test("buildActionSessionParams lets action config override defaults", () => {
  const action: ResolvedAgentAction = {
    prompt: "Review the pull request.",
    config: {
      oneShot: false,
      systemPrompt: "Follow the security checklist.",
      cwd: "/tmp/action-cwd",
    },
    path: "/tmp/review.md",
  }

  const params = buildActionSessionParams(action, {
    cwd: "/tmp/caller-cwd",
    initialPrompt: "Check the latest changes.",
    systemPrompt: "Start with repo conventions.",
  })

  assert.equal(params.cwd, "/tmp/action-cwd")
  assert.equal("oneShot" in params ? params.oneShot : undefined, undefined)
  assert.equal(
    params.systemPrompt,
    ["Follow the security checklist.", "Review the pull request."].join("\n\n"),
  )
})
