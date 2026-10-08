/* oxlint-disable xplat/no-dom-globals -- ported module: 'prompt' is a parameter name, not the DOM global. */

import type {
  AgentSession,
  BossSpeechRequest,
  GoalOperation,
  MessageAttachment,
  PendingPermission,
  PendingUserInput,
  ProviderKind,
  SequencedEvent,
  UserInputAnswer,
} from '@waku/client';

import {
  bossTerminalIntent,
  continuationPrompt,
  managedGoalDecision,
  managedGoalEvaluation,
  managedGoalOperation,
  MANAGED_GOAL_QUESTION,
  sessionAcceptsImmediateSteer,
} from '@waku/client';

import { isAgentQueuedMessage, reduceRuntimeEvent } from '@waku/client/event-reducer';
import { writeProviderProbeCache } from '@waku/client/provider-probe-cache';
import { randomUUID } from './uuid';
import { spokenBlock } from './boss-secondary';
import { pinBossTerminal } from './boss-secondary-store';
import { createStore } from '@octane-xplat/ui';
import { daemonState } from './daemon-store';
import {
  daemonQueryEntry,
  getDaemonQueryData,
  setDaemonQueryData,
  invalidateDaemonQueriesMatching,
  removeDaemonQuery,
} from './daemon-data';

import {
  attachDaemonSession,
  daemonKeys,
  hydrateSession,
  loadDaemonSettings,
  loadTaskState,
  materializeWorktree,
  persistSession,
  probeProvider,
  removeDaemonSession,
  type TaskState,
} from './daemon-api';

import { persistentStorageSync } from './composer-preferences-store';

import { sessionHasStarted } from './session-presentation';
import {
  applySessionOptions,
  beginTurn,
  createSession,
  providerPromptForSubmission,
  queueSubmission,
  sessionBusy,
  sessionCwd,
  sessionIsRunning,
  shouldApplyRuntimeEvent,
  submittedTurnIdentity,
  type NewSessionOptions,
  type SessionOptionChanges,
} from './mobile-runtime';

import {
  clearDaemonDisconnectErrors,
  isDaemonDisconnectError,
} from './runtime-errors';

export interface MobileRuntime {
  runtimeId: string;
  supportsSteer: boolean;
  supportsUserInputActions: boolean;
  starting: boolean;
  running: boolean;
}

interface RuntimeEntry extends MobileRuntime {
  /** Latest full projection, retained while subscribed even if the screen's
   * query is evicted. Never fall back to the snapshot captured on attach. */
  session: AgentSession;
  lastDriverError: string | null;
  unsubscribe: () => void;
  /** Buffered runtime events awaiting the next commit. */
  pending: SequencedEvent[];
  flushTimer: ReturnType<typeof setTimeout> | null;
  lastFlushAt: number;
}

interface PendingSubmission {
  providerPrompt: string;
  displayContent: string;
  attachments: MessageAttachment[];
}

/** Stream deltas commit at ≤ ~8.3 Hz, the desktop stream pump's cadence: the
 * transcript re-renders per commit, not per provider chunk. Interactive
 * events (permissions, turn lifecycle) flush the buffer immediately. */
const STREAM_COMMIT_MS = 120;

function deferrableEvent(event: SequencedEvent): boolean {
  const kind = event.event.kind;
  return kind === 'textDelta' || kind === 'reasoningDelta' || kind === 'usageUpdated';
}

interface RuntimeContextValue {
  runtimes: Record<string, MobileRuntime | undefined>;
  permissions: Record<string, PendingPermission | undefined>;
  /** Daemon-owned `agentRenameSelf` requests: they outlive the turn that
   * raised them, so they ride their own map — turn settles never clear
   * them, `requestSettled` events and answers do. */
  renameRequests: Record<string, PendingPermission | undefined>;
  userInputs: Record<string, PendingUserInput | undefined>;
  errors: Record<string, string | undefined>;
  attachSession: (session: AgentSession) => Promise<boolean>;
  sendPrompt: (
    session: AgentSession,
    prompt: string,
    attachments?: MessageAttachment[],
    providerPromptOverride?: string,
  ) => Promise<AgentSession>;
  steerPrompt: (
    session: AgentSession,
    prompt: string,
    attachments?: MessageAttachment[],
    providerPromptOverride?: string,
  ) => Promise<void>;
  createTask: (
    projectId: string,
    provider: ProviderKind,
    isolated: boolean,
    prompt: string,
    options?: NewSessionOptions,
    providerPromptOverride?: string,
    onCreated?: (session: AgentSession) => void,
  ) => Promise<AgentSession>;
  cancel: (sessionId: string) => Promise<void>;
  compactSession: (sessionId: string) => Promise<void>;
  rollbackSession: (sessionId: string, turns: number) => Promise<void>;
  rewindSessionToMessage: (
    sessionId: string,
    turnCount: number,
  ) => Promise<{ session: AgentSession; warning: string | null }>;
  forkSessionFromResponse: (
    sessionId: string,
    turnCount: number,
  ) => Promise<{ session: AgentSession; warning: string | null }>;
  respond: (sessionId: string, requestId: string, optionId: string) => Promise<void>;
  respondUserInput: (
    sessionId: string,
    requestId: string,
    answers: UserInputAnswer[],
  ) => Promise<void>;
  clarifyUserInput: (sessionId: string, requestId: string, content: string) => Promise<void>;
  cancelUserInput: (sessionId: string, requestId: string) => Promise<void>;
  updateSessionOptions: (sessionId: string, changes: SessionOptionChanges) => Promise<void>;
  grantAgentRename: (sessionId: string) => Promise<void>;
  sendGoalOperation: (session: AgentSession, operation: GoalOperation, managed?: boolean) => Promise<void>;
  renameSession: (sessionId: string, title: string) => Promise<void>;
  setSessionPinned: (sessionId: string, pinned: boolean) => Promise<void>;
  setSessionArchived: (sessionId: string, archived: boolean) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;
  removeQueuedMessage: (sessionId: string, messageId: string) => Promise<void>;
  dismissError: (sessionId: string) => void;
}

/** Facade matching the react-query calls the ported logic makes, so the
 * session-cache code survives verbatim on top of daemon-data stores. */
const queryClient = {
  getQueryData: <T,>(key: readonly unknown[]) => getDaemonQueryData<T>(key),
  setQueryData: <T,>(key: readonly unknown[], valueOrFn: T | ((current: T | undefined) => T | undefined)) =>
    setDaemonQueryData<T>(key, valueOrFn),
  fetchQuery: <T,>(opts: { queryKey: readonly unknown[]; queryFn: () => Promise<T> }) =>
    daemonQueryEntry(opts.queryKey, opts.queryFn).refetch(),
  invalidateQueries: (opts: { queryKey: readonly unknown[]; predicate?: (q: { queryKey: readonly unknown[] }) => boolean }) =>
    invalidateDaemonQueriesMatching(opts.queryKey, opts.predicate),
  removeQueries: (opts: { queryKey: readonly unknown[] }) => removeDaemonQuery(opts.queryKey),
};

