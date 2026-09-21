import type { AgentSession } from '@waku/client';
import { FlashList, type ListRenderItemInfo } from '@shopify/flash-list';
import * as Haptics from 'expo-haptics';
import { router, useGlobalSearchParams, usePathname } from 'expo-router';
import {
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  ActivityIndicator,
  Alert,
  AppState,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  RefreshControl,
  StyleSheet,
  Text,
  TextInput,
  View,
  useWindowDimensions,
} from 'react-native';
import { Drawer, useDrawerProgress } from 'react-native-drawer-layout';
import Animated, {
  useAnimatedStyle,
  useSharedValue,
  withDelay,
  withTiming,
} from 'react-native-reanimated';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { AppSymbol } from '@/components/app-symbol';
import {
  ConnectionBanner,
  useConnectionNotice,
  type ConnectionNotice,
} from '@/components/connection-banner';
import { DaemonPickerSheet } from '@/components/daemon-picker-sheet';
import { GlassSurface } from '@/components/glass-surface';
import { ConnectionStatus, connectionPhaseLabel } from '@/components/connection-status';
import { ProviderIcon } from '@/components/provider-icon';
import { RenameDialog } from '@/components/rename-dialog';
import { TaskRowMenu } from '@/components/task-row-menu';
import { NativeTint, Radius, Spacing } from '@/constants/theme';
import { useTaskState } from '@/hooks/use-daemon-data';
import { useReducedMotion } from '@/hooks/use-reduced-motion';
import { useTheme } from '@/hooks/use-theme';
import { useDaemon } from '@/lib/daemon-context';
import { useDisplayCornerRadius } from '@/lib/display-corner-radius';
import { sessionIsRunning } from '@/lib/mobile-runtime';
import { useRuntime } from '@/lib/runtime-context';
import {
  displaySessionTitle,
  groupSessions,
  providerLabel,
  relativeSessionTime,
  type SessionGroup,
  type SessionListItem,
} from '@/lib/session-presentation';

const DaemonPickerHeight = 38;
const SearchDockGap = 14;

/** FlashList has no section API, so groups flatten into header/session rows;
 * `getItemType` keeps the two shapes in separate recycling pools. */
type DrawerListRow =
  | { kind: 'header'; key: string; title: string }
  | { kind: 'session'; key: string; item: SessionListItem };

function drawerRows(sections: SessionGroup[]): DrawerListRow[] {
  return sections.flatMap((section) => [
    { kind: 'header' as const, key: `header:${section.id}`, title: section.title },
    ...section.data.map((item): DrawerListRow => ({
      kind: 'session',
      key: `session:${item.session.id}`,
      item,
    })),
  ]);
}

function drawerRowKey(row: DrawerListRow): string {
  return row.key;
}

function drawerRowType(row: DrawerListRow): string {
  return row.kind;
}
interface TaskDrawerContextValue {
  openTaskDrawer: () => void;
  closeTaskDrawer: () => void;
}

const TaskDrawerContext = createContext<TaskDrawerContextValue | null>(null);

