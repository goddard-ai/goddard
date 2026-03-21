import { Database } from "bun:sqlite"
import { getDatabasePath, getGoddardGlobalDir } from "@goddard-ai/paths/node"
import { BunSQLiteDatabase, drizzle } from "drizzle-orm/bun-sqlite"
import { migrate } from "drizzle-orm/bun-sqlite/migrator"
import fs from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import * as schema from "./schema.ts"

/** Drizzle database handle for daemon-owned SQLite persistence. */
export type DaemonDatabase = BunSQLiteDatabase<typeof schema>

/** Opens the SQLite database, applies migrations, and returns the shared Drizzle handle. */
function createDatabase(): DaemonDatabase {
  fs.mkdirSync(getGoddardGlobalDir(), { recursive: true })

  const client = new Database(getDatabasePath(), { create: true })
  const database: DaemonDatabase = drizzle({
    client,
    schema,
  })

  try {
    applySchemaMigrations(database)
    return database
  } catch (error) {
    client.close()
    throw error
  }
}

/** Returns the shared daemon database handle backed by Bun's native SQLite runtime. */
export async function getDatabaseInstance(): Promise<DaemonDatabase> {
  return db
}

/** Shared Drizzle database handle for daemon-owned SQLite persistence. */
const db = createDatabase()

/** Applies checked-in SQL migrations to the live SQLite database. */
function applySchemaMigrations(database: DaemonDatabase): void {
  migrate(database, {
    migrationsFolder: resolve(dirname(fileURLToPath(import.meta.url)), "../../../drizzle"),
  })
}
