import type { KindInput } from "kindstore"
import type { DaemonSessionMetadata } from "@goddard-ai/schema/daemon"
import { db } from "./store.ts"

function stripDaemonOwnedMetadata(metadata: DaemonSessionMetadata | null | undefined) {
  if (typeof metadata !== "object" || metadata === null) {
    return null
  }

  const { worktree: _worktree, workforce: _workforce, ...rest } = metadata
  return Object.keys(rest).length > 0 ? rest : null
}

/** Converts one session insert shape into the stored kindstore payload. */
export function normalizeSessionInsert(
  data: KindInput<typeof db.schema.sessions>,
): KindInput<typeof db.schema.sessions> {
  const repository = data.repository ?? null
  const prNumber = typeof data.prNumber === "number" ? data.prNumber : null

  return {
    acpSessionId: data.acpSessionId,
    status: data.status,
    agentName: data.agentName,
    cwd: data.cwd,
    mcpServers: data.mcpServers,
    connectionMode: data.connectionMode ?? "none",
    activeDaemonSession: data.activeDaemonSession ?? false,
    errorMessage: data.errorMessage ?? null,
    blockedReason: data.blockedReason ?? null,
    initiative: data.initiative ?? null,
    lastAgentMessage: data.lastAgentMessage ?? null,
    repository,
    prNumber,
    token: data.token ?? null,
    permissions: data.permissions ?? null,
    metadata: stripDaemonOwnedMetadata(data.metadata),
    models: data.models ?? null,
  }
}
