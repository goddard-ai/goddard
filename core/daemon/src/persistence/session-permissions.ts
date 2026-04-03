import { db } from "./store.ts"

/** Durable authorization record that scopes one daemon session's repo access. */
export type SessionPermissionsRecord = {
  sessionId: string
  token: string
  owner: string
  repo: string
  allowedPrNumbers: number[]
  createdAt: number
}

/** Stored permission payload including the internal kindstore document id. */
export type StoredSessionPermissionsRecord = NonNullable<
  ReturnType<typeof db.sessionPermissions.get>
>

/** Builds one new permission input and lets kindstore manage ids and timestamps. */
export function createSessionPermissionsRecord(
  record: Omit<SessionPermissionsRecord, "createdAt">,
): Parameters<typeof db.sessionPermissions.create>[0] {
  return record
}
