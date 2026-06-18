/*
 * Starts the local development supervisor for the daemon and app processes with
 * development environment defaults.
 */
import { supervise } from "procband"
import { concat } from "radashi"

async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"

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
