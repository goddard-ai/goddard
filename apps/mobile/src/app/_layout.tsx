import {
  DarkTheme,
  DefaultTheme,
  Stack,
  ThemeProvider,
  router,
  type NativeStackNavigationOptions,
} from "expo-router";
import * as QuickActions from "expo-quick-actions";
import * as SplashScreen from "expo-splash-screen";
import { StatusBar } from "expo-status-bar";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useEffect, useMemo } from "react";
import { Platform, StyleSheet, useColorScheme } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";

import { Colors } from "@/constants/theme";
import {
  HeaderAction,
  HeaderActionGroup,
  nativeHeaderButtons,
  type HeaderActionSpec,
} from "@/components/screen-header";
import { TaskDrawerHost, useTaskDrawer } from "@/components/task-drawer";
import { useTaskState } from "@/hooks/use-daemon-data";
import { DaemonProvider, useDaemon } from "@/lib/daemon-context";
import { RuntimeProvider } from "@/lib/runtime-context";
import {
  displaySessionTitle,
  sessionHasStarted,
} from "@/lib/session-presentation";

/** Deep links and state restores keep the new-task home as the stack anchor. */
export const unstable_settings = { anchor: "index" };

void SplashScreen.preventAutoHideAsync();
SplashScreen.setOptions({ duration: 250, fade: true });

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnReconnect: true,
    },
  },
});

/**
 * Native navigation bar with no background of its own: content scrolls
 * beneath it and its Liquid Glass items float. Every screen on the main path
 * shows the bar, the task list included, because that is what lets UIKit hold
 * the bar's buttons in place and crossfade them during an interactive
 * swipe-back; popping to a screen without a bar makes UIKit slide the whole
 * bar out with the page instead.
 */
const floatingHeader = {
  headerShown: true,
  headerStyle: { backgroundColor: "transparent" },
  headerTransparent: true,
} satisfies NativeStackNavigationOptions;

export default function RootLayout() {
  const colorScheme = useColorScheme();
  const colors = Colors[colorScheme === "dark" ? "dark" : "light"];
  const navigationTheme =
    colorScheme === "dark"
      ? {
          ...DarkTheme,
          colors: {
            ...DarkTheme.colors,
            background: colors.background,
            card: colors.background,
          },
        }
      : {
          ...DefaultTheme,
          colors: {
            ...DefaultTheme.colors,
            background: colors.background,
            card: colors.background,
          },
        };
  return (
    <GestureHandlerRootView style={styles.root}>
      <QueryClientProvider client={queryClient}>
        <DaemonProvider>
          <RuntimeProvider>
            <ThemeProvider value={navigationTheme}>
              <TaskDrawerHost>
                <AppNavigator />
              </TaskDrawerHost>
              <StatusBar style="auto" />
            </ThemeProvider>
          </RuntimeProvider>
        </DaemonProvider>
      </QueryClientProvider>
    </GestureHandlerRootView>
  );
}

