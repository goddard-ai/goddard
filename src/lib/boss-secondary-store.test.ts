import { afterEach, describe, expect, mock, test } from 'bun:test';
import type { BossBrowseRequest, BossSpeechRequest, Command, EventListener, WakuClient, WakuConnectionState } from '@waku/client';

const stored = new Map<string, string>();
mock.module('@octane-xplat/ui/data', () => ({
  createStore: <T>(initial: T) => {
    let value = initial;
    return {
      getSnapshot: () => value,
      set: (next: T) => { value = next; },
      update: (updater: (current: T) => T) => { value = updater(value); },
    };
  },
}));

mock.module('@octane-xplat/platform', () => ({ storage: { getString: (key: string) => stored.get(key), setString: (key: string, value: string) => stored.set(key, value) } }));
mock.module('./app-foreground', () => ({ subscribeAppForeground: () => () => {} }));
mock.module('./daemon-api', () => ({ browseDaemonDirectory: async (_client: unknown, path: string | null) => {
  if (path === '/missing') {throw new Error('No such directory');}
  return { path: path ?? '/home', home: '/home' };
} }));

const {
  bindBossSecondary, bossSecondary, pinBossTerminal, listenPinnedTerminal, closePinnedTerminal,
} = await import('./boss-secondary-store');

