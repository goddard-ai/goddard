import type * as acp from "@agentclientprotocol/sdk"
import { getDatabasePath } from "@goddard-ai/paths/node"
import { type DaemonSessionMetadata } from "@goddard-ai/schema/daemon"
import { SessionStatus } from "@goddard-ai/schema/db"
import { kind, kindstore, type DatabaseOptions } from "kindstore"
import { mkdirSync, rmSync } from "node:fs"
import { dirname } from "node:path"
import { z } from "zod"
import type { SessionConnectionMode, SessionDiagnosticEvent } from "./session-state.ts"

type StoreConnectionOptions = {
  filename: string
  databaseOptions?: DatabaseOptions
}

const metadata = {
  authToken: z.string(),
}

const schema = {
  sessions: kind(
    "ses",
    z.object({
      sessionId: z.string(),
      acpId: z.string(),
      status: z.enum(SessionStatus),
      agentName: z.string(),
      cwd: z.string(),
      mcpServers: z.custom<acp.McpServer[]>(),
      errorMessage: z.string().nullable(),
      blockedReason: z.string().nullable(),
      initiative: z.string().nullable(),
      lastAgentMessage: z.string().nullable(),
      repository: z.string().nullable(),
      prNumber: z.number().int().nullable(),
      repositoryPrKey: z.string().nullable(),
      metadata: z.custom<DaemonSessionMetadata | null>(),
      models: z.custom<acp.SessionModelState | null>(),
      sortKey: z.string(),
    }),
  )
    .createdAt()
    .updatedAt()
    .index("sessionId")
    .index("acpId")
    .index("repository")
    .index("repositoryPrKey")
    .index("sortKey"),

  sessionStates: kind(
    "sst",
    z.object({
      sessionId: z.string(),
      acpId: z.string(),
      connectionMode: z.custom<SessionConnectionMode>(),
      history: z.custom<acp.AnyMessage[]>(),
      diagnostics: z.custom<SessionDiagnosticEvent[]>(),
      activeDaemonSession: z.boolean(),
    }),
  )
    .createdAt()
    .updatedAt()
    .index("sessionId"),

  sessionPermissions: kind(
    "spr",
    z.object({
      sessionId: z.string(),
      token: z.string(),
      owner: z.string(),
      repo: z.string(),
      allowedPrNumbers: z.array(z.number().int()),
    }),
  )
    .createdAt()
    .index("sessionId")
    .index("token"),

  managedPrLocations: kind(
    "mpl",
    z.object({
      locationKey: z.string(),
      owner: z.string(),
      repo: z.string(),
      prNumber: z.number().int(),
      cwd: z.string(),
    }),
  )
    .updatedAt()
    .index("locationKey"),
}

function createStore(options: StoreConnectionOptions) {
  if (options.filename !== ":memory:") {
    mkdirSync(dirname(options.filename), { recursive: true })
  }

  return kindstore({
    filename: options.filename,
    databaseOptions: options.databaseOptions,
    metadata,
    schema,
  })
}

function removeDatabaseArtifacts(filename: string) {
  for (const suffix of ["", "-shm", "-wal"]) {
    rmSync(`${filename}${suffix}`, { force: true })
  }
}

function openStore(connection: StoreConnectionOptions) {
  try {
    return createStore(connection)
  } catch (error) {
    if (
      connection.filename === ":memory:" ||
      (error instanceof Error &&
        error.message.includes("kindstore format version") === false &&
        error.message.includes("cannot be opened safely") === false &&
        error.message.includes("newer than supported version") === false)
    ) {
      throw error
    }

    removeDatabaseArtifacts(connection.filename)
    return createStore(connection)
  }
}

/**
 * Shared kindstore handle for daemon persistence.
 * Tests that override HOME should call `resetDb()` after changing it.
 */
export let db = openStore({ filename: getDatabasePath() })

/** Recreates the shared kindstore handle, optionally with explicit connection options. */
export function resetDb(connection: StoreConnectionOptions = { filename: getDatabasePath() }) {
  if (db) {
    db.close()
  }
  db = openStore(connection)
  return db
}
