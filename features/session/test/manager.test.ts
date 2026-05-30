import { mkdtemp, rm, stat } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { agentBinaryPlatforms } from "@goddard-ai/schema/agent-distribution"
import * as acp from "acp-client/protocol"
import { afterEach, expect, test, vi } from "bun:test"

import {
  detectBinaryTargetPayloadFormat,
  installBinaryTargetPayload,
  resolveInstalledBinaryCommand,
} from "../src/daemon/archive.ts"
import { injectSystemPrompt, resolveAgentProcessSpec } from "../src/daemon/manager.ts"

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
