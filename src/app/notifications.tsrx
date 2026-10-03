import { Text, Column } from '@octane-xplat/ui';
import { useTheme } from '../lib/use-theme';

/** TODO(port): notification inbox (426 lines reference — notification
 * groups, mark-read, task deep links). */
export default function NotificationsScreen() {
	const theme = useTheme();
	return (
		<Column style={{ flex: 1, alignItems: 'center', justifyContent: 'center', backgroundColor: theme.background, padding: 32 }}>
			<Text style={{ color: theme.textSecondary }}>Notifications — port in progress</Text>
		</Column>
	);
}
