import { z } from "zod"

/** Request payload that starts one GitHub device authorization flow. */
export const DeviceFlowStart = z.object({
  githubUsername: z.string().optional(),
})

export type DeviceFlowStart = z.infer<typeof DeviceFlowStart>

/** Device authorization session returned by the backend before login completes. */
export const DeviceFlowSession = z.object({
  deviceCode: z.string(),
  userCode: z.string(),
  verificationUri: z.string(),
  expiresIn: z.number(),
  interval: z.number(),
})

export type DeviceFlowSession = z.infer<typeof DeviceFlowSession>

/** Request payload that completes one GitHub device authorization flow. */
export const DeviceFlowComplete = z.object({
  deviceCode: z.string(),
  githubUsername: z.string(),
})

export type DeviceFlowComplete = z.infer<typeof DeviceFlowComplete>

/** Authenticated backend session persisted for one GitHub user. */
export const AuthSession = z.object({
  token: z.string(),
  githubUsername: z.string(),
  githubUserId: z.number(),
})

export type AuthSession = z.infer<typeof AuthSession>
