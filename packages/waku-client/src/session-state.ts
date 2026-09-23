import type { AgentSession } from './generated'

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
