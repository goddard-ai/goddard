import type * as acp from "@agentclientprotocol/sdk"
import { mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { dirname, join } from "node:path"
import { getGoddardGlobalDir } from "./paths.js"

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

export namespace SessionStateStorage {
  export async function list(): Promise<SessionStateRecord[]> {
    const directory = getSessionStateDir()
    try {
      const entries = await readDirectory(directory)
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

async function readDirectory(path: string): Promise<string[]> {
  const { readdir } = await import("node:fs/promises")
  return readdir(path)
}

function getSessionStateDir(): string {
  return join(getGoddardGlobalDir(), "session-state")
}

function getSessionStatePath(sessionId: string): string {
  return join(getSessionStateDir(), `${sessionId}.json`)
}

async function readStateFile(path: string): Promise<SessionStateRecord | null> {
  try {
    const raw = await readFile(path, "utf-8")
    return JSON.parse(raw) as SessionStateRecord
  } catch {
    return null
  }
}

async function writeStateFile(record: SessionStateRecord): Promise<void> {
  const path = getSessionStatePath(record.sessionId)
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, `${JSON.stringify(record, null, 2)}\n`, "utf-8")
}
