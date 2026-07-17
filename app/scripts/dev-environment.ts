import { delimiter, join } from "node:path"
import { fileURLToPath } from "node:url"

export const appDir = fileURLToPath(new URL("..", import.meta.url))

const nodeModulesBin = join(appDir, "node_modules", ".bin")

export function getAppDevProcessEnv() {
  return {
    ...process.env,
    NODE_ENV: "development",
    FORCE_COLOR: "1",
    PATH: process.env.PATH ? `${nodeModulesBin}${delimiter}${process.env.PATH}` : nodeModulesBin,
  }
}
