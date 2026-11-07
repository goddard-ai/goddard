import { describe, expect, test } from 'bun:test';
import { LandingCanceledError, LandingStore } from './landing';
import type { WakuClient } from './client';
import type { LandingJob, LandingQueueSnapshot, WorkspaceOperation } from './generated';

function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
class Daemon {
  connected = true;
  landingQueueAvailable = true;
  sequence = 0;
  starts = 0;
  reuseFirst = false;
  startReply?: ReturnType<typeof deferred<void>>;
  snapshot: LandingQueueSnapshot = { epoch: 'epoch', queueId: 'queue', revision: 0, jobs: [] };
  changed?: (queue: string, revision: number) => void;
  connection?: (phase: string) => void;
  cancelReply?: ReturnType<typeof deferred<string>>;
  createOperationId() { return `job-${++this.sequence}`; }
  subscribeLandingQueue(callback: (queue: string, revision: number) => void) { this.changed = callback; return () => {}; }
  subscribeConnectionState(callback: (phase: string) => void) { this.connection = callback; return () => {}; }
  async request({ operation }: { operation: WorkspaceOperation }) {
    let result;
    switch (operation.type) {
      case 'startLanding': {
        this.starts++;
        if (this.reuseFirst && this.snapshot.jobs.length) {
          result = { type: 'landingJob', job: this.snapshot.jobs[0] };
          break;
        }
        const job: LandingJob = { id: operation.id, sequence: this.sequence, queueId: 'queue', taskId: 'task', cwd: '/repo', kind: 'land', base: 'main', strategy: 'rebase', state: 'queued', position: 2, result: null };
        this.snapshot.jobs = [...this.snapshot.jobs, job];
        if (this.startReply) await this.startReply.promise;
        result = { type: 'landingJob', job };
        break;
      }
      case 'landingQueue': case 'landingSnapshot': result = { type: 'landingQueue', snapshot: { ...structuredClone(this.snapshot), jobs: this.snapshot.jobs.map(job => ({ ...job, result: null })) } }; break;
      case 'landingJob': result = { type: 'landingJob', job: this.snapshot.jobs.find(job => job.id === operation.id) ?? null }; break;
      case 'cancelQueuedLanding': {
        const outcome = await this.cancelReply!.promise;
        result = { type: 'landingCanceled', outcome };
        break;
      }
      default: throw new Error(`unexpected ${operation.type}`);
    }
    return { type: 'workspace', result };
  }
  update(state: LandingJob['state'], result: LandingJob['result'] = null) {
    const job = this.snapshot.jobs.at(-1)!;
    this.snapshot = { ...this.snapshot, revision: this.snapshot.revision + 1, jobs: [{ ...job, state, position: state === 'queued' ? 1 : null, result }] };
    this.changed?.('queue', this.snapshot.revision);
  }
}
async function flush() { for (let i = 0; i < 12; i++) await Promise.resolve(); }
function setup() { const daemon = new Daemon(); const store = new LandingStore(daemon as unknown as WakuClient); return { daemon, store }; }

describe('landing projection', () => {
  test('deduplicates starts, survives navigation, and cancellation loses truthfully to execution', async () => {
    const { daemon, store } = setup();
    const unsubscribe = store.subscribe('task', '/repo', () => {});
    const landing = store.land('task', '/repo', 'main');
    expect(store.land('task', '/repo', 'main')).toBe(landing);
    await flush();
    expect(store.get('task', '/repo').job?.position).toBe(2);
    expect(daemon.starts).toBe(1);
    unsubscribe();
    store.subscribe('other-task', '/other', () => {});
    expect(store.get('task', '/repo').job?.state).toBe('queued');
    daemon.cancelReply = deferred<string>();
    const cancel = store.cancel('task', '/repo');
    expect(store.get('task', '/repo').phase).toBe('canceling');
    daemon.update('running');
    daemon.cancelReply.resolve('alreadyRunning');
    await cancel;
    expect(store.get('task', '/repo').job?.state).toBe('running');
    daemon.update('finished', { type: 'land', outcome: { alreadyLanded: { base: 'main' } } });
    await expect(landing).resolves.toEqual({ alreadyLanded: { base: 'main' } });
    expect(daemon.starts).toBe(1);
  });

  test('acknowledged cancellation is neutral, and disconnection never resubmits', async () => {
    const { daemon, store } = setup();
    const landing = store.land('task', '/repo', 'main').catch(error => error);
    await flush();
    daemon.cancelReply = deferred<string>();
    const cancel = store.cancel('task', '/repo');
    expect(store.get('task', '/repo').job?.state).toBe('queued');
    daemon.update('canceled');
    daemon.cancelReply.resolve('canceled');
    await cancel;
    expect(await landing).toBeInstanceOf(LandingCanceledError);
    const next = store.land('task', '/repo', 'main').catch(error => error);
    await flush();
    daemon.connected = false;
    daemon.connection?.('disconnected');
    expect(store.get('task', '/repo').phase).toBe('unavailable');
    expect(await next).toBeInstanceOf(Error);
    daemon.connected = true;
    daemon.connection?.('connected');
    await flush();
    expect(daemon.starts).toBe(2);
    expect(store.get('task', '/repo').job?.state).toBe('queued');
  });
  test('a late admission response cannot regress an already finished job', async () => {
    const { daemon, store } = setup();
    daemon.startReply = deferred<void>();
    const landing = store.land('task', '/repo', 'main');
    daemon.update('finished', { type: 'land', outcome: { alreadyLanded: { base: 'main' } } });
    await expect(landing).resolves.toEqual({ alreadyLanded: { base: 'main' } });
    daemon.startReply.resolve();
    await flush();
    expect(store.get('task', '/repo').job?.state).toBe('finished');
    daemon.connected = false;
    daemon.connection?.('disconnected');
    daemon.snapshot = { ...daemon.snapshot, epoch: 'new-daemon', jobs: [] };
    daemon.connected = true;
    daemon.connection?.('connected');
    await flush();
    expect(store.get('task', '/repo').phase).toBe('unavailable');
    expect(daemon.starts).toBe(1);
  });

  test('one result lookup settles every task attached to the same job', async () => {
    const { daemon, store } = setup();
    daemon.reuseFirst = true;
    const first = store.land('task', '/repo', 'main');
    const second = store.land('other-task', '/repo', 'main');
    await flush();
    expect(store.get('task', '/repo').job?.id).toBe(store.get('other-task', '/repo').job?.id);
    daemon.update('finished', { type: 'land', outcome: { alreadyLanded: { base: 'main' } } });
    const results = await Promise.all([first, second]);
    expect(results).toHaveLength(2);
    expect(results[0]).toEqual(results[1]);
  });

});
