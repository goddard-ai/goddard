import { layoutsForRoute, Screen, screenFor, Text, useRoute, useThemeScheme } from '@octane-xplat/ui';
import { TaskDrawerHost } from '../components/task-drawer';
import { useDaemonBootstrap } from '../lib/daemon-hooks';
import HomeScreen from './index.tsrx';

const resolveRoute = (name: string, params: Record<string, unknown>) => {
	const C = screenFor(name);
	if (!C) return <Text>404: {name}</Text>;
	let content = <C {...params} />;
	const chain = layoutsForRoute(name);
	for (let i = chain.length - 1; i >= 0; i--) {
		const Layout = chain[i];
		const nested = content;
		content = <Layout>{nested}</Layout>;
	}

	return content;
};

/** Web shell — same stack model as native: '/' shows home, pushed root
 * routes cover it through the URL-backed outlet. */
export default function RootLayout(props: { children?: any }) @{
	useDaemonBootstrap();
	const scheme = useThemeScheme();
	if (props.children != null) {
		return <TaskDrawerHost>{props.children}</TaskDrawerHost>;
	}

	const route = useRoute('root');

	<Screen className={['vx-app', 'flex-1', scheme === 'dark' ? 'dark ns-dark' : '']}>
		<TaskDrawerHost>
			{route ? resolveRoute(route.name, route.params) : <HomeScreen />}
		</TaskDrawerHost>
	</Screen>
}
