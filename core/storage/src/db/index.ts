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

    CREATE TABLE IF NOT EXISTS session_permissions (
      sessionId TEXT PRIMARY KEY REFERENCES sessions(id),
      token TEXT NOT NULL UNIQUE,
      owner TEXT NOT NULL,
      repo TEXT NOT NULL,
      allowedPrNumbers TEXT NOT NULL,
      createdAt TEXT NOT NULL
    );
  `)
}
