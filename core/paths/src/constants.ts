/** Root directory name used for all Goddard-managed files and directories. */
export const GODDARD_DIRECTORY_NAME = ".goddard"

/** Directory name used for Goddard-managed OS cache contents. */
export const GODDARD_CACHE_DIRECTORY_NAME = "goddard"

/** Directory name used under the OS temp root for agent-readable runtime logs. */
export const GODDARD_TEMP_DIRECTORY_NAME = "goddard"

/** Directory name under the Goddard temp root that holds process logs. */
export const GODDARD_TEMP_LOG_DIRECTORY_NAME = "logs"

/** Filename used for the root JSON configuration document. */
export const GODDARD_CONFIG_FILENAME = "config.json"

/** Filename used for the daemon auth token store. */
export const GODDARD_AUTH_TOKEN_FILENAME = "credentials.json"

/** Filename used for the daemon SQLite database. */
export const GODDARD_DATABASE_FILENAME = "goddard.db"

/** Directory name used for development-only daemon persistence. */
export const GODDARD_DEVELOPMENT_DATA_DIRECTORY = "development"

/** Directory name used for daemon session-state JSON payloads. */
export const GODDARD_SESSION_STATE_DIRECTORY = "session-state"

/** Filename used for daemon session permission grants. */
export const GODDARD_SESSION_PERMISSIONS_FILENAME = "session-permissions.json"

/** Filename used for daemon-managed pull request location metadata. */
export const GODDARD_MANAGED_PR_LOCATIONS_FILENAME = "managed-pr-locations.json"

/** Directory name used for app-only user preference files. */
export const GODDARD_USER_DIRECTORY = "user"

/** Filename used for the app-owned state JSON file. */
export const GODDARD_APP_STATE_FILENAME = "app-state.json"

/** Filename used for the app keyboard shortcut keymap. */
export const GODDARD_SHORTCUT_KEYMAP_FILENAME = "keymap.json"
