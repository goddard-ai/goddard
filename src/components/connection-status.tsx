import { ActivityIndicator, Row, Text, View } from '@octane-xplat/ui';

import { useTheme } from '@/lib/use-theme';
import type { ConnectionPhase } from '@/lib/daemon-store';

export function ConnectionStatus({
  phase,
  compact = false,
}: {
  phase: ConnectionPhase;
  compact?: boolean;
}) {
  const theme = useTheme();
  const presentation = statusPresentation(phase, theme);
  return (
    <Row style={{ alignItems: 'center', gap: 6 }}>
      {presentation.busy ? (
        <ActivityIndicator busy style={{ height: 12, width: 12 }} />
      ) : (
        <View
          style={{
            borderRadius: 999,
            height: 8,
            width: 8,
            backgroundColor: presentation.color,
          }}
        />
      )}
      {!compact && (
        <Text style={{ fontSize: 13, fontWeight: '600', color: presentation.color }}>
          {presentation.label}
        </Text>
      )}
    </Row>
  );
}

/** The phase as a short word, for labels and accessibility. */
export function connectionPhaseLabel(phase: ConnectionPhase): string {
  switch (phase) {
    case 'connected':
      return 'Connected';
    case 'connecting':
    case 'booting':
      return 'Connecting';
    case 'reconnecting':
      return 'Reconnecting';
    case 'error':
      return 'Needs attention';
    default:
      return 'Offline';
  }
}

function statusPresentation(
  phase: ConnectionPhase,
  theme: ReturnType<typeof useTheme>,
): { label: string; color: string; busy: boolean } {
  const label = connectionPhaseLabel(phase);
  switch (phase) {
    case 'connected':
      return { label, color: theme.success, busy: false };
    case 'connecting':
    case 'booting':
    case 'reconnecting':
      return { label, color: theme.warning, busy: true };
    case 'error':
      return { label, color: theme.danger, busy: false };
    default:
      return { label, color: theme.textTertiary, busy: false };
  }
}
