import { mkdirSync, rmSync } from "node:fs"
import { dirname } from "node:path"
import { getDatabasePath } from "@goddard-ai/paths/node"
import {
  DaemonPullRequest,
  DaemonSession,
  DaemonSessionDiagnostics,
  DaemonSessionTurn,
  DaemonSessionTurnDraft,
  DaemonWorkforce,
  DaemonWorktree,
} from "@goddard-ai/schema/daemon/store"
import {
  kind,
  kindstore,
  UnrecoverableStoreOpenError,
  type DatabaseOptions,
  type KindRegistry,
  type Kindstore,
} from "kindstore"
import { z } from "zod"

type StoreConnectionOptions = {
  filename: string
  databaseOptions?: DatabaseOptions
}

const metadata = {
  authToken: z.string(),
}

const coreDbSchema = {
  sessions: kind("ses", DaemonSession)
    .createdAt()
    .updatedAt()
    .index("acpSessionId")
    .index("repository")
    .index("token")
    .multi("repository_prNumber", {
      repository: "asc",
      prNumber: "asc",
    })
    .multi("updatedAt_id", {
      updatedAt: "desc",
      id: "desc",
    })
    .multi("completedHidden_updatedAt_id", {
      completedHidden: "asc",
      updatedAt: "desc",
      id: "desc",
    }),

  sessionTurns: kind("trn", DaemonSessionTurn)
    .index("sessionId", { type: "text" })
    .index("sequence", { type: "integer" })
    .multi("sessionId_sequence", {
      sessionId: "asc",
      sequence: "desc",
    }),

  sessionTurnDrafts: kind("drf", DaemonSessionTurnDraft)
    .index("sessionId", { type: "text" })
    .index("sequence", { type: "integer" })
    .multi("sessionId_sequence", {
      sessionId: "asc",
      sequence: "desc",
    }),

  sessionDiagnostics: kind("dgn", DaemonSessionDiagnostics).index("sessionId", {
    type: "text",
  }),

  worktrees: kind("wt", DaemonWorktree).index("sessionId", { type: "text" }),

  workforces: kind("wf", DaemonWorkforce).index("sessionId", { type: "text" }),

  pullRequests: kind("pr", DaemonPullRequest).updatedAt().multi(
    "host_owner_repo_prNumber",
    {
      host: "asc",
      owner: "asc",
      repo: "asc",
      prNumber: "asc",
    },
    { unique: true },
  ),
}

type DaemonStore = Kindstore<typeof coreDbSchema & KindRegistry, typeof metadata>

let activeSchema: KindRegistry = coreDbSchema
let activeConnection: StoreConnectionOptions = { filename: getDatabasePath() }

/** Opens one kindstore handle for a concrete daemon store schema. */
function createStore<const TSchema extends KindRegistry>(
  options: StoreConnectionOptions & { schema: keyof TSchema extends never ? never : TSchema },
) {
  if (options.filename !== ":memory:") {
    mkdirSync(dirname(options.filename), { recursive: true })
  }

  return kindstore({
    filename: options.filename,
    databaseOptions: options.databaseOptions,
    metadata,
    schema: options.schema,
  })
}

function removeDatabaseArtifacts(filename: string) {
  for (const suffix of ["", "-shm", "-wal"]) {
    rmSync(`${filename}${suffix}`, { force: true })
  }
}

function openStore(connection: StoreConnectionOptions) {
  try {
    return createStore({ ...connection, schema: activeSchema }) as DaemonStore
  } catch (error) {
    if (connection.filename === ":memory:" || !(error instanceof UnrecoverableStoreOpenError)) {
      throw error
    }

    removeDatabaseArtifacts(connection.filename)
    return createStore({ ...connection, schema: activeSchema }) as DaemonStore
  }
}

/** Sets the feature-contributed store schema before the shared daemon store is opened. */
export function configureDbSchema(pluginSchema: KindRegistry) {
  activeSchema = mergeDbSchema(pluginSchema)
  if (process.env.NODE_ENV !== "test" || db) {
    db?.close()
    db = openStore(activeConnection)
  }
}

function mergeDbSchema(pluginSchema: KindRegistry) {
  const schema: KindRegistry = { ...coreDbSchema }

  for (const [key, kindDefinition] of Object.entries(pluginSchema)) {
    if (Object.hasOwn(coreDbSchema, key)) {
      throw new Error(`Daemon plugin DB collection conflicts with core store schema: ${key}`)
    }
    schema[key] = kindDefinition
  }

  return schema
}

/**
 * Shared kindstore handle for daemon persistence.
 * Tests that override HOME should call `resetDb()` after changing it.
 */
export let db: DaemonStore = null!

/** Recreates the shared kindstore handle, optionally with explicit connection options. */
export function resetDb(connection: StoreConnectionOptions = { filename: getDatabasePath() }) {
  activeConnection = connection
  db?.close()
  db = openStore(connection)
  return db
}
