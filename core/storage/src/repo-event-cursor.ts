import { mkdir, readFile, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { getRepoEventCursorPath } from "./paths.js"

/** Durable replay cursor stored for one authenticated GitHub user. */
export type RepoEventCursorRecord = {
  githubUsername: string
  lastEventId: number
  updatedAt: string
}

/** JSON file shape used to persist managed pull-request replay cursors. */
type RepoEventCursorFile = {
  cursors: Record<string, RepoEventCursorRecord>
}

/** Local storage for daemon-managed backend replay cursors. */
export namespace RepoEventCursorStorage {
  export async function get(githubUsername: string): Promise<RepoEventCursorRecord | null> {
    const data = await readCursorFile()
    return data.cursors[githubUsername] ?? null
  }

  export async function upsert(
    githubUsername: string,
    lastEventId: number,
  ): Promise<RepoEventCursorRecord> {
    const data = await readCursorFile()
    data.cursors[githubUsername] = {
      githubUsername,
      lastEventId,
      updatedAt: new Date().toISOString(),
    }
    await writeCursorFile(data)
    return data.cursors[githubUsername]!
  }
}

/** Reads the daemon replay cursor store, defaulting to an empty map. */
async function readCursorFile(): Promise<RepoEventCursorFile> {
  try {
    const raw = await readFile(getRepoEventCursorPath(), "utf-8")
    const parsed = JSON.parse(raw) as Partial<RepoEventCursorFile>
    return {
      cursors: parsed.cursors ?? {},
    }
  } catch {
    return { cursors: {} }
  }
}

/** Persists the daemon replay cursor store back to local storage. */
async function writeCursorFile(data: RepoEventCursorFile): Promise<void> {
  const path = getRepoEventCursorPath()
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, `${JSON.stringify(data, null, 2)}\n`, "utf-8")
}
