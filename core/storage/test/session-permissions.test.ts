import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { getSessionPermissionsPath } from "../src/paths.ts"
import { SessionPermissionsStorage } from "../src/session-permissions.ts"

vi.mock("../src/paths.js", async (importOriginal): Promise<typeof import("../src/paths.ts")> => {
  const actual = await importOriginal<typeof import("../src/paths.ts")>()
  return {
    ...actual,
    getSessionPermissionsPath: vi.fn<typeof actual.getSessionPermissionsPath>(),
  }
})

describe("SessionPermissionsStorage", () => {
  let tmpDir: string

  beforeEach(async () => {
    tmpDir = await mkdtemp(join(tmpdir(), "goddard-session-permissions-test-"))
    vi.mocked(getSessionPermissionsPath).mockReturnValue(join(tmpDir, "session-permissions.json"))
  })

  afterEach(async () => {
    await rm(tmpDir, { recursive: true, force: true })
    vi.resetAllMocks()
  })

  it("creates and retrieves a session permissions record", async () => {
    const record = await SessionPermissionsStorage.create({
      sessionId: "sess-1",
      token: "tok-1",
      owner: "goddard-ai",
      repo: "storage",
      allowedPrNumbers: [1, 2],
    })

    expect(record.sessionId).toBe("sess-1")
    expect(record.createdAt).toBeDefined()

    const retrieved = await SessionPermissionsStorage.get("sess-1")
    expect(retrieved).toBeDefined()
    expect(retrieved?.sessionId).toBe("sess-1")
    expect(retrieved?.token).toBe("tok-1")
    expect(retrieved?.allowedPrNumbers).toEqual([1, 2])
  })

  it("returns null when record does not exist", async () => {
    const retrieved = await SessionPermissionsStorage.get("non-existent")
    expect(retrieved).toBeNull()
  })

  it("lists all session permissions records", async () => {
    await SessionPermissionsStorage.create({
      sessionId: "sess-1",
      token: "tok-1",
      owner: "goddard-ai",
      repo: "storage",
      allowedPrNumbers: [],
    })

    await SessionPermissionsStorage.create({
      sessionId: "sess-2",
      token: "tok-2",
      owner: "goddard-ai",
      repo: "backend",
      allowedPrNumbers: [1],
    })

    const list = await SessionPermissionsStorage.list()
    expect(list.length).toBe(2)
    const ids = list.map((r) => r.sessionId)
    expect(ids).toContain("sess-1")
    expect(ids).toContain("sess-2")
  })

  it("gets record by token", async () => {
    await SessionPermissionsStorage.create({
      sessionId: "sess-1",
      token: "tok-1",
      owner: "goddard-ai",
      repo: "storage",
      allowedPrNumbers: [],
    })

    await SessionPermissionsStorage.create({
      sessionId: "sess-2",
      token: "tok-2",
      owner: "goddard-ai",
      repo: "backend",
      allowedPrNumbers: [],
    })

    const retrieved = await SessionPermissionsStorage.getByToken("tok-2")
    expect(retrieved).toBeDefined()
    expect(retrieved?.sessionId).toBe("sess-2")
  })

  it("returns null when getting by non-existent token", async () => {
    const retrieved = await SessionPermissionsStorage.getByToken("non-existent")
    expect(retrieved).toBeNull()
  })

  it("adds an allowed PR number to an existing record idempotently", async () => {
    await SessionPermissionsStorage.create({
      sessionId: "sess-1",
      token: "tok-1",
      owner: "goddard-ai",
      repo: "storage",
      allowedPrNumbers: [1],
    })

    await SessionPermissionsStorage.addAllowedPr("sess-1", 2)
    let retrieved = await SessionPermissionsStorage.get("sess-1")
    expect(retrieved?.allowedPrNumbers).toEqual([1, 2])

    // Should not add duplicate
    await SessionPermissionsStorage.addAllowedPr("sess-1", 2)
    retrieved = await SessionPermissionsStorage.get("sess-1")
    expect(retrieved?.allowedPrNumbers).toEqual([1, 2])
  })

  it("revokes an existing record", async () => {
    await SessionPermissionsStorage.create({
      sessionId: "sess-1",
      token: "tok-1",
      owner: "goddard-ai",
      repo: "storage",
      allowedPrNumbers: [1],
    })

    let retrieved = await SessionPermissionsStorage.get("sess-1")
    expect(retrieved).toBeDefined()

    await SessionPermissionsStorage.revoke("sess-1")

    retrieved = await SessionPermissionsStorage.get("sess-1")
    expect(retrieved).toBeNull()
  })
})
