import { drizzle } from "drizzle-orm/better-sqlite3"
import Database from "better-sqlite3"
import path from "node:path"
import os from "node:os"
import fs from "node:fs"
import * as schema from "./schema.js"
import { eq } from "drizzle-orm"

const dir = path.join(os.homedir(), ".goddard")
if (!fs.existsSync(dir)) {
  fs.mkdirSync(dir, { recursive: true })
}

let sqlite: Database.Database | undefined
export let db: ReturnType<typeof drizzle> | undefined

try {
  sqlite = new Database(path.join(dir, "session.db"))
  db = drizzle({ client: sqlite, schema })
} catch (e) {
  console.warn("Failed to initialize database", e)
}

export * as dbSchema from "./schema.js"

export async function insertMessage(sessionId: string, type: string, payload: string) {
  if (!db) throw new Error("Database not initialized")
  await db.insert(schema.messages).values({
    sessionId,
    type,
    payload,
    createdAt: new Date(),
  })
}

export async function getMessagesBySessionId(sessionId: string) {
  if (!db) throw new Error("Database not initialized")
  return await db
    .select()
    .from(schema.messages)
    .where(eq(schema.messages.sessionId, sessionId))
    .orderBy(schema.messages.createdAt)
}
