import type { BossBrowseRequest, BossSpeechRequest, BossTerminalIntent, WakuClient } from '@waku/client';
import { createStore } from '@octane-xplat/ui/data';
import { storage } from '@octane-xplat/platform';
import { browseDaemonDirectory } from './daemon-api';
import { subscribeAppForeground } from './app-foreground';
import { LiveRequestWindow, safeBrowseRequest } from './boss-secondary';
import { randomUUID } from './uuid';

export interface PinnedBossTerminal extends BossTerminalIntent {
  id: string;
  intentIdentity: string;
  sessionId: string;
  commandSent: boolean;
  notice: string | null;
  error: string | null;
  exited: boolean;
}

interface SecondaryState {
  profileId: string | null;
  browse: BossBrowseRequest[];
  speech: BossSpeechRequest[];
  terminals: PinnedBossTerminal[];
  foreground: boolean;
  readySessionId: string | null;
  notice: string | null;
}

export const bossSecondary = createStore<SecondaryState>({
  profileId: null, browse: [], speech: [], terminals: [], foreground: true, readySessionId: null, notice: null,
});

const requests = new LiveRequestWindow();
let client: WakuClient | null = null;
let generation = 0;
let subscriptions: (() => void)[] = [];
const terminalListeners = new Map<string, Set<(data: string) => void>>();
const tails = new Map<string, string>();
const opening = new Set<string>();
const terminalSubscriptions = new Map<string, () => void>();
let terminalIntents: string[] = [];
const storageKey = (profileId: string) => `waku.mobile.boss-terminals.v1.${profileId}`;

function saveTerminals() {
  const state = bossSecondary.getSnapshot();
  if (state.profileId) {storage.setString(storageKey(state.profileId), JSON.stringify(state.terminals));}
}

function updateTerminal(id: string, update: Partial<PinnedBossTerminal>) {
  bossSecondary.update((state) => ({ ...state, terminals: state.terminals.map((item) => item.id === id ? { ...item, ...update } : item) }));
  saveTerminals();
}

/** Attached once per paired link, outside route lifetime. Broadcasts still
 * arrive while the drawer, another task, or a review sheet is on screen. */
export function bindBossSecondary(next: WakuClient, profileId: string): () => void {
  generation++;
  subscriptions.forEach((unsubscribe) => unsubscribe());
  subscriptions = [];
  terminalSubscriptions.forEach((unsubscribe) => unsubscribe());
  terminalSubscriptions.clear();
  client = next;
  opening.clear();
  if (bossSecondary.getSnapshot().profileId !== profileId) {
    let terminals: PinnedBossTerminal[] = [];
    try {
      const parsed: unknown = JSON.parse(storage.getString(storageKey(profileId)) ?? '[]');
      if (Array.isArray(parsed)) {
        terminals = parsed.filter((item): item is PinnedBossTerminal => item && typeof item.id === 'string'
          && typeof item.sessionId === 'string' && typeof item.title === 'string' && typeof item.cwd === 'string'
          && typeof item.commandSent === 'boolean' && typeof item.exited === 'boolean'
          && (item.command == null || typeof item.command === 'string'));
      }
    } catch { /* An unreadable local pin library starts empty. */ }

    try {
      const parsed: unknown = JSON.parse(storage.getString(`${storageKey(profileId)}.seen`) ?? '[]');
      terminalIntents = Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === 'string').slice(-512) : [];
    } catch {terminalIntents = [];}

    tails.clear();
    terminalListeners.clear();
    bossSecondary.update((state) => ({ ...state, profileId, browse: [], speech: [], terminals, readySessionId: null, notice: null }));
  }

  subscriptions.push(next.subscribeBossBrowse((request) => {
    if (!safeBrowseRequest(request) || !requests.accept(profileId, `browse:${request.requestId}`)) {return;}
    bossSecondary.update((state) => ({ ...state, browse: [...state.browse.slice(-31), request] }));
  }));

  subscriptions.push(next.subscribeBossSpeech((request) => {
    if (!requests.accept(profileId, `speech:${request.requestId}`)) {return;}
    bossSecondary.update((state) => ({ ...state, speech: [...state.speech, request] }));
  }));

  subscriptions.push(next.subscribeConnectionState((phase) => {
    if (phase === 'disconnected') {
      generation++;
      opening.clear();
      terminalSubscriptions.forEach((unsubscribe) => unsubscribe());
      terminalSubscriptions.clear();
    }

    if (phase === 'connected') {
      for (const terminal of bossSecondary.getSnapshot().terminals) {
        if (!terminal.exited) {void openPinnedTerminal(terminal.id);}
      }
    }
  }));

  return () => {
    generation++;
    subscriptions.forEach((unsubscribe) => unsubscribe());
    subscriptions = [];
    terminalSubscriptions.forEach((unsubscribe) => unsubscribe());
    terminalSubscriptions.clear();
    client = null;
    bossSecondary.update((state) => ({ ...state, readySessionId: null, notice: null }));
  };
}

export function dismissSecondaryNotice() {
  bossSecondary.update((state) => ({ ...state, notice: null }));
}

export function setBossChatReady(sessionId: string | null) {
  if (bossSecondary.getSnapshot().readySessionId === sessionId) {return;}
  bossSecondary.update((state) => ({ ...state, readySessionId: sessionId }));
}

export function consumeBrowse(requestId: string) {
  bossSecondary.update((state) => ({ ...state, browse: state.browse.filter((item) => item.requestId !== requestId) }));
}

export function consumeSpeech(requestId: string, profileId: string) {
  if (bossSecondary.getSnapshot().profileId !== profileId) {return;}
  bossSecondary.update((state) => ({ ...state, speech: state.speech.filter((item) => item.requestId !== requestId) }));
}

