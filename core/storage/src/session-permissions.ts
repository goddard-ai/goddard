import { mkdir, readFile, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { getSessionPermissionsPath } from "./paths.js"

export type SessionPermissionsRecord = {
  sessionId: string
  token: string
  owner: string
  repo: string
  allowedPrNumbers: number[]
  createdAt: string
}

type SessionPermissionsFile = {
  sessions: Record<string, SessionPermissionsRecord>
}

export namespace SessionPermissionsStorage {
  export async function create(record: Omit<SessionPermissionsRecord, "createdAt">) {
    const data = await readPermissionsFile()
    data.sessions[record.sessionId] = {
      ...record,
      createdAt: new Date().toISOString(),
    }
    await writePermissionsFile(data)
    return data.sessions[record.sessionId]!
  }

  export async function get(sessionId: string): Promise<SessionPermissionsRecord | null> {
    const data = await readPermissionsFile()
    return data.sessions[sessionId] ?? null
  }

  export async function getByToken(token: string): Promise<SessionPermissionsRecord | null> {
    const data = await readPermissionsFile()
    return Object.values(data.sessions).find((record) => record.token === token) ?? null
  }

  export async function list(): Promise<SessionPermissionsRecord[]> {
    const data = await readPermissionsFile()
    return Object.values(data.sessions)
  }

  export async function addAllowedPr(sessionId: string, prNumber: number): Promise<void> {
    const data = await readPermissionsFile()
    const record = data.sessions[sessionId]
    if (!record) {
      return
    }

    if (!record.allowedPrNumbers.includes(prNumber)) {
      record.allowedPrNumbers = [...record.allowedPrNumbers, prNumber]
      await writePermissionsFile(data)
    }
  }

  export async function revoke(sessionId: string): Promise<void> {
    const data = await readPermissionsFile()
    if (data.sessions[sessionId]) {
      delete data.sessions[sessionId]
      await writePermissionsFile(data)
    }
  }
}

async function readPermissionsFile(): Promise<SessionPermissionsFile> {
  try {
    const raw = await readFile(getSessionPermissionsPath(), "utf-8")
    const parsed = JSON.parse(raw) as Partial<SessionPermissionsFile>
    return {
      sessions: parsed.sessions ?? {},
    }
  } catch {
    return { sessions: {} }
  }
}

async function writePermissionsFile(data: SessionPermissionsFile): Promise<void> {
  const path = getSessionPermissionsPath()
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, `${JSON.stringify(data, null, 2)}\n`, "utf-8")
}
