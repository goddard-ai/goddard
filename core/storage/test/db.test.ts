import { describe, it, expect, beforeEach, afterEach, vi } from "vitest"
import { SessionStorage } from "../src/session.js"
import { LoopStorage } from "../src/loop.js"
import { getDatabasePath } from "../src/paths.js"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { mkdtemp, rm } from "node:fs/promises"

vi.mock("../src/paths.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/paths.js")>()
  return {
    ...actual,
    getDatabasePath: vi.fn(),
  }
})

describe("Database Storage (Session & Loop)", () => {
  let tmpDir: string
  let dbPath: string

  beforeEach(async () => {
    tmpDir = await mkdtemp(join(tmpdir(), "goddard-db-test-"))
    dbPath = join(tmpDir, "goddard.db")
    vi.mocked(getDatabasePath).mockReturnValue(dbPath)
  })

  afterEach(async () => {
    await rm(tmpDir, { recursive: true, force: true })
    vi.resetModules()
    vi.resetAllMocks()
  })

  describe("SessionStorage", () => {
    it("creates and retrieves a session", async () => {
      const { SessionStorage } = await import("../src/session.js")

      const now = new Date()
      await SessionStorage.create({
        id: "sess-1",
        acpId: "acp-1",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: "[]",
        createdAt: now,
        updatedAt: now,
      })

      const retrieved = await SessionStorage.get("sess-1")
      expect(retrieved).toBeDefined()
      expect(retrieved?.id).toBe("sess-1")
      expect(retrieved?.acpId).toBe("acp-1")

      const byAcpId = await SessionStorage.getByAcpId("acp-1")
      expect(byAcpId).toBeDefined()
      expect(byAcpId?.id).toBe("sess-1")
    })

    it("lists sessions", async () => {
      const { SessionStorage } = await import("../src/session.js")

      const now = new Date()
      await SessionStorage.create({
        id: "sess-1",
        acpId: "acp-1",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: "[]",
        createdAt: now,
        updatedAt: now,
      })

      await SessionStorage.create({
        id: "sess-2",
        acpId: "acp-2",
        status: "running",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: "[]",
        createdAt: now,
        updatedAt: now,
      })

      const list = await SessionStorage.list()
      expect(list.length).toBe(2)
      const ids = list.map(r => r.id)
      expect(ids).toContain("sess-1")
      expect(ids).toContain("sess-2")
    })

    it("updates a session", async () => {
      const { SessionStorage } = await import("../src/session.js")

      const now = new Date()
      await SessionStorage.create({
        id: "sess-1",
        acpId: "acp-1",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: "[]",
        createdAt: now,
        updatedAt: now,
      })

      await SessionStorage.update("sess-1", { status: "running" })

      const retrieved = await SessionStorage.get("sess-1")
      expect(retrieved?.status).toBe("running")
    })
  })

  describe("LoopStorage", () => {
    it("creates and retrieves a loop", async () => {
      const { LoopStorage } = await import("../src/loop.js")

      const now = new Date()
      await LoopStorage.create({
        id: "loop-1",
        agent: "test-agent",
        systemPrompt: "You are helpful",
        displayName: "Test Loop",
        cwd: "/tmp",
        mcpServers: "[]",
        gitRemote: "origin",
        createdAt: now,
        updatedAt: now,
      })

      const retrieved = await LoopStorage.get("loop-1")
      expect(retrieved).toBeDefined()
      expect(retrieved?.id).toBe("loop-1")
      expect(retrieved?.agent).toBe("test-agent")
    })

    it("updates a loop", async () => {
      const { LoopStorage } = await import("../src/loop.js")

      const now = new Date()
      await LoopStorage.create({
        id: "loop-1",
        agent: "test-agent",
        systemPrompt: "You are helpful",
        displayName: "Test Loop",
        cwd: "/tmp",
        mcpServers: "[]",
        gitRemote: "origin",
        createdAt: now,
        updatedAt: now,
      })

      await LoopStorage.update("loop-1", { displayName: "Updated Loop" })

      const retrieved = await LoopStorage.get("loop-1")
      expect(retrieved?.displayName).toBe("Updated Loop")
    })
  })
})
