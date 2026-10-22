import { useEffect, useRef, useState } from 'octane';
import { Absolute, Pressable, ScrollableArea, BottomSheet, Text, View, useSafeAreaInsets } from '@octane-xplat/ui';
import { pushRoute, useRoute } from '@octane-xplat/ui/routing';
import { useStore } from '@octane-xplat/ui/data';
import { useDaemon } from '@/lib/daemon-hooks';
import { useBossState, useSession } from '@/lib/daemon-queries';
import { useTheme } from '@/lib/use-theme';
import { bossSurfaceSessionId, browseReady } from '@/lib/boss-secondary';
import {
  bossSecondary, consumeBrowse, consumeSpeech, listenPinnedTerminal,
  closePinnedTerminal, writePinnedTerminal, resizePinnedTerminal, dismissSecondaryNotice,
  type PinnedBossTerminal,
} from '@/lib/boss-secondary-store';

import { runtime, runtimeState } from '@/lib/runtime-store';
import { openUrl } from '@/lib/open-url';
import { TerminalView } from './terminal-view';
import type { TerminalViewHandle } from '@/lib/terminal-page';

const recording = new Set<string>();
const activating = new Set<string>();
/** Route-independent listener; browser handoff waits for the owning Boss
 * to be visible. Opening the chat is the mobile activation equivalent. */
export function BossSecondaryHost() {
  const daemon = useDaemon();
  const boss = useBossState().data;
  const bossSession = useSession(boss?.sessionId ?? undefined).data;
  const attached = useStore(runtimeState).runtimes[boss?.sessionId ?? ''];
  const state = useStore(bossSecondary);
  const route = useRoute('root');
  const visibleSessionId = bossSurfaceSessionId(route?.name, route?.name === 'session/:id' ? String(route.params?.id ?? '') : undefined, boss?.sessionId ?? null);
  const theme = useTheme();
  const insets = useSafeAreaInsets();
  const [error, setError] = useState<string | null>(null);
  const profileId = daemon.activeProfile?.id;
  const current = profileId === state.profileId;
  useEffect(() => setError(null), [profileId]);
  const request = current ? state.browse.find((item) => item.sessionId === boss?.sessionId) : null;
  useEffect(() => {
    if (bossSession && daemon.phase === 'connected') {void runtime.attachSession(bossSession).catch(() => {});}
  }, [bossSession, daemon.phase, attached?.runtimeId]);

  useEffect(() => {
    if (!current || !boss?.sessionId || daemon.phase !== 'connected') {return;}
    for (const speech of state.speech) {
      const key = `${profileId}:${speech.requestId}`;
      if (recording.has(key)) {continue;}
      recording.add(key);
      void runtime.recordBossSpeech(boss.sessionId, speech)
        .then(() => consumeSpeech(speech.requestId, profileId!))
        .catch((cause) => {if (bossSecondary.getSnapshot().profileId === profileId) {setError(`Couldn’t save spoken receipt: ${String(cause)}`);}})
        .finally(() => recording.delete(key));
    }
  }, [current, boss?.sessionId, daemon.phase, profileId, state.speech]);

  useEffect(() => {
    if (!request || !state.foreground || daemon.phase !== 'connected') {return;}
    if (visibleSessionId !== request.sessionId) {
      const key = `${profileId}:${request.requestId}`;
      if (!activating.has(key)) {
        activating.add(key);
        pushRoute({ stack: 'root', name: 'index', params: {} });
      }

      return;
    }

    if (state.readySessionId !== visibleSessionId || !browseReady(request, visibleSessionId, boss?.sessionId ?? null, state.foreground)) {return;}
    // Claim before opening: native retained pages can share the route store.
    if (!bossSecondary.getSnapshot().browse.some((item) => item.requestId === request.requestId)) {return;}
    consumeBrowse(request.requestId);
    openUrl(request.url);
  }, [request, visibleSessionId, boss?.sessionId, state.foreground, state.readySessionId, daemon.phase, profileId]);

  const notice = current ? state.notice : null;
  if ((!request || visibleSessionId === request.sessionId) && !error && !notice) {return null;}
  return <Absolute style={{ left: 12, right: 12, bottom: insets.bottom + 12 }}><View className="boss-secondary-banner" style={{ backgroundColor: theme.surface }}>
    {notice ? <Pressable accessibilityRole="button" onPress={dismissSecondaryNotice}><Text style={{ color: theme.text }}>{notice} · Dismiss</Text></Pressable> : null}
    {error ? <Text style={{ color: theme.danger }}>{error}</Text> : null}
    {request ? <Pressable accessibilityRole="button" onPress={() => pushRoute({ stack: 'root', name: 'index', params: {} })}>
      <Text style={{ color: theme.text }}>Boss wants to open {request.title || request.url}. Open Boss chat</Text>
    </Pressable> : null}
  </View></Absolute>;
}

