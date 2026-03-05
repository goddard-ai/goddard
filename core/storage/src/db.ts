import { createClient } from "@libsql/client";
import { drizzle } from "drizzle-orm/libsql";
import { join } from "node:path";
import { getGoddardGlobalDir } from "./paths.ts";
import * as schema from "./schema.ts";

export function getLocalDbPath(): string {
  return join(getGoddardGlobalDir(), "goddard.db");
}

let _dbInstance: ReturnType<typeof drizzle> | null = null;

export function createLocalDb() {
  if (!_dbInstance) {
    const dbPath = getLocalDbPath();
    const client = createClient({ url: `file:${dbPath}` });
    _dbInstance = drizzle(client, { schema });
  }
  return _dbInstance;
}
