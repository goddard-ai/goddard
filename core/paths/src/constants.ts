/** Root directory name used for all Goddard-managed files and directories. */
export const GODDARD_DIRECTORY_NAME = ".goddard"

/** Filename used for the root JSON configuration document. */
export const GODDARD_CONFIG_FILENAME = "config.json"

/** Filename used for the daemon Unix socket or named-pipe basename. */
export const GODDARD_DAEMON_SOCKET_FILENAME = "daemon.sock"

/** Filename used for the daemon auth token store. */
export const GODDARD_AUTH_TOKEN_FILENAME = "credentials.json"

/** Filename used for the daemon SQLite database. */
export const GODDARD_DATABASE_FILENAME = "goddard.db"

/** Directory name used for daemon session-state JSON payloads. */
export const GODDARD_SESSION_STATE_DIRECTORY = "session-state"

/** Filename used for daemon session permission grants. */
export const GODDARD_SESSION_PERMISSIONS_FILENAME = "session-permissions.json"

/** Filename used for daemon-managed pull request location metadata. */
export const GODDARD_MANAGED_PR_LOCATIONS_FILENAME = "managed-pr-locations.json"
