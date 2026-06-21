import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"
import { z } from "zod"

/** Core daemon IPC error codes shared by clients and non-feature daemon surfaces. */
export const DaemonIpcErrorCodes = {
  InvalidRequest: "daemon.invalid_request",
  Unavailable: "daemon.unavailable",
} as const

export type DaemonIpcErrorCode = (typeof DaemonIpcErrorCodes)[keyof typeof DaemonIpcErrorCodes]

export const DaemonIpcErrorCode = z.enum([
  DaemonIpcErrorCodes.InvalidRequest,
  DaemonIpcErrorCodes.Unavailable,
])

export const BrowserAccessPairingStartRequest = z.strictObject({
  label: z.string().min(1).max(120).optional(),
})

export type BrowserAccessPairingStartRequest = z.infer<typeof BrowserAccessPairingStartRequest>

export type BrowserAccessPairingStartResponse = {
  pairingId: string
  code: string
  expiresAt: string
}

export const BrowserAccessPairingConfirmRequest = z.strictObject({
  pairingId: z.string(),
  code: z.string(),
})

export type BrowserAccessPairingConfirmRequest = z.infer<typeof BrowserAccessPairingConfirmRequest>

export type BrowserAccessPairingConfirmResponse = {
  pairingId: string
  confirmed: true
}

export const BrowserAccessPairingCompleteRequest = z.strictObject({
  pairingId: z.string(),
})

export type BrowserAccessPairingCompleteRequest = z.infer<
  typeof BrowserAccessPairingCompleteRequest
>

export type BrowserAccessPairingCompleteResponse = {
  token: string
  clientId: string
  origin: string
}

export type BrowserAccessClientSummary = {
  clientId: string
  origin: string
  label: string | null
  createdAt: string
  revokedAt: string | null
}

export type BrowserAccessClientListResponse = {
  clients: BrowserAccessClientSummary[]
}

export const BrowserAccessClientRevokeRequest = z.strictObject({
  clientId: z.string(),
})

export type BrowserAccessClientRevokeRequest = z.infer<typeof BrowserAccessClientRevokeRequest>

export type BrowserAccessClientRevokeResponse = {
  revoked: boolean
}

export const BrowserAccessWebviewTokenCreateRequest = z.strictObject({
  origin: z.string(),
})

export type BrowserAccessWebviewTokenCreateRequest = z.infer<
  typeof BrowserAccessWebviewTokenCreateRequest
>

export type BrowserAccessWebviewTokenCreateResponse = {
  token: string
  origin: string
  expiresAt: string
}

/** Core daemon IPC routes that are not owned by feature packages. */
export const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
    browserAccess: http.resource("browser-access", {
      pairing: http.resource("pairing", {
        start: http.post("start", {
          body: BrowserAccessPairingStartRequest,
          response: $type<BrowserAccessPairingStartResponse>(),
        }),
        confirm: http.post("confirm", {
          body: BrowserAccessPairingConfirmRequest,
          response: $type<BrowserAccessPairingConfirmResponse>(),
        }),
        complete: http.post("complete", {
          body: BrowserAccessPairingCompleteRequest,
          response: $type<BrowserAccessPairingCompleteResponse>(),
        }),
      }),
      client: http.resource("client", {
        list: http.get("list", {
          response: $type<BrowserAccessClientListResponse>(),
        }),
        revoke: http.post("revoke", {
          body: BrowserAccessClientRevokeRequest,
          response: $type<BrowserAccessClientRevokeResponse>(),
        }),
      }),
      webviewToken: http.resource("webview-token", {
        create: http.post("create", {
          body: BrowserAccessWebviewTokenCreateRequest,
          response: $type<BrowserAccessWebviewTokenCreateResponse>(),
        }),
      }),
    }),
  }),
})
