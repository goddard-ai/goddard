import { $type, defineIpcRoutes, http, metadata, ndjson } from "@goddard-ai/ipc"
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

export const DaemonEventOptions = z.object({
  debug: z.string().optional(),
})

export type DaemonEventOptions = z.infer<typeof DaemonEventOptions>

export const DaemonEventEnvelope = z.object({
  id: z.string(),
  at: z.string(),
  name: z.string(),
  payload: z.unknown(),
  options: DaemonEventOptions.optional(),
})

export type DaemonEventEnvelope = z.infer<typeof DaemonEventEnvelope>

export const DaemonEventPropertyFilter = z.object({
  path: z.string().min(1),
  equals: z.unknown(),
})

export type DaemonEventPropertyFilter = z.infer<typeof DaemonEventPropertyFilter>

export const DaemonEventsStreamRequest = z.object({
  names: z.array(z.string().min(1)).optional(),
  where: z.array(DaemonEventPropertyFilter).optional(),
})

export type DaemonEventsStreamRequest = z.infer<typeof DaemonEventsStreamRequest>

/** Core IPC routes that are not owned by feature packages. */
export const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    ...metadata({
      description: "Core health and browser-access control.",
    }),
    health: http.get("health", {
      ...metadata({
        description: "Checks whether the IPC server is responding.",
      }),
      response: $type<{ ok: boolean }>(),
    }),
    browserAccess: http.resource("browser-access", {
      ...metadata({
        description: "Browser-access pairing and client authorization control.",
      }),
      pairing: http.resource("pairing", {
        ...metadata({
          description: "Browser-access pairing flow control.",
        }),
        start: http.post("start", {
          ...metadata({
            description: "Starts one browser-access pairing flow.",
          }),
          body: BrowserAccessPairingStartRequest,
          response: $type<BrowserAccessPairingStartResponse>(),
        }),
        confirm: http.post("confirm", {
          ...metadata({
            description: "Confirms one browser-access pairing code.",
          }),
          body: BrowserAccessPairingConfirmRequest,
          response: $type<BrowserAccessPairingConfirmResponse>(),
        }),
        complete: http.post("complete", {
          ...metadata({
            description: "Completes one confirmed browser-access pairing flow.",
          }),
          body: BrowserAccessPairingCompleteRequest,
          response: $type<BrowserAccessPairingCompleteResponse>(),
        }),
      }),
      client: http.resource("client", {
        ...metadata({
          description: "Browser-access client authorization management.",
        }),
        list: http.get("list", {
          ...metadata({
            description: "Lists browser-access client authorizations.",
          }),
          response: $type<BrowserAccessClientListResponse>(),
        }),
        revoke: http.post("revoke", {
          ...metadata({
            description: "Revokes one browser-access client authorization.",
          }),
          body: BrowserAccessClientRevokeRequest,
          response: $type<BrowserAccessClientRevokeResponse>(),
        }),
      }),
      webviewToken: http.resource("webview-token", {
        ...metadata({
          description: "Browser webview access-token creation.",
        }),
        create: http.post("create", {
          ...metadata({
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