export function BossPinnedTerminals({ sessionId }: { sessionId: string }) {
  const state = useStore(bossSecondary);
  const daemon = useDaemon();
  const theme = useTheme();
  const [selected, setSelected] = useState<string | null>(null);
  const terminals = state.profileId === daemon.activeProfile?.id ? state.terminals.filter((item) => item.sessionId === sessionId) : [];
  const terminal = terminals.find((item) => item.id === selected);
  if (!terminals.length) {return null;}
  return <View className="boss-secondary-pins" style={{ backgroundColor: theme.surface }}>
    <ScrollableArea axis="inline"><View className="boss-secondary-pin-row">{terminals.map((item) => <Pressable accessibilityRole="button" onPress={() => setSelected(item.id)}>
      <Text numberOfLines={1} className="boss-secondary-pin" style={{ color: theme.text }}>Pinned terminal · {item.title}{item.exited ? ' · Exited' : ''}</Text>
    </Pressable>)}</View></ScrollableArea>
    <BottomSheet label="Terminal" isOpen={Boolean(terminal)} snapPoints={[0.75, 1]} onOpenChange={(isOpen) => {if (!isOpen) {setSelected(null);}}}>
      {terminal ? <PinnedTerminalSheet terminal={terminal} onDismiss={() => setSelected(null)} /> : null}
    </BottomSheet>
  </View>;
}

function PinnedTerminalSheet({ terminal, onDismiss }: { terminal: PinnedBossTerminal; onDismiss: () => void }) {
  const daemon = useDaemon();
  const theme = useTheme();
  const handle = useRef<TerminalViewHandle | null>(null);
  const unsubscribe = useRef<(() => void) | null>(null);
  useEffect(() => () => unsubscribe.current?.(), [terminal.id]);
  const unavailable = daemon.phase !== 'connected' || terminal.exited;
  return <View className="boss-secondary-sheet" style={{ backgroundColor: theme.surface }}>
    <Text style={{ color: theme.text }}>{terminal.title}</Text>
    <Text style={{ color: theme.textSecondary }}>{terminal.cwd}</Text>
    {terminal.notice ? <Text style={{ color: theme.textSecondary }}>{terminal.notice}</Text> : null}
    <View className="boss-secondary-actions">
      <Pressable accessibilityRole="button" onPress={onDismiss}><Text style={{ color: theme.text }}>Done</Text></Pressable>
      <Pressable accessibilityRole="button" disabled={daemon.phase !== 'connected'} onPress={() => {void closePinnedTerminal(terminal.id).then(onDismiss).catch(() => {});}}><Text style={{ color: theme.danger }}>Close terminal</Text></Pressable>
    </View>
    {terminal.error || unavailable ? <Text style={{ color: theme.textSecondary }}>{terminal.error || (terminal.exited ? 'Shell exited' : 'Disconnected. Terminal input is unavailable.')}</Text> : null}
    <TerminalView bridgeId={terminal.id} bind={(value) => {handle.current = value;}}
      onReady={() => {unsubscribe.current?.(); unsubscribe.current = listenPinnedTerminal(terminal.id, (data) => handle.current?.write(data));}}
      onInput={(data) => {if (!unavailable) {writePinnedTerminal(terminal.id, data);}}}
      onResize={(cols, rows) => {if (!unavailable) {resizePinnedTerminal(terminal.id, cols, rows);}}}
      theme={{ background: theme.inset, foreground: theme.text, cursor: theme.text, selectionBackground: theme.overlay }}
      className="boss-secondary-fill" />
  </View>;
}
