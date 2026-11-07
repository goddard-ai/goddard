import { useCallback, useSyncExternalStore } from 'react';
import { Pressable, Text, View } from 'react-native';
import { landingStore, landingStatusText, type WakuClient } from '@waku/client';
import { useTheme } from '@/hooks/use-theme';

// Mobile currently uses English UI copy; keep the shared status keys aligned
// with the native/web catalog until mobile gains a catalog provider.
const labels: Record<string, string> = {
  'git_panel.land_conflict_description': 'Landing onto %{base} stopped on conflicts. Resolve them in the worktree, then run /land again.',
  'git_panel.land_status_unavailable': 'Landing status unavailable',
  'git_panel.land_requesting': 'Requesting landing…',
  'git_panel.land_canceling': 'Canceling queued landing…',
  'git_panel.land_queued_next': 'Queued · next',
  'git_panel.land_queued_position': 'Queued · #%{position}',
  'git_panel.landing_onto': 'Landing onto %{base}…',
  'git_panel.landing': 'Landing…',
  'git_panel.land_canceled': 'Landing canceled',
};
const translate = (key: string, params?: Record<string, string | number>) => (labels[key] ?? key).replace(/%\{(\w+)\}/g, (_, name: string) => String(params?.[name] ?? ''));

export function LandingStatus({ client, taskId, cwd }: { client: WakuClient; taskId: string; cwd: string }) {
  const theme = useTheme();
  const store = landingStore(client);
  const subscribe = useCallback((listener: () => void) => store.subscribe(taskId, cwd, listener), [store, taskId, cwd]);
  const get = useCallback(() => store.get(taskId, cwd), [store, taskId, cwd]);
  const view = useSyncExternalStore(subscribe, get, get);
  if (!client.landingQueueAvailable) return null;
  const text = landingStatusText(view, translate);
  if (!text) return null;
  const queued = view.phase === 'known' && view.job?.state === 'queued';
  return (
    <View style={{ borderColor: theme.border, borderWidth: 1, borderRadius: 10, paddingHorizontal: 12, marginBottom: 8, flexDirection: 'row', alignItems: 'center', gap: 12 }}>
      <Text accessibilityLiveRegion="polite" style={{ color: theme.text, flex: 1, paddingVertical: 10 }}>{text}</Text>
      {(queued || view.phase === 'unavailable') && <Pressable accessibilityRole="button" onPress={() => queued ? void store.cancel(taskId, cwd) : store.retryStatus(taskId, cwd)} style={({ pressed }) => ({ minHeight: 44, justifyContent: 'center', opacity: pressed ? 0.5 : 1 })}>
        <Text style={{ color: theme.accent }}>{queued ? 'Cancel queued landing' : 'Refresh status'}</Text>
      </Pressable>}
    </View>
  );
}
