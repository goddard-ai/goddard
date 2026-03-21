import { describe, it, expect, beforeEach, afterEach, vi } from "vitest"
import { SessionStateStorage, SessionDiagnosticEvent } from "../src/session-state.js"
import { getGoddardGlobalDir } from "../src/paths.js"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { mkdtemp, rm } from "node:fs/promises"

vi.mock("../src/paths.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/paths.js")>()
  return {
    ...actual,
    getGoddardGlobalDir: vi.fn(),
  }
})

describe("SessionStateStorage", () => {
  let tmpDir: string

  beforeEach(async () => {
    tmpDir = await mkdtemp(join(tmpdir(), "goddard-session-state-test-"))
    vi.mocked(getGoddardGlobalDir).mockReturnValue(tmpDir)
  })

  afterEach(async () => {
    await rm(tmpDir, { recursive: true, force: true })
    vi.resetAllMocks()
  })

  it("creates and retrieves a session state record", async () => {
    const record = await SessionStateStorage.create({
      sessionId: "sess-1",
      acpId: "acp-1",
      connectionMode: "live",
      history: [],
      diagnostics: [],
      activeDaemonSession: true,
    })

    expect(record.sessionId).toBe("sess-1")
    expect(record.createdAt).toBeDefined()
    expect(record.updatedAt).toBeDefined()

    const retrieved = await SessionStateStorage.get("sess-1")
    expect(retrieved).toBeDefined()
    expect(retrieved?.sessionId).toBe("sess-1")
    expect(retrieved?.acpId).toBe("acp-1")
  })

  it("lists all session state records", async () => {
    await SessionStateStorage.create({
      sessionId: "sess-1",
      acpId: "acp-1",
      connectionMode: "live",
      history: [],
      diagnostics: [],
      activeDaemonSession: true,
    })

    await SessionStateStorage.create({
      sessionId: "sess-2",
      acpId: "acp-2",
      connectionMode: "history",
      history: [],
      diagnostics: [],
      activeDaemonSession: false,
    })

    const list = await SessionStateStorage.list()
    expect(list.length).toBe(2)
    const ids = list.map((r) => r.sessionId)
    expect(ids).toContain("sess-1")
    expect(ids).toContain("sess-2")
  })

  it("updates a session state record", async () => {
    await SessionStateStorage.create({
      sessionId: "sess-1",
      acpId: "acp-1",
      connectionMode: "live",
      history: [],
      diagnostics: [],
      activeDaemonSession: true,
    })

    const updated = await SessionStateStorage.update("sess-1", {
      connectionMode: "history",
      activeDaemonSession: false,
    })

    expect(updated).toBeDefined()
    expect(updated?.connectionMode).toBe("history")
    expect(updated?.activeDaemonSession).toBe(false)

    const retrieved = await SessionStateStorage.get("sess-1")
    expect(retrieved?.connectionMode).toBe("history")
  })

  it("returns null when updating non-existent record", async () => {
    const updated = await SessionStateStorage.update("non-existent", {
      connectionMode: "history",
    })
    expect(updated).toBeNull()
  })

  it("appends history to a record", async () => {
    await SessionStateStorage.create({
      sessionId: "sess-1",
      acpId: "acp-1",
      connectionMode: "live",
      history: [],
      diagnostics: [],
      activeDaemonSession: true,
    })

    const mockMessage = { type: "text", text: "hello" } as any
    const updated = await SessionStateStorage.appendHistory("sess-1", mockMessage)

    expect(updated?.history.length).toBe(1)
    expect(updated?.history[0]).toEqual(mockMessage)

    const retrieved = await SessionStateStorage.get("sess-1")
    expect(retrieved?.history.length).toBe(1)
  })

  it("appends diagnostic to a record", async () => {
    await SessionStateStorage.create({
      sessionId: "sess-1",
      acpId: "acp-1",
      connectionMode: "live",
      history: [],
      diagnostics: [],
      activeDaemonSession: true,
    })

    const diagnostic: SessionDiagnosticEvent = {
      type: "error",
      at: new Date().toISOString(),
      sessionId: "sess-1",
      detail: { msg: "failed" },
    }

    const updated = await SessionStateStorage.appendDiagnostic("sess-1", diagnostic)

    expect(updated?.diagnostics.length).toBe(1)
    expect(updated?.diagnostics[0]).toEqual(diagnostic)

    const retrieved = await SessionStateStorage.get("sess-1")
    expect(retrieved?.diagnostics.length).toBe(1)
  })

  it("removes a record", async () => {
    await SessionStateStorage.create({
      sessionId: "sess-1",
      acpId: "acp-1",
      connectionMode: "live",
      history: [],
      diagnostics: [],
      activeDaemonSession: true,
    })

    let retrieved = await SessionStateStorage.get("sess-1")
    expect(retrieved).toBeDefined()

    await SessionStateStorage.remove("sess-1")

    retrieved = await SessionStateStorage.get("sess-1")
    expect(retrieved).toBeNull()
  })
})
