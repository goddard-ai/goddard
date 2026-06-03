import { spawnSync } from "node:child_process"
import { mkdtemp, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { agentBinaryPlatforms } from "@goddard-ai/schema/agent-distribution"
import * as acp from "acp-client/protocol"
import { afterEach, expect, test, vi } from "bun:test"

import {
  createWorktreeBranchReadableId,
  injectSystemPrompt,
  resolveAgentProcessSpec,
  resolveAvailableWorktreeBranchName,
  resolveWorktreeBranchName,
  resolveWorktreeBranchPrefix,
} from "../src/daemon/manager.ts"

const cleanupDirs: string[] = []
const originalHome = process.env.HOME
const originalFetch = globalThis.fetch

afterEach(async () => {
  globalThis.fetch = originalFetch

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  while (cleanupDirs.length > 0) {
    await rm(cleanupDirs.pop()!, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 })
  }
})

test("resolveAgentProcessSpec installs archive-backed binaries into the global cache", async () => {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-home-"))
  cleanupDirs.push(homeDir)
  process.env.HOME = homeDir

  const fetchMock = vi.fn(async () => new Response("#!/bin/sh\nexit 0\n", { status: 200 }))
  globalThis.fetch = fetchMock as unknown as typeof fetch

  const binaryTarget = {
    archive:
      "https://raw.githubusercontent.com/agentclientprotocol/registry/refs/heads/main/codex-acp/agent",
    cmd: "bin/agent",
    args: ["--serve"],
    env: {
      FOO: "bar",
    },
  }

  const agent = {
    id: "node-agent",
    name: "Node Agent",
    version: "1.0.0",
    description: "Archive-backed ACP test agent.",
    distribution: {
      binary: Object.fromEntries(
        agentBinaryPlatforms.map((platform) => [platform, binaryTarget]),
      ) as Record<(typeof agentBinaryPlatforms)[number], typeof binaryTarget>,
    },
  }

  const firstSpec = await resolveAgentProcessSpec(agent)
  const secondSpec = await resolveAgentProcessSpec(agent)

  expect(fetchMock).toHaveBeenCalledTimes(1)
  expect(firstSpec).toEqual(secondSpec)
  expect(firstSpec.args).toEqual(["--serve"])
  expect(firstSpec.env).toEqual({ FOO: "bar" })
  expect(firstSpec.cmd.startsWith(join(homeDir, ".goddard", "binaries"))).toBe(true)
  expect(firstSpec.cmd.endsWith(join("bin", "agent"))).toBe(true)
  await expect(stat(firstSpec.cmd)).resolves.toBeTruthy()
})

test("injectSystemPrompt leaves prompts unchanged when the daemon system prompt is empty", () => {
  const request = {
    sessionId: "acp-session-1",
    prompt: [{ type: "text", text: "Say hello." }],
  } satisfies acp.PromptRequest

  expect(injectSystemPrompt(request, "")).toEqual(request)
})

test("injectSystemPrompt prepends the daemon system prompt with the goddard tag name", () => {
  const request = {
    sessionId: "acp-session-1",
    prompt: [{ type: "text", text: "Say hello." }],
  } satisfies acp.PromptRequest

  expect(injectSystemPrompt(request, "Keep responses short.")).toEqual({
    sessionId: "acp-session-1",
    prompt: [
      {
        type: "text",
        text: '<system-prompt name="goddard">Keep responses short.</system-prompt>',
      },
      { type: "text", text: "Say hello." },
    ],
  } satisfies acp.PromptRequest)
})

test("resolveWorktreeBranchPrefix defaults to a local-user branch prefix", () => {
  expect(resolveWorktreeBranchPrefix()).toMatch(/^[a-z0-9._-]+(?:\/[a-z0-9._-]+)*$/)
})

test("createWorktreeBranchReadableId creates an easy-to-type word id", () => {
  expect(createWorktreeBranchReadableId()).toMatch(/^[a-z]+-[a-z]+-[a-z]+$/)
})

test("resolveWorktreeBranchName joins the configured branch prefix with a readable id", () => {
  expect(resolveWorktreeBranchName({ readableId: "Cape Town", branchPrefix: "agent" })).toBe(
    "agent/cape-town",
  )
})

test("resolveWorktreeBranchName uses host-scoped pull request branch names", () => {
  expect(
    resolveWorktreeBranchName({
      readableId: "quito",
      repository: "github.com/acme/widgets",
      prNumber: 123,
      branchPrefix: "agent",
    }),
  ).toBe("github.com/pr/123")
})

test("resolveWorktreeBranchName defaults pull request branches to GitHub host", () => {
  expect(
    resolveWorktreeBranchName({
      readableId: "quito",
      repository: "acme/widgets",
      prNumber: 123,
    }),
  ).toBe("github.com/pr/123")
})

test("resolveAvailableWorktreeBranchName skips existing generated branches", async () => {
  const repoDir = await createRepoFixture()

  const firstBranch = await resolveAvailableWorktreeBranchName({
    cwd: repoDir,
    branchPrefix: "agent",
  })
  runGit(repoDir, ["branch", firstBranch])

  const secondBranch = await resolveAvailableWorktreeBranchName({
    cwd: repoDir,
    branchPrefix: "agent",
  })

  expect(secondBranch).toMatch(/^agent\/[a-z]+-[a-z]+-[a-z]+$/)
  expect(secondBranch).not.toBe(firstBranch)
})

async function createRepoFixture() {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-manager-repo-"))
  cleanupDirs.push(repoDir)

  await writeFile(join(repoDir, "package.json"), JSON.stringify({ name: "repo" }), "utf-8")

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "."])
  runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })

  expect(result.status).toBe(0)
}
