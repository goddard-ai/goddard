import type { AgentSession } from './generated'

/** Whether the session has a provider conversation whose provider must be preserved. */
export function sessionProviderLocked(session: AgentSession): boolean {
  return session.detail_loaded === false
    || Boolean(session.provider_cursor)
    || Boolean(session.provider_session_id)
    || session.messages.some((message) => message.role === 'user')
    || session.turns.some((turn) => turn.provider_turn_started)
}
