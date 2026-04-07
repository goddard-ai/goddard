/** Daemon URL connection details for a unix domain socket transport. */
export type DaemonSocketConnection = {
  type: "socket"
  socketPath: string
}

/** Daemon URL connection details for a TCP transport. */
export type DaemonTcpConnection = {
  type: "tcp"
  host: string
  port: number
}

/** Parsed daemon connection details shared by daemon client hosts. */
export type DaemonConnection = DaemonSocketConnection | DaemonTcpConnection

/** Builds the canonical daemon URL for unix domain socket IPC. */
export function createDaemonUrl(socketPath: string): string {
  const url = new URL("http://unix")
  url.searchParams.set("socketPath", socketPath)
  return url.toString()
}

/** Builds the canonical daemon URL for TCP daemon IPC. */
export function createTcpDaemonUrl(host: string, port: number): string {
  const url = new URL(`http://${host}:${port}`)
  return url.toString()
}

/** Parses the daemon URL into transport-specific connection details. */
export function readDaemonConnectionFromDaemonUrl(rawUrl: string): DaemonConnection {
  let url: URL
  try {
    url = new URL(rawUrl)
  } catch {
    throw new Error("GODDARD_DAEMON_URL must be a valid URL")
  }

  if (url.protocol !== "http:") {
    throw new Error("GODDARD_DAEMON_URL must use the http protocol")
  }

  if (url.hostname === "unix") {
    const socketPath = url.searchParams.get("socketPath")
    if (!socketPath) {
      throw new Error("GODDARD_DAEMON_URL is missing socketPath")
    }

    return {
      type: "socket",
      socketPath,
    }
  }

  if (!url.hostname) {
    throw new Error("GODDARD_DAEMON_URL is missing a TCP host")
  }

  if (!url.port) {
    throw new Error("GODDARD_DAEMON_URL is missing a TCP port")
  }

  const parsedPort = Number(url.port)
  if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65_535) {
    throw new Error("GODDARD_DAEMON_URL TCP port must be an integer between 1 and 65535")
  }

  return {
    type: "tcp",
    host: url.hostname,
    port: parsedPort,
  }
}

/** Reads and validates only unix-socket daemon URLs. */
export function readSocketPathFromDaemonUrl(rawUrl: string): string {
  const connection = readDaemonConnectionFromDaemonUrl(rawUrl)
  if (connection.type !== "socket") {
    throw new Error("GODDARD_DAEMON_URL does not use a unix socket daemon URL")
  }

  return connection.socketPath
}
