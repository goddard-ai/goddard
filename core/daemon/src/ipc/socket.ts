import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import {
  DEFAULT_DAEMON_LOOPBACK_ORIGIN,
  createDaemonUrl,
  readSocketPathFromDaemonUrl,
} from "@goddard-ai/schema/daemon-url"
import { mkdir, rm } from "node:fs/promises"
import * as path from "node:path"

import { ipcPath } from "../ipc-path.ts"

export { createDaemonUrl, readSocketPathFromDaemonUrl }

export function getDefaultDaemonSocketPath(): string {
  if (process.platform === "win32") {
    // Bun's Windows `node:http` server still fails to boot reliably on named pipes,
    // so the daemon uses a fixed loopback origin for local-only IPC instead.
    return DEFAULT_DAEMON_LOOPBACK_ORIGIN
  }

  return ipcPath.resolve(path.posix.join(toPosixPath(getGoddardGlobalDir()), "daemon.sock"))
}

export async function prepareSocketPath(socketPath: string): Promise<void> {
  if (readNetworkOrigin(socketPath)) {
    return
  }

  await mkdir(path.dirname(socketPath), { recursive: true })
  await ensureSocketPathAvailable(socketPath)
}

export async function cleanupSocketPath(socketPath: string): Promise<void> {
  if (readNetworkOrigin(socketPath)) {
    return
  }

  await rm(socketPath, { force: true }).catch(() => {})
}

async function ensureSocketPathAvailable(socketPath: string): Promise<void> {
  try {
    await requestSocket(socketPath, "/health")
    throw new Error(`A Goddard daemon is already listening at ${socketPath}`)
  } catch (error) {
    const code = typeof error === "object" && error && "code" in error ? error.code : undefined
    if (code === "FailedToOpenSocket") {
      await rm(socketPath, { force: true }).catch(() => {})
      return
    }

    throw error
  }
}

async function requestSocket(socketPath: string, pathname: string): Promise<void> {
  const networkOrigin = readNetworkOrigin(socketPath)
  const response = await fetch(
    networkOrigin ? new URL(pathname, networkOrigin) : `http://localhost${pathname}`,
    networkOrigin
      ? {
          method: "GET",
        }
      : {
          method: "GET",
          unix: socketPath,
        },
  )

  // This probe only cares that the daemon accepted the socket request, not the payload body.
  await response.body?.cancel()
}

/** Parses one IPC target into a loopback URL when the daemon is using network transport. */
function readNetworkOrigin(socketPath: string) {
  try {
    const url = new URL(socketPath)
    return url.protocol === "http:" ? url : null
  } catch {
    return null
  }
}

function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
