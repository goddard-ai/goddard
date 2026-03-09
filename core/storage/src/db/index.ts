import Database from "better-sqlite3"
import { drizzle } from "drizzle-orm/better-sqlite3"
import fs from "node:fs"
import { getDatabasePath, getGoddardGlobalDir } from "../paths.js"
import * as schema from "./schema.js"

const dir = getGoddardGlobalDir()
if (!fs.existsSync(dir)) {
  fs.mkdirSync(dir, { recursive: true })
}

export const db = drizzle({
  client: new Database(getDatabasePath()),
  schema,
})
