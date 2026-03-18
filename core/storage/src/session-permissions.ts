import { eq } from "drizzle-orm"
import { db } from "./db/index.js"
import { sessionPermissions } from "./db/schema.js"

export type SessionPermissionsRecord = {
  sessionId: string
  token: string
  owner: string
  repo: string
  allowedPrNumbers: number[]
  createdAt: string
}

export namespace SessionPermissionsStorage {
  export async function create(record: Omit<SessionPermissionsRecord, "createdAt">) {
    const newRecord = {
      ...record,
      createdAt: new Date().toISOString(),
    }
    db.insert(sessionPermissions)
      .values(newRecord)
      .onConflictDoUpdate({
        target: sessionPermissions.sessionId,
        set: newRecord,
      })
      .run()
    return newRecord
  }

  export async function get(sessionId: string): Promise<SessionPermissionsRecord | null> {
    const records = db
      .select()
      .from(sessionPermissions)
      .where(eq(sessionPermissions.sessionId, sessionId))
      .limit(1)
      .all()
    return records.length > 0 ? (records[0] as SessionPermissionsRecord) : null
  }

  export async function getByToken(token: string): Promise<SessionPermissionsRecord | null> {
    const records = db
      .select()
      .from(sessionPermissions)
      .where(eq(sessionPermissions.token, token))
      .limit(1)
      .all()
    return records.length > 0 ? (records[0] as SessionPermissionsRecord) : null
  }

  export async function list(): Promise<SessionPermissionsRecord[]> {
    return db.select().from(sessionPermissions).all() as SessionPermissionsRecord[]
  }

  export async function addAllowedPr(sessionId: string, prNumber: number): Promise<void> {
    const record = await get(sessionId)
    if (!record) {
      return
    }

    if (!record.allowedPrNumbers.includes(prNumber)) {
      const updatedPrNumbers = [...record.allowedPrNumbers, prNumber]
      db.update(sessionPermissions)
        .set({ allowedPrNumbers: updatedPrNumbers })
        .where(eq(sessionPermissions.sessionId, sessionId))
        .run()
    }
  }

  export async function revoke(sessionId: string): Promise<void> {
    db.delete(sessionPermissions)
      .where(eq(sessionPermissions.sessionId, sessionId))
      .run()
  }
}
