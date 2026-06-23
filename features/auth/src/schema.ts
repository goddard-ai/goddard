import { z } from "zod"

/** Standard authenticated HTTP headers shared by auth-owned backend routes. */
export const BearerHeaders = z.object({
  authorization: z.string(),
})

export type BearerHeaders = z.infer<typeof BearerHeaders>

/** Identity from one provider that is linked to a Goddard backend principal. */
export const ProviderIdentity = z.object({
  provider: z.string().min(1),
  subject: z.string().min(1),
  displayName: z.string().min(1).optional(),
})

export type ProviderIdentity = z.infer<typeof ProviderIdentity>

/** Authenticated backend principal without provider-specific identity fields. */
export const BackendPrincipal = z.object({
  id: z.string().min(1),
  providerIdentities: z.array(ProviderIdentity),
})

export type BackendPrincipal = z.infer<typeof BackendPrincipal>

/** Request payload that starts one provider-backed device authorization flow. */
export const DeviceFlowStart = z.object({
  provider: z.string().min(1).optional(),
  loginHint: z.string().min(1).optional(),
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

/** Request payload that completes one provider-backed device authorization flow. */
export const DeviceFlowComplete = z.object({
  deviceCode: z.string(),
  providerIdentity: ProviderIdentity,
})

export type DeviceFlowComplete = z.infer<typeof DeviceFlowComplete>

/** Authenticated backend session persisted for one provider-neutral principal. */
export const AuthSession = z.object({
  token: z.string(),
  principal: BackendPrincipal,
})

export type AuthSession = z.infer<typeof AuthSession>
