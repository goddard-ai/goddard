import { join } from "node:path"
import { homedir } from "node:os"

/** Returns the user-scoped global `.goddard` directory. */
export function getGoddardGlobalDir(): string {
  return join(homedir(), ".goddard")
}

/** Returns the global root config file path. */
export function getGlobalConfigPath(): string {
  return join(getGoddardGlobalDir(), "config.json")
}

/** Returns the repository-scoped local `.goddard` directory. */
export function getGoddardLocalDir(cwd: string = process.cwd()): string {
  return join(cwd, ".goddard")
}

/** Returns the local root config file path for one working directory. */
export function getLocalConfigPath(cwd: string = process.cwd()): string {
  return join(getGoddardLocalDir(cwd), "config.json")
}

/** Returns the daemon auth token file path. */
export function getAuthTokenPath(): string {
  return join(getGoddardGlobalDir(), "credentials.json")
}

/** Returns the daemon SQLite database path. */
export function getDatabasePath(): string {
  return join(getGoddardGlobalDir(), "goddard.db")
}

/** Returns the directory that holds daemon session-state JSON files. */
export function getSessionStateDir(): string {
  return join(getGoddardGlobalDir(), "session-state")
}

/** Returns the daemon session-state file path for one session id. */
export function getSessionStatePath(sessionId: string): string {
  return join(getSessionStateDir(), `${sessionId}.json`)
}

/** Returns the daemon session-permissions file path. */
export function getSessionPermissionsPath(): string {
  return join(getGoddardGlobalDir(), "session-permissions.json")
}

/** Returns the daemon managed-PR locations file path. */
export function getManagedPrLocationsPath(): string {
  return join(getGoddardGlobalDir(), "managed-pr-locations.json")
}

/** Returns the global and local root config paths in precedence order. */
export function getConfigRootPaths(cwd: string = process.cwd()): string[] {
  return [getGlobalConfigPath(), getLocalConfigPath(cwd)]
}