function AppNavigator() {
  const { phase, profiles } = useDaemon();
  const { openTaskDrawer } = useTaskDrawer();
  const taskState = useTaskState();
  const colorScheme = useColorScheme();
  const theme = Colors[colorScheme === "dark" ? "dark" : "light"];
  const daemonRoutesAvailable = phase === "booting" || profiles.length > 0;
  const drawerAction = useMemo<HeaderActionSpec>(() => ({
    icon: { ios: "sidebar.left", android: "menu", web: "menu" },
    label: "Task history",
    onPress: openTaskDrawer,
  }), [openTaskDrawer]);
  const drawerHeader = useMemo<NativeStackNavigationOptions>(() => ({
    ...floatingHeader,
    gestureEnabled: false,
    headerLeft: () => (
      <HeaderActionGroup>
        <HeaderAction {...drawerAction} />
      </HeaderActionGroup>
    ),
    unstable_headerLeftItems: () => nativeHeaderButtons([drawerAction]),
  }), [drawerAction]);

  useEffect(() => {
    if (phase !== "booting") void SplashScreen.hideAsync();
  }, [phase]);

  // Home-screen quick actions: New task plus the three most recent tasks.
  // taskState churns during streaming, so items refresh only when the
  // recent-session list itself changes.
  const recents = useMemo(() => {
    const data = taskState.data;
    if (!data) return [];
    const projectNames = new Map(
      data.projects.map((project) => [project.id, project.name]),
    );
    return data.sessions
      .filter((session) => sessionHasStarted(session) && session.archived_at == null)
      .sort((a, b) =>
        (b.last_reply_at ?? b.created_at) - (a.last_reply_at ?? a.created_at),
      )
      .slice(0, 3)
      .map((session) => ({
        id: session.id,
        title: displaySessionTitle(session),
        subtitle: projectNames.get(session.project_id) ?? "",
      }));
  }, [taskState.data]);
  const recentsKey = recents.map((item) => `${item.id}:${item.title}`).join("|");
  useEffect(() => {
    if (Platform.OS === "web") return;
    const icon = (name: string) =>
      Platform.select({ ios: `symbol:${name}`, default: "ic_recent_task" });
    void QuickActions.setItems([
      {
        id: "new-task",
        title: "New task",
        icon: Platform.select({
          ios: "symbol:square.and.pencil",
          default: "ic_new_task",
        }),
      },
      ...recents.map((item) => ({
        id: `session:${item.id}`,
        title: item.title,
        subtitle: item.subtitle || null,
        icon: icon("clock"),
        params: { sessionId: item.id },
      })),
    ]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recentsKey]);

  useEffect(() => {
    if (Platform.OS === "web") return;
    const open = (action: QuickActions.Action | undefined) => {
      if (!action) return;
      const sessionId = action.params?.sessionId;
      if (typeof sessionId === "string" && sessionId) {
        router.push({ pathname: "/session/[id]", params: { id: sessionId } });
      } else if (action.id === "new-task") {
        router.dismissTo("/");
      }
    };
    const subscription = QuickActions.addListener(open);
    // A press that cold-started the app never reaches the listener.
    open(QuickActions.initial);
    return () => subscription.remove();
  }, []);

  return (
    <Stack
      screenOptions={{
        contentStyle: { backgroundColor: theme.background },
        headerBackButtonDisplayMode: "minimal",
        headerShadowVisible: false,
        headerStyle: { backgroundColor: theme.background },
        headerTintColor: theme.text,
      }}
    >
      <Stack.Screen
        name="index"
        options={daemonRoutesAvailable
          ? { ...drawerHeader, title: "New Task" }
          : { headerShown: false, title: "Goddard" }}
      />
      {/* Removing the final saved daemon also removes every daemon-backed
       * route from navigation, returning restored and open tasks to home. */}
      <Stack.Protected guard={daemonRoutesAvailable}>
        <Stack.Screen
          name="daemons"
          options={{ headerLargeTitle: true, title: "Daemons" }}
        />
        <Stack.Screen
          name="new-task"
          options={{ ...drawerHeader, title: "New Task" }}
        />
        <Stack.Screen
          name="session/[id]"
          options={{
            ...drawerHeader,
            animation: "none",
            headerTitleAlign: "left",
            title: "Task",
          }}
        />
        <Stack.Screen
          name="notifications"
          options={{ ...drawerHeader, title: "Notifications" }}
        />
      </Stack.Protected>
      <Stack.Screen
        name="daemon-editor"
        options={{
          presentation: "pageSheet",
          title: "Add Daemon",
        }}
      />
      <Stack.Screen
        name="daemon-scan"
        options={{
          presentation: "pageSheet",
          title: "Scan QR Code",
        }}
      />
      {/* The `goddard://connect` deep link — outside the daemon guard so it
       * can add the very first profile. */}
      <Stack.Screen name="connect" options={{ title: "Connect" }} />
    </Stack>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1 },
});