/** Fresh-enough daemon snapshot — updated synchronously on every store
 * write, which is the same render-time recency the context gave callers. */
let daemon = daemonState.get();

const clock = {
  nowSeconds: () => Math.floor(Date.now() / 1_000),
  nowMillis: () => Date.now(),
  randomUUID: randomUUID,
};

export interface RuntimeState {
  runtimes: Record<string, MobileRuntime | undefined>;
  permissions: Record<string, PendingPermission | undefined>;
  renameRequests: Record<string, PendingPermission | undefined>;
  userInputs: Record<string, PendingUserInput | undefined>;
  errors: Record<string, string | undefined>;
}

export const runtimeState = createStore<RuntimeState>({
  runtimes: {},
  permissions: {},
  renameRequests: {},
  userInputs: {},
  errors: {},
});

type SliceUpdater<T> = T | ((current: T) => T);
function setSlice<K extends keyof RuntimeState>(key: K, update: SliceUpdater<RuntimeState[K]>) {
  runtimeState.set((current) => ({
    ...current,
    [key]: typeof update === 'function' ? (update as (c: RuntimeState[K]) => RuntimeState[K])(current[key]) : update,
  }));
}

const setRuntimes = (u: SliceUpdater<RuntimeState['runtimes']>) => setSlice('runtimes', u);
const setPermissions = (u: SliceUpdater<RuntimeState['permissions']>) => setSlice('permissions', u);
const setRenameRequests = (u: SliceUpdater<RuntimeState['renameRequests']>) => setSlice('renameRequests', u);
const setUserInputs = (u: SliceUpdater<RuntimeState['userInputs']>) => setSlice('userInputs', u);
const setErrors = (u: SliceUpdater<RuntimeState['errors']>) => setSlice('errors', u);

const entries = { current: new Map<string, RuntimeEntry>() };
const attachRequests = { current: new Map<string, Promise<boolean>>() };
/** The daemon connection count whose runtimes were last revalidated. */
const revalidatedConnections = { current: 0 };
const persistTails = { current: new Map<string, Promise<AgentSession>>() };
const persistTimers = { current: new Map<string, ReturnType<typeof setTimeout>>() };
/** Advanced on every cache write of new local state, so a save reply that
 * lands after newer runtime events cannot roll the cache back to the
 * snapshot it saved. */
const cacheGenerations = { current: new Map<string, number>() };
const pendingSteers = { current: new Map<string, PendingSubmission[]>() };
const drainingQueues = { current: new Set<string>() };
const sendPromptRef: { current: ((
  session: AgentSession,
  prompt: string,
  attachments?: MessageAttachment[],
  providerPromptOverride?: string,
) => Promise<AgentSession>) | null } = { current: null };

