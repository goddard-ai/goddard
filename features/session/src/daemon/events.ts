import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"
import mitt from "mitt"

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

export const sessionEvents = mitt<{
  "lifecycle.worktreePrepared": SessionWorktreeLifecycleEvent & {
    request: CreateSessionRequest
  }
  "lifecycle.sessionPersisted": {
    sessionId: SessionId
    request: CreateSessionRequest
  }
  "lifecycle.sessionActivated": {
    sessionId: SessionId
    worktree: SessionWorktreeLifecycleState | null
  }
  "lifecycle.launchFinished": SessionWorktreeLifecycleEvent & {
    reason: "one_shot_completed"
  }
  "lifecycle.launchFailed": SessionWorktreeLifecycleEvent & {
    error: unknown
  }
  "lifecycle.sessionStopping": {
    sessionId: SessionId
    reason: "agent_process_exit" | "session_shutdown" | "daemon_shutdown"
    worktree: SessionWorktreeLifecycleState | null
  }
  "lifecycle.blocked": SessionAttentionEvent & {
    reason: string
  }
  "lifecycle.turnEnded": SessionAttentionEvent
  "lifecycle.replied": {
    sessionId: SessionId
  }
  "lifecycle.completed": {
    sessionId: SessionId
  }
}>()

export type SessionEventEmitter = typeof sessionEvents
