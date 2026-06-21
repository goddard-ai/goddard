import { AsyncLocalStorage } from "node:async_hooks"
import { createHash, randomBytes, randomInt, timingSafeEqual } from "node:crypto"
import { IpcClientError } from "@goddard-ai/ipc"
import type { BrowserAccessConfig } from "@goddard-ai/schema/config"

import type { ComposedDaemonStore } from "./plugins.ts"

const pairingTtlMs = 5 * 60 * 1000
const desktopWebviewTokenTtlMs = 5 * 60 * 1000

type BrowserAccessState = {
  pendingPairings: Record<
    string,
    {
      origin: string
      codeHash: string
      label: string | null
      createdAt: string
      expiresAt: string
      confirmedAt: string | null
      completedAt: string | null
    }
  >
  browserTokens: Record<
    string,
    {
      origin: string
      tokenHash: string
      label: string | null
      createdAt: string
      revokedAt: string | null
    }
  >
}

type BrowserAccessRuntimeConfig = {
  readonly enabled: boolean
  readonly hostedOrigins: ReadonlySet<string>
  readonly desktopWebviewOrigins: ReadonlySet<string>
  readonly allowedOrigins: readonly string[]
}

type BrowserAccessRequestContext = {
  readonly origin: string | null
}

type DesktopWebviewToken = {
  readonly origin: string
  readonly tokenHash: string
  readonly expiresAt: number
}

const requestContext = new AsyncLocalStorage<BrowserAccessRequestContext>()

export function runBrowserAccessRequestContext<T>(request: Request, callback: () => T): T {
  return requestContext.run(
    {
      origin: request.headers.get("origin"),
    },
    callback,
  )
}

export function resolveBrowserAccessRuntimeConfig(
  config: BrowserAccessConfig | undefined,
): BrowserAccessRuntimeConfig {
  if (config?.enabled !== true) {
    return {
      enabled: false,
      hostedOrigins: new Set(),
      desktopWebviewOrigins: new Set(),
      allowedOrigins: [],
    }
  }

  const hostedOrigins = new Set((config.allowedOrigins ?? []).map(normalizeOrigin))
  const desktopWebviewOrigins = new Set((config.desktopWebviewOrigins ?? []).map(normalizeOrigin))

  return {
    enabled: true,
    hostedOrigins,
    desktopWebviewOrigins,
    allowedOrigins: [...hostedOrigins, ...desktopWebviewOrigins],
  }
}

