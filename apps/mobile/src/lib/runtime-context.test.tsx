import './test-support/async-storage';
import { afterEach, describe, expect, mock, test } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { AgentSession, Command, ResponsePayload, SequencedEvent, WakuClient } from '@waku/client';
import { reduceRuntimeEvent } from '@waku/client/event-reducer';
import { renderToStaticMarkup } from 'react-dom/server';

import { daemonKeys, hydrateSession } from './daemon-api';
import { beginTurn, createSession, queueSubmission } from './mobile-runtime';

let activeDaemon: { activeProfile: { id: string }; client: WakuClient; phase: 'connected' };
mock.module('./daemon-context', () => ({ useDaemon: () => activeDaemon }));
mock.module('expo-crypto', () => ({ randomUUID: () => crypto.randomUUID() }));


const { RuntimeProvider, useRuntime } = await import('./runtime-context');
const cleanups: Array<() => Promise<void>> = [];

afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function fixture(options: { attached?: boolean; provider?: 'codex' | 'claude' } = {}) {
  let nextId = 0;
  const clock = {
    nowSeconds: () => 100,
    nowMillis: () => 100_000,
    randomUUID: () => `id-${++nextId}`,
  };
  let sequence = 0;
  let history = createSession('project', options.provider ?? 'codex', false, clock);
  function event(kind: string, payload: SequencedEvent['event']['payload']): SequencedEvent {
    return {
      sessionId: history.id,
      runtimeId: 'runtime',
      epoch: 'epoch',
      sequence: ++sequence,
      event: { kind, payload },
    };
  }
  history = beginTurn(history, 'Previous request', clock);
  history = reduceRuntimeEvent(history, event('turnStarted', null), clock).session;
  history = reduceRuntimeEvent(history, event('textDelta', 'Previous answer'), clock).session;
  history = reduceRuntimeEvent(history, event('turnFinished', { success: true }), clock).session;
  history = beginTurn(history, 'Current request', clock);
  history = reduceRuntimeEvent(history, event('turnStarted', null), clock).session;
  history = reduceRuntimeEvent(history, event('textDelta', 'Streaming response'), clock).session;
  const skeleton: AgentSession = {
    ...history,
    messages: [],
    turns: [],
    transcript_blocks: [],
    runtime_event_cursor: null,
  };
  const hydration = deferred<AgentSession | null>();
  const commands: Command[] = [];
  const addressed: Array<{ type: string; runtimeId?: string }> = [];
  let currentRuntime = options.attached === false ? null : 'runtime';
  let canonical: AgentSession | undefined;
  let listener: ((event: SequencedEvent) => void) | undefined;
  const client = {
    connected: true,
    request: async (command: Command, _sessionId?: string, runtimeId?: string): Promise<ResponsePayload> => {
      commands.push(command);
      addressed.push({ type: command.type, runtimeId });
      switch (command.type) {
        case 'hydrateSession':
          return { type: 'session', session: canonical ?? await hydration.promise };
        case 'attachSession':
          return { type: 'sessionRuntime', runtimeId: currentRuntime, supportsSteer: true, supportsUserInputActions: true };
        case 'getSettings':
          return { type: 'settings', settings: {
            provider_binary_overrides: {}, disabled_providers: [],
            computer_use_enabled: false, computer_use_experiment_enabled: false,
            computer_use_allowed_apps: [],
            agent_tools_enabled: false, agent_settings_enabled: true,
            subagents_enabled: false,
            integrations_enabled: false,
            composer_drafts_experiment_enabled: false,
            sandbox_experiment_enabled: false, sandbox_default_enabled: false,
            keep_awake: false, qa_branch: 'qa', auto_prompts: [],
            boss_experiment_enabled: false, boss_rotation_disabled: false,
            boss_rotation_context_threshold: 0.75, employee_chat_search_days: 3,
            wireframes_experiment_enabled: false,
          } };
        case 'loadTaskState':
          return {
            type: 'taskState',
            defaultCwd: '/repo',
            projectlessRoot: null,
            projects: [{ id: 'project', name: 'Project', path: '/repo', created_at: 0, temporary: false, starred: false, submissions_enabled: false }],
            sessions: [history],
          };
        case 'probeProvider':
          return {
            type: 'providerProbe', version: null,
            probe: { provider: history.provider, installed: true, path: `/bin/${history.provider}`, models: [], agent_presets: [] },
          };
        case 'start':
          currentRuntime = runtimeId ?? null;
          return { type: 'started', supportsSteer: true, supportsUserInputActions: true };
        case 'prompt':
        case 'promptWithConfiguration':
          if (runtimeId !== currentRuntime) throw new Error('daemon session is not running');
          return { type: 'ack' };
        case 'saveTaskState':
          return { type: 'taskStateSaved', sessions: command.sessions.map(session => {
            if (!canonical) return session;
            if ((session.history_generation ?? 0) !== (canonical.history_generation ?? 0)) return canonical;
            canonical = session;
            return session;
          }) };
        default:
          return { type: 'ack' };
      }
    },
    subscribe: (_sessionId: string, _runtimeId: string, next: typeof listener) => {
      listener = next;
      return () => { listener = undefined; };
    },
  } as unknown as WakuClient;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  const key = daemonKeys.session('profile', history.id);
  activeDaemon = { activeProfile: { id: 'profile' }, client, phase: 'connected' };
  let runtime!: ReturnType<typeof useRuntime>;
  function CaptureRuntime() {
    runtime = useRuntime();
    return null;
  }
  // Capture the real runtime callbacks without mounting a native view tree.
  // The tests exercise their refs, subscriptions and actual query-cache writes.
  renderToStaticMarkup(
    <QueryClientProvider client={queryClient}>
      <RuntimeProvider><CaptureRuntime /></RuntimeProvider>
    </QueryClientProvider>,
  );
  cleanups.push(async () => {
    await runtime.deleteSession(history.id);
    queryClient.clear();
  });
  return {
    history, skeleton, hydration, commands, addressed, client, queryClient, key, runtime,
    isSubscribed: () => listener !== undefined,
    retire(session: AgentSession) { currentRuntime = null; canonical = { ...session, history_generation: (session.history_generation ?? 0) + 1 }; },
    emit(kind: string, payload: SequencedEvent['event']['payload']) {
      if (!listener) throw new Error('The runtime is not subscribed');
      listener(event(kind, payload));
    },
    current: () => queryClient.getQueryData<AgentSession>(key)!,
  };
}

