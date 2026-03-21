import Database from "better-sqlite3"
import { drizzle } from "drizzle-orm/better-sqlite3"
import fs from "node:fs"
import { getDatabasePath, getGoddardGlobalDir } from "../paths.js"
import * as schema from "./schema.js"

const dir = getGoddardGlobalDir()
if (!fs.existsSync(dir)) {
  fs.mkdirSync(dir, { recursive: true })
}

// Lazy init the DB to avoid better-sqlite3 loading issues in environments where it's imported but not used, or bindings not found at test time.
let _db: ReturnType<typeof drizzle> | null = null
export const db = new Proxy({} as ReturnType<typeof drizzle>, {
  get(target, prop) {
    if (!_db) {
      const client = new Database(getDatabasePath())
      ensureSchema(client)
      _db = drizzle({
        client,
        schema,
      })
    }
    return (_db as any)[prop]
  },
})

function ensureSchema(client: Database.Database): void {
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
function ensureSessionRepositoryColumns(client: Database.Database): void {
  const sessionColumns = new Set(
    (client.prepare("PRAGMA table_info(sessions)").all() as Array<{ name: string }>).map(
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
