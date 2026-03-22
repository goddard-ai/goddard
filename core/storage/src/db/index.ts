import Database from "better-sqlite3"
import { BetterSQLite3Database, drizzle } from "drizzle-orm/better-sqlite3"
import { migrate } from "drizzle-orm/better-sqlite3/migrator"
import fs from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { getDatabasePath, getGoddardGlobalDir } from "../paths.js"
import * as schema from "./schema.js"

/** Drizzle database handle for storage-owned SQLite persistence. */
export type StorageDatabase = BetterSQLite3Database<typeof schema>

/** Returns the storage database after opening the file and applying schema migrations. */
export const getDatabaseInstance = (() => {
  let databasePromise: Promise<StorageDatabase> | null = null

  return (): Promise<StorageDatabase> => {
    databasePromise ??= initializeDatabase().catch((error) => {
      databasePromise = null
      throw error
    })
    return databasePromise
  }
})()

/** Opens the SQLite database and synchronizes it to the declared Drizzle schema. */
async function initializeDatabase(): Promise<StorageDatabase> {
  fs.mkdirSync(getGoddardGlobalDir(), { recursive: true })

  const client = new Database(getDatabasePath())
  const database: StorageDatabase = drizzle({
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
function applySchemaMigrations(database: StorageDatabase): void {
  migrate(database, {
    migrationsFolder: resolve(dirname(fileURLToPath(import.meta.url)), "../../drizzle"),
  })
}
