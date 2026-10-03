/** @jsxImportSource @nativescript-community/octane */
import { Screen, useThemeScheme } from '@octane-xplat/ui';
import { TaskDrawerHost } from '../components/task-drawer';
import { useDaemonBootstrap } from '../lib/daemon-hooks';
import HomeScreen from './index.tsrx';

/** App shell — the root Frame page renders this; pushed Pages wrap their
 * screen through the same layout via children. Goddard mobile is a plain
 * navigation stack (no tab bar): the base screen is the home route and
 * everything else pushes over it. */
export default function RootLayout(props: { children?: any }) @{
	useDaemonBootstrap();
	const scheme = useThemeScheme();
	if (props.children != null) {
		return <TaskDrawerHost>{props.children}</TaskDrawerHost>;
	}

	<Screen className={['vx-app', 'flex-1', scheme === 'dark' ? 'dark ns-dark' : '']}>
		<TaskDrawerHost>
			<HomeScreen />
		</TaskDrawerHost>
	</Screen>
}