export function pinBossTerminal(intent: BossTerminalIntent, sessionId: string, identity: string, fallback: string | null) {
  const state = bossSecondary.getSnapshot();
  if (!state.profileId || terminalIntents.includes(identity) || state.terminals.some((item) => item.intentIdentity === identity)
    || !requests.accept(state.profileId, `terminal:${identity}`)) {return;}

  terminalIntents = [...terminalIntents.slice(-511), identity];
  storage.setString(`${storageKey(state.profileId)}.seen`, JSON.stringify(terminalIntents));
  const record: PinnedBossTerminal = {
    ...intent, id: randomUUID(), intentIdentity: identity, sessionId, commandSent: false, notice: null, error: null, exited: false,
  };

  bossSecondary.update((current) => ({ ...current, terminals: [...current.terminals, record] }));
  saveTerminals();
  void openPinnedTerminal(record.id, fallback);
}

export async function openPinnedTerminal(id: string, fallback: string | null = null) {
  const current = bossSecondary.getSnapshot().terminals.find((item) => item.id === id);
  const activeClient = client;
  const attempt = generation;
  if (!current || !activeClient || opening.has(id)) {return;}
  opening.add(id);
  const live = () => attempt === generation && client === activeClient
    && bossSecondary.getSnapshot().terminals.some((item) => item.id === id);

  try {
    let cwd = current.cwd.trim();
    try {await browseDaemonDirectory(activeClient, cwd || '__invalid_boss_cwd__');}
    catch {
      const directory = await browseDaemonDirectory(activeClient, fallback).catch(() => browseDaemonDirectory(activeClient, null));
      cwd = directory.path;
      if (!live()) {return;}
      const notice = `Directory ${current.cwd || '(empty)'} unavailable. Using ${cwd}.`;
      updateTerminal(id, { cwd, notice });
      bossSecondary.update((state) => ({ ...state, notice }));
    }

    if (!live()) {return;}
    // Output is bounded and retained while the sheet is dismissed. The PTY
    // belongs to the pin, so dismissal never closes it.
    terminalSubscriptions.get(id)?.();
    const unsubscribe = activeClient.subscribe(id, id, (event) => {
      if (event.event.kind === 'terminalOutput') {
        const payload = event.event.payload as { data?: unknown };
        if (typeof payload?.data !== 'string') {return;}
        tails.set(id, ((tails.get(id) ?? '') + payload.data).slice(-100_000));
        terminalListeners.get(id)?.forEach((listener) => listener(payload.data as string));
      } else if (event.event.kind === 'terminalExited') {updateTerminal(id, { exited: true });}
      else if (event.event.kind === 'terminalError') {updateTerminal(id, { error: String(event.event.payload) });}
    });

    terminalSubscriptions.set(id, unsubscribe);
    // OpenTerminal replaces an existing PTY. An empty write instead claims
    // its channel and replays the daemon tail without restarting the shell.
    let attached = false;
    if (current.commandSent) {
      tails.delete(id);
      try {
        await activeClient.request({ type: 'writeTerminal', data: '' }, id, id);
        attached = true;
      } catch {
        if (!live() || !activeClient.connected) {return;}
        // A restarted daemon has lost its PTY; restore a plain shell.
        updateTerminal(id, { notice: 'The previous shell is unavailable. Restored a shell without rerunning its command.' });
      }
    }

    if (!live()) {unsubscribe(); return;}
    if (!attached) {await activeClient.request({ type: 'openTerminal', cwd, cols: 80, rows: 24 }, id, id);}
    if (!live()) {unsubscribe(); return;}
    updateTerminal(id, { cwd, error: null });
    if (!current.commandSent) {
      // Mark before dispatch: reconnect must never re-run a launch command.
      updateTerminal(id, { commandSent: true });
      if (current.command) {await activeClient.request({ type: 'writeTerminal', data: `${current.command}\n` }, id, id);}
    }
  } catch (cause) {
    if (live()) {updateTerminal(id, { error: cause instanceof Error ? cause.message : String(cause) });}
    if (live()) {opening.delete(id);}
  }
}

export function listenPinnedTerminal(id: string, listener: (data: string) => void): () => void {
  const listeners = terminalListeners.get(id) ?? new Set();
  listeners.add(listener);
  terminalListeners.set(id, listeners);
  listener(tails.get(id) ?? '');
  return () => {listeners.delete(listener);};
}

export function writePinnedTerminal(id: string, data: string) {
  void client?.notify({ type: 'writeTerminal', data }, id, id).catch((cause) => updateTerminal(id, { error: String(cause) }));
}

export function resizePinnedTerminal(id: string, cols: number, rows: number) {
  const dimension = (value: number) => Math.max(1, Math.min(65535, Math.round(value) || 1));
  void client?.notify({ type: 'resizeTerminal', cols: dimension(cols), rows: dimension(rows) }, id, id).catch(() => {});
}

export async function closePinnedTerminal(id: string) {
  const activeClient = client;
  if (!activeClient) {throw new Error('Disconnected. Reconnect before closing this terminal.');}
  try {await activeClient.request({ type: 'closeTerminal' }, id, id);}
  catch (cause) {
    updateTerminal(id, { error: cause instanceof Error ? cause.message : String(cause) });
    throw cause;
  }

  if (client !== activeClient) {return;}
  bossSecondary.update((state) => ({ ...state, terminals: state.terminals.filter((item) => item.id !== id) }));
  tails.delete(id);
  terminalSubscriptions.get(id)?.();
  terminalSubscriptions.delete(id);
  opening.delete(id);
  saveTerminals();
}

subscribeAppForeground((foreground) => bossSecondary.update((state) => ({ ...state, foreground })));
