/** @jsxImportSource @nativescript-community/octane */
import { Pressable, ScrollableArea, Text, View } from '@octane-xplat/ui';
import { useRoute } from '@octane-xplat/ui/routing';
import { PlanDetail } from '@/components/plans';
import { navigateBack, ScreenHeaderBackdrop } from '@/components/screen-header';
import { useBossState } from '@/lib/daemon-queries';
import { useTheme } from '@/lib/use-theme';

export default function PlanScreen() {
  const theme = useTheme();
  const route = useRoute('root');
  const id = (route?.params as { id?: string } | undefined)?.id;
  const boss = useBossState();
  const plan = boss.data?.planning.find((entry) => entry.id === id || entry.sessionId === id || entry.planFile === id);
  return <View style={{ flex: '1 1', backgroundColor: theme.background }}>
    <ScreenHeaderBackdrop />
    <Pressable accessibilityRole="button" accessibilityLabel="Back" onPress={navigateBack} style={{ padding: 14 }}>
      <Text style={{ color: theme.accent }}>Back</Text>
    </Pressable>
    <ScrollableArea style={{ flex: '1 1' }}>
      {plan ? <PlanDetail plan={plan} /> : <Text style={{ color: theme.textSecondary, padding: 14 }}>{boss.error ? `Couldn’t load plan: ${String(boss.error)}` : boss.isPending ? 'Loading plan…' : 'Plan unavailable'}</Text>}
    </ScrollableArea>
  </View>;
}
