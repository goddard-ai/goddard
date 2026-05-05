import { z } from "zod"

/** Stable key used to address one daemon-owned app state record. */
export const AppStateKey = z.string().min(1)

export type AppStateKey = z.output<typeof AppStateKey>

/** Top-level partition for one daemon-owned app state. */
export const AppStateScopeKind = z.enum(["global", "window"])

export type AppStateScopeKind = z.output<typeof AppStateScopeKind>

/** Stable identifier inside one app state scope kind. */
export const AppStateScopeId = z.string().min(1)

export type AppStateScopeId = z.output<typeof AppStateScopeId>

/** Versioned app state payload persisted by the daemon for desktop hosts. */
export const AppStateRecord = z.strictObject({
  version: z.number().int().nonnegative(),
  savedAt: z.number().int().nonnegative(),
  value: z.unknown(),
})

export type AppStateRecord = z.output<typeof AppStateRecord>

/** Scope selector for one app state. */
export const AppStateScope = z.strictObject({
  scopeKind: AppStateScopeKind,
  scopeId: AppStateScopeId,
})

export type AppStateScope = z.output<typeof AppStateScope>

/**
 * Persisted daemon-owned app state row keyed by one scope plus desktop app storage key.
 */
export const DaemonAppState = AppStateRecord.extend({
  scopeKind: AppStateScopeKind,
  scopeId: AppStateScopeId,
  key: AppStateKey,
})

export type DaemonAppState = z.output<typeof DaemonAppState> & {
  id: `ast_${string}`
}

/** Request payload for reading one daemon-owned app state record. */
export const GetAppStateRequest = AppStateScope.extend({
  key: AppStateKey,
})

export type GetAppStateRequest = z.output<typeof GetAppStateRequest>

/** Response returned when reading one daemon-owned app state record. */
export type GetAppStateResponse = {
  state: AppStateRecord | null
}

/** Request payload for replacing one daemon-owned app state record. */
export const SetAppStateRequest = AppStateScope.extend({
  key: AppStateKey,
  record: AppStateRecord,
})

export type SetAppStateRequest = z.output<typeof SetAppStateRequest>

/** Response returned after replacing one daemon-owned app state record. */
export type SetAppStateResponse = {
  state: AppStateRecord
}

/** Request payload for deleting one daemon-owned app state record. */
export const DeleteAppStateRequest = AppStateScope.extend({
  key: AppStateKey,
})

export type DeleteAppStateRequest = z.output<typeof DeleteAppStateRequest>

/** Response returned after deleting one daemon-owned app state record. */
export type DeleteAppStateResponse = {
  deleted: boolean
}
