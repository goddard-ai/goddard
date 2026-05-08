import type { DaemonSession } from "@goddard-ai/sdk"

function basename(path: string) {
  const normalized = path.replace(/[\\/]+$/, "")
  const segments = normalized.split(/[\\/]/)
  return segments.at(-1) || path
}

export function getSessionDisplayTitle(session: DaemonSession) {
  return (
    session.title.trim() ||
    session.initiative?.trim() ||
    session.repository ||
    basename(session.cwd)
  )
}

export function getSessionRepositoryLabel(session: DaemonSession) {
  return session.repository?.trim() || basename(session.cwd)
}
