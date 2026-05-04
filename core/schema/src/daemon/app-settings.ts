import { z } from "zod"

/** Stable key used to address one daemon-owned app setting record. */
export const AppSettingKey = z.string().min(1)

export type AppSettingKey = z.output<typeof AppSettingKey>

/** Versioned app setting payload persisted by the daemon for desktop hosts. */
export const AppSettingRecord = z.strictObject({
  version: z.number().int().nonnegative(),
  savedAt: z.number().int().nonnegative(),
  value: z.unknown(),
})

export type AppSettingRecord = z.output<typeof AppSettingRecord>

/**
 * Persisted daemon-owned app setting row keyed by one desktop app storage key.
 */
export const DaemonAppSetting = AppSettingRecord.extend({
  key: AppSettingKey,
})

export type DaemonAppSetting = z.output<typeof DaemonAppSetting> & {
  id: `aps_${string}`
}

/** Request payload for reading one daemon-owned app setting record. */
export const GetAppSettingRequest = z.strictObject({
  key: AppSettingKey,
})

export type GetAppSettingRequest = z.output<typeof GetAppSettingRequest>

/** Response returned when reading one daemon-owned app setting record. */
export type GetAppSettingResponse = {
  setting: AppSettingRecord | null
}

/** Request payload for replacing one daemon-owned app setting record. */
export const SetAppSettingRequest = z.strictObject({
  key: AppSettingKey,
  record: AppSettingRecord,
})

export type SetAppSettingRequest = z.output<typeof SetAppSettingRequest>

/** Response returned after replacing one daemon-owned app setting record. */
export type SetAppSettingResponse = {
  setting: AppSettingRecord
}

/** Request payload for deleting one daemon-owned app setting record. */
export const DeleteAppSettingRequest = z.strictObject({
  key: AppSettingKey,
})

export type DeleteAppSettingRequest = z.output<typeof DeleteAppSettingRequest>

/** Response returned after deleting one daemon-owned app setting record. */
export type DeleteAppSettingResponse = {
  deleted: boolean
}