export function TaskDrawerHost({ children }: { children: ReactNode }) {
  const theme = useTheme();
  const daemon = useDaemon();
  const pathname = usePathname();
  const params = useGlobalSearchParams<{ id?: string | string[] }>();
  const { width } = useWindowDimensions();
  const [open, setOpen] = useState(false);
  const [listRefreshTick, setListRefreshTick] = useState(0);
  const drawerWidth = Math.max(0, Math.min(360, width - 44));
  const drawerEnabled = daemon.phase === 'booting' || daemon.profiles.length > 0;
  const openTaskDrawer = useCallback(() => {
    if (drawerEnabled) setOpen(true);
  }, [drawerEnabled]);
  const closeTaskDrawer = useCallback(() => setOpen(false), []);
  // A swipe fires this the moment the pan activates, while the drawer is still
  // covered — the list re-renders from live data before the reveal shows it.
  const refreshTaskList = useCallback(() => setListRefreshTick((tick) => tick + 1), []);
  const controls = useMemo(
    () => ({ openTaskDrawer, closeTaskDrawer }),
    [closeTaskDrawer, openTaskDrawer],
  );
  const swipeEnabled = pathname === '/'
    || pathname === '/new-task'
    || pathname.startsWith('/session/');
  const selectedSessionId = pathname.startsWith('/session/')
    ? Array.isArray(params.id) ? params.id[0] : params.id ?? null
    : null;

  useEffect(() => setOpen(false), [drawerEnabled, pathname]);

  return (
    <TaskDrawerContext.Provider value={controls}>
      {drawerEnabled ? (
        <Drawer
          drawerStyle={{ backgroundColor: theme.background, width: drawerWidth }}
          drawerType="back"
          open={open}
          overlayAccessibilityLabel="Close task history"
          // The library overlay stays as an invisible press-catcher; the lift
          // comes from the card's own edge shadow, so nothing paints a dim.
          overlayStyle={{ backgroundColor: 'transparent' }}
          renderDrawerContent={() => (
            <TaskDrawerContent
              drawerWidth={drawerWidth}
              open={open}
              refreshTick={listRefreshTick}
              selectedSessionId={selectedSessionId}
              onClose={closeTaskDrawer}
            />
          )}
          swipeEdgeWidth={width}
          swipeEnabled={swipeEnabled}
          onClose={closeTaskDrawer}
          onGestureStart={refreshTaskList}
          onOpen={openTaskDrawer}>
          <DrawerCard>{children}</DrawerCard>
        </Drawer>
      ) : (
        <>{children}</>
      )}
    </TaskDrawerContext.Provider>
  );
}

/** Slides and rounds with the content as the drawer reveals, so the app lifts
 * off as a card matching the display's corner radius. The radius is applied at
 * rest too — at the true display value the clip coincides with the screen's own
 * curve, so it's invisible until the card starts to slide. The shadow lives on
 * a non-clipping wrapper: iOS drops a shadow set on the same view that clips. */
function DrawerCard({ children }: { children: ReactNode }) {
  const progress = useDrawerProgress();
  const cornerRadius = useDisplayCornerRadius();
  const shadowStyle = useAnimatedStyle(() => ({
    shadowOpacity: progress.value * 0.08,
  }));
  return (
    <Animated.View
      style={[styles.drawerCardShadow, { borderRadius: cornerRadius }, shadowStyle]}>
      <Animated.View style={[styles.drawerCard, { borderRadius: cornerRadius }]}>
        {children}
      </Animated.View>
    </Animated.View>
  );
}

export function useTaskDrawer(): TaskDrawerContextValue {
  const context = useContext(TaskDrawerContext);
  if (!context) throw new Error('useTaskDrawer must be used inside TaskDrawerHost');
  return context;
}

