import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"
import { event, type EventDefinition } from "@goddard-ai/sdk-plugin"

import type {
  CreateSessionRequest,
  SessionId,
  SessionLifecycleEvent,
  SessionMessageEvent,
} from "./schema.ts"

export type RoutedSessionMessageEvent = {
  id: SessionId
  message: SessionMessageEvent
}

export type SessionAttentionEvent = {
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

export type SessionWorktreeLifecycleEvent = {
  sessionId: SessionId
  worktree: SessionWorktreeLifecycleState
}

export type SessionWorktreePreparedEvent = SessionWorktreeLifecycleEvent & {
  request: CreateSessionRequest
}

export type SessionPersistedEvent = {
  sessionId: SessionId
  request: CreateSessionRequest
}

export type SessionActivatedEvent = {
  sessionId: SessionId
  worktree: SessionWorktreeLifecycleState | null
}

export type SessionLaunchFinishedEvent = SessionWorktreeLifecycleEvent & {
  reason: "one_shot_completed"
}

export type SessionLaunchFailedEvent = SessionWorktreeLifecycleEvent & {
  error: unknown
}

export type SessionStoppingEvent = {
  sessionId: SessionId
  reason: "agent_process_exit" | "session_shutdown" | "daemon_shutdown"
  worktree: SessionWorktreeLifecycleState | null
}

export type SessionBlockedEvent = SessionAttentionEvent & {
  reason: string
}

export type SessionIdEvent = {
  sessionId: SessionId
}

export type SessionEventDefinitions = {
  "session.worktree.prepared": EventDefinition<SessionWorktreePreparedEvent>
  "session.persisted": EventDefinition<SessionPersistedEvent>
  "session.activated": EventDefinition<SessionActivatedEvent>
  "session.launch.finished": EventDefinition<SessionLaunchFinishedEvent>
  "session.launch.failed": EventDefinition<SessionLaunchFailedEvent>
  "session.stopping": EventDefinition<SessionStoppingEvent>
  "session.blocked": EventDefinition<SessionBlockedEvent>
  "session.turn.ended": EventDefinition<SessionAttentionEvent>
  "session.replied": EventDefinition<SessionIdEvent>
  "session.completed": EventDefinition<SessionIdEvent>
  "session.message": EventDefinition<RoutedSessionMessageEvent>
  "session.lifecycle.updated": EventDefinition<
    Extract<SessionLifecycleEvent, { kind: "sessionUpdated" }>
  >
  "session.lifecycle.deleted": EventDefinition<
    Extract<SessionLifecycleEvent, { kind: "sessionDeleted" }>
  >
}

export const sessionEvents = {
  "session.worktree.prepared": event<SessionWorktreePreparedEvent>(),
  "session.persisted": event<SessionPersistedEvent>(),
  "session.activated": event<SessionActivatedEvent>(),
  "session.launch.finished": event<SessionLaunchFinishedEvent>(),
  "session.launch.failed": event<SessionLaunchFailedEvent>(),
  "session.stopping": event<SessionStoppingEvent>(),
  "session.blocked": event<SessionBlockedEvent>(),
  "session.turn.ended": event<SessionAttentionEvent>(),
  "session.replied": event<SessionIdEvent>(),
  "session.completed": event<SessionIdEvent>(),
  "session.message": event<RoutedSessionMessageEvent>({ debug: "session.stream" }),
  "session.lifecycle.updated": event<Extract<SessionLifecycleEvent, { kind: "sessionUpdated" }>>({
    debug: "session.lifecycle",
  }),
  "session.lifecycle.deleted": event<Extract<SessionLifecycleEvent, { kind: "sessionDeleted" }>>({
    debug: "session.lifecycle",
  }),
} satisfies SessionEventDefinitions
