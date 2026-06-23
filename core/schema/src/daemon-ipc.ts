import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"
import { z } from "zod"

/** Core IPC error codes shared by clients and non-feature surfaces. */
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

/** Core IPC routes that are not owned by feature packages. */
export const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    ...ipcMetadata({
      description: "Core health and browser-access control.",
    }),
    health: http.get("health", {
      ...ipcMetadata({
        description: "Checks whether the IPC server is responding.",
      }),
      response: $type<{ ok: boolean }>(),
    }),
    browserAccess: http.resource("browser-access", {
      ...ipcMetadata({
        description: "Browser-access pairing and client authorization control.",
      }),
      pairing: http.resource("pairing", {
        ...ipcMetadata({
          description: "Browser-access pairing flow control.",
        }),
        start: http.post("start", {
          ...ipcMetadata({
            description: "Starts one browser-access pairing flow.",
          }),
          body: BrowserAccessPairingStartRequest,
          response: $type<BrowserAccessPairingStartResponse>(),
        }),
        confirm: http.post("confirm", {
          ...ipcMetadata({
            description: "Confirms one browser-access pairing code.",
          }),
          body: BrowserAccessPairingConfirmRequest,
          response: $type<BrowserAccessPairingConfirmResponse>(),
        }),
        complete: http.post("complete", {
          ...ipcMetadata({
            description: "Completes one confirmed browser-access pairing flow.",
          }),
          body: BrowserAccessPairingCompleteRequest,
          response: $type<BrowserAccessPairingCompleteResponse>(),
        }),
      }),
      client: http.resource("client", {
        ...ipcMetadata({
          description: "Browser-access client authorization management.",
        }),
        list: http.get("list", {
          ...ipcMetadata({
            description: "Lists browser-access client authorizations.",
          }),
          response: $type<BrowserAccessClientListResponse>(),
        }),
        revoke: http.post("revoke", {
          ...ipcMetadata({
            description: "Revokes one browser-access client authorization.",
          }),
          body: BrowserAccessClientRevokeRequest,
          response: $type<BrowserAccessClientRevokeResponse>(),
        }),
      }),
      webviewToken: http.resource("webview-token", {
        ...ipcMetadata({
          description: "Browser webview access-token creation.",
        }),
        create: http.post("create", {
          ...ipcMetadata({
            description: "Creates one browser webview access token.",
          }),
          body: BrowserAccessWebviewTokenCreateRequest,
          response: $type<BrowserAccessWebviewTokenCreateResponse>(),
        }),
      }),
    }),
  }),
  events: http.resource("events", {
    /** Streams typed daemon events declared by the composed daemon plugins. */
    stream: http.post("stream", {
      body: DaemonEventsStreamRequest,
      response: ndjson.$type<DaemonEventEnvelope>(),
    }),
  }),
})