function TaskDrawerContent({
  drawerWidth,
  open,
  refreshTick,
  selectedSessionId,
  onClose,
}: {
  drawerWidth: number;
  open: boolean;
  refreshTick: number;
  selectedSessionId: string | null;
  onClose: () => void;
}) {
  const theme = useTheme();
  const insets = useSafeAreaInsets();
  const daemon = useDaemon();
  const runtime = useRuntime();
  const taskState = useTaskState();
  const [search, setSearch] = useState('');
  const [refreshing, setRefreshing] = useState(false);
  const [daemonPickerOpen, setDaemonPickerOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<AgentSession | null>(null);
  const [now, setNow] = useState(Date.now);
  // Share one clock across visible rows, and park it while the drawer or app
  // is hidden so recency labels add no work to the conversation screen.
  useEffect(() => {
    if (!open) return;
    let timer: ReturnType<typeof setInterval> | undefined;
    const refresh = () => setNow(Date.now());
    const updateClock = (state: string | null) => {
      clearInterval(timer);
      if (state === 'active' || state === null) {
        refresh();
        timer = setInterval(refresh, 30_000);
      }
    };
    updateClock(AppState.currentState);
    const subscription = AppState.addEventListener('change', updateClock);
    return () => {
      clearInterval(timer);
      subscription.remove();
    };
  }, [open]);
  // The drawer stays mounted while closed, and stream commits rewrite
  // taskState several times a second. Render the list from a snapshot instead
  // of the live query so those commits cost nothing while hidden — but keep
  // the snapshot fresh anyway: once a burst settles (~300ms quiet), refresh
  // in the background, and a swipe refreshes the instant it starts, so the
  // list is already current as the drawer peels back.
  const [settleTick, setSettleTick] = useState(0);
  useEffect(() => {
    if (open) return;
    const timer = setTimeout(() => setSettleTick((tick) => tick + 1), 300);
    return () => clearTimeout(timer);
  }, [open, taskState.data, runtime.runtimes, selectedSessionId]);
  const frameRef = useRef({
    data: taskState.data,
    runtimes: runtime.runtimes,
    selectedSessionId,
  });
  const lastRefresh = useRef({ gesture: refreshTick, settle: 0 });
  if (
    open
    || refreshTick !== lastRefresh.current.gesture
    || settleTick !== lastRefresh.current.settle
  ) {
    lastRefresh.current = { gesture: refreshTick, settle: settleTick };
    frameRef.current = {
      data: taskState.data,
      runtimes: runtime.runtimes,
      selectedSessionId,
    };
  }
  const frame = frameRef.current;
  const listExtraData = useMemo(
    () => ({ runtimes: frame.runtimes, now, selected: frame.selectedSessionId }),
    [frame.runtimes, frame.selectedSessionId, now],
  );
  const visibleSessions = useMemo(() => {
    if (!frame.data) return [];
    const query = search.trim().toLocaleLowerCase();
    if (!query) return frame.data.sessions;
    const projects = new Map(frame.data.projects.map((project) => [project.id, project]));
    return frame.data.sessions.filter((session) => {
      const project = projects.get(session.project_id);
      return [
        displaySessionTitle(session),
        project?.name,
        project?.path,
        providerLabel(session.provider),
        session.model,
      ].some((value) => value?.toLocaleLowerCase().includes(query));
    });
  }, [search, frame.data]);
  const sections = useMemo(
    () => frame.data ? groupSessions(frame.data.projects, visibleSessions) : [],
    [frame.data, visibleSessions],
  );
  const rows = useMemo(() => drawerRows(sections), [sections]);
  const notice = useConnectionNotice();
  const reducedMotion = useReducedMotion();
  const hasRows = rows.length > 0;
  // TaskListEmpty renders nothing while a notice is explaining the wait, so
  // the empty layer only has content to fade when no notice is blocking it.
  const emptyShown = !hasRows && !(notice && notice.kind !== 'restored');
  // The list rests at full opacity in every state so the banner and the
  // pull-to-refresh spinner stay live while empty; it only dips to 0 for the
  // beat between the empty state fading out and the fresh rows fading in.
  const listOpacity = useSharedValue(1);
  const emptyOpacity = useSharedValue(emptyShown ? 1 : 0);
  // The drag itself lifts the list layers from 15% to full as the drawer
  // reveals, multiplied with the crossfade values so both fades compose.
  const drawerProgress = useDrawerProgress();
  const listFadeStyle = useAnimatedStyle(() => ({
    opacity: listOpacity.value * (0.15 + 0.85 * drawerProgress.value),
  }));
  const emptyFadeStyle = useAnimatedStyle(() => ({
    opacity: emptyOpacity.value * (0.15 + 0.85 * drawerProgress.value),
  }));
  const fadeState = useRef({ emptyShown, hasRows });
  useLayoutEffect(() => {
    const prev = fadeState.current;
    if (prev.hasRows === hasRows && prev.emptyShown === emptyShown) return;
    const revealed = hasRows && !prev.hasRows;
    fadeState.current = { emptyShown, hasRows };
    if (reducedMotion) {
      listOpacity.value = 1;
      emptyOpacity.value = emptyShown ? 1 : 0;
      return;
    }
    if (revealed) {
      // Swap, don't pop: the empty state fades out, then the list fades in.
      // Setting opacity here in the layout effect beats the first paint of
      // the new rows, so they never flash before the fade starts.
      listOpacity.value = 0;
      emptyOpacity.value = withTiming(0, { duration: 140 });
      listOpacity.value = withDelay(140, withTiming(1, { duration: 220 }));
      return;
    }
    listOpacity.value = 1;
    emptyOpacity.value = withTiming(emptyShown ? 1 : 0, { duration: 200 });
  }, [emptyOpacity, emptyShown, hasRows, listOpacity, reducedMotion]);
  const showNewTask = useCallback(() => {
    onClose();
    router.dismissTo('/');
  }, [onClose]);
  const showSession = useCallback((sessionId: string) => {
    // setParams keeps the pathname, so the effect that closes the drawer on
    // navigation never fires for session-to-session switches — close it here.
    onClose();
    if (frame.selectedSessionId === sessionId) return;
    if (frame.selectedSessionId) {
      router.setParams({ id: sessionId });
    } else {
      router.replace({ pathname: '/session/[id]', params: { id: sessionId } });
    }
  }, [frame.selectedSessionId, onClose]);
  const confirmDelete = useCallback((session: AgentSession) => {
    Alert.alert(
      `Delete “${displaySessionTitle(session)}”?`,
      'This removes the task and its transcript from the daemon for every device.',
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Delete',
          style: 'destructive',
          onPress: () => {
            void runtime.deleteSession(session.id)
              .then(() => Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success))
              .catch((cause) => {
                Alert.alert(
                  'Couldn’t delete task',
                  cause instanceof Error ? cause.message : String(cause),
                );
              });
          },
        },
      ],
    );
  }, [runtime.deleteSession]);
  const renameSession = useCallback(
    (session: AgentSession) => setRenameTarget(session),
    [],
  );
  const selectSession = useCallback(
    (session: AgentSession) => showSession(session.id),
    [showSession],
  );
  const renderRow = useCallback(({ item: row }: ListRenderItemInfo<DrawerListRow>) => {
    if (row.kind === 'header') return <SectionHeader title={row.title} />;
    const { session, projectName } = row.item;
    return (
      <SessionRow
        drawerWidth={drawerWidth}
        lastReplyLabel={session.last_reply_at == null
          ? null
          : relativeSessionTime(session.last_reply_at, now)}
        projectName={projectName}
        running={frame.runtimes[session.id]?.running ?? sessionIsRunning(session)}
        selected={session.id === frame.selectedSessionId}
        session={session}
        onDelete={confirmDelete}
        onRename={renameSession}
        onSelect={selectSession}
      />
    );
  }, [
    confirmDelete,
    drawerWidth,
    frame.runtimes,
    frame.selectedSessionId,
    now,
    renameSession,
    selectSession,
  ]);

  async function refreshTasks() {
    setRefreshing(true);
    try {
      if (daemon.phase === 'connected') await taskState.refetch();
      else await daemon.reconnect();
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <View style={[styles.screen, { backgroundColor: theme.background }]}>
      <View
        pointerEvents="box-none"
        style={[styles.daemonFloat, { top: insets.top + 8 }]}>
        <DaemonPill onPress={() => setDaemonPickerOpen(true)} />
      </View>

      <Animated.View style={[styles.listHost, listFadeStyle]}>
        <FlashList
          data={rows}
          extraData={listExtraData}
          getItemType={drawerRowType}
          keyExtractor={drawerRowKey}
          contentContainerStyle={[
            styles.listContent,
            {
              paddingTop: insets.top + DaemonPickerHeight + 20,
            },
            rows.length === 0 && styles.listContentEmpty,
          ]}
          refreshControl={(
            <RefreshControl
              colors={[theme.textTertiary]}
              progressViewOffset={insets.top + DaemonPickerHeight + 12}
              refreshing={refreshing}
              tintColor={theme.textTertiary}
              onRefresh={() => void refreshTasks()}
            />
          )}
          renderItem={renderRow}
          ListHeaderComponent={<ConnectionBanner />}
          showsVerticalScrollIndicator={false}
        />
      </Animated.View>

      <Animated.View
        pointerEvents={hasRows ? 'none' : 'box-none'}
        style={[
          styles.emptyOverlay,
          {
            paddingBottom: insets.bottom + 90,
            paddingTop: insets.top + DaemonPickerHeight + 20,
          },
          emptyFadeStyle,
        ]}>
        <TaskListEmpty
          error={taskState.error}
          notice={notice}
          searching={Boolean(search.trim())}
          onNewTask={showNewTask}
        />
      </Animated.View>

      {(daemon.profiles.length > 0 || daemon.phase === 'booting') && (
        <KeyboardAvoidingView
          behavior={Platform.OS === 'ios' ? 'position' : undefined}
          keyboardVerticalOffset={SearchDockGap}
          pointerEvents="box-none"
          style={[styles.searchDockAvoider, { bottom: insets.bottom + SearchDockGap }]}>
          <View pointerEvents="box-none" style={styles.searchDock}>
            <GlassSurface interactive style={styles.searchCapsule}>
              <View style={styles.searchCapsuleInner}>
                <AppSymbol
                  name={{ ios: 'magnifyingglass', android: 'search', web: 'search' }}
                  size={17}
                  tintColor={theme.textSecondary}
                />
                <TextInput
                  accessibilityLabel="Search tasks"
                  autoCapitalize="none"
                  autoCorrect={false}
                  placeholder="Search"
                  placeholderTextColor={theme.textTertiary}
                  selectionColor={NativeTint}
                  style={[styles.searchInput, { color: theme.text }]}
                  value={search}
                  onChangeText={setSearch}
                />
                {search.length > 0 && (
                  <Pressable
                    accessibilityLabel="Clear search"
                    accessibilityRole="button"
                    hitSlop={8}
                    onPress={() => setSearch('')}
                    style={({ pressed }) => ({ opacity: pressed ? 0.5 : 1 })}>
                    <AppSymbol
                      name={{ ios: 'xmark.circle.fill', android: 'cancel', web: 'cancel' }}
                      size={16}
                      tintColor={theme.textTertiary}
                    />
                  </Pressable>
                )}
              </View>
            </GlassSurface>
            {daemon.phase === 'connected' && (
              <GlassSurface interactive style={styles.composeButton}>
                <Pressable
                  accessibilityLabel="New task"
                  accessibilityRole="button"
                  hitSlop={6}
                  onPress={showNewTask}
                  style={({ pressed }) => [styles.roundInner, { opacity: pressed ? 0.5 : 1 }]}>
                  <AppSymbol
                    name={{ ios: 'square.and.pencil', android: 'edit_square', web: 'edit' }}
                    size={20}
                    tintColor={theme.text}
                  />
                </Pressable>
              </GlassSurface>
            )}
          </View>
        </KeyboardAvoidingView>
      )}

      {renameTarget && (
        <RenameDialog
          initialValue={displaySessionTitle(renameTarget)}
          onDismiss={() => setRenameTarget(null)}
          onSubmit={(title) => runtime.renameSession(renameTarget.id, title)}
          visible
        />
      )}
      <DaemonPickerSheet
        onDismiss={() => setDaemonPickerOpen(false)}
        visible={daemonPickerOpen}
      />
    </View>
  );
}

