import { popRoute, ScrollableArea, VStack } from '@octane-xplat/ui';

import { DaemonEditor } from '../components/daemon-picker-sheet';
import { useTheme } from '../lib/use-theme';
import { useDaemon } from '../lib/daemon-hooks';

/** Standalone daemon editor — same form as the picker's inline editor,
 * including the pairing section when this profile is the active daemon. */
export default function DaemonEditorScreen(props: { id?: string }) {
  const theme = useTheme();
  const daemon = useDaemon();
  const profile = daemon.profiles.find((item) => item.id === props.id);

  return (
    <VStack style={{ flex: '1 1', backgroundColor: theme.background }}>
      <ScrollableArea style={{ flex: '1 1' }}>
        <VStack style={{ paddingBottom: 36, paddingHorizontal: 16 }}>
          <DaemonEditor
            key={profile?.id ?? 'new'}
            profile={profile}
            onRemoved={() => popRoute('root')}
            onSaved={() => popRoute('root')}
          />
        </VStack>
      </ScrollableArea>
    </VStack>
  );
}
