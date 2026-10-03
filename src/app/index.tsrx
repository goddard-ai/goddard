/** @jsxImportSource @nativescript-community/octane */
import { useState } from 'octane';
import { View } from '@octane-xplat/ui';

import { DaemonOnboarding } from '@/components/daemon-onboarding';
import { DaemonPickerSheet } from '@/components/daemon-picker-sheet';
import { useDaemon } from '@/lib/daemon-hooks';

import NewTaskScreen from './new-task.tsrx';

export default function HomeScreen() {
  const daemon = useDaemon();
  const [daemonPickerOpen, setDaemonPickerOpen] = useState(false);

  if (daemon.phase === 'booting') return null;

  return (
    <>
      {daemon.profiles.length ? (
        <NewTaskScreen />
      ) : (
        <DaemonOnboarding onAddDaemon={() => setDaemonPickerOpen(true)} />
      )}
      {(!daemon.profiles.length || daemonPickerOpen) && (
        <DaemonPickerSheet
          onDismiss={() => setDaemonPickerOpen(false)}
          visible={daemonPickerOpen}
        />
      )}
    </>
  );
}