function DaemonPill({ onPress }: { onPress: () => void }) {
  const theme = useTheme();
  const daemon = useDaemon();
  return (
    <GlassSurface interactive style={styles.daemonButton}>
      <Pressable
        accessibilityHint="Opens the daemon switcher"
        accessibilityLabel={daemon.activeProfile
          ? `${connectionPhaseLabel(daemon.phase)}: ${daemon.activeProfile.name}`
          : 'Add a daemon'}
        accessibilityRole="button"
        hitSlop={8}
        onPress={onPress}
        style={({ pressed }) => [styles.daemonButtonInner, { opacity: pressed ? 0.62 : 1 }]}>
        {daemon.activeProfile ? <ConnectionStatus compact phase={daemon.phase} /> : (
          <AppSymbol
            name={{ ios: 'plus', android: 'add', web: 'add' }}
            size={14}
            tintColor={theme.text}
          />
        )}
        <Text numberOfLines={1} style={[styles.daemonButtonText, { color: theme.text }]}>
          {daemon.activeProfile?.name ?? 'Add daemon'}
        </Text>
        <AppSymbol
          name={{ ios: 'chevron.down', android: 'keyboard_arrow_down', web: 'keyboard_arrow_down' }}
          size={12}
          tintColor={theme.textTertiary}
        />
      </Pressable>
    </GlassSurface>
  );
}

