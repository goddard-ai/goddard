import { Text, Column } from '@octane-xplat/ui';
import { useTheme } from '../../lib/use-theme';

/** TODO(port): the session view — streaming transcript, composer,
 * task-surface sheet (~2.5k lines of reference components). */
export default function SessionScreen(props: { id?: string }) {
	const theme = useTheme();
	return (
		<Column style={{ flex: 1, alignItems: 'center', justifyContent: 'center', backgroundColor: theme.background, padding: 32 }}>
			<Text style={{ color: theme.textSecondary }}>Session {props.id} — port in progress</Text>
		</Column>
	);
}
