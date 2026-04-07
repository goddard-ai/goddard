import {
  createDaemonUrl,
  createTcpDaemonUrl,
  readSocketPathFromDaemonUrl,
} from "@goddard-ai/schema/daemon-url"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import { mkdir, rm } from "node:fs/promises"
import * as path from "node:path"
import { ipcPath } from "../ipc-path.ts"
import type { NodeIpcServerTarget } from "@goddard-ai/ipc/node"

export { createDaemonUrl, createTcpDaemonUrl, readSocketPathFromDaemonUrl }

/** Daemon IPC listen settings resolved for either unix-socket or TCP transports. */
export type DaemonIpcListenTarget =
  | {
      type: "socket"
      socketPath: string
      daemonUrl: string
      bindTarget: string
    }
  | {
      type: "tcp"
      host: string
      port: number
      daemonUrl: string
      bindTarget: NodeIpcServerTarget
    }

export function getDefaultDaemonSocketPath(): string {
  return ipcPath.resolve(path.posix.join(toPosixPath(getGoddardGlobalDir()), "daemon.sock"))
}

/** Creates one listen target for daemon IPC transport setup. */
export function createDaemonIpcListenTarget(input: {
  socketPath: string | null
  tcpHost: string | null
  tcpPort: number | null
}): DaemonIpcListenTarget {
  if (input.tcpPort !== null) {
    const host = input.tcpHost ?? "127.0.0.1"
    return {
      type: "tcp",
      host,
      port: input.tcpPort,
      daemonUrl: createTcpDaemonUrl(host, input.tcpPort),
      bindTarget: {
        host,
        port: input.tcpPort,
      },
    }
  }

  if (!input.socketPath) {
    throw new Error("Daemon IPC socket path is required when TCP transport is disabled")
  }

  return {
    type: "socket",
    socketPath: input.socketPath,
    daemonUrl: createDaemonUrl(input.socketPath),
    bindTarget: input.socketPath,
  }
}

export async function prepareSocketPath(socketPath: string): Promise<void> {
  if (process.platform === "win32") {
    return
  }

  await mkdir(path.dirname(socketPath), { recursive: true })
  await ensureSocketPathAvailable(socketPath)
}

export async function cleanupSocketPath(socketPath: string): Promise<void> {
  if (process.platform === "win32") {
    return
  }

  await rm(socketPath, { force: true }).catch(() => {})
}

async function ensureSocketPathAvailable(socketPath: string): Promise<void> {
  try {
    await requestDaemonSocket(socketPath, "/health")
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

async function requestDaemonSocket(socketPath: string, pathname: string): Promise<void> {
  const response = await fetch(`http://localhost${pathname}`, {
    method: "GET",
    unix: socketPath,
  })

  // This probe only cares that the daemon accepted the socket request, not the payload body.
  await response.body?.cancel()
}

function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
