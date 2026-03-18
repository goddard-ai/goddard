import { createBackendClient, type BackendClient } from "@goddard-ai/backend/client"
import type {
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowStart,
  RepoRef,
} from "@goddard-ai/schema/backend"
import { InMemoryTokenStorage, type TokenStorage } from "@goddard-ai/storage"

// Constructor options for the SDK facade.
export type GoddardSdkOptions = {
  backendUrl: string
  tokenStorage?: TokenStorage
  fetch?: typeof globalThis.fetch
}

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
      startDeviceFlow: async (input: DeviceFlowStart = {}) => {
        return this.#backend.auth.startDeviceFlow(input)
      },
      completeDeviceFlow: async (input: DeviceFlowComplete) => {
        return this.#backend.auth.completeDeviceFlow(input)
      },
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
              githubUsername: githubUsername ?? "",
            })
          } catch (e: any) {
            if (
              e.message &&
              !e.message.includes("authorization_pending") &&
              !e.message.includes("slow_down")
            ) {
              throw e
            }
            if (e.message && e.message.includes("slow_down")) {
              delay += 5000
            }
          }
          await new Promise((resolve) => setTimeout(resolve, delay))
        }
        throw new Error("Device flow authentication timed out.")
      },
      whoami: async () => {
        return this.#backend.auth.whoami()
      },
      logout: async () => {
        await this.#backend.auth.logout()
      },
    }
  }

  get pr() {
    return {
      create: async (input: CreatePrInput) => {
        return this.#backend.pr.create(input)
      },
      isManaged: async ({ owner, repo, prNumber }: RepoRef & { prNumber: number }) => {
        return this.#backend.pr.isManaged({ owner, repo, prNumber })
      },
      reply: async (input: { owner: string; repo: string; prNumber: number; body: string }) => {
        return this.#backend.pr.reply(input)
      },
    }
  }

  get stream() {
    return {
      subscribe: async () => {
        return this.#backend.stream.subscribe()
      },
    }
  }
}
