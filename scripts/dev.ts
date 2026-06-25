/*
 * Starts the local development supervisor for the daemon and app processes with
 * development environment defaults.
 */
import { supervise } from "procband"
import { concat } from "radashi"

async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"

  const backend = supervise({
    name: "backend",
    command: "pnpm",
    args: ["--dir", "core/backend", "run", "dev"],
  })

  await backend.waitFor(/Ready on http:\/\/(?:localhost|127\.0\.0\.1):8787/)

  const daemon = supervise({
    name: "daemon",
    command: "bun",
    args: concat(
      ["--watch", "run", "daemon/src/main.ts", "run"],
      process.argv.includes("--verbose") ? "--verbose" : undefined,
    ),
    cwd: "core",
  })

  await daemon.waitFor(/ipc\.server_listening/)

  await import("../app/scripts/dev.ts")
}

await main()