describe('mobile runtime history', () => {
  test('configured late mobile refusal cannot fail a replacement history turn', async () => {
    const f = fixture({ provider: 'claude' });
    const before: AgentSession = { ...f.history, status: 'idle', model: 'selected-model',
      turns: f.history.turns.map(turn => ({ ...turn, status: 'completed' })),
    };
    f.hydration.resolve(before);
    f.queryClient.setQueryData(f.key, before);
    await f.runtime.attachSession(before);
    const entered = deferred<void>();
    const release = deferred<void>();
    const original = f.client.request.bind(f.client);
    f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
      if (args[0].type === 'promptWithConfiguration') {
        entered.resolve(); await release.promise;
        throw new Error('Old configuration refused');
      }
      return original(...args);
    }) as WakuClient['request'];
    const outcome = f.runtime.sendPrompt(before, 'discarded intent').then(() => null, error => error);
    await entered.promise;
    const replacement = beginTurn({ ...before, history_generation: 1, turns: [], messages: [],
      model: 'latest-model' }, 'replacement intent', {
      nowSeconds: () => 101, randomUUID: () => crypto.randomUUID(),
    });
    f.queryClient.setQueryData(f.key, replacement);
    release.resolve();
    expect(String(await outcome)).toContain('Old configuration refused');
    expect(f.current()).toEqual(replacement);
  });

  test('configured rejected mobile steers retain their original selected model when parked', async () => {
    const f = fixture({ provider: 'claude' });
    const before = { ...f.history, model: 'selected-model' };
    f.hydration.resolve(before);
    f.queryClient.setQueryData(f.key, before);
    await f.runtime.attachSession(before);
    // End the streamed text so this is admitted as a live steer first.
    f.queryClient.setQueryData(f.key, { ...f.current(), messages: f.current().messages.map(
      message => ({ ...message, streaming: false }),
    ) });
    await f.runtime.steerPrompt(f.current(), 'Keep the model selected for this steer');
    expect(f.commands.some(command => command.type === 'steer')).toBe(true);
    f.queryClient.setQueryData(f.key, { ...f.current(), model: 'latest-model' });
    f.emit('steerRejected', { message: 'Keep the model selected for this steer', reason: 'busy' });
    expect(f.current().model).toBe('latest-model');
    expect(f.current().queued_messages?.at(-1)?.configuration?.options.model).toBe('selected-model');
  });

  test('configured mobile prompts retain their admitted selection and preserve a later choice on refusal', async () => {
    for (const [refused, queued] of [[false, false], [true, false], [false, true]] as const) {
      const f = fixture({ provider: 'claude' });
      const before: AgentSession = {
        ...f.history, status: 'idle', model: queued ? 'latest-model' : 'selected-model',
        turns: f.history.turns.map(turn => ({ ...turn, status: 'completed' })),
      };
      f.hydration.resolve(before);
      f.queryClient.setQueryData(f.key, before);
      await f.runtime.attachSession(before);
      const entered = deferred<Command>();
      const release = deferred<void>();
      const original = f.client.request.bind(f.client);
      f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
        if (args[0].type === 'promptWithConfiguration') {
          entered.resolve(args[0]);
          await release.promise;
          if (refused) throw new Error('The selected configuration was refused');
        }
        return original(...args);
      }) as WakuClient['request'];
      const parked = queueSubmission({ ...before, model: 'selected-model' },
        'Keep this prompt and my model', { nowSeconds: () => 100, randomUUID: () => 'queued-prompt' });
      const outcome = f.runtime.sendPrompt(before, 'Keep this prompt and my model', [], undefined,
        false, queued ? parked.queued_messages?.[0]?.configuration ?? undefined : undefined)
        .then(() => null, error => error);
      const command = await entered.promise;
      expect(command).toMatchObject({ type: 'promptWithConfiguration', configuration: {
        provider: 'claude', options: { model: 'selected-model' },
      } });
      f.queryClient.setQueryData(f.key, { ...f.current(), model: 'latest-model' });
      release.resolve();
      const error = await outcome;
      expect(f.current().model).toBe('latest-model');
      expect(f.current().messages.some(message => message.role === 'user'
        && message.content === 'Keep this prompt and my model')).toBe(true);
      if (refused) {
        expect(String(error)).toContain('selected configuration was refused');
        expect(f.current().turns.at(-1)?.status).toBe('failed');
      } else expect(error).toBeNull();
      expect(f.commands.filter(command => command.type === 'prompt')).toHaveLength(0);
    }
  });

  test('a picker change during prompt persistence survives the acknowledgement and prevents stale provider work', async () => {
    const f = fixture({ attached: false });
    const before: AgentSession = {
      ...f.history, status: 'idle',
      turns: f.history.turns.map(turn => ({ ...turn, status: 'completed' })),
    };
    f.hydration.resolve(before);
    f.queryClient.setQueryData(f.key, before);
    const entered = deferred<void>();
    const release = deferred<void>();
    const original = f.client.request.bind(f.client);
    f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
      if (args[0].type === 'saveTaskState') {
        entered.resolve();
        await release.promise;
      }
      return original(...args);
    }) as WakuClient['request'];
    const outcome = f.runtime.sendPrompt(before, 'Use the selected model')
      .then(() => null, error => error);
    await entered.promise;
    f.queryClient.setQueryData(f.key, { ...f.current(), model: 'newer-model' });
    release.resolve();
    expect(String(await outcome)).toContain('task changed');
    expect(f.current().model).toBe('newer-model');
    expect(f.commands.some(command => command.type === 'start' || (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
  });

  test('a rewind during prompt persistence rejects the discarded intent before provider work', async () => {
    for (const attached of [false, true]) {
      const f = fixture({ attached });
      const before: AgentSession = {
        ...f.history, status: 'idle',
        turns: f.history.turns.map(turn => ({ ...turn, status: 'completed' })),
      };
      f.hydration.resolve(before);
      f.queryClient.setQueryData(f.key, before);
      if (attached) await f.runtime.attachSession(before);
      const entered = deferred<void>();
      const release = deferred<void>();
      const original = f.client.request.bind(f.client);
      let held = false;
      f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
        if (args[0].type === 'saveTaskState' && !held) {
          held = true;
          entered.resolve();
          await release.promise;
        }
        return original(...args);
      }) as WakuClient['request'];
      const outcome = f.runtime.sendPrompt(before, 'Keep this request for retry')
        .then(() => null, error => error);
      await entered.promise;
      f.retire({ ...before, turns: before.turns.slice(0, 1), messages: [], status: 'idle' });
      release.resolve();
      expect(String(await outcome)).toContain('task changed');
      expect(f.commands.some(command => command.type === 'start' || (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
      expect(f.current().history_generation).toBe(1);
      expect(f.current().turns).toHaveLength(1);
      await f.runtime.sendPrompt(f.current(), 'Retry from the authoritative task');
      expect(f.commands.some(command => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(true);
      expect(f.current().history_generation).toBe(1);
    }
  });

  test('quiesces a retired subscription while its old save returns canonical rewound history', async () => {
    const f = fixture();
    f.hydration.resolve(f.history);
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.attachSession(f.history);
    const entered = deferred<void>();
    const release = deferred<void>();
    const original = f.client.request.bind(f.client);
    let held = false;
    f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
      if (args[0].type === 'saveTaskState' && !held) {
        held = true;
        entered.resolve();
        await release.promise;
      }
      return original(...args);
    }) as WakuClient['request'];
    f.emit('turnFinished', { success: true });
    await entered.promise;
    const before = f.current();
    f.retire({
      ...before,
      turns: before.turns.slice(0, 1),
      messages: before.messages.filter(message => message.turn_id === before.turns[0]!.id),
      transcript_blocks: [],
      status: 'idle',
    });
    const sending = f.runtime.sendPrompt(before, 'Continue with the authoritative history');
    void sending.catch(() => undefined);
    try {
      for (let i = 0; i < 50 && f.isSubscribed(); i++) await Bun.sleep(1);
      expect(f.isSubscribed()).toBe(false);
    } finally {
      release.resolve();
    }
    await sending;
    const prepared = f.commands.filter(command => command.type === 'saveTaskState')
      .flatMap(command => command.type === 'saveTaskState' ? command.sessions : [])
      .find(session => session.turns.at(-1)?.status === 'running');
    expect(prepared?.history_generation).toBe(1);
    expect(prepared?.turns).toHaveLength(2);
    expect(prepared?.turns.some(turn => turn.id === before.turns[1]!.id)).toBe(false);
    expect(f.addressed.find(item => item.type === 'prompt')?.runtimeId).not.toBe('runtime');
  });

  test('reattaches after a stopped rewind without resaving the removed historical turn', async () => {
    const f = fixture();
    f.hydration.resolve(f.history);
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.attachSession(f.history);
    f.emit('turnFinished', { success: true });
    const before = f.current();
    const rewound: AgentSession = {
      ...before,
      turns: before.turns.slice(0, 1),
      messages: before.messages.filter(message => message.turn_id === before.turns[0]!.id),
      transcript_blocks: [],
      status: 'idle',
    };
    f.retire(rewound);
    await f.runtime.sendPrompt(before, 'Continue after stopping rewind');
    const sent = f.addressed.find(item => item.type === 'prompt');
    expect(sent?.runtimeId).not.toBe('runtime');
    expect(f.commands.some(command => command.type === 'start')).toBe(true);
    const saved = f.commands.filter(command => command.type === 'saveTaskState')
      .flatMap(command => command.type === 'saveTaskState' ? command.sessions : [])
      .find(session => session.turns.at(-1)?.status === 'running');
    expect(saved?.history_generation).toBe(1);
    expect(saved?.turns).toHaveLength(2);
    expect(saved?.turns.some(turn => turn.id === before.turns[1]!.id)).toBe(false);
    expect(saved?.messages.some(message => message.content === 'Current request')).toBe(false);
  });

  test('a newer cache update during retired-runtime validation fails before sending or saving stale history', async () => {
    const f = fixture();
    const before: AgentSession = {
      ...f.history, status: 'idle',
      turns: f.history.turns.map(turn => ({ ...turn, status: 'completed' })),
    };
    f.hydration.resolve(before);
    f.queryClient.setQueryData(f.key, before);
    await f.runtime.attachSession(before);
    f.retire({ ...before, turns: before.turns.slice(0, 1), messages: [], status: 'idle' });
    const entered = deferred<void>();
    const release = deferred<void>();
    const original = f.client.request.bind(f.client);
    f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
      if (args[0].type === 'attachSession') {
        entered.resolve();
        await release.promise;
      }
      return original(...args);
    }) as WakuClient['request'];
    const sent = f.runtime.sendPrompt(before, 'Keep this prompt safe');
    const outcome = sent.then(() => null, error => error);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([entered.promise, new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error('runtime validation did not begin')), 2000);
      })]);
      f.emit('availableCommands', [{ name: 'newerCommand', description: 'Keep this newer state' }]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
      release.resolve();
    }
    expect(String(await outcome)).toContain('task changed');
    expect(f.current().available_commands?.map(command => command.name)).toEqual(['newerCommand']);
    expect(f.commands.some(command => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
    expect(f.commands.some(command => command.type === 'saveTaskState')).toBe(false);
  });

  test('a turn that settles during runtime validation starts the next prompt instead of stranding it in the queue', async () => {
    const f = fixture();
    f.hydration.resolve(f.history);
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.attachSession(f.history);
    const entered = deferred<void>();
    const release = deferred<void>();
    const original = f.client.request.bind(f.client);
    f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
      if (args[0].type === 'attachSession') {
        entered.resolve();
        await release.promise;
      }
      return original(...args);
    }) as WakuClient['request'];
    const sending = f.runtime.sendPrompt(f.current(), 'Follow up after settlement');
    void sending.catch(() => undefined);
    await entered.promise;
    f.emit('turnFinished', { success: true });
    release.resolve();
    await sending;
    expect(f.commands.some(command => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(true);
    expect(f.current().queued_messages).toHaveLength(0);
    expect(f.current().turns[1]?.status).toBe('completed');
  });

  test('a selected model changed during runtime validation is an explicit failure before provider execution', async () => {
    const f = fixture();
    const before: AgentSession = {
      ...f.history, status: 'idle',
      turns: f.history.turns.map(turn => ({ ...turn, status: 'completed' })),
    };
    f.hydration.resolve(before);
    f.queryClient.setQueryData(f.key, before);
    await f.runtime.attachSession(before);
    const entered = deferred<void>();
    const release = deferred<void>();
    const original = f.client.request.bind(f.client);
    f.client.request = (async (...args: Parameters<WakuClient['request']>) => {
      if (args[0].type === 'attachSession') {
        entered.resolve();
        await release.promise;
      }
      return original(...args);
    }) as WakuClient['request'];
    const outcome = f.runtime.sendPrompt(before, 'Use the configuration I selected')
      .then(() => null, error => error);
    await entered.promise;
    f.queryClient.setQueryData(f.key, { ...before, model: 'newer-model' });
    release.resolve();
    expect(String(await outcome)).toContain('task changed');
    expect(f.commands.some(command => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
    expect(f.current().model).toBe('newer-model');
  });

  test('a retired runtime with different confirmed options refreshes the selection and requires an explicit retry', async () => {
    const f = fixture();
    const before: AgentSession = {
      ...f.history, status: 'idle', model: 'old-model',
      turns: f.history.turns.map(turn => ({ ...turn, status: 'completed' })),
    };
    f.hydration.resolve(before);
    f.queryClient.setQueryData(f.key, before);
    await f.runtime.attachSession(before);
    f.retire({ ...before, model: 'confirmed-model', turns: before.turns.slice(0, 1) });
    await expect(f.runtime.sendPrompt(before, 'Use my selected model')).rejects.toThrow('task changed');
    expect(f.commands.some(command => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
    expect(f.current().model).toBe('confirmed-model');
    await f.runtime.sendPrompt(f.current(), 'Retry with the confirmed selection');
    expect(f.commands.find(command => command.type === 'start')).toMatchObject({
      options: { model: 'confirmed-model' },
    });
    expect(f.commands.some(command => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(true);
  });

  test('creates a task with expanded provider text while retaining its slash command for display', async () => {
    const f = fixture({ attached: false });
    const session = await f.runtime.createTask('project', 'codex', false, '/deploy production', {}, '$deploy production');
    cleanups.push(() => f.runtime.deleteSession(session.id));
    expect(f.commands.find((command) => (command.type === 'prompt' || command.type === 'promptWithConfiguration')))
      .toMatchObject({ prompt: '$deploy production' });
    expect(session.messages[0]).toMatchObject({ content: '$deploy production', display_content: '/deploy production' });
  });

  test('keeps expanded command content separate from display text when steering', async () => {
    const f = fixture();
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.attachSession(f.history);
    // A new tool ends the fixture's streamed reply text, so this steer
    // reaches the still-running turn.
    f.emit('activity', { id: 'tool-1', kind: 'tool', title: 'Inspect', complete: false });
    expect(f.current().messages.at(-1)?.streaming).toBe(false);
    await f.runtime.steerPrompt(f.current(), '/review changes', [], 'Review changes carefully');
    expect(f.commands.find((command) => command.type === 'steer'))
      .toEqual({ type: 'steer', prompt: 'Review changes carefully' });
    f.emit('steerAccepted', { message: 'Review changes carefully' });
    expect(f.current().messages.at(-1))
      .toMatchObject({ content: 'Review changes carefully', display_content: '/review changes' });
  });

  test('parks a steer while assistant text is streaming behind a transcript preview', async () => {
    const f = fixture();
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.attachSession(f.history);
    // The fixture's tail is still-streaming assistant text, so the provider
    // can't take the steer — it parks instead of erroring or chipping.
    await f.runtime.steerPrompt(f.current(), 'Follow up');
    expect(f.commands.some((command) => command.type === 'steer')).toBe(false);
    const queued = f.current().queued_messages?.at(-1);
    expect(queued?.content).toBe('Follow up');
    expect(f.current().messages.at(-1)).toMatchObject({
      id: queued?.id,
      turn_id: null,
      role: 'user',
      content: 'Follow up',
    });
  });

  test('a rejected steer parks behind a transcript preview too', async () => {
    const f = fixture();
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.attachSession(f.history);
    // A new tool ends the fixture's streamed reply text, so the steer goes
    // out live; the driver's rejection parks it.
    f.emit('activity', { id: 'tool-1', kind: 'tool', title: 'Inspect', complete: false });
    await f.runtime.steerPrompt(f.current(), 'Follow up');
    expect(f.commands.some((command) => command.type === 'steer')).toBe(true);
    f.emit('steerRejected', { message: 'Follow up', reason: 'busy' });
    const queued = f.current().queued_messages?.at(-1);
    expect(queued?.content).toBe('Follow up');
    expect(f.current().messages.at(-1)).toMatchObject({
      id: queued?.id,
      role: 'user',
      content: 'Follow up',
    });
  });

  test('queues expanded command content without starting another provider turn', async () => {
    const f = fixture();
    f.queryClient.setQueryData(f.key, f.history);
    const session = await f.runtime.sendPrompt(f.history, '/review changes', [], 'Review changes carefully');
    expect(session.queued_messages?.at(-1))
      .toMatchObject({ content: 'Review changes carefully', display_content: '/review changes' });
    expect(f.commands.some((command) => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
  });

  test('sends a goal operation to a live runtime without adding a prompt', async () => {
    const f = fixture();
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.sendGoalOperation(f.history, { kind: 'set', objective: null, status: 'paused', replace: false });
    expect(f.commands.at(-1)).toEqual({ type: 'goal', operation: { kind: 'set', objective: null, status: 'paused', replace: false } });
    expect(f.current().messages).toEqual(f.history.messages);
    expect(f.commands.some((command) => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
  });

  test('starts a provider for a fresh goal and applies its native updates', async () => {
    const f = fixture({ attached: false });
    const fresh = { ...f.history, messages: [], turns: [], transcript_blocks: [], status: 'idle' as const };
    f.queryClient.setQueryData(f.key, fresh);
    await f.runtime.sendGoalOperation(fresh, { kind: 'set', objective: 'Ship mobile', status: 'active', replace: false });
    expect(f.commands.find((command) => command.type === 'start'))
      .toMatchObject({ options: { cwd: '/repo', provider: 'codex' } });
    expect(f.commands.at(-1)).toEqual({ type: 'goal', operation: { kind: 'set', objective: 'Ship mobile', status: 'active', replace: false } });
    expect(f.commands.some((command) => (command.type === 'prompt' || command.type === 'promptWithConfiguration'))).toBe(false);
    f.emit('goalUpdated', { objective: 'Ship mobile', status: 'active', tokensUsed: 0, timeUsedSeconds: 0 });
    expect(f.current().thread_goal?.objective).toBe('Ship mobile');
    expect(f.current().messages).toEqual([]);
  });

  test('preserves history across two desktop steers after attaching from a list placeholder', async () => {
    const f = fixture();
    const loading = f.queryClient.fetchQuery({
      queryKey: f.key,
      queryFn: () => hydrateSession(f.client, f.history.id),
    });
    const attaching = f.runtime.attachSession(f.skeleton);
    await Promise.resolve();
    const commandsBeforeHydration = f.commands.map((command) => command.type);
    f.hydration.resolve(f.history);
    await loading;
    await attaching;

    f.emit('textDelta', ' before steering');
    f.emit('availableCommands', []);
    // The runtime outlives the screen's query-cache entry.
    f.queryClient.removeQueries({ queryKey: f.key });
    f.emit('steerAccepted', { message: 'First desktop steer' });
    f.emit('steerAccepted', { message: 'Second desktop steer' });

    expect(f.current().messages.map((message) => message.content)).toEqual([
      'Previous request', 'Previous answer', 'Current request',
      'Streaming response before steering', 'First desktop steer', 'Second desktop steer',
    ]);
    expect(f.current().turns).toEqual(f.history.turns);
    expect(commandsBeforeHydration).toEqual(['hydrateSession']);
    expect(f.commands.filter((command) => command.type === 'hydrateSession')).toHaveLength(1);
  });

  test('keeps the latest streamed history when the query cache is evicted before two desktop steers', async () => {
    const f = fixture();
    f.queryClient.setQueryData(f.key, f.history);
    await f.runtime.attachSession(f.history);
    f.emit('textDelta', ' before steering');
    // An interactive event flushes pending streaming text immediately.
    f.emit('availableCommands', []);
    f.queryClient.removeQueries({ queryKey: f.key });
    f.emit('steerAccepted', { message: 'First desktop steer' });
    f.emit('steerAccepted', { message: 'Second desktop steer' });
    f.emit('textDelta', 'Continued response');
    f.emit('availableCommands', []);

    expect(f.current().messages.map((message) => message.content)).toEqual([
      'Previous request', 'Previous answer', 'Current request',
      'Streaming response before steering', 'First desktop steer', 'Second desktop steer',
      'Continued response',
    ]);
    expect(f.current().turns).toEqual(f.history.turns);
  });
});
