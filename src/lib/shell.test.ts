import { expect, test } from 'bun:test';
import type { AgentSession, BossState } from '@waku/client';
import { ordinarySessions } from './shell';
import { daemonQueryEntry, removeDaemonQueries } from './daemon-data';

const session = (id: string, boss_managed = false) => ({ id, boss_managed }) as AgentSession;

test('ordinary groups exclude all Boss records, including legacy summaries', () => {
  const boss = {
    sessionId: 'boss',
    planning: [{ sessionId: 'plan' }],
    employees: [{ sessionId: 'live' }],
    retiredEmployees: [{ sessionId: 'retired' }],
  } as BossState;

  const sessions = ['boss', 'plan', 'live', 'retired', 'direct'].map((id) => session(id));
  sessions.push(session('managed', true));
  expect(ordinarySessions(sessions, boss).map((item) => item.id)).toEqual(['direct']);
  expect(ordinarySessions(sessions, null).map((item) => item.id)).toEqual(['boss', 'plan', 'live', 'retired', 'direct']);
});

test('reconnecting a host keeps cached data and replaces the closed client fetcher', async () => {
  const key = ['daemon', 'shell-test', 'boss-state'];
  const entry = daemonQueryEntry(key, async () => 'cached');
  await entry.refetch();
  const reconnected = daemonQueryEntry(key, async () => 'fresh');
  expect(reconnected.getSnapshot().data).toBe('cached');
  expect(await reconnected.refetch()).toBe('fresh');
  removeDaemonQueries('shell-test');
});