function TaskListEmpty({
  error,
  notice,
  searching,
  onNewTask,
}: {
  error: unknown;
  notice: ConnectionNotice | null;
  searching: boolean;
  onNewTask: () => void;
}) {
  const theme = useTheme();
  const { phase } = useDaemon();
  // The banner above the list is already explaining the wait.
  if (notice && notice.kind !== 'restored') return null;
  if (phase === 'booting' || phase === 'connecting' || phase === 'reconnecting') {
    return (
      <View style={styles.emptyState}>
        <ActivityIndicator color={theme.textTertiary} />
        <Text style={[styles.emptyTitle, { color: theme.textSecondary }]}>
          {phase === 'reconnecting' ? 'Reconnecting…' : 'Connecting to daemon…'}
        </Text>
      </View>
    );
  }
  if (searching) {
    return (
      <View style={styles.emptyState}>
        <Text style={[styles.emptyTitle, { color: theme.text }]}>No matching tasks</Text>
        <Text style={[styles.emptyBody, { color: theme.textSecondary }]}>Try another title, project, or agent.</Text>
      </View>
    );
  }
  if (error) {
    return (
      <View style={styles.emptyState}>
        <Text style={[styles.emptyTitle, { color: theme.text }]}>Couldn’t load tasks</Text>
        <Text style={[styles.emptyBody, { color: theme.textSecondary }]}>
          {error instanceof Error ? error.message : String(error)}
        </Text>
      </View>
    );
  }
  return (
    <View style={styles.emptyState}>
      <View style={[styles.emptyIcon, { backgroundColor: theme.overlayStrong }]}>
        <AppSymbol
          name={{ ios: 'text.bubble', android: 'chat_bubble', web: 'chat' }}
          size={25}
          tintColor={theme.textTertiary}
        />
      </View>
      <Text style={[styles.emptyTitle, { color: theme.text }]}>No tasks yet</Text>
      <Text style={[styles.emptyBody, { color: theme.textSecondary }]}>
        Start an agent on anything — a bug, a feature, a question about the code.
      </Text>
      {phase === 'connected' && (
        <Pressable
          accessibilityRole="button"
          onPress={onNewTask}
          style={({ pressed }) => [
            styles.emptyAction,
            { backgroundColor: theme.inverse, opacity: pressed ? 0.7 : 1 },
          ]}>
          <Text style={[styles.emptyActionText, { color: theme.onInverse }]}>New task</Text>
        </Pressable>
      )}
    </View>
  );
}

