import { realpath } from "node:fs/promises"
import { isAbsolute, resolve } from "node:path"

export async function normalizePath(path: string) {
  return await realpath(resolve(path))
}

export function resolveGitOutputPath(cwd: string, value: string) {
  return isAbsolute(value) ? value : resolve(cwd, value)
}
