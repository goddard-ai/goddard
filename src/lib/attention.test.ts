import { describe, expect, test } from 'bun:test';

import type { AgentSession, BossDeliverable, BossEmployee, BossPlan, BossState } from '@waku/client';

import {
  attentionCounts,
  bossAttentionCounts,
  bossAttentionEvents,
  collectAttentionItems,
  nextAttentionItem,
  stripItems,
  waitingTransitions,
} from './attention';

const NOW = 1_800_000_000;

let seq = 0;
function session(overrides: Partial<AgentSession> = {}): AgentSession {
  seq += 1;
  return {
    id: `session-${seq}`,
    title: 'New task',
    auto_title: `Task ${seq}`,
    project_id: 'project-1',
    provider: 'codex',
    runtime_mode: 'autoAcceptEdits',
    status: 'idle',
    created_at: NOW - 100,
    updated_at: NOW - 10,
    last_reply_at: null,
    provider_cursor: null,
    messages: [],
    transcript_blocks: [],
    turns: [],
    queued_messages: [],
    ...overrides,
  };
}

function employee(overrides: Partial<BossEmployee> = {}): BossEmployee {
  seq += 1;
  return {
    sessionId: `employee-session-${seq}`,
    supervisorId: 'boss-session',
    identity: { id: `employee-${seq}`, name: `E${seq}`, avatarSeed: 'seed' },
    jobTitle: 'Verify the login fix',
    personaId: 'persona-1',
    workGoal: 'errand',
    permissions: {
      bucketIds: [],
      integrationIds: [],
      summonEmployees: false,
      computerUse: false,
    },
    pinnedFiles: [],
    expired: false,
    cancelled: false,
    state: 'working',
    ...overrides,
  };
}

function plan(overrides: Partial<BossPlan> = {}): BossPlan {
  seq += 1;
  return {
    id: `plan-${seq}`,
    sessionId: `plan-session-${seq}`,
    planFile: `plans/plan-${seq}.md`,
    idea: `Plan ${seq}`,
    finalizedAt: null,
    ...overrides,
  };
}

function deliverable(overrides: Partial<BossDeliverable> = {}): BossDeliverable {
  seq += 1;
  return {
    id: `deliverable-${seq}`,
    name: `Report ${seq}`,
    path: `/var/deliverables/report-${seq}.md`,
    directory: false,
    createdAt: NOW - 100,
    updatedAt: NOW - 10,
    pinnedAt: null,
    dormantAt: null,
    archivedAt: null,
    viewedAt: null,
    ...overrides,
  };
}

function state(overrides: Partial<BossState> = {}): BossState {
  return {
    identity: { id: 'boss-1', name: 'Boss', avatarSeed: 'boss' },
    personaId: 'persona-1',
    sessionId: 'boss-session',
    personas: [],
    employees: [],
    retiredEmployees: [],
    deliverables: [],
    planning: [],
    goalsViewedAt: null,
    resourcePolicy: { revision: 0, modelLimits: [] },
    nextSequence: 1,
    nextEventId: 1,
    nameCursor: 0,
    revision: 1,
    ...overrides,
  };
}

