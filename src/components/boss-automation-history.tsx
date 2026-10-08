import { useEffect } from 'octane';
import { Pressable, ScrollView, Sheet, Text, View } from '@octane-xplat/ui';
import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import { useTheme } from '@/lib/use-theme';

/** Mobile v1 consumes the daemon's bounded history; definition editing and
 * manual triggering remain on desktop. Never render webhook credentials. */
export function BossAutomationHistory({ visible, onDismiss }: { visible: boolean; onDismiss: () => void }) {
  const daemon = useDaemon();
  const theme = useTheme();
  const profileId = daemon.activeProfile?.id ?? 'none';
  const connected = daemon.phase === 'connected' && Boolean(daemon.client);
  const query = useDaemonQuery(
    ['daemon', profileId, 'automation-history'],
    () => daemon.client!.request({ type: 'getAutomations' }).then((response) => {
      if (response.type !== 'automations') {throw new Error('Unexpected automation history response');}
      return {
        names: Object.fromEntries(response.state.automations.map((item) => [item.id, item.name])),
        runs: response.state.runs.slice(0, 100),
      };
    }),
    { enabled: visible && connected },
  );

  useEffect(() => {
    if (!visible || !connected) {return;}
    void query.refetch();
    const timer = setInterval(() => void query.refetch(), 15_000);
    return () => clearInterval(timer);
  }, [visible, connected, profileId]);

  return <Sheet open={visible} detents={[0.65, 1]} onDismiss={onDismiss}>
    <View className="boss-automation-sheet" style={{ backgroundColor: theme.surface }}>
      <Text className="boss-secondary-heading" style={{ color: theme.text }}>Automation run history</Text>
      <Text style={{ color: theme.textSecondary }}>Manage automations on desktop.</Text>
      <Pressable accessibilityRole="button" onPress={onDismiss}><Text className="boss-secondary-done" style={{ color: theme.text }}>Done</Text></Pressable>
      {!connected ? <Text style={{ color: theme.textSecondary }}>Disconnected. Cached history may be stale.</Text> : null}
      {query.error ? <Text style={{ color: theme.danger }}>Couldn’t load run history. Reopen this sheet to retry.</Text> : null}
      {query.isPending ? <Text style={{ color: theme.textSecondary }}>Loading run history…</Text> : null}
      {!query.isPending && !query.error && !query.data?.runs.length ? <Text style={{ color: theme.textSecondary }}>No recorded runs. Ask the Boss to set up an automation.</Text> : null}
      <ScrollView className="boss-secondary-fill">
        {query.data?.runs.map((run) => <View className="boss-automation-run" style={{ borderBottomColor: theme.border }}>
          <Text style={{ color: theme.text }}>{query.data?.names[run.automationId] || 'Deleted automation'}</Text>
          <Text style={{ color: theme.textSecondary }}>{run.status} · {run.trigger} · {new Date(run.createdAt * 1000).toLocaleString()}</Text>
          {run.refusalCount > 1 ? <Text style={{ color: theme.textSecondary }}>{run.refusalCount} repeated refusals</Text> : null}
          {run.error ? <Text style={{ color: theme.danger }}>{run.error}</Text> : null}
        </View>)}
      </ScrollView>
    </View>
  </Sheet>;
}
