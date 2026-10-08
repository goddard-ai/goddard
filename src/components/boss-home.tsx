import { useEffect, useState } from 'octane';
import { ActivityIndicator, Pressable, Text, View } from '@octane-xplat/ui';
import { openBossSession } from '@waku/client/boss';

import { ConnectionBanner } from '@/components/connection-banner';
import { SessionView } from '@/components/session-view';
import { useTaskDrawer } from '@/components/task-drawer';
import { useDaemon } from '@/lib/daemon-hooks';
import { useBossState, useProviderCatalog } from '@/lib/daemon-queries';
import { useTheme } from '@/lib/use-theme';
import NewTaskScreen from '../app/new-task.tsrx';

/** Home is scoped to one host. A cancelled connection attempt must never
 * put another host's session into the current home's transcript. */
export function BossHome() {
  const daemon = useDaemon();
  const boss = useBossState();
  const catalog = useProviderCatalog();
  const theme = useTheme();
  const { openTaskDrawer } = useTaskDrawer();
  const [sessionId, setSessionId] = useState<string | undefined>();
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const cachedId = boss.data?.sessionId ?? undefined;

  useEffect(() => {
    if (cachedId || sessionId || !boss.data || !daemon.client || daemon.phase !== 'connected' || catalog.isPending) {return;}
    let cancelled = false;
    setError(null);
    const installed = catalog.providers.filter((item) => item.installed).map((item) => item.id);
    const provider = installed.includes('codex') ? 'codex' : installed[0] ?? 'codex';
    void openBossSession(daemon.client, { provider, model: null, mode: 'autoAcceptEdits' })
      .then(({ session }) => { if (!cancelled) {setSessionId(session.id);} })
      .catch((cause) => { if (!cancelled) {setError(cause instanceof Error ? cause.message : String(cause));} });

    return () => { cancelled = true; };
  }, [cachedId, sessionId, boss.data, daemon.client, daemon.phase, catalog.isPending, retry]);

  if (cachedId || sessionId) {
    return <SessionView sessionId={cachedId ?? sessionId} />;
  }

  // Hosts with the Boss experiment disabled retain their existing entry.
  if (boss.data === null) {return <NewTaskScreen />;}
  const message = error ?? (boss.error instanceof Error ? boss.error.message : null);
  return (
    <View style={{ flex: '1 1', backgroundColor: theme.background }}>
      <ConnectionBanner />
      <View style={{ flex: '1 1', alignItems: 'center', justifyContent: 'center', gap: 16, padding: 32 }}>
        <Text style={{ color: theme.text, fontSize: 22, fontWeight: '600' }}>
          {boss.data?.identity.name ?? 'Your Boss'}
        </Text>
        <Text style={{ color: theme.textSecondary, textAlign: 'center' }}>
          {message ?? (daemon.phase === 'connected' ? 'Opening Boss chat…' : 'Connect to this host to talk to your Boss.')}
        </Text>
        {!message && daemon.phase === 'connected' ? <ActivityIndicator busy /> : null}
        {message ? (
          <Pressable accessibilityRole="button" onPress={() => { setRetry((value) => value + 1); void boss.refetch(); }}>
            <Text style={{ color: theme.accent }}>Try again</Text>
          </Pressable>
        ) : null}
        <Pressable accessibilityRole="button" onPress={openTaskDrawer}>
          <Text style={{ color: theme.accent }}>Bosses and chats</Text>
        </Pressable>
      </View>
    </View>
  );
}