const SectionHeader = memo(function SectionHeader({ title }: { title: string }) {
  const theme = useTheme();
  return (
    <Text style={[styles.sectionTitle, { color: theme.textTertiary }]}>
      {title}
    </Text>
  );
});

// Memoized on primitives: stream commits rebuild the row list several times a
// second, and recycling must never reach into a row whose inputs are equal.
const SessionRow = memo(function SessionRow({
  drawerWidth,
  session,
  projectName,
  lastReplyLabel,
  running,
  selected,
  onDelete,
  onRename,
  onSelect,
}: {
  drawerWidth: number;
  session: AgentSession;
  projectName: string;
  lastReplyLabel: string | null;
  running: boolean;
  selected: boolean;
  onDelete: (session: AgentSession) => void;
  onRename: (session: AgentSession) => void;
  onSelect: (session: AgentSession) => void;
}) {
  const theme = useTheme();
  const rowWidth = Math.max(0, drawerWidth - 24);
  return (
    <TaskRowMenu
      accessibilityLabel={`${displaySessionTitle(session)}, ${providerLabel(session.provider)} in ${projectName}${running ? ', Running' : ''}${lastReplyLabel ? `, Last reply: ${lastReplyLabel}` : ''}`}
      onDelete={() => onDelete(session)}
      onRename={() => onRename(session)}
      onSelect={() => onSelect(session)}
      renderTrigger={(pressed) => (
        <View
          style={[
            styles.sessionRow,
            {
              backgroundColor: pressed
                ? theme.surfaceMuted
                : selected ? theme.backgroundSelected : 'transparent',
              width: rowWidth,
            },
          ]}>
          <View style={styles.sessionHeading}>
            <Text numberOfLines={1} style={[styles.sessionTitle, { color: theme.text }]}>
              {displaySessionTitle(session)}
            </Text>
            {running && (
              <ActivityIndicator
                accessibilityLabel="Running"
                color={theme.textTertiary}
                size="small"
                style={styles.sessionSpinner}
              />
            )}
          </View>
          <View style={styles.sessionMetadata}>
            <ProviderIcon color={theme.textTertiary} provider={session.provider} size={12} />
            <Text
              numberOfLines={1}
              style={[styles.sessionProject, { color: theme.textTertiary }]}>
              {projectName}
            </Text>
            {lastReplyLabel !== null && (
              <Text
                numberOfLines={1}
                style={[styles.sessionTime, { color: theme.textTertiary }]}>
                {lastReplyLabel}
              </Text>
            )}
          </View>
        </View>
      )}
      selected={selected}
      style={[styles.sessionMenu, { width: rowWidth }]}
    />
  );
});

