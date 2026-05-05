import { z } from "zod"

/** Stable key used to address one daemon-owned app setting record. */
export const AppSettingKey = z.string().min(1)

export type AppSettingKey = z.output<typeof AppSettingKey>

/** Top-level partition for one daemon-owned app setting. */
export const AppSettingScopeKind = z.enum(["global", "window"])

export type AppSettingScopeKind = z.output<typeof AppSettingScopeKind>

/** Stable identifier inside one app setting scope kind. */
export const AppSettingScopeId = z.string().min(1)

export type AppSettingScopeId = z.output<typeof AppSettingScopeId>

/** Versioned app setting payload persisted by the daemon for desktop hosts. */
export const AppSettingRecord = z.strictObject({
  version: z.number().int().nonnegative(),
  savedAt: z.number().int().nonnegative(),
  value: z.unknown(),
})

export type AppSettingRecord = z.output<typeof AppSettingRecord>

/** Scope selector for one app setting. Defaults to today's single primary app window. */
export const AppSettingScope = z.strictObject({
  scopeKind: AppSettingScopeKind.optional(),
  scopeId: AppSettingScopeId.optional(),
})

export type AppSettingScope = z.output<typeof AppSettingScope>

/** Scope used by today's single desktop window until callers provide a window id. */
export const DEFAULT_APP_SETTING_SCOPE = {
  scopeKind: "window",
  scopeId: "primary",
} as const satisfies Required<AppSettingScope>

/**
 * Persisted daemon-owned app setting row keyed by one scope plus desktop app storage key.
 */
export const DaemonAppSetting = AppSettingRecord.extend({
  scopeKind: AppSettingScopeKind,
  scopeId: AppSettingScopeId,
  key: AppSettingKey,
})

export type DaemonAppSetting = z.output<typeof DaemonAppSetting> & {
  id: `aps_${string}`
}

/** Request payload for reading one daemon-owned app setting record. */
export const GetAppSettingRequest = AppSettingScope.extend({
  key: AppSettingKey,
})

export type GetAppSettingRequest = z.output<typeof GetAppSettingRequest>

/** Response returned when reading one daemon-owned app setting record. */
export type GetAppSettingResponse = {
  setting: AppSettingRecord | null
}

/** Request payload for replacing one daemon-owned app setting record. */
export const SetAppSettingRequest = AppSettingScope.extend({
  key: AppSettingKey,
  record: AppSettingRecord,
})

export type SetAppSettingRequest = z.output<typeof SetAppSettingRequest>

/** Response returned after replacing one daemon-owned app setting record. */
export type SetAppSettingResponse = {
  setting: AppSettingRecord
}

/** Request payload for deleting one daemon-owned app setting record. */
export const DeleteAppSettingRequest = AppSettingScope.extend({
  key: AppSettingKey,
})

export type DeleteAppSettingRequest = z.output<typeof DeleteAppSettingRequest>

/** Response returned after deleting one daemon-owned app setting record. */
export type DeleteAppSettingResponse = {
  deleted: boolean
}
