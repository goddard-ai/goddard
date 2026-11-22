import { WakuClient } from './client';
import type { LandingJob, LandingQueueSnapshot, LandOutcome, WorkspaceOperation, WorkspaceResult } from './generated';

export class LandingCanceledError extends Error {
  constructor() { super('Landing canceled'); }
}
export type LandingView = {
  job: LandingJob | null;
  phase: 'known' | 'submitting' | 'canceling' | 'unavailable';
  error: string | null;
};
const EMPTY: LandingView = { job: null, phase: 'known', error: null };
type Context = {
  task: string;
  cwd: string;
  view: LandingView;
  listeners: Set<() => void>;
  id?: string;
  pending?: Promise<LandOutcome>;
  resolve?: (outcome: LandOutcome) => void;
  reject?: (error: Error) => void;
};
const stores = new WeakMap<WakuClient, LandingStore>();
export function landingStore(client: WakuClient): LandingStore {
  let store = stores.get(client);
  if (!store) { store = new LandingStore(client); stores.set(client, store); }
  return store;
}

/** One connection-level projection, shared by task surfaces across navigation. */
export class LandingStore {
  private contexts = new Map<string, Context>();
  private snapshots = new Map<string, LandingQueueSnapshot>();
  private refreshing = new Map<string, Promise<void>>();
  private dirty = new Set<string>();
  private fetchingResults = new Set<string>();
  constructor(private client: WakuClient) {
    client.subscribeLandingQueue((queue, revision) => {
      if ((this.snapshots.get(queue)?.revision ?? -1) < revision) void this.refresh(queue);
    });
    client.subscribeConnectionState((state) => {
      if (state === 'connected') {
        this.snapshots.clear();
        for (const context of this.contexts.values()) void this.discover(context);
      } else if (state === 'disconnected') {
        for (const context of this.contexts.values()) {
          if (context.pending || context.view.job?.state === 'queued' || context.view.job?.state === 'running') {
            this.unavailable(context, new Error('Landing status unavailable; reconnect to check its result'));
          }
        }
      }
    });
  }
  private context(task: string, cwd: string): Context {
    const key = JSON.stringify([task, cwd]);
    let context = this.contexts.get(key);
    if (!context) {
      context = { task, cwd, view: EMPTY, listeners: new Set() };
      this.contexts.set(key, context);
    }
    return context;
  }
  get(task: string, cwd: string): LandingView { return this.context(task, cwd).view; }
  subscribe(task: string, cwd: string, listener: () => void): () => void {
    const context = this.context(task, cwd);
    context.listeners.add(listener);
    void this.discover(context);
    return () => { context.listeners.delete(listener); };
  }
  private publish(context: Context, view: LandingView) {
    context.view = view;
    for (const listener of context.listeners) listener();
  }
  private async request(operation: WorkspaceOperation, task?: string): Promise<WorkspaceResult> {
    const response = await this.client.request({ type: 'workspace', operation }, task);
    if (response.type !== 'workspace') throw new Error('Unexpected landing response');
    return response.result;
  }
  private accept(context: Context, job: LandingJob) {
    if (!job.result && context.view.job?.id === job.id && context.view.job.result) job = { ...job, result: context.view.job.result };
    context.id = job.id;
    this.publish(context, { job, phase: 'known', error: null });
    if (job.state === 'canceled') this.settle(context, new LandingCanceledError());
    if (job.state === 'finished') {
      if (!job.result) {
        if (!this.fetchingResults.has(job.id)) {
          this.fetchingResults.add(job.id);
          void this.request({ type: 'landingJob', id: job.id }).then(result => {
            if (result.type !== 'landingJob' || !result.job?.result) throw new Error('Landing result expired; inspect the worktree before retrying');
            for (const attached of this.contexts.values()) {
              if (attached.id === job.id) this.accept(attached, result.job);
            }
          }).catch((cause: unknown) => {
            for (const attached of this.contexts.values()) {
              if (attached.id === job.id) this.unavailable(attached, asError(cause));
            }
          }).finally(() => this.fetchingResults.delete(job.id));
        }
        return;
      }
      if (job.result.type === 'land') this.settle(context, job.result.outcome);
      else if (job.result?.type === 'failed') this.settle(context, new Error(job.result.message));
      else if (context.pending) this.settle(context, new Error('Unexpected landing result'));
    }
  }
  private settle(context: Context, result: LandOutcome | Error) {
    const resolve = context.resolve, reject = context.reject;
    context.pending = undefined; context.resolve = undefined; context.reject = undefined;
    if (result instanceof Error) reject?.(result); else resolve?.(result);
  }
  private unavailable(context: Context, error: Error) {
    this.publish(context, { ...context.view, phase: 'unavailable', error: error.message });
    this.settle(context, error);
  }
  private apply(snapshot: LandingQueueSnapshot) {
    const previous = this.snapshots.get(snapshot.queueId);
    if (previous?.epoch === snapshot.epoch && previous.revision > snapshot.revision) return;
    this.snapshots.set(snapshot.queueId, snapshot);
    for (const context of this.contexts.values()) {
      const own = context.id ? snapshot.jobs.find(job => job.id === context.id) : undefined;
      if (context.pending && context.id && !own) continue;
      const latest = snapshot.jobs
        .filter(job => job.taskId === context.task || job.cwd === context.cwd)
        .sort((a, b) => b.sequence - a.sequence)[0];
      const job = context.pending || own?.state === 'queued' || own?.state === 'running' ? own : latest ?? own;
      if (job) this.accept(context, job);
    }
  }
  private async refresh(queue: string): Promise<void> {
    if (this.refreshing.has(queue)) { this.dirty.add(queue); return this.refreshing.get(queue); }
    const request = (async () => {
      do {
        this.dirty.delete(queue);
        const result = await this.request({ type: 'landingSnapshot', queue_id: queue });
        if (result.type === 'landingQueue') this.apply(result.snapshot);
      } while (this.dirty.has(queue));
    })().catch((cause: unknown) => {
      for (const context of this.contexts.values()) {
        if (context.view.job?.queueId === queue) this.unavailable(context, asError(cause));
      }
    }).finally(() => { this.refreshing.delete(queue); if (this.dirty.has(queue)) void this.refresh(queue); });
    this.refreshing.set(queue, request);
    return request;
  }
  private async discover(context: Context) {
    if (!this.client.landingQueueAvailable || !this.client.connected) return;
    try {
      if (context.id) {
        const result = await this.request({ type: 'landingJob', id: context.id });
        if (result.type !== 'landingJob' || !result.job) {
          this.unavailable(context, new Error('Landing result expired or daemon restarted; inspect the worktree before retrying'));
          return;
        }
        await this.refresh(result.job.queueId);
      } else {
        const result = await this.request({ type: 'landingQueue', cwd: context.cwd });
        if (result.type === 'landingQueue') this.apply(result.snapshot);
      }
    } catch (cause) { this.unavailable(context, asError(cause)); }
  }
  land(task: string, cwd: string, base: string | null): Promise<LandOutcome> {
    const context = this.context(task, cwd);
    if (context.pending) return context.pending;
    const existing = context.view.job;
    const attach = existing?.state === 'queued' || existing?.state === 'running';
    context.pending = new Promise((resolve, reject) => { context.resolve = resolve; context.reject = reject; });
    const pending = context.pending;
    if (attach) { void this.discover(context); return pending; }
    const id = this.client.createOperationId();
    context.id = id;
    this.publish(context, { job: null, phase: 'submitting', error: null });
    void this.request({ type: 'startLanding', id, cwd, base, strategy: 'rebase' }, task).then(async result => {
      if (result.type !== 'landingJob' || !result.job) throw new Error('The daemon did not admit the landing');
      if (!context.view.job) this.accept(context, result.job);
      await this.refresh(result.job.queueId);
    }).catch((cause: unknown) => this.unavailable(context, asError(cause)));
    return pending;
  }
  async cancel(task: string, cwd: string): Promise<void> {
    const context = this.context(task, cwd);
    const job = context.view.job;
    if (!job || job.state !== 'queued' || context.view.phase !== 'known') return;
    this.publish(context, { ...context.view, phase: 'canceling', error: null });
    try {
      const result = await this.request({ type: 'cancelQueuedLanding', id: job.id }, task);
      if (result.type !== 'landingCanceled' || result.outcome === 'notFound') throw new Error('Landing status unavailable; refresh before retrying');
      await this.refresh(job.queueId);
    } catch (cause) { this.unavailable(context, asError(cause)); }
  }
  retryStatus(task: string, cwd: string): void { void this.discover(this.context(task, cwd)); }
}
function asError(cause: unknown): Error { return cause instanceof Error ? cause : new Error(String(cause)); }

/** Shared status vocabulary; platforms supply their existing translator. */
export function landingStatusText(view: LandingView, t: (key: string, params?: Record<string, string | number>) => string): string | null {
  if (view.phase === 'unavailable') return view.error ?? t('git_panel.land_status_unavailable');
  if (view.phase === 'submitting') return t('git_panel.land_requesting');
  if (view.phase === 'canceling') return t('git_panel.land_canceling');
  const job = view.job;
  if (!job) return null;
  if (job.state === 'queued') return job.position === 1
    ? t('git_panel.land_queued_next') : t('git_panel.land_queued_position', { position: job.position ?? 1 });
  if (job.state === 'running') return job.base
    ? t('git_panel.landing_onto', { base: job.base }) : t('git_panel.landing');
  if (job.state === 'canceled') return t('git_panel.land_canceled');
  if (job.result?.type === 'failed') return job.result.message;
  if (job.result?.type === 'land' && 'conflict' in job.result.outcome) return t('git_panel.land_conflict_description', { base: job.result.outcome.conflict.base });
  return null;
}
