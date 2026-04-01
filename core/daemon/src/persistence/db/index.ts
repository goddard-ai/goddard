import { Database } from "bun:sqlite"
import { getDatabasePath, getGoddardGlobalDir } from "@goddard-ai/paths/node"
import { BunSQLiteDatabase, drizzle } from "drizzle-orm/bun-sqlite"
import { migrate } from "drizzle-orm/bun-sqlite/migrator"
import fs from "node:fs"
import { dirname, join } from "node:path"
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
    migrationsFolder: resolveMigrationsFolder(),
  })
}

/** Resolves the checked-in migration directory from either source or bundled package output. */
function resolveMigrationsFolder(): string {
  const packageRoot = findDaemonPackageRoot(dirname(fileURLToPath(import.meta.url)))
  return join(packageRoot, "drizzle")
}

/** Walks up from the current module to the daemon package root that owns the migration files. */
function findDaemonPackageRoot(startDir: string): string {
  let currentDir = startDir

  while (true) {
    const packageJsonPath = join(currentDir, "package.json")
    if (fs.existsSync(packageJsonPath)) {
      return currentDir
    }

    const parentDir = dirname(currentDir)
    if (parentDir === currentDir) {
      throw new Error(`Unable to locate daemon package root from ${startDir}`)
    }

    currentDir = parentDir
  }
}