let cleanup: (() => void) | undefined;
afterEach(() => cleanup?.());
let index = 0;
function harness() {
  let browse: (request: BossBrowseRequest) => void = () => {};
  let speech: (request: BossSpeechRequest) => void = () => {};
  let connection: (phase: WakuConnectionState) => void = () => {};
  const channels = new Map<string, EventListener>();
  const calls: Command[] = [];
  let missingPty = false;
  let failClose = false;
  const client = {
    connected: true,
    subscribeBossBrowse: (listener: typeof browse) => {browse = listener; return () => {};},
    subscribeBossSpeech: (listener: typeof speech) => {speech = listener; return () => {};},
    subscribeConnectionState: (listener: typeof connection) => {connection = listener; return () => {};},
    subscribe: (id: string, _runtimeId: string, listener: EventListener) => {channels.set(id, listener); return () => channels.delete(id);},
    request: async (command: Command) => {
      calls.push(command);
      if (missingPty && command.type === 'writeTerminal' && !command.data) {throw new Error('Terminal is not running');}
      if (failClose && command.type === 'closeTerminal') {throw new Error('Connection lost');}
      return { type: 'ack' };
    },
    notify: async (command: Command) => {calls.push(command);},
  } as unknown as WakuClient;

  const profile = `host-${++index}`;
  cleanup = bindBossSecondary(client, profile);
  return { setMissingPty: () => {missingPty = true;}, setFailClose: () => {failClose = true;}, browse: (request: BossBrowseRequest) => browse(request), speech: (request: BossSpeechRequest) => speech(request), connection: (phase: WakuConnectionState) => connection(phase), calls, channels, profile, client };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('Boss secondary transport', () => {
  test('live broadcasts dedupe across reconnect and clear on host switch', () => {
    const h = harness();
    const request = { requestId: 'r', sessionId: 'boss', url: 'https://example.com', title: null };
    h.browse(request);
    h.browse(request);
    h.speech({ requestId: 's', parts: ['hello'] });
    h.connection('disconnected');
    h.connection('connected');
    h.speech({ requestId: 's', parts: ['hello'] });
    expect(bossSecondary.getSnapshot().browse).toHaveLength(1);
    expect(bossSecondary.getSnapshot().speech).toHaveLength(1);
    cleanup?.();
    harness();
    expect(bossSecondary.getSnapshot().browse).toHaveLength(0);
    expect(bossSecondary.getSnapshot().speech).toHaveLength(0);
  });

  test('terminal fallback creates a pin and announces the actual directory', async () => {
    const h = harness();
    pinBossTerminal({ title: 'Server', cwd: '/missing', command: 'bun run dev' }, 'boss', 'event-1', '/project');
    pinBossTerminal({ title: 'Server', cwd: '/missing' }, 'boss', 'event-1', '/project');
    await settle();
    expect(bossSecondary.getSnapshot().terminals).toHaveLength(1);
    expect(bossSecondary.getSnapshot().notice).toContain('Using /project');
    expect(h.calls).toContainEqual({ type: 'openTerminal', cwd: '/project', cols: 80, rows: 24 });
    expect(h.calls).toContainEqual({ type: 'writeTerminal', data: 'bun run dev\n' });
    const pin = bossSecondary.getSnapshot().terminals[0]!;
    expect(pin.commandSent).toBe(true);
    expect(stored.get(`waku.mobile.boss-terminals.v1.${h.profile}`)).toContain(pin.id);
  });

  test('reconnect reclaims a pinned PTY without replacing it or rerunning the command', async () => {
    const h = harness();
    pinBossTerminal({ title: 'Server', cwd: '/project', command: 'bun run dev' }, 'boss', 'event-2', null);
    await settle();
    h.connection('disconnected');
    h.connection('connected');
    await settle();
    expect(h.calls.filter((call) => call.type === 'openTerminal')).toHaveLength(1);
    expect(h.calls.filter((call) => call.type === 'writeTerminal' && call.data === 'bun run dev\n')).toHaveLength(1);
    expect(h.calls).toContainEqual({ type: 'writeTerminal', data: '' });
  });

  test('a lost PTY restores a plain shell and explains why the command was not replayed', async () => {
    const h = harness();
    pinBossTerminal({ title: 'Server', cwd: '/project', command: 'bun run dev' }, 'boss', 'event-restore', null);
    await settle();
    h.setMissingPty();
    h.connection('disconnected');
    h.connection('connected');
    await settle();
    expect(h.calls.filter((call) => call.type === 'openTerminal')).toHaveLength(2);
    expect(h.calls.filter((call) => call.type === 'writeTerminal' && call.data === 'bun run dev\n')).toHaveLength(1);
    expect(bossSecondary.getSnapshot().terminals[0]?.notice).toContain('without rerunning');
  });

  test('failed explicit close preserves the pin and exposes the error', async () => {
    const h = harness();
    pinBossTerminal({ title: 'Shell', cwd: '/project' }, 'boss', 'event-close', null);
    await settle();
    const id = bossSecondary.getSnapshot().terminals[0]!.id;
    h.setFailClose();
    await expect(closePinnedTerminal(id)).rejects.toThrow('Connection lost');
    expect(bossSecondary.getSnapshot().terminals[0]?.error).toBe('Connection lost');
  });

  test('sheet listeners can detach while a bounded output tail and pin survive', async () => {
    const h = harness();
    pinBossTerminal({ title: 'Shell', cwd: '/project' }, 'boss', 'event-3', null);
    await settle();
    const id = bossSecondary.getSnapshot().terminals[0]!.id;
    h.channels.get(id)!({ sessionId: id, runtimeId: id, epoch: 'e', sequence: 1, event: { kind: 'terminalOutput', payload: { data: 'x'.repeat(150_000) } } });
    let output = '';
    const detach = listenPinnedTerminal(id, (data) => {output += data;});
    expect(output.length).toBe(100_000);
    detach();
    expect(h.calls.some((call) => call.type === 'closeTerminal')).toBe(false);
    expect(bossSecondary.getSnapshot().terminals).toHaveLength(1);
    await closePinnedTerminal(id);
    expect(h.calls.at(-1)?.type).toBe('closeTerminal');
    expect(bossSecondary.getSnapshot().terminals).toHaveLength(0);
    cleanup?.();
    harness();
    cleanup?.();
    cleanup = bindBossSecondary(h.client, h.profile);
    pinBossTerminal({ title: 'Shell', cwd: '/project' }, 'boss', 'event-3', null);
    expect(bossSecondary.getSnapshot().terminals).toHaveLength(0);
  });
});
