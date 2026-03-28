import { homedir } from "node:os"
import { join } from "node:path"
import {
  GODDARD_AUTH_TOKEN_FILENAME,
  GODDARD_CONFIG_FILENAME,
  GODDARD_DATABASE_FILENAME,
  GODDARD_DAEMON_SOCKET_FILENAME,
  GODDARD_DIRECTORY_NAME,
  GODDARD_MANAGED_PR_LOCATIONS_FILENAME,
  GODDARD_SESSION_PERMISSIONS_FILENAME,
  GODDARD_SESSION_STATE_DIRECTORY,
} from "../constants.ts"

/** Returns the user-scoped global `.goddard` directory. */
export function getGoddardGlobalDir(): string {
  return join(homedir(), GODDARD_DIRECTORY_NAME)
}

/** Returns the global root config file path. */
export function getGlobalConfigPath(): string {
  return join(getGoddardGlobalDir(), GODDARD_CONFIG_FILENAME)
}

/** Returns the daemon socket file path under the global `.goddard` directory. */
export function getDaemonSocketPath(): string {
  return join(getGoddardGlobalDir(), GODDARD_DAEMON_SOCKET_FILENAME)
}

/** Returns the repository-scoped local `.goddard` directory. */
export function getGoddardLocalDir(cwd: string): string {
  return join(cwd, GODDARD_DIRECTORY_NAME)
}

/** Returns the local root config file path for one working directory. */
export function getLocalConfigPath(cwd: string): string {
  return join(getGoddardLocalDir(cwd), GODDARD_CONFIG_FILENAME)
}

/** Returns the daemon auth token file path. */
export function getAuthTokenPath(): string {
  return join(getGoddardGlobalDir(), GODDARD_AUTH_TOKEN_FILENAME)
}

/** Returns the daemon SQLite database path. */
export function getDatabasePath(): string {
  return join(getGoddardGlobalDir(), GODDARD_DATABASE_FILENAME)
}

/** Returns the directory that holds daemon session-state JSON files. */
export function getSessionStateDir(): string {
  return join(getGoddardGlobalDir(), GODDARD_SESSION_STATE_DIRECTORY)
}

/** Returns the daemon session-state file path for one session id. */
export function getSessionStatePath(sessionId: string): string {
  return join(getSessionStateDir(), `${sessionId}.json`)
}

/** Returns the daemon session-permissions file path. */
export function getSessionPermissionsPath(): string {
  return join(getGoddardGlobalDir(), GODDARD_SESSION_PERMISSIONS_FILENAME)
}

/** Returns the daemon managed-PR locations file path. */
export function getManagedPrLocationsPath(): string {
  return join(getGoddardGlobalDir(), GODDARD_MANAGED_PR_LOCATIONS_FILENAME)
}

/** Returns the global and local root config paths in precedence order. */
export function getConfigRootPaths(cwd: string): string[] {
  return [getGlobalConfigPath(), getLocalConfigPath(cwd)]
}