describe('collectAttentionItems', () => {
  test('a blocker reports as a decision item targeting the employee session', () => {
    const blocked = employee({ blocker: 'Needs the API key', sessionId: 'emp-1' });
    const items = collectAttentionItems({
      state: state({ employees: [blocked] }),
      sessions: [session({ id: 'emp-1' })],
      watermarks: {},
    });

    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      kind: 'blocker',
      detail: 'Needs the API key',
      target: { type: 'session', sessionId: 'emp-1' },
    });
  });

  test('a waiting boss chat, planning session, and employee session classify separately', () => {
    const planRecord = plan({ sessionId: 'plan-1', finalizedAt: null, idea: 'Auth rework' });
    const worker = employee({ sessionId: 'emp-1' });
    const items = collectAttentionItems({
      state: state({ employees: [worker], planning: [planRecord] }),
      sessions: [
        session({ id: 'boss-session', status: 'waiting' }),
        session({ id: 'plan-1', status: 'waiting', boss_managed: true }),
        session({ id: 'emp-1', status: 'waiting', boss_managed: true }),
        session({ id: 'ordinary-1', status: 'waiting' }),
      ],
      watermarks: {},
    });

    expect(items.map((item) => item.kind)).toEqual([
      'bossQuestion',
      'planApproval',
      'employeeWaiting',
      'waiting',
    ]);

    expect(items[1]).toMatchObject({ detail: 'Auth rework' });
  });

  test('unread deliverables, goal finishes, and plan finishes follow their seen contracts', () => {
    const fresh = deliverable({ id: 'd1', updatedAt: NOW - 10 });
    const viewed = deliverable({ id: 'd2', updatedAt: NOW - 10, viewedAt: NOW });
    const archived = deliverable({ id: 'd3', archivedAt: NOW });
    const sweptUnpinned = deliverable({ id: 'd4', dormantAt: NOW - 5 });
    const goal = employee({
      workGoal: 'goal',
      expired: true,
      state: 'expired',
      expiredAt: NOW - 10,
      sessionId: 'goal-1',
    });

    const oldGoal = employee({
      workGoal: 'goal',
      expired: true,
      state: 'expired',
      expiredAt: NOW - 500,
      sessionId: 'goal-2',
    });

    const cancelledGoal = employee({
      workGoal: 'goal',
      expired: true,
      state: 'expired',
      expiredAt: NOW - 10,
      cancelled: true,
      sessionId: 'goal-3',
    });

    const blockedGoal = employee({
      workGoal: 'goal',
      expired: true,
      state: 'expired',
      expiredAt: NOW - 10,
      blocker: 'Failed at the end',
      sessionId: 'goal-4',
    });

    const closedPlan = plan({
      outcome: 'completed',
      finalizedAt: NOW - 50,
      history: [{ outcome: 'approved', at: NOW - 50, actor: 'user' }, { outcome: 'completed', at: NOW - 10, actor: 'boss' }],
    });

    const oldPlan = plan({
      outcome: 'abandoned',
      finalizedAt: NOW - 500,
      history: [{ outcome: 'abandoned', at: NOW - 400, actor: 'user' }],
    });

    const items = collectAttentionItems({
      state: state({
        deliverables: [fresh, viewed, archived, sweptUnpinned],
        employees: [goal, oldGoal, cancelledGoal, blockedGoal],
        planning: [closedPlan, oldPlan],
        goalsViewedAt: NOW - 100,
      }),
      sessions: [session({ id: 'goal-1' }), session({ id: 'goal-4' })],
      watermarks: {},
    });

    const kinds = items.map((item) => `${item.kind}:${item.key}`);
    expect(kinds).toEqual([
      `blocker:blocker:goal-4`,
      `deliverable:deliverable:d1`,
      `goalFinished:goalFinished:goal-1`,
      `planFinished:planFinished:${closedPlan.id}`,
    ]);
  });

  test('unseen replies trail the list and the boss chat scans last', () => {
    const bossChat = session({ id: 'boss-session', last_reply_at: NOW - 5 });
    const task = session({ id: 'task-1', last_reply_at: NOW - 5 });
    const items = collectAttentionItems({
      state: state(),
      sessions: [bossChat, task],
      watermarks: { 'boss-session': NOW - 100, 'task-1': NOW - 100 },
    });

    expect(items.map((item) => item.target)).toEqual([
      { type: 'session', sessionId: 'task-1' },
      { type: 'session', sessionId: 'boss-session' },
    ]);
  });

  test('nothing pulls when the daemon has no boss document', () => {
    const items = collectAttentionItems({
      state: null,
      sessions: [session({ status: 'waiting' })],
      watermarks: {},
    });

    // An ordinary waiting task still counts — only boss kinds need the document.
    expect(items.map((item) => item.kind)).toEqual(['waiting']);
  });
});

describe('stripItems', () => {
  test('the strip keeps the boss pull set and drops ordinary waits, unreads, and the on-screen ask', () => {
    const items = collectAttentionItems({
      state: state({
        employees: [employee({ blocker: 'Stuck', sessionId: 'emp-1' })],
        planning: [plan({ sessionId: 'plan-1' })],
        deliverables: [deliverable()],
      }),
      sessions: [
        session({ id: 'boss-session', status: 'waiting' }),
        session({ id: 'plan-1', status: 'waiting' }),
        session({ id: 'other-1', status: 'waiting' }),
        session({ id: 'other-2', last_reply_at: NOW }),
      ],
      watermarks: { 'other-2': NOW - 100 },
    });

    expect(stripItems(items).map((item) => item.kind)).toEqual([
      'blocker',
      'planApproval',
      'deliverable',
    ]);
  });
});

describe('nextAttentionItem', () => {
  const items = collectAttentionItems({
    state: state({
      employees: [employee({ blocker: 'Stuck', sessionId: 'emp-1' })],
      deliverables: [deliverable({ id: 'd1' })],
    }),
    sessions: [session({ id: 'emp-1' }), session({ id: 'task-1', last_reply_at: NOW })],
    watermarks: { 'task-1': NOW - 100 },
  });

  test('decisions lead; the viewed session and visited keys are skipped', () => {
    expect(items[0]!.kind).toBe('blocker');
    expect(nextAttentionItem(items, new Set(), null)?.key).toBe('blocker:emp-1');
    expect(nextAttentionItem(items, new Set(), 'emp-1')?.key).toBe('deliverable:d1');
    expect(
      nextAttentionItem(items, new Set(['blocker:emp-1', 'deliverable:d1']), null)?.key,
    ).toBe('unseen:task-1');

    expect(
      nextAttentionItem(items, new Set(['blocker:emp-1', 'deliverable:d1', 'unseen:task-1']), null),
    ).toBeNull();
  });
});

