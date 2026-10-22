/** @jsxImportSource @nativescript-community/octane */
import { useState } from 'octane';
import { Pressable, ScrollableArea, SegmentedControl, Text, View } from '@octane-xplat/ui';
import { pushRoute } from '@octane-xplat/ui/routing';
import { navigateBack, ScreenHeaderBackdrop } from '@/components/screen-header';
import { useDaemon } from '@/lib/daemon-hooks';
import { useBossState, useTaskState } from '@/lib/daemon-queries';
import { planFilter, type PlansFilter } from '@/lib/plans';
import { useTheme } from '@/lib/use-theme';

export default function PlansScreen() {
  const theme = useTheme();
  const boss = useBossState();
  const daemon = useDaemon();
  const tasks = useTaskState();
  const [filter, setFilter] = useState<PlansFilter>('active');
  const sessions = new Map((tasks.data?.sessions ?? []).map((session) => [session.id, session]));
  const visible = tasks.data ? (boss.data?.planning ?? [])
    .filter((entry) => planFilter(entry, sessions.get(entry.sessionId)) === filter)
    .sort((a, b) => (b.finalizedAt ?? sessions.get(b.sessionId)?.created_at ?? 0) - (a.finalizedAt ?? sessions.get(a.sessionId)?.created_at ?? 0)) : [];

  return <View style={{ flex: '1 1', backgroundColor: theme.background }}>
    <ScreenHeaderBackdrop />
    <View style={{ flexDirection: 'row', padding: 14, gap: 16 }}>
      <Pressable accessibilityRole="button" onPress={navigateBack}><Text style={{ color: theme.accent }}>Back</Text></Pressable>
      <Text style={{ color: theme.text, fontWeight: '600' }}>Plans</Text>
    </View>
    <>
      <SegmentedControl accessibilityLabel="Filter plans" value={filter} onValueChange={(value) => setFilter(value as PlansFilter)} options={[
        { label: 'Active', value: 'active' }, { label: 'Approved', value: 'approved' }, { label: 'Archived', value: 'archived' },
      ]} />
      <ScrollableArea style={{ flex: '1 1' }}>
        {boss.error ? <Text style={{ color: theme.textSecondary, padding: 14 }}>Couldn’t load plans: {String(boss.error)}</Text> : null}
        {!visible.length ? <Text style={{ color: theme.textSecondary, padding: 14 }}>{daemon.phase !== 'connected' ? 'Connect to a daemon to read plans.' : boss.isPending || tasks.isPending ? 'Loading plans…' : boss.data == null ? 'Boss is disabled on this daemon.' : `No ${filter} plans`}</Text> : null}
        {visible.map((entry) => <Pressable accessibilityRole="button" onPress={() => pushRoute({ stack: 'root', name: 'plans/:id', params: { id: entry.id } })} style={{ padding: 14 }}>
          <Text style={{ color: theme.text }}>{entry.idea}</Text>
          <Text style={{ color: theme.textTertiary }}>{entry.planFile}</Text>
        </Pressable>)}
      </ScrollableArea>
    </>
  </View>;
}