const managedGoalSettleRef: { current: ((session: AgentSession) => Promise<void>) | null } = { current: null };

  /** Writes a session into the query cache without advancing its
   * generation: the daemon's echo of a snapshot this client already holds. */
  const writeSessionCache = (session: AgentSession) => {
    const profileId = daemon.activeProfile?.id;
    if (!profileId) {return;}
    const entry = entries.current.get(session.id);
    if (entry) {entry.session = session;}
    queryClient.setQueryData(daemonKeys.session(profileId, session.id), session);
    queryClient.setQueryData<TaskState>(daemonKeys.taskState(profileId), (current) => {
      if (!current) {return current;}
      const sessions = current.sessions.some((item) => item.id === session.id)
        ? current.sessions.map((item) => item.id === session.id ? session : item)
        : [...current.sessions, session];

      return { ...current, sessions };
    });
  };

  /** Caches new local state — a submission, a runtime event, an edit. */
  const cacheSession = (session: AgentSession) => {
    cacheGenerations.current.set(
      session.id,
      (cacheGenerations.current.get(session.id) ?? 0) + 1,
    );

    const entry = entries.current.get(session.id);
    const running = sessionIsRunning(session);
    if (entry && entry.running !== running) {
      entry.running = running;
      setRuntimes((current) => ({ ...current, [session.id]: publicRuntime(entry) }));
    }

    writeSessionCache(session);
  };

  /** Full session from the query cache, hydrating from the daemon when only
   * the list skeleton is known. A skeleton must never be persisted back — it
   * would blank the stored transcript. */
  const loadFullSession = async (sessionId: string): Promise<AgentSession> => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId) {throw new Error('Goddard daemon is disconnected');}
    const cached = queryClient.getQueryData<AgentSession>(
      daemonKeys.session(profileId, sessionId),
    ) ?? entries.current.get(sessionId)?.session;

    if (cached) {return cached;}
    // Share the screen's in-flight hydration. An independent fetch could
    // finish after subscription and overwrite events already applied live.
    const hydrated = await queryClient.fetchQuery({
      queryKey: daemonKeys.session(profileId, sessionId),
      queryFn: () => hydrateSession(client, sessionId),
    });

    if (!hydrated) {throw new Error('This task no longer exists on the daemon');}
    return hydrated;
  };

  const persistOrdered = (session: AgentSession): Promise<AgentSession> => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId) {return Promise.reject(new Error('Goddard daemon is disconnected'));}
    const generation = cacheGenerations.current.get(session.id);
    const previous = persistTails.current.get(session.id);
    const operation = (previous ?? Promise.resolve(session))
      .catch(() => session)
      .then(() => persistSession(client, session))
      .then((saved) => {
        // The reply describes the snapshot that was saved. Runtime events
        // committed while it was in flight are newer than that snapshot, so
        // once the cache has moved on keep it and take only what the daemon
        // owns — turn checkpoints — from the reply.
        if (cacheGenerations.current.get(session.id) === generation) {
          writeSessionCache(saved);
        } else {
          const latest = queryClient.getQueryData<AgentSession>(
            daemonKeys.session(profileId, session.id),
          );

          if (latest) {writeSessionCache(withDaemonCheckpoints(latest, saved));}
        }

        return saved;
      });

    persistTails.current.set(session.id, operation);
    void operation.finally(() => {
      if (persistTails.current.get(session.id) === operation) {persistTails.current.delete(session.id);}
    }).catch(() => {});

    return operation;
  };

  const schedulePersist = (sessionId: string) => {
    const profileId = daemon.activeProfile?.id;
    if (!profileId) {return;}
    const previous = persistTimers.current.get(sessionId);
    if (previous) {clearTimeout(previous);}
    const timer = setTimeout(() => {
      persistTimers.current.delete(sessionId);
      const latest = queryClient.getQueryData<AgentSession>(
        daemonKeys.session(profileId, sessionId),
      );

      if (latest) {void persistOrdered(latest).catch((cause) => {
        setErrors((current) => ({ ...current, [sessionId]: errorMessage(cause) }));
      });}
    }, 500);

    persistTimers.current.set(sessionId, timer);
  };

  const removeRuntime = (sessionId: string) => {
    const entry = entries.current.get(sessionId);
    entry?.unsubscribe();
    entries.current.delete(sessionId);
    pendingSteers.current.delete(sessionId);
    setRuntimes((current) => removeKey(current, sessionId));
  };

  /** After a settled turn is persisted, submit the oldest queued message as
   * the next turn. Guarded per session so one settle drains one message. */
  const drainQueue = async (sessionId: string) => {
    const profileId = daemon.activeProfile?.id;
    if (!profileId || drainingQueues.current.has(sessionId)) {return;}
    const latest = queryClient.getQueryData<AgentSession>(
      daemonKeys.session(profileId, sessionId),
    );

    if (!latest) {return;}
    if (latest.status === 'failed') {
      await managedGoalSettleRef.current?.(latest);
      return;
    }

    if (latest.status !== 'idle') {return;}
    // Daemon-owned entries drain on the daemon's schedule — submitting one
    // here would double-deliver it once the daemon's drain fires.
    const next = latest.queued_messages?.find((message) => !isAgentQueuedMessage(message));
    if (!next) {
      await managedGoalSettleRef.current?.(latest);
      return;
    }

    drainingQueues.current.add(sessionId);
    try {
      const dequeued = {
        ...latest,
        queued_messages: latest.queued_messages?.filter((message) => message.id !== next.id),
      };

      cacheSession(dequeued);
      const persisted = await persistOrdered(dequeued);
      await sendPromptRef.current?.(
        persisted,
        next.display_content ?? next.content,
        next.attachments ?? [],
        next.content,
      );
    } catch (cause) {
      setErrors((values) => ({ ...values, [sessionId]: errorMessage(cause) }));
    } finally {
      drainingQueues.current.delete(sessionId);
    }
  };

  const subscribe = (
    session: AgentSession,
    runtimeId: string,
    supportsSteer = false,
    supportsUserInputActions = false,
    starting = true,
  ): RuntimeEntry => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId) {throw new Error('Goddard daemon is disconnected');}
    const existing = entries.current.get(session.id);
    if (existing) {return existing;}

    const entry: RuntimeEntry = {
      session,
      runtimeId,
      supportsSteer,
      supportsUserInputActions,
      starting,
      running: sessionIsRunning(session),
      lastDriverError: null,
      unsubscribe: () => {},
      pending: [],
      flushTimer: null,
      lastFlushAt: 0,
    };

    entries.current.set(session.id, entry);
    cacheSession(session);
    setRuntimes((current) => ({ ...current, [session.id]: publicRuntime(entry) }));

    /** Apply one (possibly coalesced) event to `current` and collect its
     * side effects; the caller commits once per flush. */
    interface FlushState {
      current: AgentSession;
      mutated: boolean;
      settled: boolean;
      removeRuntime: boolean;
    }

    const reduceOne = (state: FlushState, event: SequencedEvent) => {
      if (event.event.kind === 'connected' || event.event.kind === 'turnStarted' || event.event.kind === 'turnFinished') {
        entry.lastDriverError = null;
      } else if (event.event.kind === 'error' && typeof event.event.payload === 'string') {
        entry.lastDriverError = event.event.payload;
      }

      if (event.event.kind === 'steerAccepted') {
        const payload = event.event.payload as { message?: string; hidden?: boolean };
        // A hidden steer is daemon-injected context — provider-facing
        // only, so nothing lands in the transcript.
        if (payload.hidden) {return;}
        // The provider folded a steer into the live turn. Ours is pending
        // here; another client's is not, and its message belongs in this
        // transcript just the same — the desktop mirrors it too.
        const pending = pendingSteers.current.get(session.id)?.shift();
        const content = payload.message ?? pending?.providerPrompt;
        if (content) {
          state.current = {
            ...state.current,
            messages: [
              ...state.current.messages,
              {
                id: clock.randomUUID(),
                turn_id: state.current.turns.at(-1)?.id ?? null,
                role: 'user',
                content,
                display_content: pending && (
                  pending.attachments.length || pending.providerPrompt !== pending.displayContent
                ) ? pending.displayContent : null,
                attachments: pending?.attachments ?? [],
                created_at: clock.nowSeconds(),
                streaming: false,
              },
            ],
          };
        }
      } else if (event.event.kind === 'steerRejected') {
        const pending = pendingSteers.current.get(session.id)?.shift();
        if (pending) {
          state.current = queueSubmission(
            state.current,
            pending.displayContent,
            clock,
            pending.attachments,
            pending.providerPrompt,
          );

          void persistOrdered(state.current).catch(() => {});
          setErrors((values) => ({
            ...values,
            [session.id]: 'The agent couldn’t take that mid-turn, so it was queued for the next turn.',
          }));
        }
      }

      const result = reduceRuntimeEvent(state.current, event, clock, entry.lastDriverError);
      state.current = result.session;
      state.mutated = true;
      if (result.permission !== undefined) {
        setPermissions((values) => ({ ...values, [session.id]: result.permission ?? undefined }));
      }

      if (result.renameRequest !== undefined) {
        setRenameRequests((values) => ({ ...values, [session.id]: result.renameRequest ?? undefined }));
      }

      if (result.settledRequestId !== undefined) {
        const settled = result.settledRequestId;
        const drop = <T extends { requestId: string }>(values: Record<string, T | undefined>) =>
          values[session.id]?.requestId === settled ? { ...values, [session.id]: undefined } : values;

        setPermissions(drop);
        setRenameRequests(drop);
        setUserInputs(drop);
      }

      if (result.userInput !== undefined) {
        setUserInputs((values) => ({ ...values, [session.id]: result.userInput ?? undefined }));
      }

      if (result.error) {setErrors((values) => ({ ...values, [session.id]: result.error }));}
      if (result.settled) {state.settled = true;}
      if (result.removeRuntime) {state.removeRuntime = true;}
    };

    /** Drain the buffer as one commit. Adjacent same-kind stream deltas
     * coalesce into a single reduce (the desktop's `pop_stream_batch`), with
     * per-event dedupe gates so a replay straddling the cursor stays exact. */
    const flush = () => {
      entry.flushTimer = null;
      entry.lastFlushAt = Date.now();
      if (entries.current.get(session.id) !== entry) {
        entry.pending.length = 0;
        return;
      }

      const batch = entry.pending.splice(0);
      if (!batch.length) {return;}
      const key = daemonKeys.session(profileId, session.id);
      const state: FlushState = {
        current: queryClient.getQueryData<AgentSession>(key) ?? entry.session,
        mutated: false,
        settled: false,
        removeRuntime: false,
      };

      let run: { kind: 'textDelta' | 'reasoningDelta'; text: string; envelope: SequencedEvent } | null = null;
      const flushRun = () => {
        if (!run) {return;}
        reduceOne(state, { ...run.envelope, event: { kind: run.kind, payload: run.text } });
        run = null;
      };

      for (const event of batch) {
        if (!shouldApplyRuntimeEvent(state.current, event)) {continue;}
        const kind = event.event.kind;
        if ((kind === 'textDelta' || kind === 'reasoningDelta') && typeof event.event.payload === 'string') {
          if (run && run.kind === kind) {
            run.text += event.event.payload;
            run.envelope = event;
          } else {
            flushRun();
            run = { kind, text: event.event.payload, envelope: event };
          }

          continue;
        }

        flushRun();
        reduceOne(state, event);
      }

      flushRun();
      if (!state.mutated) {return;}
      cacheSession(state.current);
      if (state.settled) {
        const timer = persistTimers.current.get(session.id);
        if (timer) {clearTimeout(timer);}
        persistTimers.current.delete(session.id);
        void persistOrdered(state.current)
          .then(() => queryClient.invalidateQueries({ queryKey: daemonKeys.taskState(profileId) }))
          .then(() => drainQueue(session.id))
          .catch((cause) => {
            setErrors((values) => ({ ...values, [session.id]: errorMessage(cause) }));
          });
      } else {
        schedulePersist(session.id);
      }

      if (state.removeRuntime) {removeRuntime(session.id);}
    };

    const unsubscribe = client.subscribe(session.id, runtimeId, (event) => {
      if (entries.current.get(session.id) !== entry) {return;}
      const intent = bossTerminalIntent(event.event);
      if (intent) {
        const profileId = daemon.activeProfile?.id;
        const boss = profileId ? getDaemonQueryData<{ sessionId: string | null }>(daemonKeys.bossState(profileId)) : null;
        if (boss?.sessionId === session.id) {
          const taskState = profileId ? getDaemonQueryData<TaskState>(daemonKeys.taskState(profileId)) : null;
          const project = taskState?.projects.find((item) => item.id === entry.session.project_id);
          pinBossTerminal(intent, session.id, `${event.epoch}:${event.runtimeId}:${event.sequence}`, project?.path ?? null);
        }

        return;
      }

      entry.pending.push(event);
      if (!deferrableEvent(event)) {
        if (entry.flushTimer) {
          clearTimeout(entry.flushTimer);
          entry.flushTimer = null;
        }

        flush();
        return;
      }

      if (entry.flushTimer) {return;}
      const wait = Math.max(0, STREAM_COMMIT_MS - (Date.now() - entry.lastFlushAt));
      entry.flushTimer = setTimeout(flush, wait);
    });

    const teardown = () => {
      if (entry.flushTimer) {
        clearTimeout(entry.flushTimer);
        entry.flushTimer = null;
      }

      entry.pending.length = 0;
      unsubscribe();
    };

    if (entries.current.get(session.id) === entry) {entry.unsubscribe = teardown;}
    else {teardown();}

    return entry;
  };

  const attachSession = (session: AgentSession): Promise<boolean> => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId || daemon.phase !== 'connected') {return Promise.resolve(false);}
    if (entries.current.has(session.id)) {return Promise.resolve(true);}
    const pending = attachRequests.current.get(session.id);
    if (pending) {return pending;}

    const request = (async () => {
      // The screen may still be displaying a list placeholder. Its empty
      // arrays are not a transcript, so hydrate before replaying any events.
      const hydrated = await loadFullSession(session.id);
      const attached = await attachDaemonSession(client, session.id);
      if (!client.connected || !attached) {return false;}
      const current = queryClient.getQueryData<AgentSession>(
        daemonKeys.session(profileId, session.id),
      ) ?? hydrated;

      subscribe(current, attached.runtimeId, attached.supportsSteer, attached.supportsUserInputActions, false);
      return true;
    })().finally(() => {
      if (attachRequests.current.get(session.id) === request) {attachRequests.current.delete(session.id);}
    });

    attachRequests.current.set(session.id, request);
    return request;
  };

  const sendPrompt = async (
    inputSession: AgentSession,
    rawPrompt: string,
    attachments: MessageAttachment[] = [],
    providerPromptOverride?: string,
  ): Promise<AgentSession> => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId || daemon.phase !== 'connected') {
      throw new Error('Goddard daemon is disconnected');
    }

    const prompt = rawPrompt.trim();
    if (!prompt && attachments.length === 0) {return inputSession;}
    const providerPrompt = providerPromptOverride === undefined
      ? providerPromptForSubmission(prompt, attachments)
      : providerPromptOverride.trim();

    // The screen can hand over a task-list skeleton while hydration is
    // still in flight; building the turn on that would persist a transcript
    // with only the new messages. Always start from the full session.
    let current = await loadFullSession(inputSession.id);
    if (sessionBusy(current)) {
      const queued = queueSubmission(current, prompt, clock, attachments, providerPrompt);
      cacheSession(queued);
      return persistOrdered(queued);
    }

    if (!entries.current.has(current.id)) {await attachSession(current);}
    let runtime = entries.current.get(current.id);
    let startup: { binary: string; cwd: string } | null = null;
    if (!runtime) {
      const state = await loadTaskState(client);
      const project = state.projects.find((item) => item.id === current.project_id);
      if (!project) {throw new Error('This task’s project is no longer available on the daemon');}
      if (current.workspace?.kind === 'newWorktree') {
        current = await materializeWorktree(client, current, project.path);
        cacheSession(current);
        current = await persistOrdered(current);
      }

      const settings = await loadDaemonSettings(client);
      const probe = await probeProvider(client, current.provider, settings);
      if (!probe.installed || !probe.path) {
        throw new Error(`${current.provider} is not installed on the daemon host`);
      }

      if (daemon.activeProfile) {
        writeProviderProbeCache(
          persistentStorageSync(),
          daemon.activeProfile.address,
          current.provider,
          settings.provider_binary_overrides?.[current.provider] ?? null,
          probe,
        );
      }

      startup = { binary: probe.path, cwd: sessionCwd(current, project) };
    }

    current = beginTurn(current, prompt, clock, attachments, providerPrompt);
    // The ids beginTurn gave the turn and its user message ride along with
    // the prompt, so every other client attached to the runtime mirrors the
    // same rows instead of minting its own.
    const submitted = submittedTurnIdentity(current);
    cacheSession(current);
    current = await persistOrdered(current);
    try {
      if (!runtime) {
        const runtimeId = randomUUID();
        runtime = subscribe(current, runtimeId);
        const response = await client.request({
          type: 'start',
          options: {
            provider: current.provider,
            binary: startup!.binary,
            cwd: startup!.cwd,
            mode: current.runtime_mode,
            model: current.model ?? null,
            reasoningEffort: current.reasoning_effort ?? null,
            serviceTier: current.service_tier ?? null,
            contextWindow: current.context_window ?? null,
            agentPreset: current.agent_preset ?? null,
            computerUseEnabled: false,
            readOwnTranscript: current.side_chat_of != null
              || Boolean(current.suspended_provider_sessions?.length)
              || current.pending_provider_context != null,
            providerCursor: current.provider_cursor as never,
          },
        }, current.id, runtimeId);

        if (response.type !== 'started') {
          throw new Error(`Expected daemon response started, received ${response.type}`);
        }

        runtime.supportsSteer = response.supportsSteer;
        runtime.supportsUserInputActions = response.supportsUserInputActions;
        runtime.starting = false;
        setRuntimes((values) => ({ ...values, [current.id]: publicRuntime(runtime!) }));
      }

      await client.request(
        {
          type: 'prompt',
          prompt: providerPrompt,
          turnId: submitted.turnId,
          messageId: submitted.messageId,
          attachments,
        },
        current.id,
        runtime.runtimeId,
      );

      setErrors((values) => removeKey(values, current.id));
      return current;
    } catch (cause) {
      if (runtime?.starting) {removeRuntime(current.id);}
      const latest = queryClient.getQueryData<AgentSession>(
        daemonKeys.session(profileId, current.id),
      ) ?? current;

      const cursor = latest.runtime_event_cursor;
      const failed = reduceRuntimeEvent(latest, {
        sessionId: latest.id,
        runtimeId: runtime?.runtimeId ?? randomUUID(),
        epoch: cursor?.epoch ?? `mobile-${Date.now()}`,
        sequence: (cursor?.sequence ?? 0) + 1,
        event: { kind: 'turnFinished', payload: { success: false, summary: errorMessage(cause) } },
      }, clock).session;

      cacheSession(failed);
      await persistOrdered(failed).catch(() => failed);
      try { await managedGoalSettleRef.current?.(failed); } catch { /* retain the provider error */ }
      throw cause;
    }
  };

