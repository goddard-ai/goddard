import Database from "better-sqlite3"
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { getDatabaseInstance } from "../src/db/index.js"
import { getDatabasePath } from "../src/paths.js"

vi.mock("../src/paths.js", async (importOriginal): Promise<typeof import("../src/paths.js")> => {
  const actual = await importOriginal<typeof import("../src/paths.js")>()
  return {
    ...actual,
    getDatabasePath: vi.fn<typeof actual.getDatabasePath>(),
  }
})

describe("storage database migrations", () => {
  let tmpDir: string
  let dbPath: string

  beforeEach(async () => {
    tmpDir = await mkdtemp(join(tmpdir(), "goddard-db-migration-test-"))
    dbPath = join(tmpDir, "goddard.db")
    vi.mocked(getDatabasePath).mockReturnValue(dbPath)
    vi.resetModules()
  })

  afterEach(async () => {
    await rm(tmpDir, { recursive: true, force: true })
    vi.resetAllMocks()
    vi.resetModules()
  })

  it("applies checked-in migrations to a legacy database without losing rows", async () => {
    const legacy = new Database(dbPath)
    legacy.exec(`
      CREATE TABLE sessions (
        id TEXT PRIMARY KEY,
        acpId TEXT NOT NULL UNIQUE,
        status TEXT NOT NULL DEFAULT 'idle',
        agentName TEXT NOT NULL,
        cwd TEXT NOT NULL,
        mcpServers TEXT NOT NULL,
        createdAt INTEGER NOT NULL,
        updatedAt INTEGER NOT NULL,
        errorMessage TEXT,
        blockedReason TEXT,
        initiative TEXT,
        lastAgentMessage TEXT,
        repository TEXT,
        prNumber INTEGER,
        metadata TEXT
      );

      CREATE TABLE loops (
        id TEXT PRIMARY KEY,
        agent TEXT NOT NULL,
        systemPrompt TEXT NOT NULL,
        strategy TEXT,
        displayName TEXT NOT NULL,
        cwd TEXT NOT NULL,
        mcpServers TEXT NOT NULL,
        gitRemote TEXT NOT NULL DEFAULT 'origin',
        createdAt INTEGER NOT NULL,
        updatedAt INTEGER NOT NULL
      );

      CREATE INDEX sessions_repository_idx ON sessions (repository);
      CREATE INDEX sessions_repository_pr_number_idx ON sessions (repository, prNumber);
    `)

    legacy
      .prepare(`
        INSERT INTO sessions (
          id,
          acpId,
          status,
          agentName,
          cwd,
          mcpServers,
          createdAt,
          updatedAt
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      `)
      .run("sess-legacy", "acp-legacy", "idle", "test-agent", "/tmp", "[]", Date.now(), Date.now())
    legacy.close()

    const db = await getDatabaseInstance()

    const tableNames = (
      db.all(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('sessions', 'loops', 'artifacts') ORDER BY name",
      ) as Array<{ name: string }>
    ).map((table) => table.name)
    expect(tableNames).toEqual(["artifacts", "loops", "sessions"])

    expect(
      db.all("SELECT id, acpId FROM sessions WHERE id = 'sess-legacy'") as Array<{
        id: string
        acpId: string
      }>,
    ).toEqual([{ id: "sess-legacy", acpId: "acp-legacy" }])

    ;(db as typeof db & { $client: Database.Database }).$client.close()
  })
})
