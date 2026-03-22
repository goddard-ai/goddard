import { eq } from "drizzle-orm"
import { getDatabaseInstance } from "./db/index.ts"
import { loops } from "./db/schema.ts"

/** Full SQL row shape accepted when creating a durable loop record. */
export type SQLLoopInsert = typeof loops.$inferInsert

/** Partial SQL row updates that may mutate an existing durable loop record. */
export type SQLLoopUpdate = Partial<SQLLoopInsert>

export namespace LoopStorage {
  export async function create(data: SQLLoopInsert) {
    const db = await getDatabaseInstance()
    await db.insert(loops).values(data)
  }

  export async function get(id: string) {
    const db = await getDatabaseInstance()
    const result = await db.select().from(loops).where(eq(loops.id, id))
    return result[0]
  }

  export async function update(id: string, data: SQLLoopUpdate) {
    const db = await getDatabaseInstance()
    await db
      .update(loops)
      .set({ ...data, updatedAt: new Date() })
      .where(eq(loops.id, id))
  }
}