sendPromptRef.current = sendPrompt;

  managedGoalSettleRef.current = async (settled) => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId) {return;}
    const plan = managedGoalEvaluation(settled);
    if (!plan) {return;}
    let shouldEvaluate = !plan.stop;
    if (shouldEvaluate) {
      try {
        const claim = await client.request({
          type: 'claimManagedGoalTurn', goalId: plan.goalId, turnId: plan.turnId,
        }, settled.id);

        if (claim.type !== 'managedGoalTurnClaimed') {shouldEvaluate = false;}
        else if (!claim.claimed) {return;}
      } catch {
        shouldEvaluate = false;
      }
    }

    const current = queryClient.getQueryData<AgentSession>(daemonKeys.session(profileId, settled.id));
    if (!current || current.thread_goal?.managedId !== plan.goalId
      || current.thread_goal.status !== 'active'
      || current.turns.at(-1)?.id !== plan.turnId
      || current.messages.length !== settled.messages.length
      || current.queued_messages?.length) {return;}

    const marked: AgentSession = {
      ...current,
      thread_goal: { ...current.thread_goal, managedLastTurn: plan.turnId },
    };

    cacheSession(marked);
    await persistOrdered(marked);
    let decision: ReturnType<typeof managedGoalDecision> = 'paused';
    if (shouldEvaluate) {
      try {
        const response = await client.request({
          type: 'evaluate',
          state: plan.state,
          questions: MANAGED_GOAL_QUESTION,
          feature: 'managed-goal',
        }, settled.id);

        if (response.type === 'evaluation') {
          decision = managedGoalDecision(response.evaluation.answers.next);
        }
      } catch {
        // A missing or failed evaluator pauses the loop.
      }
    }

    const latest = queryClient.getQueryData<AgentSession>(daemonKeys.session(profileId, settled.id));
    if (!latest || latest.thread_goal?.managedId !== plan.goalId
      || latest.thread_goal.managedLastTurn !== plan.turnId
      || latest.thread_goal.status !== 'active'
      || latest.turns.at(-1)?.id !== plan.turnId
      || latest.queued_messages?.length) {return;}

    if (decision === 'continue') {
      await sendPromptRef.current?.(latest, continuationPrompt(latest.thread_goal.objective));
    } else {
      const updated: AgentSession = {
        ...latest,
        thread_goal: { ...latest.thread_goal, status: decision },
      };

      cacheSession(updated);
      await persistOrdered(updated);
    }
  };

  /** Inject a prompt while the open turn can still consume it. */
  const steerPrompt = async (
    session: AgentSession,
    rawPrompt: string,
    attachments: MessageAttachment[] = [],
    providerPromptOverride?: string,
  ) => {
    const client = daemon.client;
    const prompt = rawPrompt.trim();
    if (!prompt && attachments.length === 0) {return;}
    const providerPrompt = providerPromptOverride === undefined
      ? providerPromptForSubmission(prompt, attachments)
      : providerPromptOverride.trim();

    if (!client || daemon.phase !== 'connected') {throw new Error('Goddard daemon is disconnected');}
    const runtime = entries.current.get(session.id);
    if (!runtime || !runtime.supportsSteer || !sessionAcceptsImmediateSteer(session)) {
      await sendPrompt(session, prompt, attachments, providerPrompt);
      return;
    }

    const pending = pendingSteers.current.get(session.id) ?? [];
    pending.push({ providerPrompt, displayContent: prompt, attachments });
    pendingSteers.current.set(session.id, pending);
    await client.request({ type: 'steer', prompt: providerPrompt }, session.id, runtime.runtimeId);
  };

  const createTask = async (
    projectId: string,
    provider: ProviderKind,
    isolated: boolean,
    prompt: string,
    options: NewSessionOptions = {},
    providerPromptOverride?: string,
    onCreated?: (session: AgentSession) => void,
  ): Promise<AgentSession> => {
    const profileId = daemon.activeProfile?.id;
    if (!profileId || !daemon.client || daemon.phase !== 'connected') {
      throw new Error('Goddard daemon is disconnected');
    }

    const draft = createSession(projectId, provider, isolated, clock, options);
    cacheSession(draft);
    const saved = await persistOrdered(draft);
    onCreated?.(saved);
    try {
      return await sendPrompt(saved, prompt, [], providerPromptOverride);
    } catch (cause) {
      setErrors((values) => ({ ...values, [saved.id]: errorMessage(cause) }));
      return queryClient.getQueryData<AgentSession>(
        daemonKeys.session(profileId, saved.id),
      ) ?? saved;
    }
  };

  /** Local /goal commands use the native provider operation without adding
   * a user prompt. A fresh task still needs a subscribed provider runtime;
   * its goalUpdated/turnStarted events supply the authoritative state. */
  const sendGoalOperation = async (inputSession: AgentSession, operation: GoalOperation, managed = false) => {
    const client = daemon.client;
    if (!client || daemon.phase !== 'connected') {throw new Error('Goddard daemon is disconnected');}
    let current = await loadFullSession(inputSession.id);
    if (managed || current.provider !== 'codex' || current.thread_goal?.managedId) {
      let base = current;
      if (operation.kind === 'set' && operation.objective) {
        base = {
          ...base,
          messages: [...base.messages, {
            id: randomUUID(),
            turn_id: null,
            role: 'system',
            content: `Goal set: ${operation.objective}`,
            created_at: Math.floor(Date.now() / 1000),
            streaming: false,
          }],
        };
      }

      const updated = managedGoalOperation(base, operation, randomUUID);
      cacheSession(updated.session);
      const saved = await persistOrdered(updated.session);
      if (updated.prompt) {await sendPromptRef.current?.(saved, updated.prompt);}
      return;
    }

    if (!entries.current.has(current.id)) {await attachSession(current);}
    let runtime = entries.current.get(current.id);
    if (!runtime) {
      const state = await loadTaskState(client);
      const project = state.projects.find((item) => item.id === current.project_id);
      if (!project) {throw new Error('This task’s project is no longer available on the daemon');}
      const settings = await loadDaemonSettings(client);
      const probe = await probeProvider(client, current.provider, settings);
      if (!probe.installed || !probe.path) {throw new Error('Codex is not installed on the daemon host');}
      current = await materializeWorktree(client, current, project.path);
      cacheSession(current);
      current = await persistOrdered(current);
      const runtimeId = randomUUID();
      runtime = subscribe(current, runtimeId);
      try {
        const response = await client.request({
          type: 'start',
          options: {
            provider: current.provider,
            binary: probe.path,
            cwd: sessionCwd(current, project),
            mode: current.runtime_mode,
            model: current.model ?? null,
            reasoningEffort: current.reasoning_effort ?? null,
            serviceTier: current.service_tier ?? null,
            contextWindow: current.context_window ?? null,
            agentPreset: current.agent_preset ?? null,
            computerUseEnabled: false,
            readOwnTranscript: current.side_chat_of != null
              || Boolean(current.suspended_provider_sessions?.length)
              || current.pending_provider_context != null,
            providerCursor: current.provider_cursor as never,
          },
        }, current.id, runtimeId);

        if (response.type !== 'started') {throw new Error('The daemon could not start Codex');}
        runtime.supportsSteer = response.supportsSteer;
        runtime.supportsUserInputActions = response.supportsUserInputActions;
        runtime.starting = false;
        setRuntimes((values) => ({ ...values, [current.id]: publicRuntime(runtime!) }));
      } catch (cause) {
        removeRuntime(current.id);
        throw cause;
      }
    }

    await client.request({ type: 'goal', operation }, current.id, runtime.runtimeId);
  };

  const cancel = async (sessionId: string) => {
    const client = daemon.client;
    if (!client) {throw new Error('Goddard daemon is disconnected');}
    const runtime = entries.current.get(sessionId);
    if (!runtime) {throw new Error('This task has no live agent runtime');}
    await client.request({ type: 'cancel' }, sessionId, runtime.runtimeId);
  };

  const compactSession = async (sessionId: string) => {
    const client = daemon.client;
    if (!client) {throw new Error('Goddard daemon is disconnected');}
    const runtime = entries.current.get(sessionId);
    if (!runtime) {throw new Error('This task has no live agent runtime');}
    await client.request({ type: 'compact' }, sessionId, runtime.runtimeId);
  };

  const rollbackSession = async (sessionId: string, turns: number) => {
    const client = daemon.client;
    if (!client) {throw new Error('Goddard daemon is disconnected');}
    const runtime = entries.current.get(sessionId);
    if (!runtime) {throw new Error('This task has no live agent runtime');}
    await client.request({ type: 'rollback', turns }, sessionId, runtime.runtimeId);
  };

  /** Rewind to before one user turn — the daemon restores the checkpoint,
   * truncates the provider conversation, and returns the rewritten session. */
  const rewindSessionToMessage = async (
    sessionId: string,
    turnCount: number,
  ): Promise<{ session: AgentSession; warning: string | null }> => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId) {throw new Error('Goddard daemon is disconnected');}
    const response = await client.request(
      { type: 'rewindSessionToMessage', turnCount },
      sessionId,
    );

    if (response.type !== 'sessionRewound') {
      throw new Error(`Expected sessionRewound, received ${response.type}`);
    }

    const next = { ...response.session, detail_loaded: true };
    cacheSession(next);
    void queryClient.invalidateQueries({ queryKey: daemonKeys.taskState(profileId) });
    return { session: next, warning: response.cleanupWarning };
  };

  /** Fork the session through one response — the daemon copies the provider
   * conversation and checkpoint refs into a new task and returns it. */
  const forkSessionFromResponse = async (
    sessionId: string,
    turnCount: number,
  ): Promise<{ session: AgentSession; warning: string | null }> => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId) {throw new Error('Goddard daemon is disconnected');}
    const response = await client.request(
      { type: 'forkSessionFromResponse', turnCount },
      sessionId,
    );

    if (response.type !== 'sessionForked') {
      throw new Error(`Expected sessionForked, received ${response.type}`);
    }

    const next = { ...response.session, detail_loaded: true };
    cacheSession(next);
    void queryClient.invalidateQueries({ queryKey: daemonKeys.taskState(profileId) });
    return { session: next, warning: response.checkpointWarning };
  };

  const markWorking = (sessionId: string) => {
    const profileId = daemon.activeProfile?.id;
    if (!profileId) {return;}
    const key = daemonKeys.session(profileId, sessionId);
    const session = queryClient.getQueryData<AgentSession>(key);
    if (!session) {return;}
    const updated = { ...session, status: 'working' as const };
    cacheSession(updated);
    void persistOrdered(updated).catch((cause) => {
      setErrors((values) => ({ ...values, [sessionId]: errorMessage(cause) }));
    });
  };

  const respond = async (sessionId: string, requestId: string, optionId: string) => {
    const client = daemon.client;
    const runtime = entries.current.get(sessionId);
    if (!client || !runtime) {throw new Error('This task has no live agent runtime');}
    await client.request({ type: 'respond', requestId, optionId }, sessionId, runtime.runtimeId);
    setPermissions((values) => ({ ...values, [sessionId]: undefined }));
    setRenameRequests((values) => ({ ...values, [sessionId]: undefined }));
    markWorking(sessionId);
  };

  const respondUserInput = async (
    sessionId: string,
    requestId: string,
    answers: UserInputAnswer[],
  ) => {
    const client = daemon.client;
    const runtime = entries.current.get(sessionId);
    if (!client || !runtime) {throw new Error('This task has no live agent runtime');}
    await client.request(
      { type: 'respondUserInput', requestId, answers },
      sessionId,
      runtime.runtimeId,
    );

    setUserInputs((values) => ({ ...values, [sessionId]: undefined }));
    markWorking(sessionId);
  };

  const clarifyUserInput = async (
    sessionId: string,
    requestId: string,
    content: string,
  ) => {
    const client = daemon.client;
    const runtime = entries.current.get(sessionId);
    if (!client || !runtime) {throw new Error('This task has no live agent runtime');}
    await client.request(
      { type: 'clarifyUserInput', requestId, content },
      sessionId,
      runtime.runtimeId,
    );

    setUserInputs((values) => ({ ...values, [sessionId]: undefined }));
    markWorking(sessionId);
  };

  const cancelUserInput = async (sessionId: string, requestId: string) => {
    const client = daemon.client;
    const runtime = entries.current.get(sessionId);
    if (!client || !runtime) {throw new Error('This task has no live agent runtime');}
    await client.request(
      { type: 'cancelUserInput', requestId },
      sessionId,
      runtime.runtimeId,
    );

    setUserInputs((values) => ({ ...values, [sessionId]: undefined }));
    markWorking(sessionId);
  };

  /** Change model traits / access mode. Applied live via
   * applyOptions when a runtime exists; a runtime that can't take the change
   * is closed so the next prompt restarts with the new options. */
  const updateSessionOptions = async (
    sessionId: string,
    changes: SessionOptionChanges,
  ) => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId || daemon.phase !== 'connected') {
      throw new Error('Goddard daemon is disconnected');
    }

    const current = await loadFullSession(sessionId);
    const next = applySessionOptions(current, changes, clock);
    cacheSession(next);
    try {
      const runtime = entries.current.get(sessionId);
      if (runtime && !runtime.starting) {
        const response = await client.request({
          type: 'applyOptions',
          options: {
            mode: next.runtime_mode,
            model: next.model ?? null,
            reasoningEffort: next.reasoning_effort ?? null,
            serviceTier: next.service_tier ?? null,
            contextWindow: next.context_window ?? null,
          },
        }, sessionId, runtime.runtimeId);

        if (response.type !== 'optionsApplied' || !response.applied) {
          await client.request({ type: 'closeSession' }, sessionId, runtime.runtimeId);
          removeRuntime(sessionId);
        }
      }

      await persistOrdered(next);
    } catch (cause) {
      cacheSession(current);
      throw cause;
    }
  };

  const renameSession = async (sessionId: string, title: string) => {
    const current = await loadFullSession(sessionId);
    const trimmed = title.trim();
    const next = {
      ...current,
      title: trimmed || 'New task',
      updated_at: clock.nowSeconds(),
    };

    cacheSession(next);
    await persistOrdered(next);
  };

  const grantAgentRename = async (sessionId: string) => {
    const current = await loadFullSession(sessionId);
    if (current.agent_rename_allowed) {return;}
    const next = {
      ...current,
      agent_rename_allowed: true,
      updated_at: clock.nowSeconds(),
    };

    cacheSession(next);
    await persistOrdered(next);
  };

  /** Pin/unpin — desktop's toggle_session_pin: a plain flag flip plus save,
   * limited to started, unarchived tasks. */
  const setSessionPinned = async (sessionId: string, pinned: boolean) => {
    const current = await loadFullSession(sessionId);
    if (!sessionHasStarted(current) || current.archived_at != null) {return;}
    const now = clock.nowSeconds();
    const next = {
      ...current,
      pinned_at: pinned ? now : null,
      updated_at: now,
    };

    cacheSession(next);
    await persistOrdered(next);
  };

  /** Archive/unarchive — desktop's finish_archive_session minus its
   * app-local bookkeeping. The daemon cascades the flag to side chats and
   * purges past the retention window; a busy runtime is closed first so a
   * mobile archive stops the turn the way desktop's does. */
  const setSessionArchived = async (sessionId: string, archived: boolean) => {
    const client = daemon.client;
    const current = await loadFullSession(sessionId);
    if (!sessionHasStarted(current) || current.side_chat_of != null) {return;}
    if ((current.archived_at != null) === archived) {return;}
    if (archived) {
      const runtime = entries.current.get(sessionId);
      if (runtime && client) {
        await client
          .request({ type: 'closeSession' }, sessionId, runtime.runtimeId)
          .catch(() => {});

        removeRuntime(sessionId);
      }
    }

    const now = clock.nowSeconds();
    const next = {
      ...current,
      archived_at: archived ? now : null,
      updated_at: now,
    };

    cacheSession(next);
    await persistOrdered(next);
    if (archived) {
      setPermissions((values) => removeKey(values, sessionId));
      setRenameRequests((values) => removeKey(values, sessionId));
      setUserInputs((values) => removeKey(values, sessionId));
    }
  };

  const deleteSession = async (sessionId: string) => {
    const client = daemon.client;
    const profileId = daemon.activeProfile?.id;
    if (!client || !profileId || daemon.phase !== 'connected') {
      throw new Error('Goddard daemon is disconnected');
    }

    const runtime = entries.current.get(sessionId);
    if (runtime) {
      await client.request({ type: 'closeSession' }, sessionId, runtime.runtimeId).catch(() => {});
      removeRuntime(sessionId);
    }

    const timer = persistTimers.current.get(sessionId);
    if (timer) {clearTimeout(timer);}
    persistTimers.current.delete(sessionId);
    await persistTails.current.get(sessionId)?.catch(() => {});
    await removeDaemonSession(client, sessionId);
    queryClient.removeQueries({ queryKey: daemonKeys.session(profileId, sessionId) });
    queryClient.setQueryData<TaskState>(daemonKeys.taskState(profileId), (current) => (
      current
        ? { ...current, sessions: current.sessions.filter((item) => item.id !== sessionId) }
        : current
    ));

    setPermissions((values) => removeKey(values, sessionId));
    setRenameRequests((values) => removeKey(values, sessionId));
    setUserInputs((values) => removeKey(values, sessionId));
    setErrors((values) => removeKey(values, sessionId));
  };

  const removeQueuedMessage = async (sessionId: string, messageId: string) => {
    const profileId = daemon.activeProfile?.id;
    if (!profileId) {throw new Error('Goddard daemon is disconnected');}
    const current = queryClient.getQueryData<AgentSession>(
      daemonKeys.session(profileId, sessionId),
    );

    if (!current) {return;}
    const message = (current.queued_messages ?? []).find((item) => item.id === messageId);
    // A daemon-owned chip is cancelled at the source: the daemon drops the
    // parked prompt and the `queuedMessagesChanged` echo removes the row.
    // Mutating locally would only hide a prompt that still delivers.
    if (message && isAgentQueuedMessage(message)) {
      const client = daemon.client;
      if (!client) {throw new Error('Goddard daemon is disconnected');}
      await client.request({ type: 'cancelQueuedPrompt', queuedMessageId: messageId }, sessionId);
      return;
    }

    const next = {
      ...current,
      queued_messages: (current.queued_messages ?? []).filter((item) => item.id !== messageId),
    };

    cacheSession(next);
    await persistOrdered(next);
  };

  const recordBossSpeech = async (sessionId: string, request: BossSpeechRequest) => {
    const profileId = daemon.activeProfile?.id;
    const client = daemon.client;
    const hydrated = await loadFullSession(sessionId);
    if (!profileId || client !== daemon.client || profileId !== daemon.activeProfile?.id) {
      throw new Error('The daemon changed before the spoken receipt could be saved');
    }

    const current = getDaemonQueryData<AgentSession>(daemonKeys.session(profileId, sessionId)) ?? hydrated;
    if (current.transcript_blocks.some((block) => block.content.kind === 'activities'
      && block.content.data.some((item) => item.id === request.requestId))) {
      await persistOrdered(current);
      return;
    }

    const next = { ...current, transcript_blocks: [...current.transcript_blocks, spokenBlock(current, request)] };
    cacheSession(next);
    await persistOrdered(next);
  };

  const dismissError = (sessionId: string) => {
    setErrors((values) => removeKey(values, sessionId));
  };

