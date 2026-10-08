import type { AgentSession, BossPlan } from '@waku/client';

export type PlansFilter = 'active' | 'approved' | 'archived';
export const PLAN_APPROVAL_PROMPT = 'The design is approved — finalize it and hand implementation to the boss.';

export function planFilter(plan: BossPlan, session?: Pick<AgentSession, 'archived_at'>): PlansFilter {
  if (plan.finalizedAt != null) {return 'approved';}
  return session && session.archived_at == null ? 'active' : 'archived';
}

export function planApprovalSent(session: Pick<AgentSession, 'messages' | 'queued_messages'>, approvalPrompt = PLAN_APPROVAL_PROMPT): boolean {
  const matches = (text: string) => text.trim().toLowerCase() === approvalPrompt.trim().toLowerCase();
  return session.messages.some((message) => message.role === 'user' && !message.hidden && matches(message.display_content ?? message.content))
    || (session.queued_messages ?? []).some((message) => matches(message.display_content ?? message.content));
}

export function planReferenceId(url: string): string | null {
  const match = /^goddard:\/\/plan\/([^/?#]+)\/?$/u.exec(url);
  if (!match) {return null;}
  try {
    return decodeURIComponent(match[1]!);
  } catch {
    return null;
  }
}

/** A plan mention is a readable internal link, so transcript and provider
 * see the same reference and tapping it opens the frozen-or-draft document. */
export function planMention(plan: Pick<BossPlan, 'id' | 'idea'>): string {
  const label = plan.idea.replace(/[\\[\]]/gu, '\\$&').replace(/\s+/gu, ' ').trim() || 'Plan';
  return `[@${label}](goddard://plan/${encodeURIComponent(plan.id)})`;
}

export function insertPlanMention(text: string, selection: { start: number; end: number }, plan: Pick<BossPlan, 'id' | 'idea'>): { text: string; cursor: number } {
  let start = Math.max(0, Math.min(selection.start, text.length));
  const end = Math.max(start, Math.min(selection.end, text.length));
  if (start === end) {
    const partial = text.slice(0, start).match(/(?:^|\s)(@\S*)$/u);
    if (partial) {start -= partial[1]!.length;}
  }

  const before = text.slice(0, start);
  const mention = `${before && !/\s$/u.test(before) ? ' ' : ''}${planMention(plan)} `;
  return { text: before + mention + text.slice(end).replace(/^ /u, ''), cursor: before.length + mention.length };
}
