import type { DeviceFlowComplete, DeviceFlowStart } from "@goddard-ai/schema/backend"
import { resolveDaemonClient, type DaemonClientOptions } from "./daemon/client.ts"

/** Detects the OAuth polling response that means the user has not finished approving yet. */
function isAuthorizationPendingError(error: unknown): boolean {
  return error instanceof Error && error.message.includes("authorization_pending")
}

/** Detects the OAuth polling response that asks the client to slow its polling cadence. */
function isSlowDownError(error: unknown): boolean {
  return error instanceof Error && error.message.includes("slow_down")
}

/** Constructor options for the daemon-backed SDK facade. */
export type GoddardSdkOptions = DaemonClientOptions

/** Public SDK facade for daemon-backed authentication. */
export class GoddardSdk {
  readonly #options: DaemonClientOptions

  constructor(options: GoddardSdkOptions) {
    this.#options = options
  }

  get auth() {
    return {
      /** Initiates a new GitHub device authorization flow through the daemon. */
      startDeviceFlow: async (input: DeviceFlowStart = {}) => {
        return resolveDaemonClient(this.#options).send("authDeviceStart", input)
      },
      /** Polls or finalizes a previously started device authorization flow through the daemon. */
      completeDeviceFlow: async (input: DeviceFlowComplete) => {
        return resolveDaemonClient(this.#options).send("authDeviceComplete", input)
      },
      /** High-level helper that manages the full daemon-backed device flow lifecycle. */
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

            if (isSlowDownError(error)) {
              delay += 5000
            }
          }

          await new Promise((resolve) => setTimeout(resolve, delay))
        }

        throw new Error("Device flow authentication timed out.")
      },
      /** Retrieves the currently authenticated user's identity from the daemon-owned auth state. */
      whoami: async () => {
        return resolveDaemonClient(this.#options).send("authWhoami", {})
      },
      /** Clears daemon-owned authentication state. */
      logout: async () => {
        await resolveDaemonClient(this.#options).send("authLogout", {})
      },
    }
  }
}
