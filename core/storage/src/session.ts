import { eq } from "drizzle-orm"
import { db } from "./db/index.js"
import { sessions } from "./db/schema.js"

/** Full SQL row shape accepted when creating a durable session record. */
export type SQLSessionInsert = typeof sessions.$inferInsert

/** Partial SQL row updates that may mutate an existing durable session record. */
export type SQLSessionUpdate = Partial<SQLSessionInsert>

/** SQL-backed storage for the primary durable facts about daemon sessions. */
export namespace SessionStorage {
  export async function create(data: SQLSessionInsert) {
    await db.insert(sessions).values(data)
  }

  export async function list() {
    return db.select().from(sessions)
  }

  export async function get(id: string) {
    return (await db.select().from(sessions).where(eq(sessions.id, id)))[0]
  }

  export async function getByAcpId(acpId: string) {
    return (await db.select().from(sessions).where(eq(sessions.acpId, acpId)))[0]
  }

  export async function update(id: string, data: SQLSessionUpdate) {
    await db
      .update(sessions)
      .set({ ...data, updatedAt: new Date() })
      .where(eq(sessions.id, id))
  }
}
