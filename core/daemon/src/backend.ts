import { authBackendRoutes } from "@goddard-ai/auth/backend"
import {
  composeBackendRoutes,
  createClient,
  ndjson,
  type RouzerClient,
} from "@goddard-ai/backend-plugin"
import { pullRequestBackendRoutes } from "@goddard-ai/pull-request/backend"
import { remoteRepoBackendRoutes } from "@goddard-ai/remote-repo/backend"

/** Fetch implementation consumed by the daemon's backend client. */
type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>

const notAuthenticatedMessage = "Not authenticated. Run login first."

/** Backend routes available to daemon-owned backend clients. */
export const backendRoutes = composeBackendRoutes([
  authBackendRoutes,
  pullRequestBackendRoutes,
  remoteRepoBackendRoutes,
])

/** Error thrown when a daemon backend call requires a login session that is not available. */
export class BackendUnauthenticatedError extends Error {
  constructor(message = notAuthenticatedMessage) {
    super(message)
    this.name = "BackendUnauthenticatedError"
  }
}

/** Returns whether a daemon backend failure was caused by missing or invalid authentication. */
export function isBackendUnauthenticatedError(error: unknown) {
  return (
    error instanceof BackendUnauthenticatedError ||
    (error instanceof Error && error.name === "BackendUnauthenticatedError")
  )
}

/** Constructor options for the daemon's direct backend client. */
export type BackendClientOptions = {
  baseUrl: string
  fetchImpl?: FetchLike
  getAuthorizationHeader?: () => Promise<string | null> | string | null
}

/** Direct backend client surface owned privately by the daemon. */
export type BackendClient = RouzerClient<typeof backendRoutes>

/** Creates the daemon's direct rouzer-backed client for backend auth, PR, and event routes. */
export function createBackendClient(options: BackendClientOptions): BackendClient {
  return createClient({
    baseURL: options.baseUrl,
    headers: {
      authorization: "Bearer unauthenticated",
    },
    fetch: createAuthorizedFetch(options) as typeof fetch,
    routes: backendRoutes,
    plugins: [ndjson.clientPlugin],
  })
}

function createAuthorizedFetch(options: BackendClientOptions): FetchLike {
  const fetchImpl = options.fetchImpl ?? fetch

  return async (input, init) => {
    const authorization = await options.getAuthorizationHeader?.()
    const headers = new Headers(init?.headers)
    if (authorization) {
      headers.set("authorization", authorization)
    } else {
      headers.delete("authorization")
    }

    const response = await fetchImpl(input, { ...init, headers })
    if (response.status !== 401) {
      return response
    }

    throw new BackendUnauthenticatedError(
      `Backend request failed (${response.status}): ${await response.text()}`,
    )
  }
}
