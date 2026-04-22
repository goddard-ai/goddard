import { homedir } from "node:os"
import { join } from "node:path"

import {
  GODDARD_ACP_REGISTRY_CACHE_DIRECTORY,
  GODDARD_AUTH_TOKEN_FILENAME,
  GODDARD_CACHE_DIRECTORY_NAME,
  GODDARD_CONFIG_FILENAME,
  GODDARD_DATABASE_FILENAME,
  GODDARD_DEVELOPMENT_DATA_DIRECTORY,
  GODDARD_DIRECTORY_NAME,
  GODDARD_MANAGED_PR_LOCATIONS_FILENAME,
  GODDARD_SESSION_PERMISSIONS_FILENAME,
  GODDARD_SESSION_STATE_DIRECTORY,
  GODDARD_SHORTCUT_KEYMAP_FILENAME,
  GODDARD_USER_DIRECTORY,
} from "../constants.ts"

/** Resolves the active home directory, preferring an explicit override for tests and Bun runs. */
function resolveHomeDir(): string {
  return process.env.HOME || homedir()
}

/** Returns whether the active process should isolate daemon persistence under the development root. */
function usesDevelopmentDataProfile(): boolean {
  return (
    process.env.GODDARD_DATA_PROFILE === "development" || process.env.NODE_ENV === "development"
  )
}

/** Returns the user-scoped global `.goddard` directory. */
export function getGoddardGlobalDir(): string {
  return join(resolveHomeDir(), GODDARD_DIRECTORY_NAME)
}

/** Returns the user-scoped OS cache directory used for disposable Goddard runtime state. */
export function getGoddardCacheDir(): string {
  if (process.platform === "darwin") {
    return join(resolveHomeDir(), "Library", "Caches", GODDARD_CACHE_DIRECTORY_NAME)
  }

  if (process.platform === "win32") {
    return join(
      process.env.LOCALAPPDATA || join(resolveHomeDir(), "AppData", "Local"),
      "Goddard",
      "Cache",
    )
  }

  return join(
    process.env.XDG_CACHE_HOME || join(resolveHomeDir(), ".cache"),
    GODDARD_CACHE_DIRECTORY_NAME,
  )
}

/** Returns the OS cache directory reserved for the ACP registry clone. */
export function getAcpRegistryCacheDir(): string {
  return join(getGoddardCacheDir(), GODDARD_ACP_REGISTRY_CACHE_DIRECTORY)
}

/** Returns the global root config file path. */
export function getGlobalConfigPath(): string {
  return join(getGoddardGlobalDir(), GODDARD_CONFIG_FILENAME)
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
  if (usesDevelopmentDataProfile()) {
    return join(
      getGoddardGlobalDir(),
      GODDARD_DEVELOPMENT_DATA_DIRECTORY,
      GODDARD_DATABASE_FILENAME,
    )
  }

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

/** Returns the global app-only user-preferences directory. */
export function getGoddardUserDir(): string {
  return join(getGoddardGlobalDir(), GODDARD_USER_DIRECTORY)
}

/** Returns the app keyboard shortcut keymap path. */
export function getShortcutKeymapPath(): string {
  return join(getGoddardUserDir(), GODDARD_SHORTCUT_KEYMAP_FILENAME)
}

/** Returns the global and local root config paths in precedence order. */
export function getConfigRootPaths(cwd: string): string[] {
  return [getGlobalConfigPath(), getLocalConfigPath(cwd)]
}
