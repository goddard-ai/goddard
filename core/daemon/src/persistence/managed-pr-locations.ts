import { db } from "./store.ts"

/** Durable local checkout metadata for one managed pull request. */
export type ManagedPrLocationRecord = {
  owner: string
  repo: string
  prNumber: number
  cwd: string
  updatedAt: number
}

/** Builds the stable storage key for one managed pull-request checkout entry. */
export function getManagedPrLocationKey(owner: string, repo: string, prNumber: number): string {
  return `${owner}/${repo}#${prNumber}`
}

/** Stored managed-PR payload including the internal kindstore document id and lookup key. */
export type StoredManagedPrLocationRecord = NonNullable<
  ReturnType<typeof db.managedPrLocations.get>
>

/** Builds one managed-PR location input and lets kindstore manage ids and timestamps. */
export function createManagedPrLocationRecord(
  record: Omit<ManagedPrLocationRecord, "updatedAt">,
): Parameters<typeof db.managedPrLocations.create>[0] {
  return {
    locationKey: getManagedPrLocationKey(record.owner, record.repo, record.prNumber),
    ...record,
  }
}
