import { join } from "node:path"
import { homedir } from "node:os"
import { access } from "node:fs/promises"
import { constants as fsConstants } from "node:fs"

export function getGoddardGlobalDir(): string {
  return join(homedir(), ".goddard")
}

export function getGlobalConfigPath(): string {
  return join(getGoddardGlobalDir(), "config.ts")
}

export function getLocalConfigPath(): string {
  return join(process.cwd(), ".goddard", "config.ts")
}

export function getDatabasePath(): string {
  return join(getGoddardGlobalDir(), "goddard.db")
}

export function getSessionPermissionsPath(): string {
  return join(getGoddardGlobalDir(), "session-permissions.json")
}

export async function fileExists(path: string): Promise<boolean> {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

export async function resolveLoopConfigPath(): Promise<string | null> {
  const localPath = getLocalConfigPath()
  if (await fileExists(localPath)) {
    return localPath
  }

  const globalPath = getGlobalConfigPath()
  if (await fileExists(globalPath)) {
    return globalPath
  }

  return null
}
