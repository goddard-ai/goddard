import { supervise } from "procband"

async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"

  const daemon = supervise({
    name: "daemon",
    command: "bun",
    args: ["dev", "run"],
    cwd: "core/daemon",
  })

  await daemon.waitFor(/ipc\.server_listening/)

  await import("../app/scripts/dev.ts")
}

await main()