function hasRecoveredDisconnectError(): boolean {
  return daemon.phase === 'connected'
    && Object.values(runtimeState.get().errors).some(isDaemonDisconnectError);
}

let lastDaemonKey: string | null = null;
let lastTaskStateClient: (() => void) | null = null;

/** Replaces RuntimeProvider's effects: fires on every daemon-store write,
 * which subsumes the dep lists the effects carried. */
function onDaemonSnapshot() {
  const daemonKey = `${daemon.activeProfile?.id ?? 'none'}:${daemon.client ? 'client' : 'none'}`;
  const client = daemon.client;
  const profileId = daemon.activeProfile?.id;

  if (daemonKey !== lastDaemonKey) {
    lastDaemonKey = daemonKey;
    setRuntimes({});
    setPermissions({});
    setRenameRequests({});
    setUserInputs({});
    setErrors({});
    revalidatedConnections.current = 0;
    for (const entry of entries.current.values()) {entry.unsubscribe();}
    entries.current.clear();
    attachRequests.current.clear();
    persistTails.current.clear();
    pendingSteers.current.clear();
    drainingQueues.current.clear();
    for (const timer of persistTimers.current.values()) {clearTimeout(timer);}
    persistTimers.current.clear();
  }

  if (hasRecoveredDisconnectError()) {
    setErrors(clearDaemonDisconnectErrors);
  }

  // After a reconnect the daemon replays whatever each followed runtime
  // emitted while the link was down, so streams resume by themselves. What
  // it cannot replay is a runtime that no longer exists (the daemon
  // restarted) or one another client replaced: confirm each entry's runtime
  // and either drop it, so the next prompt starts fresh, or follow the new
  // one, whose events are already buffered on the client.
  if (client && profileId && daemon.phase === 'connected'
      && daemon.connections > 1 && revalidatedConnections.current !== daemon.connections) {
    revalidatedConnections.current = daemon.connections;
    for (const [sessionId, entry] of entries.current) {
      void attachDaemonSession(client, sessionId).then((attached) => {
        if (entries.current.get(sessionId) !== entry) {return;}
        if (attached?.runtimeId === entry.runtimeId) {return;}
        removeRuntime(sessionId);
        if (attached) {
          const current = queryClient.getQueryData<AgentSession>(
            daemonKeys.session(profileId, sessionId),
          );

          if (current) {subscribe(current, attached.runtimeId, attached.supportsSteer, attached.supportsUserInputActions, false);}
        }

        void queryClient.invalidateQueries({
          queryKey: daemonKeys.session(profileId, sessionId),
        });
      }).catch(() => {});
    }
  }

  // Another client (the desktop, the web app) persisting a session announces
  // itself through taskStateChanged. Refetch any hydrated session we are not
  // already following live, so watching a desktop-driven task stays current.
  if (client && profileId && !lastTaskStateClient) {
    lastTaskStateClient = client.subscribeTaskState(() => {
      void queryClient.invalidateQueries({
        queryKey: ['daemon', profileId, 'session'],
        predicate: (query) => {
          const sessionId = query.queryKey[3];
          return typeof sessionId === 'string' && !entries.current.has(sessionId);
        },
      });
    });
  } else if (!client && lastTaskStateClient) {
    lastTaskStateClient();
    lastTaskStateClient = null;
  }
}

