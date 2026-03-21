import { createBackendClient, type BackendClient } from "@goddard-ai/backend-client"
import type {
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowStart,
  RepoRef,
} from "@goddard-ai/schema/backend"
import { InMemoryTokenStorage, type TokenStorage } from "@goddard-ai/storage"

/** Detects the OAuth polling response that means the user has not finished approving yet. */
function isAuthorizationPendingError(error: unknown): boolean {
  return error instanceof Error && error.message.includes("authorization_pending")
}

/** Detects the OAuth polling response that asks the client to slow its polling cadence. */
function isSlowDownError(error: unknown): boolean {
  return error instanceof Error && error.message.includes("slow_down")
}

/** Constructor options for the SDK facade. */
export type GoddardSdkOptions = {
  backendUrl: string
  tokenStorage?: TokenStorage
  fetch?: typeof globalThis.fetch
}

/** Public SDK facade for backend auth, PR operations, and stream access. */
export class GoddardSdk {
  readonly #backend: BackendClient

  constructor(options: GoddardSdkOptions) {
    this.#backend = createBackendClient({
      baseUrl: new URL(options.backendUrl).toString(),
      tokenStorage: options.tokenStorage ?? new InMemoryTokenStorage(),
      fetchImpl: options.fetch ?? fetch,
    })
  }

  get auth() {
    return {
      /** Initiates a new GitHub device authorization flow. */
      startDeviceFlow: async (input: DeviceFlowStart = {}) => {
        return this.#backend.auth.startDeviceFlow(input)
      },
      /** Polls or finalizes a previously started device authorization flow. */
      completeDeviceFlow: async (input: DeviceFlowComplete) => {
        return this.#backend.auth.completeDeviceFlow(input)
      },
      /** High-level helper that manages the entire device flow lifecycle, prompting the user and waiting for authorization. */
      login: async ({
        githubUsername,
        onPrompt,
      }: {
        githubUsername: string
        onPrompt: (verificationUri: string, userCode: string) => void
      }) => {
        const start = await this.auth.startDeviceFlow({ githubUsername })
        onPrompt(start.verificationUri, start.userCode)

        const expiresAt = Date.now() + start.expiresIn * 1000
        let delay = start.interval * 1000

        while (Date.now() < expiresAt) {
          try {
            return await this.auth.completeDeviceFlow({
              deviceCode: start.deviceCode,
              githubUsername,
            })
          } catch (error: unknown) {
            if (!isAuthorizationPendingError(error) && !isSlowDownError(error)) {
              throw error
            }

            // Honor backend backpressure without abandoning the current device flow attempt.
            if (isSlowDownError(error)) {
              delay += 5000
            }
          }
          await new Promise((resolve) => setTimeout(resolve, delay))
        }
        throw new Error("Device flow authentication timed out.")
      },
      /** Retrieves the currently authenticated user's identity. */
      whoami: async () => {
        return this.#backend.auth.whoami()
      },
      /** Clears local authentication state and revokes any active tokens. */
      logout: async () => {
        await this.#backend.auth.logout()
      },
    }
  }

  get pr() {
    return {
      /** Submits a new pull request to the connected backend for processing. */
      create: async (input: CreatePrInput) => {
        return this.#backend.pr.create(input)
      },
      /** Checks if a specific pull request is currently tracked and managed by Goddard. */
      isManaged: async ({ owner, repo, prNumber }: RepoRef & { prNumber: number }) => {
        return this.#backend.pr.isManaged({ owner, repo, prNumber })
      },
      /** Posts a new comment or reply to an existing managed pull request. */
      reply: async (input: { owner: string; repo: string; prNumber: number; body: string }) => {
        return this.#backend.pr.reply(input)
      },
    }
  }

  get stream() {
    return {
      /** Opens a persistent connection to receive real-time updates from the backend. */
      subscribe: async () => {
        return this.#backend.stream.subscribe()
      },
    }
  }
}
