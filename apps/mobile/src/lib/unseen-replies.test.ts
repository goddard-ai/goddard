import { beforeEach, describe, expect, mock, test } from 'bun:test';
import type { AgentSession } from '@waku/client';

const memory = new Map<string, string>();
mock.module('./composer-preferences-store', () => ({
  hydratePersistentStorage: () => Promise.resolve(),
  persistentStorageSync: () => ({
    getItem: (key: string) => memory.get(key) ?? null,
    setItem: (key: string, value: string) => {
      memory.set(key, value);
    },
  }),
}));

const {
  markSessionRepliesSeen,
  resetUnseenReplyStore,
  seedUnseenReplyWatermarks,
  sessionHasUnseenReply,
  subscribeUnseenReplies,
  unseenReplyWatermarks,
} = await import('./unseen-replies');

const DAEMON = 'wss://daemon.example';

beforeEach(() => {
  memory.clear();
  resetUnseenReplyStore();
});

describe('unseen reply watermarks', () => {
  test('an untracked session is not unseen', () => {
    expect(sessionHasUnseenReply(session({ id: 'a', last_reply_at: 10 }), {})).toBe(false);
  });

  test('first contact seeds existing history as read', async () => {
    await seedUnseenReplyWatermarks(DAEMON, [
      session({ id: 'a', last_reply_at: 10 }),
      session({ id: 'b', last_reply_at: null }),
    ]);
    const watermarks = unseenReplyWatermarks(DAEMON);
    expect(sessionHasUnseenReply(session({ id: 'a', last_reply_at: 10 }), watermarks)).toBe(false);
    expect(watermarks.a).toBe(10);
    expect(watermarks.b).toBe(0);
  });

  test('a session appearing after first contact flags its carried replies', async () => {
    await seedUnseenReplyWatermarks(DAEMON, [session({ id: 'a', last_reply_at: 10 })]);
    await seedUnseenReplyWatermarks(DAEMON, [
      session({ id: 'a', last_reply_at: 10 }),
      session({ id: 'new', last_reply_at: 20 }),
    ]);
    const watermarks = unseenReplyWatermarks(DAEMON);
    expect(sessionHasUnseenReply(session({ id: 'new', last_reply_at: 20 }), watermarks)).toBe(true);
  });

  test('a reply newer than the watermark flags the session; marking seen clears it', async () => {
    const viewed = session({ id: 'a', last_reply_at: 10 });
    await markSessionRepliesSeen(DAEMON, viewed);
    let watermarks = unseenReplyWatermarks(DAEMON);
    expect(sessionHasUnseenReply(session({ id: 'a', last_reply_at: 10 }), watermarks)).toBe(false);

    const advanced = session({ id: 'a', last_reply_at: 11 });
    expect(sessionHasUnseenReply(advanced, watermarks)).toBe(true);

    await markSessionRepliesSeen(DAEMON, advanced);
    watermarks = unseenReplyWatermarks(DAEMON);
    expect(sessionHasUnseenReply(advanced, watermarks)).toBe(false);
  });

  test('watermarks survive a reload from storage', async () => {
    await markSessionRepliesSeen(DAEMON, session({ id: 'a', last_reply_at: 10 }));
    resetUnseenReplyStore();
    const watermarks = unseenReplyWatermarks(DAEMON);
    expect(sessionHasUnseenReply(session({ id: 'a', last_reply_at: 11 }), watermarks)).toBe(true);
  });

  test('other daemons track their own watermarks', async () => {
    await markSessionRepliesSeen(DAEMON, session({ id: 'a', last_reply_at: 10 }));
    expect(sessionHasUnseenReply(
      session({ id: 'a', last_reply_at: 11 }),
      unseenReplyWatermarks('wss://other.example'),
    )).toBe(false);
  });

  test('mutations notify subscribers', async () => {
    let notified = 0;
    const unsubscribe = subscribeUnseenReplies(() => {
      notified += 1;
    });
    await markSessionRepliesSeen(DAEMON, session({ id: 'a', last_reply_at: 10 }));
    expect(notified).toBe(1);
    unsubscribe();
  });
});

function session(overrides: Partial<AgentSession> & Pick<AgentSession, 'id'>): AgentSession {
  return {
    title: 'Task',
    project_id: 'project',
    provider: 'codex',
    runtime_mode: 'autoAcceptEdits',
    status: 'idle',
    created_at: 1,
    updated_at: 1,
    last_reply_at: 1,
    provider_cursor: null,
    messages: [],
    transcript_blocks: [],
    turns: [],
    ...overrides,
  };
}
