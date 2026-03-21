import { join } from "node:path"
import { homedir } from "node:os"
import { access } from "node:fs/promises"
import { constants as fsConstants } from "node:fs"

export function getGoddardGlobalDir(): string {
  return join(homedir(), ".goddard")
}

export function getGlobalConfigPath(): string {
  return join(getGoddardGlobalDir(), "config.json")
}

export function getGoddardLocalDir(cwd: string = process.cwd()): string {
  return join(cwd, ".goddard")
}

export function getLocalConfigPath(cwd: string = process.cwd()): string {
  return join(getGoddardLocalDir(cwd), "config.json")
}

export function getDatabasePath(): string {
  return join(getGoddardGlobalDir(), "goddard.db")
}

export function getSessionPermissionsPath(): string {
  return join(getGoddardGlobalDir(), "session-permissions.json")
}

export function getManagedPrLocationsPath(): string {
  return join(getGoddardGlobalDir(), "managed-pr-locations.json")
}

export function getRepoEventCursorPath(): string {
  return join(getGoddardGlobalDir(), "repo-event-cursors.json")
}

export async function fileExists(path: string): Promise<boolean> {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

export function getConfigRootPaths(cwd: string = process.cwd()): string[] {
  return [getGlobalConfigPath(), getLocalConfigPath(cwd)]
}
