import { eq } from "drizzle-orm"
import { db } from "./db/index.js"
import { loops } from "./db/schema.js"

export type SQLLoopInsert = typeof loops.$inferInsert
export type SQLLoopUpdate = Partial<SQLLoopInsert>

export namespace LoopStorage {
  export async function create(data: SQLLoopInsert) {
    await db.insert(loops).values(data)
  }

  export async function get(id: string) {
    const result = await db.select().from(loops).where(eq(loops.id, id))
    return result[0]
  }

  export async function update(id: string, data: SQLLoopUpdate) {
    await db
      .update(loops)
      .set({ ...data, updatedAt: new Date() })
      .where(eq(loops.id, id))
  }
}
