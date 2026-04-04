import type * as acp from "@agentclientprotocol/sdk"
import { getDatabasePath } from "@goddard-ai/paths/node"
import { DaemonSessionId } from "@goddard-ai/schema/common/params"
import { type DaemonSessionMetadata } from "@goddard-ai/schema/daemon"
import { SessionStatus } from "@goddard-ai/schema/db"
import { kind, kindstore, UnrecoverableStoreOpenError, type DatabaseOptions } from "kindstore"
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

const SessionPermissions = z.object({
  owner: z.string(),
  repo: z.string(),
  allowedPrNumbers: z.array(z.number().int()),
})

const WorktreeMetadata = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
})

const WorkforceMetadata = z
  .object({
    rootDir: z.string().optional(),
    agentId: z.string().optional(),
    requestId: z.string().optional(),
  })
  .catchall(z.unknown())

const schema = {
  sessions: kind(
    "ses",
    z.object({
      acpSessionId: z.string(),
      status: z.enum(SessionStatus),
      agentName: z.string(),
      cwd: z.string(),
      mcpServers: z.custom<acp.McpServer[]>(),
      connectionMode: z.custom<SessionConnectionMode>().default("none"),
      activeDaemonSession: z.boolean().default(false),
      errorMessage: z.string().nullable().default(null),
      blockedReason: z.string().nullable().default(null),
      initiative: z.string().nullable().default(null),
      lastAgentMessage: z.string().nullable().default(null),
      repository: z.string().nullable().default(null),
      prNumber: z.number().int().nullable().default(null),
      token: z.string().nullable().default(null),
      permissions: SessionPermissions.nullable().default(null),
      metadata: z.custom<DaemonSessionMetadata>().nullable().default(null),
      models: z.custom<acp.SessionModelState>().nullable().default(null),
    }),
  )
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

  sessionMessages: kind(
    "msg",
    z.object({
      sessionId: DaemonSessionId,
      messages: z.custom<acp.AnyMessage[]>(),
    }),
  ).index("sessionId", { type: "text" }),

  sessionDiagnostics: kind(
    "dgn",
    z.object({
      sessionId: DaemonSessionId,
      events: z.custom<SessionDiagnosticEvent[]>(),
    }),
  ).index("sessionId", { type: "text" }),

  worktrees: kind(
    "wt",
    z.object({
      sessionId: DaemonSessionId,
      ...WorktreeMetadata.shape,
    }),
  ).index("sessionId", { type: "text" }),

  workforces: kind(
    "wf",
    z
      .object({
        sessionId: DaemonSessionId,
        ...WorkforceMetadata.shape,
      })
      .catchall(z.unknown()),
  ).index("sessionId", { type: "text" }),

  pullRequests: kind(
    "pr",
    z.object({
      host: z.enum(["github"]),
      owner: z.string(),
      repo: z.string(),
      prNumber: z.number().int(),
      cwd: z.string(),
    }),
  )
    .updatedAt()
    .multi("host_owner_repo_prNumber", {
      host: "asc",
      owner: "asc",
      repo: "asc",
      prNumber: "asc",
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
export let db = openStore({ filename: getDatabasePath() })

/** Recreates the shared kindstore handle, optionally with explicit connection options. */
export function resetDb(connection: StoreConnectionOptions = { filename: getDatabasePath() }) {
  db.close()
  db = openStore(connection)
  return db
}
