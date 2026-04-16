import { supervise } from "procband"

async function main() {
  process.env.FORCE_COLOR = "1"

  const daemon = supervise({
    name: "daemon",
    command: "bun",
    args: ["dev", "run"],
    cwd: "core/daemon",
  })

  await daemon.waitFor(/ipc\.server_listening/)

  supervise({
    name: "app",
    command: "bun",
    args: ["dev"],
    cwd: "app",
  })
}

await main()
