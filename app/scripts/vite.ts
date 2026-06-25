import { supervise } from "procband"

import { createLocalHttpUrl, getUnusedTcpPort } from "../../scripts/dev-ports.ts"
import { appDir, getAppDevProcessEnv } from "./dev-environment.ts"

export async function startViteDevServer() {
  const devServerUrl = new URL(
    (process.env.GODDARD_APP_DEV_SERVER_URL ??= createLocalHttpUrl(await getUnusedTcpPort())),
  )
  const vite = supervise({
    name: "vite",
    command: "vite",
    args: ["--port", devServerUrl.port, "--strictPort"],
    cwd: appDir,
    env: getAppDevProcessEnv(),
  })
  await vite.waitFor("ready")
}