const styles = StyleSheet.create({
  screen: { flex: 1 },
  drawerCardShadow: {
    // Continuous curve matches the display's squircle; borderRadius is set
    // per-device on the element above and shadowOpacity animates with drawer
    // progress.
    borderCurve: 'continuous',
    elevation: 3,
    flex: 1,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 0 },
    shadowRadius: 5,
  },
  drawerCard: {
    // Continuous curve matches the display's squircle; borderRadius is set
    // per-device on the element above.
    borderCurve: 'continuous',
    flex: 1,
    overflow: 'hidden',
  },
  daemonFloat: {
    left: 12,
    position: 'absolute',
    zIndex: 30,
  },
  roundInner: { alignItems: 'center', flex: 1, justifyContent: 'center' },
  searchDockAvoider: {
    left: 0,
    position: 'absolute',
    right: 0,
    zIndex: 20,
  },
  searchDock: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 10,
    paddingHorizontal: Spacing.three,
  },
  searchCapsule: { borderRadius: Radius.pill, flex: 1 },
  searchCapsuleInner: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 9,
    minHeight: 50,
    paddingHorizontal: 16,
  },
  searchInput: { flex: 1, fontSize: 17.5, paddingVertical: 10 },
  composeButton: { borderRadius: Radius.pill, height: 50, width: 50 },
  daemonButton: { borderRadius: Radius.pill, maxWidth: 176 },
  daemonButtonInner: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 6,
    minHeight: DaemonPickerHeight,
    paddingHorizontal: 12,
  },
  daemonButtonText: { flexShrink: 1, fontSize: 14, fontWeight: '600' },
  listContent: { paddingBottom: 96 },
  listContentEmpty: { flexGrow: 1 },
  listHost: { flex: 1 },
  emptyOverlay: {
    bottom: 0,
    left: 0,
    position: 'absolute',
    right: 0,
    top: 0,
  },
  sectionTitle: {
    fontSize: 14,
    fontWeight: '500',
    letterSpacing: 0,
    marginBottom: 4,
    marginHorizontal: 24,
    marginTop: 14,
  },
  emptyState: {
    alignItems: 'center',
    flex: 1,
    justifyContent: 'center',
    minHeight: 360,
    paddingHorizontal: 40,
  },
  emptyIcon: {
    alignItems: 'center',
    borderRadius: 20,
    height: 64,
    justifyContent: 'center',
    marginBottom: 18,
    width: 64,
  },
  emptyTitle: { fontSize: 18, fontWeight: '700', textAlign: 'center' },
  emptyBody: { fontSize: 15, lineHeight: 20, marginTop: 7, maxWidth: 320, textAlign: 'center' },
  emptyAction: {
    borderRadius: Radius.pill,
    justifyContent: 'center',
    marginTop: 18,
    minHeight: 42,
    paddingHorizontal: 18,
  },
  emptyActionText: { fontSize: 15, fontWeight: '700' },
  sessionMenu: { height: 62, marginHorizontal: 12 },
  sessionRow: {
    borderRadius: 10,
    gap: 3,
    height: 62,
    justifyContent: 'center',
    paddingHorizontal: 12,
  },
  sessionHeading: { alignItems: 'center', flexDirection: 'row', gap: 8 },
  sessionMetadata: { alignItems: 'center', flexDirection: 'row', gap: 5 },
  sessionProject: { flex: 1, fontSize: 13.5, lineHeight: 17 },
  sessionTime: { flexShrink: 0, fontSize: 13.5, lineHeight: 17, marginLeft: 3 },
  sessionSpinner: { height: 14, transform: [{ scale: 0.72 }], width: 14 },
  sessionTitle: {
    flex: 1,
    fontSize: 17.5,
    fontWeight: '400',
    letterSpacing: -0.2,
    lineHeight: 22,
  },
});
