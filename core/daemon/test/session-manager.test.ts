import * as acp from "@agentclientprotocol/sdk"
import { agentBinaryPlatforms } from "@goddard-ai/schema/agent-distribution"
import { afterEach, expect, test, vi } from "bun:test"
import { chmod, mkdir, mkdtemp, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { createGzip } from "node:zlib"
import * as tarFs from "tar-fs"

import { resetDb } from "../src/persistence/store.ts"
import {
  detectBinaryTargetPayloadFormat,
  installBinaryTargetPayload,
  resolveInstalledBinaryCommand,
} from "../src/session/archive.ts"
import { injectSystemPrompt, resolveAgentProcessSpec } from "../src/session/manager.ts"

const cleanupDirs: string[] = []
const originalHome = process.env.HOME
const originalFetch = globalThis.fetch
const testZipArchiveBase64 =
  "UEsDBAoAAAAAAKeSdlzihkXDEQAAABEAAAAIABwAb3BlbmNvZGVVVAkAA1prwGlaa8BpdXgLAAEE9QEAAAQUAAAAIyEvYmluL3NoCmV4aXQgMApQSwECHgMKAAAAAACnknZc4oZFwxEAAAARAAAACAAYAAAAAAABAAAA7YEAAAAAb3BlbmNvZGVVVAUAA1prwGl1eAsAAQT1AQAABBQAAABQSwUGAAAAAAEAAQBOAAAAUwAAAAAA"

afterEach(async () => {
  globalThis.fetch = originalFetch
  resetDb({ filename: ":memory:" })

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  while (cleanupDirs.length > 0) {
    await rm(cleanupDirs.pop()!, { recursive: true, force: true })
  }
})

/** Collects a Node readable stream into one in-memory buffer for fetch stubs. */
async function readStream(readable: NodeJS.ReadableStream): Promise<Uint8Array> {
  const chunks: Buffer[] = []
  for await (const chunk of readable) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
  }

  return Buffer.concat(chunks)
}

/** Builds a small `.tar.gz` archive with one top-level package directory and one executable file. */
async function createTestTarGzArchive(): Promise<Uint8Array> {
  const tempDir = await mkdtemp(join(tmpdir(), "goddard-agent-archive-"))
  cleanupDirs.push(tempDir)

  const packageDir = join(tempDir, "node-agent")
  const executablePath = join(packageDir, "bin", "agent")
  await mkdir(join(packageDir, "bin"), { recursive: true })
  await writeFile(executablePath, "#!/bin/sh\nexit 0\n", "utf8")
  await chmod(executablePath, 0o755)

  const gzip = createGzip()
  tarFs.pack(tempDir, { entries: ["node-agent"] }).pipe(gzip)
  return await readStream(gzip)
}

test("detectBinaryTargetPayloadFormat recognizes supported archive and raw binary inputs", () => {
  expect(detectBinaryTargetPayloadFormat("https://example.com/agent.zip")).toBe("zip")
  expect(
    detectBinaryTargetPayloadFormat(
      "https://raw.githubusercontent.com/agentclientprotocol/registry/refs/heads/main/codex-acp/agent.json",
    ),
  ).toBe("raw")
  expect(
    detectBinaryTargetPayloadFormat(
      "https://raw.githubusercontent.com/agentclientprotocol/registry/refs/heads/main/codex-acp/agent.tar.gz",
    ),
  ).toBe("tar.gz")
  expect(detectBinaryTargetPayloadFormat("https://example.com/agent.tgz")).toBe("tgz")
  expect(detectBinaryTargetPayloadFormat("https://example.com/agent.tar.bz2")).toBe("tar.bz2")
  expect(detectBinaryTargetPayloadFormat("https://example.com/agent.tbz2")).toBe("tbz2")
})

test("installBinaryTargetPayload writes raw binaries to the declared command path", async () => {
  const installDir = await mkdtemp(join(tmpdir(), "goddard-agent-install-"))
  cleanupDirs.push(installDir)

  // Explicit exception: remote archive downloads cross a non-local third-party boundary.
  const fetchMock = vi.fn(async () => new Response("#!/bin/sh\nexit 0\n", { status: 200 }))
  globalThis.fetch = fetchMock as unknown as typeof fetch

  await installBinaryTargetPayload({
    archiveUrl:
      "https://raw.githubusercontent.com/agentclientprotocol/registry/refs/heads/main/codex-acp/agent.json",
    cmd: "bin/agent",
    installDir,
  })

  const commandPath = await resolveInstalledBinaryCommand(installDir, "bin/agent")

  expect(fetchMock).toHaveBeenCalledTimes(1)
  await expect(stat(commandPath)).resolves.toBeTruthy()
})

test("installBinaryTargetPayload restores executability for zip payload commands", async () => {
  const installDir = await mkdtemp(join(tmpdir(), "goddard-agent-zip-install-"))
  cleanupDirs.push(installDir)

  const fetchMock = vi.fn(
    async () => new Response(Buffer.from(testZipArchiveBase64, "base64"), { status: 200 }),
  )
  globalThis.fetch = fetchMock as unknown as typeof fetch

  await installBinaryTargetPayload({
    archiveUrl: "https://example.com/opencode-darwin-arm64.zip",
    cmd: "./opencode",
    installDir,
  })

  const commandPath = await resolveInstalledBinaryCommand(installDir, "./opencode")
  const commandStat = await stat(commandPath)

  expect(fetchMock).toHaveBeenCalledTimes(1)
  expect(commandStat.mode & 0o111).not.toBe(0)
})

test("resolveAgentProcessSpec installs archive-backed binaries into the global cache", async () => {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-home-"))
  cleanupDirs.push(homeDir)
  process.env.HOME = homeDir
  resetDb()

  const archiveBody = await createTestTarGzArchive()
  const fetchMock = vi.fn(async () => new Response(Buffer.from(archiveBody), { status: 200 }))
  globalThis.fetch = fetchMock as unknown as typeof fetch

  const binaryTarget = {
    archive:
      "https://raw.githubusercontent.com/agentclientprotocol/registry/refs/heads/main/codex-acp/agent.tar.gz",
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
  expect(firstSpec.cmd.endsWith(join("node-agent", "bin", "agent"))).toBe(true)
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
