import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "bun:test"

let tmpDir = ""
let db: typeof import("../src/db/index.js").db
let loops: typeof import("../src/db/schema.js").loops
let sessions: typeof import("../src/db/schema.js").sessions
let LoopStorage: typeof import("../src/loop.js").LoopStorage
let SessionStorage: typeof import("../src/session.js").SessionStorage
const previousHome = process.env.HOME

describe("Database Storage (Session & Loop)", () => {
  beforeAll(async () => {
    tmpDir = await mkdtemp(join(tmpdir(), "goddard-db-test-"))
    process.env.HOME = tmpDir

    ;[{ db }, { loops, sessions }, { LoopStorage }, { SessionStorage }] = await Promise.all([
      import("../src/db/index.js"),
      import("../src/db/schema.js"),
      import("../src/loop.js"),
      import("../src/session.js"),
    ])
  })

  beforeEach(async () => {
    await db.delete(loops)
    await db.delete(sessions)
  })

  afterAll(async () => {
    process.env.HOME = previousHome
    await rm(tmpDir, { recursive: true, force: true })
  })

  describe("SessionStorage", () => {
    it("creates and retrieves a session", async () => {
      const now = new Date()
      await SessionStorage.create({
        id: "sess-1",
        acpId: "acp-1",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
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

    it("updates a session", async () => {
      const now = new Date()
      await SessionStorage.create({
        id: "sess-1",
        acpId: "acp-1",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        createdAt: now,
        updatedAt: now,
      })

      await SessionStorage.update("sess-1", { status: "active" })

      const retrieved = await SessionStorage.get("sess-1")
      expect(retrieved?.status).toBe("active")
    })

    it("lists sessions", async () => {
      const now = new Date()
      await SessionStorage.create({
        id: "sess-1",
        acpId: "acp-1",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        createdAt: now,
        updatedAt: now,
      })

      await SessionStorage.create({
        id: "sess-2",
        acpId: "acp-2",
        status: "active",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        createdAt: now,
        updatedAt: now,
      })

      const list = await SessionStorage.listAll()
      expect(list.length).toBe(2)
      const ids = list.map((record) => record.id)
      expect(ids).toContain("sess-1")
      expect(ids).toContain("sess-2")
    })

    it("filters sessions by repository and pull request", async () => {
      const now = new Date()
      await SessionStorage.create({
        id: "sess-1",
        acpId: "acp-1",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        repository: "acme/widgets",
        prNumber: 12,
        createdAt: now,
        updatedAt: now,
      })

      await SessionStorage.create({
        id: "sess-2",
        acpId: "acp-2",
        status: "active",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        repository: "acme/widgets",
        prNumber: 99,
        createdAt: now,
        updatedAt: now,
      })

      await SessionStorage.create({
        id: "sess-3",
        acpId: "acp-3",
        status: "done",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        repository: "other/repo",
        prNumber: 12,
        createdAt: now,
        updatedAt: now,
      })

      const repositorySessions = await SessionStorage.listByRepository("acme/widgets")
      expect(repositorySessions.map((record) => record.id).sort()).toEqual(["sess-1", "sess-2"])

      const prSessions = await SessionStorage.listByRepositoryPr("acme/widgets", 12)
      expect(prSessions.map((record) => record.id)).toEqual(["sess-1"])
    })

    it("lists recent sessions with a stable cursor", async () => {
      await SessionStorage.create({
        id: "sess-a",
        acpId: "acp-a",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        createdAt: new Date("2026-01-01T00:00:00.000Z"),
        updatedAt: new Date("2026-01-01T00:00:01.000Z"),
      })
      await SessionStorage.create({
        id: "sess-b",
        acpId: "acp-b",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        createdAt: new Date("2026-01-01T00:00:00.000Z"),
        updatedAt: new Date("2026-01-01T00:00:02.000Z"),
      })
      await SessionStorage.create({
        id: "sess-c",
        acpId: "acp-c",
        status: "idle",
        agentName: "test-agent",
        cwd: "/tmp",
        mcpServers: [],
        createdAt: new Date("2026-01-01T00:00:00.000Z"),
        updatedAt: new Date("2026-01-01T00:00:02.000Z"),
      })

      const firstPage = await SessionStorage.listRecent({ limit: 2 })
      expect(firstPage.map((record) => record.id)).toEqual(["sess-c", "sess-b"])

      const lastRecord = firstPage.at(-1)
      const secondPage = await SessionStorage.listRecent({
        limit: 2,
        cursor: {
          updatedAt: lastRecord?.updatedAt ?? new Date(0),
          id: lastRecord?.id ?? "",
        },
      })

      expect(secondPage.map((record) => record.id)).toEqual(["sess-a"])
    })
  })

  describe("LoopStorage", () => {
    it("creates and retrieves a loop", async () => {
      const now = new Date()
      await LoopStorage.create({
        id: "loop-1",
        agent: "test-agent",
        systemPrompt: "You are helpful",
        displayName: "Test Loop",
        cwd: "/tmp",
        mcpServers: [],
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
      const now = new Date()
      await LoopStorage.create({
        id: "loop-1",
        agent: "test-agent",
        systemPrompt: "You are helpful",
        displayName: "Test Loop",
        cwd: "/tmp",
        mcpServers: [],
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