describe('attentionCounts', () => {
  test('needs-you counts decisions; fresh counts arrivals', () => {
    const unread = session({ last_reply_at: NOW });
    const items = collectAttentionItems({
      state: state({
        employees: [employee({ blocker: 'Stuck' })],
        deliverables: [deliverable()],
      }),
      sessions: [session({ status: 'waiting' }), unread],
      watermarks: { [unread.id]: NOW - 100 },
    });

    const counts = attentionCounts(items);
    expect(counts.needsYou).toBe(2);
    expect(counts.fresh).toBe(2);
    // The boss row counts the pull set only — unseen replies stay on the dot.
    expect(bossAttentionCounts(items).fresh).toBe(1);
  });
});

describe('bossAttentionEvents', () => {
  test('first sync and identity changes report nothing', () => {
    const current = state({ employees: [employee({ blocker: 'x' })] });
    expect(bossAttentionEvents(null, current)).toEqual([]);
    expect(
      bossAttentionEvents(state(), state({ identity: { id: 'boss-2', name: 'Other', avatarSeed: 'o' } })),
    ).toEqual([]);
  });

  test('blocker, deliverable, and finalize transitions report; persistent conditions stay quiet', () => {
    const blocked = employee({ blocker: 'Stuck', sessionId: 'emp-1' });
    const pub = deliverable({ id: 'd1' });
    const sealed = plan({ finalizedAt: NOW });

    const events = bossAttentionEvents(
      state(),
      state({ employees: [blocked], deliverables: [pub], planning: [sealed] }),
    );

    expect(events.map((event) => event.type)).toEqual([
      'employeeBlocker',
      'deliverablePublished',
      'planFinalized',
    ]);

    // Same state again — nothing refires.
    expect(
      bossAttentionEvents(
        state({ employees: [blocked], deliverables: [pub], planning: [sealed] }),
        state({ employees: [blocked], deliverables: [pub], planning: [sealed] }),
      ),
    ).toEqual([]);
  });

  test('goal finishes report; errands and cancelled or blocked finishes stay quiet', () => {
    const finished = employee({
      workGoal: 'goal',
      sessionId: 'g1',
      expired: true,
      state: 'expired',
      expiredAt: NOW,
    });

    const errand = employee({
      sessionId: 'e1',
      expired: true,
      state: 'expired',
      expiredAt: NOW,
    });

    const cancelled = employee({
      workGoal: 'goal',
      sessionId: 'g2',
      expired: true,
      state: 'expired',
      expiredAt: NOW,
      cancelled: true,
    });

    const blocked = employee({
      workGoal: 'goal',
      sessionId: 'g3',
      expired: true,
      state: 'expired',
      expiredAt: NOW,
      blocker: 'Died on a merge conflict',
    });

    const before = state({
      employees: [
        employee({ workGoal: 'goal', sessionId: 'g1' }),
        employee({ sessionId: 'e1' }),
        employee({ workGoal: 'goal', sessionId: 'g2' }),
        employee({ workGoal: 'goal', sessionId: 'g3' }),
      ],
    });

    const after = state({ employees: [finished, errand, cancelled, blocked] });

    const events = bossAttentionEvents(before, after);
    expect(events.map((event) => event.type)).toEqual(['employeeBlocker', 'employeeFinished']);
  });

  test('plan outcome transitions report once', () => {
    const record = plan({ id: 'p1', outcome: 'completed' });
    const events = bossAttentionEvents(
      state({ planning: [plan({ id: 'p1', outcome: 'approved' })] }),
      state({ planning: [record] }),
    );

    expect(events).toEqual([{ type: 'planFinished', plan: record }]);
  });
});

describe('waitingTransitions', () => {
  test('only sessions newly entering waiting report', () => {
    const parked = session({ id: 'a', status: 'waiting' });
    const stillWaiting = session({ id: 'b', status: 'waiting' });
    const working = session({ id: 'c', status: 'working' });
    const before = new Map([
      ['a', 'working' as const],
      ['b', 'waiting' as const],
      ['c', 'working' as const],
    ]);

    expect(waitingTransitions(undefined, [parked])).toEqual([]);
    expect(
      waitingTransitions(before, [parked, stillWaiting, working]).map((item) => item.id),
    ).toEqual(['a']);
  });
});
