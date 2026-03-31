import type * as acp from "@agentclientprotocol/sdk"
import { getSessionStateDir, getSessionStatePath } from "@goddard-ai/paths/node"
import { mkdir, readdir, rm } from "node:fs/promises"
import { dirname, join } from "node:path"

/** Durable connectivity summary for a daemon session across daemon restarts. */
export type SessionConnectionMode = "live" | "history" | "none"

/** Structured session diagnostic event persisted for postmortem inspection. */
export type SessionDiagnosticEvent = {
  type: string
  at: string
  sessionId: string
  detail?: Record<string, unknown>
}

/** Durable daemon-owned session state that supplements the SQL session row. */
export type SessionStateRecord = {
  sessionId: string
  acpId: string
  connectionMode: SessionConnectionMode
  history: acp.AnyMessage[]
  diagnostics: SessionDiagnosticEvent[]
  activeDaemonSession: boolean
  createdAt: string
  updatedAt: string
}

/** File-backed storage for daemon session history, diagnostics, and connection state. */
export namespace SessionStateStorage {
  export async function list(): Promise<SessionStateRecord[]> {
    const directory = getSessionStateDir()
    try {
      const entries = await readdir(directory)
      const records = await Promise.all(
        entries.map((entry) => readStateFile(join(directory, entry))),
      )
      return records.filter((record): record is SessionStateRecord => record !== null)
    } catch {
      return []
    }
  }

  export async function get(sessionId: string): Promise<SessionStateRecord | null> {
    return readStateFile(getSessionStatePath(sessionId))
  }

  export async function create(record: Omit<SessionStateRecord, "createdAt" | "updatedAt">) {
    const now = new Date().toISOString()
    const created: SessionStateRecord = {
      ...record,
      createdAt: now,
      updatedAt: now,
    }
    await writeStateFile(created)
    return created
  }

  export async function update(
    sessionId: string,
    update: Partial<Omit<SessionStateRecord, "sessionId" | "createdAt">>,
  ) {
    const existing = await get(sessionId)
    if (!existing) {
      return null
    }

    const nextRecord: SessionStateRecord = {
      ...existing,
      ...update,
      sessionId,
      createdAt: existing.createdAt,
      updatedAt: new Date().toISOString(),
    }
    await writeStateFile(nextRecord)
    return nextRecord
  }

  export async function appendHistory(sessionId: string, message: acp.AnyMessage) {
    const existing = await get(sessionId)
    if (!existing) {
      return null
    }

    return update(sessionId, {
      history: [...existing.history, message],
    })
  }

  export async function appendDiagnostic(sessionId: string, event: SessionDiagnosticEvent) {
    const existing = await get(sessionId)
    if (!existing) {
      return null
    }

    return update(sessionId, {
      diagnostics: [...existing.diagnostics, event],
    })
  }

  export async function remove(sessionId: string): Promise<void> {
    await rm(getSessionStatePath(sessionId), { force: true }).catch(() => {})
  }
}

/** Reads and parses one persisted session-state record when it exists. */
async function readStateFile(path: string): Promise<SessionStateRecord | null> {
  try {
    const raw = await Bun.file(path).text()
    return JSON.parse(raw) as SessionStateRecord
  } catch {
    return null
  }
}

/** Writes one complete session-state snapshot back to durable storage. */
async function writeStateFile(record: SessionStateRecord): Promise<void> {
  const path = getSessionStatePath(record.sessionId)
  await mkdir(dirname(path), { recursive: true })
  await Bun.write(path, `${JSON.stringify(record, null, 2)}\n`)
}
