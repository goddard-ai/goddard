export const DEFAULT_DAEMON_HOST = "127.0.0.1"
export const DEFAULT_DAEMON_PORT = 49827

/** Parsed TCP address extracted from one daemon URL. */
export type DaemonTcpAddress = {
  hostname: string
  port: number
}

/** Creates one daemon URL from a TCP hostname and port. */
export function createDaemonUrl(port: number, hostname = DEFAULT_DAEMON_HOST): string {
  assertDaemonPort(port, "Daemon URL port")

  const url = new URL("http://localhost")
  url.hostname = hostname
  url.port = String(port)
  return url.toString()
}

/** Reads the TCP hostname and port from one daemon URL. */
export function readDaemonTcpAddressFromDaemonUrl(rawUrl: string): DaemonTcpAddress {
  let url: URL
  try {
    url = new URL(rawUrl)
  } catch {
    throw new Error("GODDARD_DAEMON_URL must be a valid URL")
  }

  if (url.protocol !== "http:") {
    throw new Error("GODDARD_DAEMON_URL must use the http:// TCP URL format")
  }

  if (url.pathname !== "/" || url.search || url.hash) {
    throw new Error("GODDARD_DAEMON_URL must not include a path, query, or hash")
  }

  if (!url.port) {
    throw new Error("GODDARD_DAEMON_URL must include an explicit TCP port")
  }

  const port = Number(url.port)
  assertDaemonPort(port, "GODDARD_DAEMON_URL")

  return {
    hostname: url.hostname,
    port,
  }
}

function assertDaemonPort(port: number, label: string) {
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    throw new Error(`${label} must use an integer TCP port between 0 and 65535`)
  }
}