/** Creates the daemon-owned browser access service for pairing and token validation. */
export function createBrowserAccessService(
  store: Pick<ComposedDaemonStore, "metadata">,
  config: BrowserAccessRuntimeConfig,
) {
  const desktopWebviewTokens = new Map<string, DesktopWebviewToken>()

  function readState(): BrowserAccessState {
    return (
      store.metadata.get("browserAccess") ?? {
        pendingPairings: {},
        browserTokens: {},
      }
    )
  }

  function writeState(state: BrowserAccessState) {
    store.metadata.set("browserAccess", state)
  }

  function requireBrowserOrigin() {
    const origin = requestContext.getStore()?.origin ?? null
    if (!origin) {
      throw new IpcClientError("Browser origin is required")
    }

    return normalizeOrigin(origin)
  }

  function requireTrustedLocalRequest() {
    if (requestContext.getStore()?.origin) {
      throw new IpcClientError("Trusted local daemon request is required")
    }
  }

  function requireHostedOrigin(origin: string) {
    if (!config.enabled || !config.hostedOrigins.has(origin)) {
      throw new IpcClientError("Browser origin is not enabled")
    }
  }

  function requireDesktopWebviewOrigin(origin: string) {
    const normalizedOrigin = normalizeOrigin(origin)
    if (!config.enabled || !config.desktopWebviewOrigins.has(normalizedOrigin)) {
      throw new IpcClientError("Desktop webview origin is not enabled")
    }

    return normalizedOrigin
  }

  function startPairing(input: { label?: string }) {
    const origin = requireBrowserOrigin()
    requireHostedOrigin(origin)

    const now = Date.now()
    const pairingId = `bap_${randomBytes(8).toString("hex")}`
    const code = randomInt(0, 1_000_000).toString().padStart(6, "0")
    const state = readState()
    state.pendingPairings[pairingId] = {
      origin,
      codeHash: hashSecret(code),
      label: input.label ?? null,
      createdAt: new Date(now).toISOString(),
      expiresAt: new Date(now + pairingTtlMs).toISOString(),
      confirmedAt: null,
      completedAt: null,
    }
    writeState(state)

    return {
      pairingId,
      code,
      expiresAt: state.pendingPairings[pairingId].expiresAt,
    }
  }

  function confirmPairing(input: { pairingId: string; code: string }) {
    requireTrustedLocalRequest()
    const state = readState()
    const pairing = state.pendingPairings[input.pairingId]
    if (!pairing || pairing.completedAt || isExpired(pairing.expiresAt)) {
      throw new IpcClientError("Invalid browser pairing")
    }
    if (!secretMatches(input.code, pairing.codeHash)) {
      throw new IpcClientError("Invalid browser pairing")
    }

    pairing.confirmedAt = new Date().toISOString()
    writeState(state)

    return {
      pairingId: input.pairingId,
      confirmed: true as const,
    }
  }

  function completePairing(input: { pairingId: string }) {
    const origin = requireBrowserOrigin()
    requireHostedOrigin(origin)

    const state = readState()
    const pairing = state.pendingPairings[input.pairingId]
    if (
      !pairing ||
      pairing.origin !== origin ||
      !pairing.confirmedAt ||
      pairing.completedAt ||
      isExpired(pairing.expiresAt)
    ) {
      throw new IpcClientError("Invalid browser pairing")
    }

    const issued = issueToken("bac")
    const createdAt = new Date().toISOString()
    pairing.completedAt = createdAt
    state.browserTokens[issued.id] = {
      origin,
      tokenHash: issued.tokenHash,
      label: pairing.label,
      createdAt,
      revokedAt: null,
    }
    writeState(state)

    return {
      token: issued.token,
      clientId: issued.id,
      origin,
    }
  }

  function listClients() {
    requireTrustedLocalRequest()
    const state = readState()
    return {
      clients: Object.entries(state.browserTokens).map(([clientId, token]) => ({
        clientId,
        origin: token.origin,
        label: token.label,
        createdAt: token.createdAt,
        revokedAt: token.revokedAt,
      })),
    }
  }

  function revokeClient(input: { clientId: string }) {
    requireTrustedLocalRequest()
    const state = readState()
    const token = state.browserTokens[input.clientId]
    if (!token || token.revokedAt) {
      return { revoked: false }
    }

    token.revokedAt = new Date().toISOString()
    writeState(state)
    return { revoked: true }
  }

  function createDesktopWebviewToken(input: { origin: string }) {
    requireTrustedLocalRequest()
    const origin = requireDesktopWebviewOrigin(input.origin)
    const issued = issueToken("dwt")
    const expiresAt = Date.now() + desktopWebviewTokenTtlMs
    desktopWebviewTokens.set(issued.id, {
      origin,
      tokenHash: issued.tokenHash,
      expiresAt,
    })

    return {
      token: issued.token,
      origin,
      expiresAt: new Date(expiresAt).toISOString(),
    }
  }

  function authorizeRequest(request: Request): Response | null {
    const origin = request.headers.get("origin")
    if (!origin) {
      return null
    }

    const normalizedOrigin = normalizeOrigin(origin)
    const pathname = new URL(request.url).pathname
    if (isBrowserPublicRoute(pathname)) {
      return config.hostedOrigins.has(normalizedOrigin) ? null : forbiddenResponse()
    }
    if (isTrustedLocalOnlyRoute(pathname)) {
      return forbiddenResponse()
    }

    const token = parseBearerToken(request.headers.get("authorization"))
    if (!token) {
      return forbiddenResponse()
    }
    if (isAuthorizedHostedBrowserToken(token, normalizedOrigin)) {
      return null
    }
    if (isAuthorizedDesktopWebviewToken(token, normalizedOrigin)) {
      return null
    }

    return forbiddenResponse()
  }

  function isAuthorizedHostedBrowserToken(input: ParsedBearerToken, origin: string) {
    const state = readState()
    const token = state.browserTokens[input.id]
    return Boolean(
      token &&
      !token.revokedAt &&
      token.origin === origin &&
      secretMatches(input.secret, token.tokenHash),
    )
  }

  function isAuthorizedDesktopWebviewToken(input: ParsedBearerToken, origin: string) {
    const token = desktopWebviewTokens.get(input.id)
    if (!token) {
      return false
    }
    if (Date.now() >= token.expiresAt) {
      desktopWebviewTokens.delete(input.id)
      return false
    }

    return token.origin === origin && secretMatches(input.secret, token.tokenHash)
  }

  return {
    authorizeRequest,
    startPairing,
    confirmPairing,
    completePairing,
    listClients,
    revokeClient,
    createDesktopWebviewToken,
  }
}

type ParsedBearerToken = {
  readonly id: string
  readonly secret: string
}

function normalizeOrigin(origin: string) {
  if (origin === "*" || origin === "null") {
    throw new Error(`Browser access origin must be explicit: ${origin}`)
  }

  const url = new URL(origin)
  if (url.origin !== origin) {
    throw new Error(`Browser access origin must not include a path, query, or hash: ${origin}`)
  }

  return url.origin
}

function isBrowserPublicRoute(pathname: string) {
  return (
    pathname === "/daemon/health" ||
    pathname === "/daemon/browser-access/pairing/start" ||
    pathname === "/daemon/browser-access/pairing/complete"
  )
}

function isTrustedLocalOnlyRoute(pathname: string) {
  return pathname.startsWith("/daemon/browser-access/")
}

function parseBearerToken(header: string | null): ParsedBearerToken | null {
  if (!header?.startsWith("Bearer ")) {
    return null
  }

  const token = header.slice("Bearer ".length)
  const separator = token.indexOf(".")
  if (separator <= 0 || separator === token.length - 1) {
    return null
  }

  return {
    id: token.slice(0, separator),
    secret: token.slice(separator + 1),
  }
}

function issueToken(prefix: string) {
  const id = `${prefix}_${randomBytes(8).toString("hex")}`
  const secret = randomBytes(32).toString("hex")
  return {
    id,
    token: `${id}.${secret}`,
    tokenHash: hashSecret(secret),
  }
}

function hashSecret(secret: string) {
  return createHash("sha256").update(secret).digest("hex")
}

function secretMatches(secret: string, expectedHash: string) {
  const actual = Buffer.from(hashSecret(secret), "hex")
  const expected = Buffer.from(expectedHash, "hex")
  return actual.length === expected.length && timingSafeEqual(actual, expected)
}

function isExpired(expiresAt: string) {
  return Date.now() >= Date.parse(expiresAt)
}

function forbiddenResponse() {
  return Response.json({ error: "Forbidden" }, { status: 403 })
}
