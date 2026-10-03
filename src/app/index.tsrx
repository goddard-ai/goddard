import { useState } from 'octane';

import { DaemonOnboarding } from '../components/daemon-onboarding';
import { DaemonPickerSheet } from '../components/daemon-picker-sheet';
import { useDaemon } from '../lib/daemon-hooks';
import NewTaskScreen from './new-task.tsrx';

export function HomeScreen() {
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

export default HomeScreen;
