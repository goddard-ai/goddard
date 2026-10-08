import type { AgentSession, QueuedMessage } from './generated'

/** Whether the session has a provider conversation whose provider must be preserved. */
export function sessionProviderLocked(session: AgentSession): boolean {
  return session.detail_loaded === false
    || Boolean(session.provider_cursor)
    || Boolean(session.provider_session_id)
    || session.messages.some((message) => message.role === 'user')
    || session.turns.some((turn) => turn.provider_turn_started)
}

/** The current turn can still consume a steer before its final text settles. */
export function sessionAcceptsImmediateSteer(session: AgentSession): boolean {
  const turn = session.turns.at(-1)
  return (session.status === 'working' || session.status === 'waiting' || session.status === 'background')
    && turn?.status === 'running'
    && turn.provider_turn_started === true
    && !(session.messages.at(-1)?.role === 'assistant' && session.messages.at(-1)?.streaming)
}

/** Whether this queue entry is a parked steer: a steer the provider could
 * not take mid-turn waits in the follow-up queue behind a transcript
 * preview — the user message carrying the entry's id. That preview renders
 * at the transcript's end, so the entry stays out of the queue card.
 * Mirrors `queued_message_is_pending_steer` in src/app/runtime.rs. */
export function queuedMessageIsPendingSteer(session: AgentSession, queued: QueuedMessage): boolean {
  return session.messages.some((message) => message.id === queued.id)
}

/** Append a parked steer's transcript preview — a user message carrying the
 * queue entry's id — mirroring `append_queued_steer_preview` in
 * src/app/runtime.rs. Hidden entries and already-previewed ids no-op. */
export function appendQueuedSteerPreview(
  session: AgentSession,
  queued: QueuedMessage,
): AgentSession {
  if (queued.hidden || session.messages.some((message) => message.id === queued.id)) {
    return session
  }
  return {
    ...session,
    messages: [
      ...session.messages,
      {
        id: queued.id,
        turn_id: null,
        role: 'user' as const,
        content: queued.content,
        display_content: queued.display_content,
        atoms: queued.atoms,
        attachments: queued.attachments,
        created_at: queued.created_at,
        streaming: false,
      },
    ],
  }
}

/** Drop a follow-up queue entry together with its parked-steer preview —
 * the preview message exists only while the entry is parked, so removal,
 * edit pull-back, and drain all clear both lists. Mirrors the paired
 * `queued_messages`/`messages` retains in src/app/runtime.rs. */
export function dropQueuedMessage(session: AgentSession, messageId: string): AgentSession {
  return {
    ...session,
    messages: session.messages.filter((message) => message.id !== messageId),
    queued_messages: (session.queued_messages ?? []).filter((queued) => queued.id !== messageId),
  }
}
