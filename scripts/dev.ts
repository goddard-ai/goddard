import { fileURLToPath } from "node:url"
import { supervise } from "procband"
import { concat } from "radashi"

import { createLocalHttpUrl, getUnusedTcpPort } from "./dev-ports.ts"

const rootDir = fileURLToPath(new URL("..", import.meta.url))

async function startBackend() {
  const backendUrl = new URL(
    (process.env.GODDARD_BASE_URL ??= createLocalHttpUrl(await getUnusedTcpPort())),
  )
  const backend = supervise({
    name: "backend",
    command: "pnpm",
    args: ["--dir", "core/backend", "run", "dev", "--port", backendUrl.port],
  })
  await backend.waitFor("Ready on")
}

async function startDaemon() {
  const daemonUrl = new URL(
    (process.env.GODDARD_DAEMON_URL ??= createLocalHttpUrl(await getUnusedTcpPort())),
  )
  const daemonArgs = []
  if (process.argv.includes("--verbose")) {
    daemonArgs.push("--verbose")
  }
  const daemon = supervise({
    name: "daemon",
    command: "bun",
    args: concat(
      ["--watch", "run", "core/daemon/src/main.ts"],
      "run",
      ["--port", daemonUrl.port],
      process.argv.includes("--verbose") ? "--verbose" : undefined,
    ),
  })
  await daemon.waitFor(/ipc\.server_listening/)
}

async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"

  process.chdir(rootDir)

  await startBackend()
  await startDaemon()
  await import("../app/scripts/dev.ts")
}

await main()
