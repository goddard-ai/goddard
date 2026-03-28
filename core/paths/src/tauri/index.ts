import { homeDir, join } from "@tauri-apps/api/path"
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

/** Returns the user-scoped global `.goddard` directory through the Tauri path API. */
export async function getGoddardGlobalDir(): Promise<string> {
  return join(await homeDir(), GODDARD_DIRECTORY_NAME)
}

/** Returns the global root config file path through the Tauri path API. */
export async function getGlobalConfigPath(): Promise<string> {
  return join(await getGoddardGlobalDir(), GODDARD_CONFIG_FILENAME)
}

/** Returns the daemon socket file path under the global `.goddard` directory. */
export async function getDaemonSocketPath(): Promise<string> {
  return join(await getGoddardGlobalDir(), GODDARD_DAEMON_SOCKET_FILENAME)
}

/** Returns the repository-scoped local `.goddard` directory. */
export async function getGoddardLocalDir(cwd: string): Promise<string> {
  return join(cwd, GODDARD_DIRECTORY_NAME)
}

/** Returns the local root config file path for one working directory. */
export async function getLocalConfigPath(cwd: string): Promise<string> {
  return join(await getGoddardLocalDir(cwd), GODDARD_CONFIG_FILENAME)
}

/** Returns the daemon auth token file path through the Tauri path API. */
export async function getAuthTokenPath(): Promise<string> {
  return join(await getGoddardGlobalDir(), GODDARD_AUTH_TOKEN_FILENAME)
}

/** Returns the daemon SQLite database path through the Tauri path API. */
export async function getDatabasePath(): Promise<string> {
  return join(await getGoddardGlobalDir(), GODDARD_DATABASE_FILENAME)
}

/** Returns the directory that holds daemon session-state JSON files. */
export async function getSessionStateDir(): Promise<string> {
  return join(await getGoddardGlobalDir(), GODDARD_SESSION_STATE_DIRECTORY)
}

/** Returns the daemon session-state file path for one session id. */
export async function getSessionStatePath(sessionId: string): Promise<string> {
  return join(await getSessionStateDir(), `${sessionId}.json`)
}

/** Returns the daemon session-permissions file path through the Tauri path API. */
export async function getSessionPermissionsPath(): Promise<string> {
  return join(await getGoddardGlobalDir(), GODDARD_SESSION_PERMISSIONS_FILENAME)
}

/** Returns the daemon managed-PR locations file path through the Tauri path API. */
export async function getManagedPrLocationsPath(): Promise<string> {
  return join(await getGoddardGlobalDir(), GODDARD_MANAGED_PR_LOCATIONS_FILENAME)
}

/** Returns the global and local root config paths in precedence order. */
export async function getConfigRootPaths(cwd: string): Promise<string[]> {
  return [await getGlobalConfigPath(), await getLocalConfigPath(cwd)]
}
