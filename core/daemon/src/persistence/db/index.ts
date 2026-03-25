import Database from "better-sqlite3"
import { getDatabasePath, getGoddardGlobalDir } from "@goddard-ai/paths"
import { BetterSQLite3Database, drizzle } from "drizzle-orm/better-sqlite3"
import { migrate } from "drizzle-orm/better-sqlite3/migrator"
import fs from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import * as schema from "./schema.ts"

/** Drizzle database handle for daemon-owned SQLite persistence. */
export type DaemonDatabase = BetterSQLite3Database<typeof schema>

/** Returns the daemon database after opening the file and applying schema migrations. */
export const getDatabaseInstance = (() => {
  let databasePromise: Promise<DaemonDatabase> | null = null

  return (): Promise<DaemonDatabase> => {
    databasePromise ??= initializeDatabase().catch((error) => {
      databasePromise = null
      throw error
    })
    return databasePromise
  }
})()

/** Opens the SQLite database and synchronizes it to the declared Drizzle schema. */
async function initializeDatabase(): Promise<DaemonDatabase> {
  fs.mkdirSync(getGoddardGlobalDir(), { recursive: true })

  const client = new Database(getDatabasePath())
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

/** Applies checked-in SQL migrations to the live SQLite database. */
function applySchemaMigrations(database: DaemonDatabase): void {
  migrate(database, {
    migrationsFolder: resolve(dirname(fileURLToPath(import.meta.url)), "../../../drizzle"),
  })
}
