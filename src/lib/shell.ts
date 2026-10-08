import type { AgentSession, BossState } from '@waku/client';

/** Managed records remain in the Boss block even when an older session
 * summary omits boss_managed. Retired employees are managed too. */
export function ordinarySessions(
  sessions: AgentSession[],
  boss: BossState | null | undefined,
): AgentSession[] {
  const managed = new Set([
    boss?.sessionId,
    ...(boss?.planning.map((plan) => plan.sessionId) ?? []),
    ...(boss?.employees.map((employee) => employee.sessionId) ?? []),
    ...(boss?.retiredEmployees.map((employee) => employee.sessionId) ?? []),
  ]);

  return sessions.filter((session) => session.boss_managed !== true && !managed.has(session.id));
}
