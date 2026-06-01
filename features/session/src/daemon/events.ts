import { event, type EventBus } from "@goddard-ai/daemon-plugin"
import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"

import type { CreateSessionRequest, SessionId } from "../schema.ts"

type SessionAttentionEvent = {
  sessionId: SessionId
  scope: AttentionScope
  headline: AttentionHeadline
  turnId: string | null
}

/** Worktree lifecycle data exposed to daemon plugins without exposing session persistence. */
export type SessionWorktreeLifecycleState = {
  sessionId: SessionId
  repoRoot: string
  requestedCwd: string
  effectiveCwd: string
  worktreeDir: string
  branchName: string
  poweredBy: string
}

type SessionWorktreeLifecycleEvent = {
  sessionId: SessionId
  worktree: SessionWorktreeLifecycleState
}

export const sessionEvents = {
  "session.worktree.prepared": event<
    SessionWorktreeLifecycleEvent & {
      request: CreateSessionRequest
    }
  >(),
  "session.persisted": event<{
    sessionId: SessionId
    request: CreateSessionRequest
  }>(),
  "session.activated": event<{
    sessionId: SessionId
    worktree: SessionWorktreeLifecycleState | null
  }>(),
  "session.launch.finished": event<
    SessionWorktreeLifecycleEvent & {
      reason: "one_shot_completed"
    }
  >(),
  "session.launch.failed": event<
    SessionWorktreeLifecycleEvent & {
      error: unknown
    }
  >(),
  "session.stopping": event<{
    sessionId: SessionId
    reason: "agent_process_exit" | "session_shutdown" | "daemon_shutdown"
    worktree: SessionWorktreeLifecycleState | null
  }>(),
  "session.blocked": event<
    SessionAttentionEvent & {
      reason: string
    }
  >(),
  "session.turn.ended": event<SessionAttentionEvent>(),
  "session.replied": event<{
    sessionId: SessionId
  }>(),
  "session.completed": event<{
    sessionId: SessionId
  }>(),
}

export type SessionEventEmitter = EventBus<typeof sessionEvents>
