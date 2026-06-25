import { mkdir } from "node:fs/promises"
import { delimiter, join } from "node:path"
import { fileURLToPath } from "node:url"
import { supervise } from "procband"

import { createLocalHttpUrl, getUnusedTcpPort } from "../../scripts/dev-ports.ts"

const appDir = fileURLToPath(new URL("..", import.meta.url))
const nodeModulesBin = join(appDir, "node_modules", ".bin")
const electrobunMainViewDir = join(appDir, "build", "views", "main")

/** Start Vite first, then launch Electrobun watch mode after the ready log appears. */
async function main() {
  process.env.NODE_ENV = "development"
  process.env.FORCE_COLOR = "1"
  process.env.PATH = process.env.PATH
    ? `${nodeModulesBin}${delimiter}${process.env.PATH}`
    : nodeModulesBin
  process.env.GODDARD_APP_DEV_SERVER_URL ??= createLocalHttpUrl(await getUnusedTcpPort())
  process.chdir(appDir)
  const devServerUrl = readDevServerUrl(process.env.GODDARD_APP_DEV_SERVER_URL)

  const vite = supervise({
    command: "vite",
    args: ["--host", devServerUrl.hostname, "--port", String(devServerUrl.port), "--strictPort"],
  })

  await vite.waitFor("ready")
  await mkdir(electrobunMainViewDir, { recursive: true })

  supervise({
    command: "electrobun",
    args: ["dev", "--watch"],
    detached: true,
    parentExitSignal: "SIGTERM",
  })
}

await main()

function readDevServerUrl(rawUrl: string) {
  const url = new URL(rawUrl)
  const port = Number(url.port)

  if (url.protocol !== "http:" || !Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("GODDARD_APP_DEV_SERVER_URL must be an http URL with an explicit TCP port")
  }

  return {
    hostname: url.hostname,
    port,
  }
}
