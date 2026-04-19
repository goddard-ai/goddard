import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { supervise } from "procband"

const appDir = fileURLToPath(new URL("..", import.meta.url))
const nodeModulesBin = join(appDir, "node_modules", ".bin")

/** Start Vite first, then launch Electrobun watch mode after the ready log appears. */
async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"
  process.env.PATH = `${nodeModulesBin}:${process.env.PATH}`
  process.chdir(appDir)

  const vite = supervise({
    command: "vite",
  })

  await vite.waitFor("ready")

  supervise({
    command: "electrobun",
    args: ["dev", "--watch"],
    detached: true,
    parentExitSignal: "SIGTERM",
  })
}

await main()
