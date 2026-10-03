import { popRoute, ScrollView, Column } from '@octane-xplat/ui';

import { DaemonEditor } from '../components/daemon-picker-sheet';
import { useTheme } from '../lib/use-theme';
import { useDaemon } from '../lib/daemon-hooks';

/** Standalone daemon editor — same form as the picker's inline editor.
 * TODO(port): the reference's PairingSection (paired-device approvals and
 * revocation against the active client) is not yet ported. */
export default function DaemonEditorScreen(props: { id?: string }) {
  const theme = useTheme();
  const daemon = useDaemon();
  const profile = daemon.profiles.find((item) => item.id === props.id);

  return (
    <Column style={{ flex: 1, backgroundColor: theme.background }}>
      <ScrollView style={{ flex: 1 }}>
        <Column style={{ paddingBottom: 36, paddingHorizontal: 16 }}>
          <DaemonEditor
            key={profile?.id ?? 'new'}
            profile={profile}
            onRemoved={() => popRoute('root')}
            onSaved={() => popRoute('root')}
          />
        </Column>
      </ScrollView>
    </Column>
  );
}
