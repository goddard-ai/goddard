import { randomUUID } from "node:crypto"
import os from "node:os"
import path from "node:path"
import type { SessionEndpoint } from "@goddard-ai/session-protocol"

export interface TransportOptions {
  transport?: "tcp" | "ipc"
  port?: number
  socketPath?: string
}

export type ServerListenTarget =
  | { kind: "tcp"; value: number; display: string }
  | { kind: "ipc"; value: string; display: string }

// Node uses the special `\\.\pipe` namespace for Windows named pipes, while
// Unix platforms expose local sockets as filesystem paths under a temp dir.
export function getDefaultIpcDirectory(platform = process.platform): string {
  if (platform === "win32") {
    return "\\\\.\\pipe"
  }

  return os.tmpdir()
}

// Session endpoints must not collide, so IPC mode generates a unique path for
// each session when the caller does not provide one explicitly.
export function createRandomSocketPath(platform = process.platform): string {
  const id = randomUUID()

  if (platform === "win32") {
    return `\\\\.\\pipe\\goddard-session-${id}`
  }

  return path.join(getDefaultIpcDirectory(platform), `goddard-session-${id}.sock`)
}

// This resolves the requested transport into the actual value passed to
// `server.listen(...)`. TCP defaults to port 0 so the OS allocates a free port.
export function resolveServerListenTarget(options: TransportOptions = {}): ServerListenTarget {
  if (options.socketPath) {
    return {
      kind: "ipc",
      value: options.socketPath,
      display: `ws+unix://${options.socketPath}`,
    }
  }

  if (options.transport === "ipc") {
    const socketPath = createRandomSocketPath()
    return {
      kind: "ipc",
      value: socketPath,
      display: `ws+unix://${socketPath}`,
    }
  }

  return {
    kind: "tcp",
    value: options.port ?? 0,
    display: `ws://localhost:${options.port ?? 0}`,
  }
}

// The externally shared endpoint must contain the real bound address, not the
// requested listen target, because TCP port 0 is resolved only after binding.
export function createServerEndpoint(
  listenTarget: ServerListenTarget,
  address: string | null,
): SessionEndpoint {
  if (listenTarget.kind === "ipc") {
    return {
      kind: "ipc",
      socketPath: listenTarget.value,
      url: `ws+unix://${listenTarget.value}`,
    }
  }

  if (!address) {
    throw new Error("A bound TCP address is required to create a server endpoint")
  }

  return {
    kind: "tcp",
    port: Number(address),
    url: `ws://localhost:${address}`,
  }
}
