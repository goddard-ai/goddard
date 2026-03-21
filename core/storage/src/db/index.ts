import { Database } from "bun:sqlite"
import { drizzle } from "drizzle-orm/bun-sqlite"
import fs from "node:fs"
import { getDatabasePath, getGoddardGlobalDir } from "../paths.js"
import * as schema from "./schema.js"

const dir = getGoddardGlobalDir()
if (!fs.existsSync(dir)) {
  fs.mkdirSync(dir, { recursive: true })
}

/** Creates the shared storage database handle backed by Bun's native SQLite runtime. */
function createDatabase() {
  const client = new Database(getDatabasePath(), { create: true })
  ensureSchema(client)
  return drizzle({ client, schema })
}

/** Shared Drizzle database handle for storage-backed persistence. */
export const db = createDatabase()

/** Ensures the durable SQL tables and indexes exist before storage access begins. */
function ensureSchema(client: Database): void {
  client.exec(`
    CREATE TABLE IF NOT EXISTS sessions (
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

    CREATE TABLE IF NOT EXISTS loops (
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
  `)

  ensureSessionRepositoryColumns(client)
}

/** Ensures direct repository and PR columns plus indexes exist for session queries. */
function ensureSessionRepositoryColumns(client: Database): void {
  const sessionColumns = new Set(
    (client.query("PRAGMA table_info(sessions)").all() as Array<{ name: string }>).map(
      (column) => column.name,
    ),
  )

  if (!sessionColumns.has("repository")) {
    client.exec("ALTER TABLE sessions ADD COLUMN repository TEXT;")
  }

  if (!sessionColumns.has("prNumber")) {
    client.exec("ALTER TABLE sessions ADD COLUMN prNumber INTEGER;")
  }

  client.exec(`
    CREATE INDEX IF NOT EXISTS sessions_repository_idx ON sessions (repository);
    CREATE INDEX IF NOT EXISTS sessions_repository_pr_number_idx ON sessions (repository, prNumber);
  `)
}
