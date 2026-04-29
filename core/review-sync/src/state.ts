/** Durable review-sync session state, event, and patch storage helpers. */
import { createHash } from "node:crypto"
import { appendFile, mkdir, readdir, readFile, rename, writeFile } from "node:fs/promises"
import { join } from "node:path"

import { isNodeErrorWithCode } from "./git.ts"
import { schemaVersion, type SessionState } from "./types.ts"

/** Creates a deterministic, ref-safe session id for one agent/review pairing. */
export function createSessionId(input: {
  repoCommonDir: string
  agentWorktree: string
  reviewWorktree: string
  agentBranch: string
}) {
  return `sha256-${createHash("sha256")
    .update(
      JSON.stringify({
        repoCommonDir: input.repoCommonDir,
        agentWorktree: input.agentWorktree,
        reviewWorktree: input.reviewWorktree,
        agentBranch: input.agentBranch,
      }),
    )
    .digest("hex")}`
}

/** Finds a session that already owns one review branch. */
export async function findSessionByReviewBranch(commonDir: string, reviewBranch: string) {
  const sessions = await listSessions(commonDir)
  return sessions.find((session) => session.reviewBranch === reviewBranch) ?? null
}

/** Reads every valid session state file for one repository common directory. */
export async function listSessions(commonDir: string) {
  const sessionsRoot = resolveSessionsRoot(commonDir)
  let entries: string[]
  try {
    entries = await readdir(sessionsRoot)
  } catch {
    return []
  }

  const sessions: SessionState[] = []
  for (const entry of entries) {
    try {
      sessions.push(await readSessionStateFile(join(sessionsRoot, entry, "state.json")))
    } catch {
      // Ignore malformed or partially written session directories during discovery.
    }
  }
  return sessions
}

/** Loads one durable session state file for a known session. */
export async function readSessionState(session: SessionState) {
  return await readSessionStateFile(
    join(resolveSessionDir(session.repoCommonDir, session.sessionId), "state.json"),
  )
}

/** Parses a durable state JSON file. */
export async function readSessionStateFile(statePath: string) {
  const parsed = JSON.parse(await readFile(statePath, "utf-8")) as SessionState
  if (parsed.schemaVersion !== schemaVersion) {
    throw new Error(`Unsupported review-sync state schema in ${statePath}.`)
  }
  return parsed
}

/** Writes state through a temporary file before renaming it over the durable state path. */
export async function writeSessionState(session: SessionState) {
  await ensureSessionDirs(session)
  const statePath = join(resolveSessionDir(session.repoCommonDir, session.sessionId), "state.json")
  const tmpPath = `${statePath}.${process.pid}.${Date.now()}.tmp`
  await writeFile(tmpPath, `${JSON.stringify(session, null, 2)}\n`)
  await rename(tmpPath, statePath)
}

/** Creates the session directory tree used for state, locks, events, and patches. */
export async function ensureSessionDirs(session: SessionState) {
  const sessionDir = resolveSessionDir(session.repoCommonDir, session.sessionId)
  await mkdir(join(sessionDir, "patches", "accepted"), { recursive: true })
  await mkdir(join(sessionDir, "patches", "rejected"), { recursive: true })
  await mkdir(join(sessionDir, "patches", "pending"), { recursive: true })
}

/** Appends one audit event as newline-delimited JSON. */
export async function appendEvent(
  session: SessionState,
  event: Record<string, string | number | boolean | null>,
) {
  const sessionDir = resolveSessionDir(session.repoCommonDir, session.sessionId)
  await mkdir(sessionDir, { recursive: true })
  await appendFile(
    join(sessionDir, "events.ndjson"),
    `${JSON.stringify({
      timestamp: new Date().toISOString(),
      sessionId: session.sessionId,
      agentBranch: session.agentBranch,
      reviewBranch: session.reviewBranch,
      ...event,
    })}\n`,
  )
}

/** Writes accepted or rejected patch contents to a deterministic fingerprinted file. */
export async function savePatch(
  session: SessionState,
  kind: "accepted" | "rejected",
  patch: string,
) {
  const patchDir = join(
    resolveSessionDir(session.repoCommonDir, session.sessionId),
    "patches",
    kind,
  )
  await mkdir(patchDir, { recursive: true })
  const patchPath = join(patchDir, `${createHash("sha256").update(patch).digest("hex")}.patch`)
  try {
    await writeFile(patchPath, patch, { flag: "wx" })
  } catch (error) {
    if (!isNodeErrorWithCode(error, "EEXIST")) {
      throw error
    }
  }
  return patchPath
}

/** Counts fingerprinted patch files in one patch category directory. */
export async function countPatchFiles(patchDir: string) {
  try {
    const entries = await readdir(patchDir)
    return entries.filter((entry) => entry.endsWith(".patch")).length
  } catch {
    return 0
  }
}

/** Returns the directory containing every session for one repository common dir. */
export function resolveSessionsRoot(commonDir: string) {
  return join(commonDir, "review-sync", "sessions")
}

/** Returns the durable state directory for one session. */
export function resolveSessionDir(commonDir: string, sessionId: string) {
  return join(resolveSessionsRoot(commonDir), sessionId)
}
