import { describe, expect, test } from 'bun:test';
import type { AgentSession, BossPlan } from '@waku/client';
import { PLAN_APPROVAL_PROMPT, insertPlanMention, planApprovalSent, planFilter, planMention, planReferenceId } from './plans';
import { parseBlocks } from '../md/parse';

const draft: BossPlan = { id: 'plan-id', sessionId: 'discussion', idea: 'Ship [auth] safely', planFile: 'plans/auth.md' };

describe('desktop plan lifecycle', () => {
  test('active drafts require a living, unarchived discussion', () => {
    expect(planFilter(draft, { archived_at: null })).toBe('active');
    expect(planFilter(draft, { archived_at: 123 })).toBe('archived');
    expect(planFilter(draft)).toBe('archived');
  });

  test('approved plans stay approved after discussion archive or deletion', () => {
    const approved = { ...draft, finalizedAt: 123 };
    expect(planFilter(approved, { archived_at: 234 })).toBe('approved');
    expect(planFilter(approved)).toBe('approved');
  });

  test('approval locks on a visible user message or queued follow-up', () => {
    const base = { messages: [], queued_messages: [] } as Pick<AgentSession, 'messages' | 'queued_messages'>;
    const approved = { role: 'user', hidden: false, content: ` ${PLAN_APPROVAL_PROMPT.toUpperCase()} ` } as AgentSession['messages'][number];
    expect(planApprovalSent(base)).toBe(false);
    expect(planApprovalSent({ messages: [approved] })).toBe(true);
    expect(planApprovalSent({ messages: [{ ...approved, hidden: true }] })).toBe(false);
    expect(planApprovalSent({ messages: [{ ...approved, role: 'assistant' }] })).toBe(false);
    const queued = { content: 'provider expansion', display_content: PLAN_APPROVAL_PROMPT } as NonNullable<AgentSession['queued_messages']>[number];
    expect(planApprovalSent({ ...base, queued_messages: [queued] })).toBe(true);
  });
});

describe('plan references', () => {
  test('picking a plan replaces a partial mention or selected text', () => {
    const reference = planMention(draft);
    expect(insertPlanMention('Ask @auth', { start: 9, end: 9 }, draft)).toEqual({ text: `Ask ${reference} `, cursor: reference.length + 5 });
    expect(insertPlanMention('Ask old scope', { start: 4, end: 7 }, draft).text).toBe(`Ask ${reference} scope`);
  });

  test('mention escapes its label, renders a link and resolves the durable id', () => {
    const node = parseBlocks(planMention(draft))[0]!.node;
    expect(node.type).toBe('paragraph');
    if (node.type !== 'paragraph') {throw new Error('Expected paragraph');}
    const link = node.children[0]!;
    expect(link.type).toBe('link');
    if (link.type !== 'link') {throw new Error('Expected link');}
    expect(planReferenceId(link.url)).toBe(draft.id);
    expect(link.children).toEqual([expect.objectContaining({ type: 'text', value: '@Ship [auth] safely' })]);
  });

  test('rejects unrelated URLs and malformed escapes without throwing', () => {
    expect(planReferenceId('goddard://task/id')).toBeNull();
    expect(planReferenceId('goddard://plan/%E0')).toBeNull();
    expect(planReferenceId('goddard://plan/')).toBeNull();
    expect(planReferenceId('goddard://plan/plan-id/')).toBe('plan-id');
  });
});
