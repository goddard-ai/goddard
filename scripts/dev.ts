/*
 * Starts the local development supervisor for the daemon and app processes with
 * development environment defaults.
 */
import { supervise } from "procband"

import { createLocalHttpUrl, getUnusedTcpPort } from "./dev-ports.ts"

async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"
  const backendPort = await getUnusedTcpPort()
  const daemonPort = await getUnusedTcpPort()
  const backendUrl = createLocalHttpUrl(backendPort)
  const daemonUrl = createLocalHttpUrl(daemonPort)
  process.env.GODDARD_BASE_URL = backendUrl
  process.env.GODDARD_DAEMON_URL = daemonUrl

  const backend = supervise({
    name: "backend",
    command: "pnpm",
    args: ["--dir", "core/backend", "run", "dev", "--port", String(backendPort)],
  })

  await backend.waitFor("Ready on")

  const daemonArgs = ["--watch", "run", "daemon/src/main.ts", "run", "--port", String(daemonPort)]
  if (process.argv.includes("--verbose")) {
    daemonArgs.push("--verbose")
  }

  const daemon = supervise({
    name: "daemon",
    command: "bun",
    args: daemonArgs,
    cwd: "core",
  })

  await daemon.waitFor(/ipc\.server_listening/)

  await import("../app/scripts/dev.ts")
}

await main()