daemonState.subscribe(() => {
  daemon = daemonState.get();
  onDaemonSnapshot();
});

function publicRuntime(entry: RuntimeEntry): MobileRuntime {
  return {
    runtimeId: entry.runtimeId,
    supportsSteer: entry.supportsSteer,
    supportsUserInputActions: entry.supportsUserInputActions,
    starting: entry.starting,
    running: entry.running,
  };
}

function removeKey<T>(record: Record<string, T | undefined>, key: string): Record<string, T | undefined> {
  const next = { ...record };
  delete next[key];
  return next;
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error && cause.message.trim() ? cause.message : String(cause);
}

/** The daemon captures ending checkpoints itself, so a reply's checkpoint for
 * a turn is the one part of it that can be newer than the local cache. */
function withDaemonCheckpoints(latest: AgentSession, saved: AgentSession): AgentSession {
  let changed = false;
  const turns = latest.turns.map((turn) => {
    const stored = saved.turns.find((candidate) => candidate.turn_count === turn.turn_count);
    if (
      !stored?.checkpoint
      || JSON.stringify(stored.checkpoint) === JSON.stringify(turn.checkpoint)
    ) {
      return turn;
    }

    changed = true;
    return { ...turn, checkpoint: stored.checkpoint };
  });

  return changed ? { ...latest, turns } : latest;
}

/** Action surface — the RuntimeContextValue without the provider. */
export const runtime = {
  attachSession,
  recordBossSpeech,
  sendPrompt,
  steerPrompt,
  createTask,
  cancel,
  compactSession,
  rollbackSession,
  rewindSessionToMessage,
  forkSessionFromResponse,
  respond,
  respondUserInput,
  clarifyUserInput,
  cancelUserInput,
  updateSessionOptions,
  grantAgentRename,
  sendGoalOperation,
  renameSession,
  setSessionPinned,
  setSessionArchived,
  deleteSession,
  removeQueuedMessage,
  dismissError,
};
