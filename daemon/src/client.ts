import { createNodeClient } from "@goddard-ai/ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import {
  createDaemonUrl,
  getDefaultDaemonSocketPath,
  readSocketPathFromDaemonUrl,
} from "./ipc/socket.ts"

export type DaemonIpcClient = ReturnType<typeof createNodeClient<typeof daemonIpcSchema>>

export function createDaemonIpcClient(options: { daemonUrl: string }): DaemonIpcClient {
  return createNodeClient(readSocketPathFromDaemonUrl(options.daemonUrl), daemonIpcSchema)
}

export function createDaemonIpcClientFromEnv(
  env: Record<string, string | undefined> = process.env,
): {
  daemonUrl: string
  sessionToken: string
  client: DaemonIpcClient
} {
  const daemonUrl = env.GODDARD_DAEMON_URL ?? createDaemonUrl(getDefaultDaemonSocketPath())
  const sessionToken = requiredEnv(env.GODDARD_SESSION_TOKEN, "GODDARD_SESSION_TOKEN")

  return {
    daemonUrl,
    sessionToken,
    client: createDaemonIpcClient({ daemonUrl }),
  }
}

function requiredEnv(value: string | undefined, name: string): string {
  if (!value) {
    throw new Error(`${name} is required`)
  }

  return value
}
