import { describe, expect, test } from 'bun:test';

import type { AgentSession, BossEmployee, BossPlan, BossState, Project } from '@waku/client';

import {
  bossGoalPlanGroups,
  bossGoalSections,
  bossGoalStatus,
  bossGoalsUnreadCount,
  goalProjectLabel,
  goalStatusMarker,
  goalTitle,
  goalUpdatedAt,
  goalUpdatedLabel,
} from './goals';

function employee(overrides: Partial<BossEmployee> = {}): BossEmployee {
  return {
    sessionId: 'session-1',
    supervisorId: 'boss-session',
    identity: { id: 'employee-1', name: 'Ada', avatarSeed: 'seed' },
    jobTitle: 'Verify the login fix',
    personaId: 'persona-1',
    workGoal: 'goal',
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

function session(overrides: Partial<AgentSession> = {}): AgentSession {
  return {
    id: 'session-1',
    title: 'New task',
    project_id: 'project-1',
    status: 'working',
    created_at: 1,
    updated_at: 10,
    turns: [],
    messages: [],
    ...overrides,
  } as AgentSession;
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
    revision: 0,
    ...overrides,
  };
}

const projects = new Map<string, Project>([
  ['project-1', { id: 'project-1', name: 'goddard', path: '/goddard' } as Project],
]);

describe('bossGoalStatus', () => {
  test('queued and dispatching lifecycle bucket before session evidence', () => {
    expect(bossGoalStatus(employee({ state: 'queued' }), undefined))
      .toEqual({ bucket: 'pending', status: 'queued' });

    expect(bossGoalStatus(employee({ state: 'dispatching' }), undefined))
      .toEqual({ bucket: 'inProgress', status: 'starting' });
  });

  test('expired lifecycle reads explicit outcomes ahead of thread state', () => {
    const expired = { expired: true, expiredAt: 50, state: 'expired' as const };
    expect(bossGoalStatus(employee(expired), session())).toEqual({
      bucket: 'finished',
      status: 'finished',
    });

    expect(
      bossGoalStatus(employee(expired), session({ status: 'failed' })).status,
    ).toBe('failed');

    expect(
      bossGoalStatus(employee({ ...expired, blocker: 'needs a key' }), session()).status,
    ).toBe('attention');

    expect(
      bossGoalStatus(
        employee(expired),
        session({ thread_goal: { objective: 'Ship', status: 'complete', tokensUsed: 0, timeUsedSeconds: 0 } }),
      ).status,
    ).toBe('complete');

    expect(
      bossGoalStatus(
        employee(expired),
        session({ thread_goal: { objective: 'Ship', status: 'budgetLimited', tokensUsed: 0, timeUsedSeconds: 0 } }),
      ).status,
    ).toBe('budgetReached');
  });

  test('a working employee reads terminal thread results as finished', () => {
    expect(
      bossGoalStatus(
        employee(),
        session({ thread_goal: { objective: 'Ship', status: 'complete', tokensUsed: 0, timeUsedSeconds: 0 } }),
      ),
    ).toEqual({ bucket: 'finished', status: 'complete' });

    expect(
      bossGoalStatus(employee(), session({ status: 'failed' })),
    ).toEqual({ bucket: 'inProgress', status: 'failed' });

    expect(
      bossGoalStatus(employee({ state: 'finishing' }), session()),
    ).toEqual({ bucket: 'inProgress', status: 'finishing' });
  });

  test('working precedence: blocker, thread states, then session status', () => {
    expect(
      bossGoalStatus(employee({ blocker: 'stuck' }), session({ status: 'working' })).status,
    ).toBe('attention');

    expect(
      bossGoalStatus(
        employee(),
        session({ thread_goal: { objective: 'Ship', status: 'paused', tokensUsed: 0, timeUsedSeconds: 0 } }),
      ).status,
    ).toBe('paused');

    expect(bossGoalStatus(employee(), session({ status: 'waiting' })).status).toBe('needsInput');
    expect(bossGoalStatus(employee(), session({ status: 'idle' })).status).toBe('active');
    expect(bossGoalStatus(employee(), undefined).status).toBe('unavailable');
  });
});

describe('goalTitle', () => {
  test('resolves explicit title, then auto, goal objective, queued prompt, job title', () => {
    const boss = employee({ identity: { id: 'e', name: 'Ada', avatarSeed: 's' } });
    expect(goalTitle(boss, session({ title: 'Ship the release' }))).toBe('Ship the release');
    expect(goalTitle(boss, session({ auto_title: 'Investigate flakes' }))).toBe('Investigate flakes');
    expect(
      goalTitle(boss, session({ thread_goal: { objective: 'Land U5', status: 'active', tokensUsed: 0, timeUsedSeconds: 0 } })),
    ).toBe('Land U5');

    expect(
      goalTitle(
        employee({ ticket: { prompt: 'Audit the queue\n\nDetails follow', project: 'x' } as never }),
        undefined,
      ),
    ).toBe('Audit the queue');

    expect(goalTitle(employee(), undefined)).toBe('Verify the login fix');
    expect(goalTitle(employee({ jobTitle: ' ' }), undefined)).toBe('Untitled goal');
  });

  test('the seeded employee-name title does not suppress the objective fallback', () => {
    const boss = employee({ identity: { id: 'e', name: 'Ada', avatarSeed: 's' } });
    const named = session({ title: 'Ada', auto_title: 'Ada' });
    expect(
      goalTitle(
        boss,
        { ...named, thread_goal: { objective: 'Audit the queue', status: 'active', tokensUsed: 0, timeUsedSeconds: 0 } },
      ),
    ).toBe('Audit the queue');
  });
});

describe('goalProjectLabel', () => {
  test('session project name, then the queued ticket project, then unavailable', () => {
    expect(goalProjectLabel(employee(), session(), projects)).toBe('goddard');
    expect(
      goalProjectLabel(
        employee({ ticket: { project: '/tmp/queued-checkout' } as never }),
        undefined,
        projects,
      ),
    ).toBe('/tmp/queued-checkout');

    expect(goalProjectLabel(employee(), undefined, projects)).toBe('Project unavailable');
  });
});

describe('goalUpdatedAt / goalUpdatedLabel', () => {
  test('a newer finish stamp lifts the lagging session update', () => {
    expect(
      goalUpdatedAt(employee({ expiredAt: 90 }), session({ updated_at: 10 })),
    ).toBe(90);

    expect(goalUpdatedAt(employee({ createdAt: 7 }), undefined)).toBe(7);
    expect(goalUpdatedAt(employee(), undefined)).toBeNull();
  });

  test('labels read as relative ago strings', () => {
    expect(goalUpdatedLabel(100, 100_000)).toBe('just now');
    expect(goalUpdatedLabel(100, (100 + 4 * 60) * 1_000)).toBe('4m ago');
    expect(goalUpdatedLabel(null, 0)).toBeNull();
  });
});

describe('bossGoalSections', () => {
  test('lists goal employees only and orders each section', () => {
    const roster = state({
      employees: [
        // errand — never appears
        employee({ sessionId: 'errand', workGoal: 'errand', createdAt: 1 }),
        employee({ sessionId: 'old-finish', state: 'expired', expired: true, expiredAt: 10 }),
        employee({ sessionId: 'new-finish', state: 'expired', expired: true, expiredAt: 20 }),
        employee({ sessionId: 'calm', createdAt: 1 }),
        employee({ sessionId: 'blocked', blocker: 'stuck', createdAt: 5 }),
        employee({
          sessionId: 'queued',
          state: 'queued',
          queuedAt: 3,
          ticket: { sequence: 2, prompt: 'p', project: 'x' } as never,
        }),
        employee({
          sessionId: 'queued-first',
          state: 'queued',
          queuedAt: 2,
          ticket: { sequence: 1, prompt: 'p', project: 'x' } as never,
        }),
      ],
    });

    const sessions = new Map<string, AgentSession>();

    const sections = bossGoalSections(roster, sessions, projects);
    expect(sections.finished.map((row) => row.employee.sessionId)).toEqual([
      'new-finish',
      'old-finish',
    ]);

    expect(sections.inProgress.map((row) => row.employee.sessionId)).toEqual([
      'blocked',
      'calm',
    ]);

    expect(sections.pending.map((row) => row.employee.sessionId)).toEqual([
      'queued-first',
      'queued',
    ]);
  });
});

describe('bossGoalPlanGroups', () => {
  test('absorbs tagged goal employees into the plan group and orders by activity', () => {
    const plan: BossPlan = {
      id: 'plan-1',
      sessionId: 'plan-session',
      planFile: 'plans/release.md',
      idea: 'Release 1.0',
      finalizedAt: 10,
      items: [{ id: 'item-1', title: 'Verify signing', state: 'toDo' }],
    };

    const roster = state({
      planning: [plan],
      employees: [
        employee({ sessionId: 'member', planId: 'plan-1', itemId: 'item-1', createdAt: 20 }),
        employee({ sessionId: 'tail', planId: 'plan-1', createdAt: 30 }),
        employee({ sessionId: 'loose', createdAt: 40 }),
      ],
    });

    const groups = bossGoalPlanGroups(roster, new Map(), projects);
    expect(groups).toHaveLength(1);
    expect(groups[0]!.title).toBe('Release 1.0');
    expect(groups[0]!.items[0]!.rows.map((row) => row.employee.sessionId)).toEqual(['member']);
    expect(groups[0]!.unallocated.map((row) => row.employee.sessionId)).toEqual(['tail']);
    expect(groups[0]!.memberIds.has('loose')).toBe(false);

    const sections = bossGoalSections(roster, new Map(), projects, groups[0]!.memberIds);
    expect(sections.inProgress.map((row) => row.employee.sessionId)).toEqual(['loose']);
  });

  test('errand members never surface as rows inside the group', () => {
    const plan: BossPlan = {
      id: 'plan-1',
      sessionId: 'plan-session',
      planFile: 'plans/release.md',
      idea: 'Release 1.0',
      finalizedAt: 10,
      items: [],
    };

    const roster = state({
      planning: [plan],
      employees: [
        employee({ sessionId: 'errand', planId: 'plan-1', workGoal: 'errand' }),
        employee({ sessionId: 'goal', planId: 'plan-1' }),
      ],
    });

    const [group] = bossGoalPlanGroups(roster, new Map(), projects);
    expect(group!.unallocated.map((row) => row.employee.sessionId)).toEqual(['goal']);
  });
});

describe('bossGoalsUnreadCount', () => {
  test('counts goal finishes past the viewed stamp; never-viewed counts them all', () => {
    const roster = state({
      employees: [
        employee({ sessionId: 'a', expired: true, state: 'expired', expiredAt: 10 }),
        employee({ sessionId: 'b', expired: true, state: 'expired', expiredAt: 20 }),
        employee({ sessionId: 'errand', workGoal: 'errand', expired: true, state: 'expired', expiredAt: 30 }),
        employee({ sessionId: 'live' }),
      ],
    });

    expect(bossGoalsUnreadCount(roster)).toBe(2);
    expect(bossGoalsUnreadCount({ ...roster, goalsViewedAt: 15 })).toBe(1);
    expect(bossGoalsUnreadCount({ ...roster, goalsViewedAt: 20 })).toBe(0);
  });
});

describe('goalStatusMarker', () => {
  test('covers the icon vocabulary per status family', () => {
    expect(goalStatusMarker('working').spin).toBe(true);
    expect(goalStatusMarker('queued').symbol).toBe('hourglass');
    expect(goalStatusMarker('blocked').symbol).toBe('exclamationmark.triangle');
    expect(goalStatusMarker('failed').symbol).toBe('xmark');
    expect(goalStatusMarker('complete').symbol).toBe('checkmark');
  });
});
