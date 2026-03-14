import { eq } from "drizzle-orm"
import { db } from "./db/index.js"
import { sessions } from "./db/schema.js"

export type SQLSessionInsert = typeof sessions.$inferInsert
export type SQLSessionUpdate = Partial<SQLSessionInsert>

export namespace SessionStorage {
  export async function create(data: SQLSessionInsert) {
    await db.insert(sessions).values(data)
  }

  export async function get(id: string) {
    const result = await db.select().from(sessions).where(eq(sessions.id, id))
    return result[0]
  }

  export async function getByServerId(serverId: string) {
    const result = await db.select().from(sessions).where(eq(sessions.serverId, serverId))
    return result[0]
  }

  export async function update(id: string, data: SQLSessionUpdate) {
    await db
      .update(sessions)
      .set({ ...data, updatedAt: new Date() })
      .where(eq(sessions.id, id))
  }
}
