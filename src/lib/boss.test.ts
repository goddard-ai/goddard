import { describe, expect, test } from 'bun:test';

import type { BossEmployee, BossState, SummonTicket } from '@waku/client';

import {
  bossModelCapacity,
  bossOpenBlockers,
  bossQueueDetail,
  bossQueueRanks,
  bossRosterSplit,
  bossEmployeeStatusLabel,
  employeeAssignmentRows,
  sessionReferenceAttachment,
} from './boss';

function employee(overrides: Partial<BossEmployee> = {}): BossEmployee {
  return {
    sessionId: 'session-1',
    supervisorId: 'boss-session',
    identity: { id: 'employee-1', name: 'Ada', avatarSeed: 'seed' },
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

function ticket(overrides: Partial<SummonTicket> = {}): SummonTicket {
  return {
    sequence: 1,
    generation: 1,
    prompt: 'Fix the login bug',
    project: 'goddard',
    provider: 'codex',
    model: 'gpt-5.4',
    workspace: 'adopt',
    resources: {
      exclusive: [],
      resident_devices: 0,
      native_builds: 0,
      desktop_input: 0,
    },
    allowBurst: false,
    resumeCount: 0,
    ...overrides,
  };
}

function state(overrides: Partial<BossState> = {}): BossState {
  return {
    identity: { id: 'boss-1', name: 'Boss', avatarSeed: 'boss' },
    personaId: 'persona-1',
    sessionId: 'boss-session',
    personas: [{
      id: 'persona-1',
      name: 'Generalist',
      markdown: '',
      pinnedFiles: [],
      permissions: {
        bucketIds: [],
        integrationIds: [],
        summonEmployees: false,
        computerUse: false,
      },
    }],
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

describe('bossRosterSplit', () => {
  test('keeps queued, dispatching, working, and finishing live; folds expired and finished', () => {
    const roster = state({
      employees: [
        employee({ sessionId: 'a', state: 'queued', createdAt: 1 }),
        employee({ sessionId: 'b', state: 'dispatching', createdAt: 2 }),
        employee({ sessionId: 'c', state: 'working', createdAt: 3 }),
        employee({ sessionId: 'd', state: 'finishing', createdAt: 4 }),
        employee({ sessionId: 'e', state: 'expired', expired: true, createdAt: 5 }),
        employee({ sessionId: 'f', state: 'expired', expired: true, cancelled: true, createdAt: 6 }),
      ],
    });

    const split = bossRosterSplit(roster);
    expect(split.live.map((item) => item.sessionId)).toEqual(['d', 'c', 'b', 'a']);
    expect(split.finished.map((item) => item.sessionId)).toEqual(['f', 'e']);
  });

  test('a cancelled or blocked record still counts live until the lifecycle expires', () => {
    const roster = state({
      employees: [employee({ sessionId: 'a', state: 'working', cancelled: true, blocker: 'stuck' })],
    });

    expect(bossRosterSplit(roster).live).toHaveLength(1);
  });
});

describe('bossQueueRanks', () => {
  test('ranks queued employees by ticket sequence', () => {
    const roster = state({
      employees: [
        employee({ sessionId: 'a', state: 'queued', ticket: ticket({ sequence: 3 }) }),
        employee({ sessionId: 'b', state: 'queued', ticket: ticket({ sequence: 1 }) }),
        employee({ sessionId: 'c', state: 'working' }),
      ],
    });

    const ranks = bossQueueRanks(roster);
    expect(ranks.get('b')).toBe(1);
    expect(ranks.get('a')).toBe(2);
    expect(ranks.has('c')).toBe(false);
  });
});

describe('bossQueueDetail', () => {
  test('prefers the ticket admission blocker and explains model limits', () => {
    const blocked = employee({
      state: 'queued',
      ticket: ticket({
        model: 'gpt-5.4',
        blockedBy: [
          { kind: 'modelLimit', used: 9, limit: 9 },
        ],
      }),
    });

    expect(bossQueueDetail(blocked, 2)).toBe('Waiting for gpt-5.4 · 9/9 slots');
  });

  test('distinguishes the head of the queue from employees waiting on earlier work', () => {
    const queued = employee({ state: 'queued', ticket: ticket() });
    expect(bossQueueDetail(queued, 1)).toBe('Waiting for admission');
    expect(bossQueueDetail(queued, 3)).toBe('Waiting for earlier work');
    expect(bossQueueDetail(employee({ state: 'working' }), 1)).toBeNull();
  });
});

describe('bossEmployeeStatusLabel', () => {
  test('expired lifecycle renders the expiry cause, not the session status', () => {
    const base = { expired: true, expiredAt: 100, state: 'expired' as const };
    expect(
      bossEmployeeStatusLabel(employee(base), { status: 'idle' } as never).label,
    ).toBe('Finished');

    expect(
      bossEmployeeStatusLabel(employee({ ...base, cancelled: true }), { status: 'idle' } as never).label,
    ).toBe('Cancelled');

    expect(
      bossEmployeeStatusLabel(employee({ ...base, blocker: 'needs input' }), { status: 'idle' } as never).label,
    ).toBe('Needs attention');
  });

  test('a working employee with a blocker reports attention ahead of the runtime status', () => {
    const status = bossEmployeeStatusLabel(
      employee({ state: 'working', blocker: 'permission needed' }),
      { status: 'working' } as never,
    );

    expect(status.label).toBe('Needs attention');
    expect(status.tone).toBe('warning');
  });
});

describe('bossOpenBlockers', () => {
  test('lists only employees carrying a blocker, live or expired', () => {
    const roster = state({
      employees: [
        employee({ sessionId: 'a', blocker: 'stuck' }),
        employee({ sessionId: 'b' }),
        employee({ sessionId: 'c', state: 'expired', expired: true, blocker: 'interrupted' }),
      ],
    });

    expect(bossOpenBlockers(roster).map((item) => item.sessionId)).toEqual(['a', 'c']);
  });
});

describe('bossModelCapacity', () => {
  test('counts claim-holding lifecycles against the policy limits', () => {
    const roster = state({
      resourcePolicy: {
        revision: 1,
        modelLimits: [{ provider: 'codex', model: 'gpt-5.4', liveLimit: 2, hardCap: 4 }],
      },
      employees: [
        employee({ sessionId: 'a', state: 'queued', ticket: ticket() }),
        employee({ sessionId: 'b', state: 'working', ticket: ticket() }),
        employee({ sessionId: 'c', state: 'finishing', ticket: ticket() }),
        employee({ sessionId: 'd', state: 'expired', expired: true, ticket: ticket() }),
      ],
    });

    expect(bossModelCapacity(roster)).toEqual([
      { provider: 'codex', model: 'gpt-5.4', used: 2, liveLimit: 2, hardCap: 4 },
    ]);
  });

  test('uncapped models still report usage', () => {
    const roster = state({
      employees: [employee({ sessionId: 'a', state: 'working', ticket: ticket() })],
    });

    expect(bossModelCapacity(roster)).toEqual([
      { provider: 'codex', model: 'gpt-5.4', used: 1, liveLimit: -1, hardCap: -1 },
    ]);
  });
});

describe('employeeAssignmentRows', () => {
  test('covers brief, persona, model, workspace, access, and resources', () => {
    const roster = state();
    const summoned = employee({
      ticket: ticket({
        resources: { exclusive: ['ios:dev'], resident_devices: 1, native_builds: 1, desktop_input: 0 },
        reservation: 'res-1',
      }),
    });

    const rows = employeeAssignmentRows(roster, undefined, summoned);
    const labels = rows.map((row) => row.label);
    expect(labels).toEqual(['Brief', 'Persona', 'Model', 'Workspace', 'Access', 'Resources']);
    expect(rows.find((row) => row.label === 'Resources')?.value).toBe(
      'Reserved: exclusive: 1 · devices: 1 · builds: 1',
    );
  });
});

describe('sessionReferenceAttachment', () => {
  test('carries the employee session id so prompt tokens splice a session reference', () => {
    const attachment = sessionReferenceAttachment('session-9', 'Ada');
    expect(attachment.session_id).toBe('session-9');
    expect(attachment.name).toBe('Ada');
    expect(attachment.is_dir).toBe(false);
  });
});
