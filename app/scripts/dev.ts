import { mkdir } from "node:fs/promises"
import { delimiter, join } from "node:path"
import { fileURLToPath } from "node:url"
import { supervise } from "procband"

import { createLocalHttpUrl, getUnusedTcpPort } from "../../scripts/dev-ports.ts"

const appDir = fileURLToPath(new URL("..", import.meta.url))
const nodeModulesBin = join(appDir, "node_modules", ".bin")
const electrobunMainViewDir = join(appDir, "build", "views", "main")

async function startViteDevServer() {
  const devServerUrl = new URL(
    (process.env.GODDARD_APP_DEV_SERVER_URL ??= createLocalHttpUrl(await getUnusedTcpPort())),
  )
  const vite = supervise({
    command: "vite",
    args: ["--port", devServerUrl.port, "--strictPort"],
  })
  await vite.waitFor("ready")
}

async function startElectrobun() {
  await mkdir(electrobunMainViewDir, { recursive: true })

  supervise({
    command: "electrobun",
    args: ["dev", "--watch"],
    detached: true,
    parentExitSignal: "SIGTERM",
  })
}

async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"
  process.env.PATH = process.env.PATH
    ? `${nodeModulesBin}${delimiter}${process.env.PATH}`
    : nodeModulesBin

  process.chdir(appDir)

  await startViteDevServer()
  await startElectrobun()
}

await main()
