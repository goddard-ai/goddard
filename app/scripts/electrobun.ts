import { mkdir } from "node:fs/promises"
import { join } from "node:path"
import { supervise } from "procband"

import { appDir, getAppDevProcessEnv } from "./dev-environment.ts"

const electrobunMainViewDir = join(appDir, "build", "views", "main")

export async function startElectrobun() {
  await mkdir(electrobunMainViewDir, { recursive: true })

  supervise({
    name: "electrobun",
    command: "electrobun",
    args: ["dev", "--watch"],
    cwd: appDir,
    env: getAppDevProcessEnv(),
    detached: true,
    parentExitSignal: "SIGTERM",
  })
}
