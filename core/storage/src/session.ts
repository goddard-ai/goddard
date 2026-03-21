import { and, desc, eq, lt, or } from "drizzle-orm"
import { db } from "./db/index.js"
import { sessions } from "./db/schema.js"

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
    await db.insert(sessions).values(data)
  }

  /** Lists every persisted session row without pagination. */
  export async function listAll() {
    return db.select().from(sessions)
  }

  /** Lists recent session rows in stable `updatedAt desc, id desc` order. */
  export async function listRecent(input: {
    limit: number
    cursor?: SQLSessionListCursor
  }) {
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
    return (await db.select().from(sessions).where(eq(sessions.id, id)))[0]
  }

  export async function getByAcpId(acpId: string) {
    return (await db.select().from(sessions).where(eq(sessions.acpId, acpId)))[0]
  }

  /** Lists all sessions currently associated with one repository. */
  export async function listByRepository(repository: string) {
    return db.select().from(sessions).where(eq(sessions.repository, repository))
  }

  /** Lists all sessions currently associated with one repository pull request. */
  export async function listByRepositoryPr(repository: string, prNumber: number) {
    return db
      .select()
      .from(sessions)
      .where(and(eq(sessions.repository, repository), eq(sessions.prNumber, prNumber)))
  }

  export async function update(id: string, data: SQLSessionUpdate) {
    await db
      .update(sessions)
      .set({ ...data, updatedAt: new Date() })
      .where(eq(sessions.id, id))
  }
}
