import type { DaemonSession } from "@goddard-ai/sdk"

function basename(path: string) {
  const normalized = path.replace(/[\\/]+$/, "")
  const segments = normalized.split(/[\\/]/)
  return segments.at(-1) || path
}

function trimFirstLine(value: string) {
  return value.split(/\r?\n/, 1)[0]?.trim() ?? ""
}

export function getSessionDisplayTitle(session: DaemonSession) {
  return (
    session.initiative?.trim() ||
    trimFirstLine(session.lastAgentMessage ?? "") ||
    session.repository ||
    basename(session.cwd)
  )
}

export function getSessionPreviewText(session: DaemonSession) {
  return (
    session.blockedReason ??
    session.lastAgentMessage ??
    session.errorMessage ??
    session.repository ??
    session.cwd
  )
}
