import { and, desc, eq, lt, or } from "drizzle-orm"
import { getDatabaseInstance } from "./db/index.ts"
import { sessions } from "./db/schema.ts"

/** Full SQL row shape accepted when creating a durable session record. */
export type SQLSessionInsert = typeof sessions.$inferInsert

/** Partial SQL row updates that may mutate an existing durable session record. */
export type SQLSessionUpdate = Partial<SQLSessionInsert>

/** Stable cursor key used for recency-ordered daemon session pagination. */
export type SQLSessionListCursor = {
  updatedAt: Date
  id: string
}

/** SQL-backed storage for the primary durable facts about daemon sessions. */
export namespace SessionStorage {
  export async function create(data: SQLSessionInsert) {
    const db = await getDatabaseInstance()
    await db.insert(sessions).values(data)
  }

  /** Lists every persisted session row without pagination. */
  export async function listAll() {
    const db = await getDatabaseInstance()
    return db.select().from(sessions)
  }

  export async function listRecent(input: { limit: number; cursor?: SQLSessionListCursor }) {
    const db = await getDatabaseInstance()
    const query = db.select().from(sessions)
    if (input.cursor) {
      query.where(
        or(
          lt(sessions.updatedAt, input.cursor.updatedAt),
          and(eq(sessions.updatedAt, input.cursor.updatedAt), lt(sessions.id, input.cursor.id)),
        ),
      )
    }

    return query.orderBy(desc(sessions.updatedAt), desc(sessions.id)).limit(input.limit)
  }

  export async function get(id: string) {
    const db = await getDatabaseInstance()
    return (await db.select().from(sessions).where(eq(sessions.id, id)))[0]
  }

  export async function getByAcpId(acpId: string) {
    const db = await getDatabaseInstance()
    return (await db.select().from(sessions).where(eq(sessions.acpId, acpId)))[0]
  }

  /** Lists all sessions currently associated with one repository. */
  export async function listByRepository(repository: string) {
    const db = await getDatabaseInstance()
    return db.select().from(sessions).where(eq(sessions.repository, repository))
  }

  /** Lists all sessions currently associated with one repository pull request. */
  export async function listByRepositoryPr(repository: string, prNumber: number) {
    const db = await getDatabaseInstance()
    return db
      .select()
      .from(sessions)
      .where(and(eq(sessions.repository, repository), eq(sessions.prNumber, prNumber)))
  }

  export async function update(id: string, data: SQLSessionUpdate) {
    const db = await getDatabaseInstance()
    await db
      .update(sessions)
      .set({ ...data, updatedAt: new Date() })
      .where(eq(sessions.id, id))
  }
}
