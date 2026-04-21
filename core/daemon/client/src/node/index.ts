/** Node-specific daemon IPC client helpers built on the shared daemon client types. */
import { createNodeClient } from "@goddard-ai/ipc/node"
import { GODDARD_DAEMON_SOCKET_FILENAME } from "@goddard-ai/paths"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import {
  DEFAULT_DAEMON_LOOPBACK_ORIGIN,
  createDaemonUrl,
  readSocketPathFromDaemonUrl,
} from "@goddard-ai/schema/daemon-url"
import * as path from "node:path"

import {
  type DaemonIpcClient,
  type DaemonIpcClientFactory,
  type DaemonIpcClientFactoryInput,
} from "../index.ts"

/** Environment variables consumed by daemon client convenience helpers. */
export type DaemonClientEnv = Record<string, string | undefined>
export type {
  DaemonIpcClient,
  DaemonIpcClientFactory,
  DaemonIpcClientFactoryInput,
} from "../index.ts"

/** Creates one daemon IPC client for a Node host using either the default or injected transport. */
export function createDaemonIpcClient<TClient = DaemonIpcClient>(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory<TClient>
}): TClient
export function createDaemonIpcClient(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory
}): DaemonIpcClient {
  return (options.createClient ?? createDefaultClient)({
    socketPath: readSocketPathFromDaemonUrl(options.daemonUrl),
  })
}

/** Creates one daemon IPC client from Node environment variables or injected env values. */
export function createDaemonIpcClientFromEnv<TClient = DaemonIpcClient>(options?: {
  env?: DaemonClientEnv
  createClient?: DaemonIpcClientFactory<TClient>
}): {
  daemonUrl: string
  client: TClient
}
export function createDaemonIpcClientFromEnv(
  options: { env?: DaemonClientEnv; createClient?: DaemonIpcClientFactory } = {},
): {
  daemonUrl: string
  client: DaemonIpcClient
} {
  const daemonUrl = resolveDaemonUrl(options.env)

  return {
    daemonUrl,
    client: createDaemonIpcClient({
      daemonUrl,
      createClient: options.createClient,
    }),
  }
}

/** Creates the default Node daemon IPC transport from one socket path. */
function createDefaultClient(input: DaemonIpcClientFactoryInput): DaemonIpcClient {
  return createNodeClient(input.socketPath, daemonIpcSchema)
}

/** Resolves the daemon URL from explicit environment variables or host defaults. */
function resolveDaemonUrl(env: DaemonClientEnv = process.env): string {
  if (env.GODDARD_DAEMON_URL) {
    return env.GODDARD_DAEMON_URL
  }

  return createDaemonUrl(env.GODDARD_DAEMON_SOCKET_PATH ?? getDefaultDaemonSocketPath())
}

/** Returns the default daemon socket path for the local Node host. */
export function getDefaultDaemonSocketPath(): string {
  if (process.platform === "win32") {
    // Bun's Windows `node:http` server still fails to boot reliably on named pipes,
    // so the daemon uses a fixed loopback origin for local-only IPC instead.
    return DEFAULT_DAEMON_LOOPBACK_ORIGIN
  }

  return path.posix.join(toPosixPath(getGoddardGlobalDir()), GODDARD_DAEMON_SOCKET_FILENAME)
}

/** Normalizes one host path into a posix-style socket path segment. */
function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
