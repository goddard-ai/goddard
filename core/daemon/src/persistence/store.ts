import { mkdirSync, rmSync } from "node:fs"
import { dirname } from "node:path"
import { getDatabasePath } from "@goddard-ai/paths/node"
import {
  DaemonInboxItem,
  DaemonPullRequest,
  DaemonSession,
  DaemonSessionDiagnostics,
  DaemonSessionTurn,
  DaemonSessionTurnDraft,
  DaemonWorkforce,
  DaemonWorktree,
} from "@goddard-ai/schema/daemon/store"
import { kind, kindstore, UnrecoverableStoreOpenError, type DatabaseOptions } from "kindstore"
import { z } from "zod"

type StoreConnectionOptions = {
  filename: string
  databaseOptions?: DatabaseOptions
}

const metadata = {
  authToken: z.string(),
}

const schema = {
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

  inboxItems: kind("inb", DaemonInboxItem)
    .index("entityId", { type: "text", unique: true })
    .index("status")
    .multi("updatedAt_id", {
      updatedAt: "desc",
      id: "desc",
    }),
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
    if (connection.filename === ":memory:" || !(error instanceof UnrecoverableStoreOpenError)) {
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
export let db = process.env.NODE_ENV !== "test" ? openStore({ filename: getDatabasePath() }) : null!

/** Recreates the shared kindstore handle, optionally with explicit connection options. */
export function resetDb(connection: StoreConnectionOptions = { filename: getDatabasePath() }) {
  db?.close()
  db = openStore(connection)
  return db
}
