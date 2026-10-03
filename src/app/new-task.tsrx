import { Pressable, pushRoute, Text, Column } from '@octane-xplat/ui';

import { ConnectionBanner } from '../components/connection-banner';
import { useTheme } from '../lib/use-theme';
import { useDaemon } from '../lib/daemon-hooks';

/** TODO(port): the full new-task composer — provider/project/workspace/
 * branch selector rows, model pickers, and the message composer
 * (mobile-composer, ~1.1k lines reference). This interim screen keeps the
 * daemon switcher reachable while the composer ports. */
export default function NewTaskScreen() {
	const theme = useTheme();
	const daemon = useDaemon();
	return (
		<Column style={{ flex: 1, backgroundColor: theme.background, padding: 16 }}>
			<ConnectionBanner />
			<Column style={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 16 }}>
				<Text style={{ fontSize: 17, color: theme.textSecondary }}>
					New task composer — port in progress
				</Text>
				<Pressable
					accessibilityRole="button"
					onPress={() => pushRoute({ stack: 'root', name: 'daemons', params: {} })}
					className="vx-pressable"
					style={{ paddingHorizontal: 18, minHeight: 44, justifyContent: 'center' }}
				>
					<Text style={{ fontSize: 16, color: theme.accent }}>
						{daemon.activeProfile ? `Daemon: ${daemon.activeProfile.name}` : 'Daemons'}
					</Text>
				</Pressable>
			</Column>
		</Column>
	);
}
