import type { AuthSession } from "@goddard-ai/schema/backend"
import type { SessionRecord } from "./api/in-memory-control-plane.js"

export function hashToInteger(value: string): number {
  let hash = 0
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(i)
    hash |= 0
  }
  return Math.abs(hash) + 1000
}

export function toPublicSession(session: SessionRecord): AuthSession {
  return {
    token: session.token,
    githubUsername: session.githubUsername,
    githubUserId: session.githubUserId,
  }
}
