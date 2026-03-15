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
let _db: ReturnType<typeof drizzle> | null = null;
export const db = new Proxy({} as ReturnType<typeof drizzle>, {
  get(target, prop) {
    if (!_db) {
      _db = drizzle({
        client: new Database(getDatabasePath()),
        schema,
      });
    }
    return (_db as any)[prop];
  }
});
