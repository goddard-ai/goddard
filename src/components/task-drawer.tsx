import type { AgentSession } from '@waku/client';
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'octane';

import {
  Absolute,
  ActivityIndicator,
  createStore,
  Pressable,
  pushRoute,
  Text,
  TextInput,
  useRoute,
  useSafeAreaInsets,
  useStore,
  View,
} from '@octane-xplat/ui';

import { useWindowSize } from '@octane-xplat/platform';

import { TaskDrawerSurface } from './task-drawer-surface';

import { AppSymbol } from '@/components/app-symbol';
import { BossSecondaryHost } from './boss-secondary';
import { BossSection } from '@/components/boss-roster';
import { ordinarySessions } from '@/lib/shell';
import { DeliverablesSection } from '@/components/deliverables';
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
import { Radius, Spacing } from '@/lib/theme-colors';
import { useBossState, useTaskState } from '@/lib/daemon-queries';
import { useTheme } from '@/lib/use-theme';
import { useUnseenReplyWatermarks } from '@/lib/use-unseen-replies';
import { listNotifications, searchSessionMessages } from '@/lib/daemon-api';
import { useDaemon } from '@/lib/daemon-hooks';
import { useDaemonQuery } from '@/lib/daemon-hooks';
import { showAlert, confirmDestructive } from '@/lib/alert';
import { Haptics } from '@/lib/haptics';
import { sessionIsRunning } from '@/lib/mobile-runtime';
import { useRuntime } from '@/lib/use-runtime';
import {
  displaySessionTitle,
  groupSessions,
  providerLabel,
  relativeSessionTime,
  type SessionGroup,
  type SessionListItem,
} from '@/lib/session-presentation';

import {
  seedUnseenReplyWatermarks,
  sessionHasUnseenReply,
} from '@/lib/unseen-replies';

import { ScrollView } from '@octane-xplat/ui';

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

function _drawerRowKey(row: DrawerListRow): string {
  return row.key;
}

function _drawerRowType(row: DrawerListRow): string {
  return row.kind;
}

interface TaskDrawerContextValue {
  openTaskDrawer: () => void;
  closeTaskDrawer: () => void;
}

const taskDrawerControls = createStore<TaskDrawerContextValue>({
  openTaskDrawer: () => {},
  closeTaskDrawer: () => {},
});

export function TaskDrawerHost({ children }: { children: ReactNode }) {
  const daemon = useDaemon();
  const route = useRoute('root');
  const homeBoss = useBossState();
  const { width } = useWindowSize();
  const [open, setOpen] = useState(false);
  const [listRefreshTick, setListRefreshTick] = useState(0);
  const drawerWidth = Math.max(0, Math.min(360, width - 44));
  const drawerEnabled = daemon.phase === 'booting' || daemon.profiles.length > 0;
  // Full-screen swipe only applies on top-level routes — a deeper screen's
  // own gestures would fight the drawer pan.
  const swipeEnabled = drawerEnabled && (
    !route || route.name === '/' || route.name === 'index' || route.name === 'new-task' || route?.name === 'session/:id'
  );

  const refreshTaskList = useCallback(
    () => setListRefreshTick((tick) => tick + 1),
    [],
  );

  const openTaskDrawer = useCallback(() => {
    if (drawerEnabled) {
      refreshTaskList();
      setOpen(true);
    }
  }, [drawerEnabled, refreshTaskList]);

  const closeTaskDrawer = useCallback(() => setOpen(false), []);
  const controls = useMemo(
    () => ({ openTaskDrawer, closeTaskDrawer }),
    [closeTaskDrawer, openTaskDrawer],
  );

  useEffect(() => {
    taskDrawerControls.set(controls);
    return () => taskDrawerControls.set({ openTaskDrawer: () => {}, closeTaskDrawer: () => {} });
  }, [controls]);

  const selectedSessionId = route?.name === 'session/:id'
    ? (route.params as { id?: string }).id ?? null
    : (!route || route.name === '/' || route.name === 'index') ? homeBoss.data?.sessionId ?? null : null;

  // Close the drawer on navigation and when it is disabled.
  useEffect(() => setOpen(false), [drawerEnabled, route?.name]);

  // The surface stays mounted even while disabled — swapping wrappers would
  // remount the whole route tree whenever the daemon phase settles.
  return (
    <TaskDrawerSurface
      open={open && drawerEnabled}
      swipeEnabled={swipeEnabled}
      drawerWidth={drawerWidth}
      onGestureStart={refreshTaskList}
      onOpenChange={setOpen}
      drawerContent={(
        <TaskDrawerContent
          drawerWidth={drawerWidth}
          open={open}
          refreshTick={listRefreshTick}
          selectedSessionId={selectedSessionId}
          onClose={closeTaskDrawer}
        />
      )}
    >
      {children}
      <BossSecondaryHost />
    </TaskDrawerSurface>
  );
}

export function useTaskDrawer(): TaskDrawerContextValue {
  return useStore(taskDrawerControls);
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
  const [contentMatches, setContentMatches] = useState<Set<string> | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [daemonPickerOpen, setDaemonPickerOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<AgentSession | null>(null);
  const [now, setNow] = useState(Date.now);
  // Share one clock across visible rows, and park it while the drawer or app
  // is hidden so recency labels add no work to the conversation screen.
  useEffect(() => {
    if (!open) {return;}
    const refresh = () => setNow(Date.now());
    refresh();
    const timer = setInterval(refresh, 30_000);
    return () => clearInterval(timer);
  }, [open]);

  // The drawer stays mounted while closed, and stream commits rewrite
  // taskState several times a second. Render the list from a snapshot instead
  // of the live query so those commits cost nothing while hidden — but keep
  // the snapshot fresh anyway: once a burst settles (~300ms quiet), refresh
  // in the background, and a swipe refreshes the instant it starts, so the
  // list is already current as the drawer peels back.
  const [settleTick, setSettleTick] = useState(0);
  useEffect(() => {
    if (open) {return;}
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
  const watermarks = useUnseenReplyWatermarks();
  // New sessions join the tracked set from the full list, not the search
  // filter — a hidden row still needs its watermark seeded.
  useEffect(() => {
    const address = daemon.activeProfile?.address;
    if (address && frame.data) {
      void seedUnseenReplyWatermarks(address, frame.data.sessions);
    }
  }, [daemon.activeProfile?.address, frame.data]);

  const _listExtraData = useMemo(
    () => ({
      runtimes: frame.runtimes,
      now,
      selected: frame.selectedSessionId,
      watermarks,
    }),
    [frame.runtimes, frame.selectedSessionId, now, watermarks],
  );

  // Local fields catch title/project/provider hits instantly; the daemon's
  // message-store search adds tasks whose transcript content matches.
  useEffect(() => {
    const client = daemon.client;
    const query = search.trim();
    if (!client || query.length < 3) {
      setContentMatches(null);
      return;
    }

    let cancelled = false;
    const timer = setTimeout(() => {
      void Promise.all([
        searchSessionMessages(client, query, 'active'),
        searchSessionMessages(client, query, 'archived'),
      ])
        .then(([active, archived]) => {
          if (!cancelled) {
            setContentMatches(new Set(
              [...active, ...archived].map((match) => match.session_id),
            ));
          }
        })
        .catch(() => {
          if (!cancelled) {setContentMatches(null);}
        });
    }, 300);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [daemon.client, search]);

  const visibleSessions = useMemo(() => {
    if (!frame.data) {return [];}
    const query = search.trim().toLocaleLowerCase();
    if (!query) {return frame.data.sessions;}
    const projects = new Map(frame.data.projects.map((project) => [project.id, project]));
    return frame.data.sessions.filter((session) => {
      if (contentMatches?.has(session.id)) {return true;}
      const project = projects.get(session.project_id);
      return [
        displaySessionTitle(session),
        project?.name,
        project?.path,
        providerLabel(session.provider),
        session.model,
      ].some((value) => value?.toLocaleLowerCase().includes(query));
    });
  }, [search, frame.data, contentMatches]);

  const bossState = useBossState();
  const sections = useMemo(
    // Boss-owned sessions — the chat, planning sessions, employee tasks —
    // leave the ordinary groups to the roster rows above them.
    () => frame.data
      ? groupSessions(
          frame.data.projects,
          ordinarySessions(visibleSessions, bossState.data),
        )
      : [],
    [frame.data, visibleSessions, bossState.data],
  );

  const rows = useMemo(() => drawerRows(sections), [sections]);
  const notice = useConnectionNotice();
  const hasRows = rows.length > 0 || Boolean(bossState.data);
  const _emptyShown = !hasRows && !(notice && notice.kind !== 'restored');
  const showNewTask = useCallback(() => {
    onClose();
    pushRoute({ stack: 'root', name: 'new-task', params: {} });
  }, [onClose]);

  const showSession = useCallback((sessionId: string) => {
    onClose();
    if (frame.selectedSessionId === sessionId) {return;}
    pushRoute({ stack: 'root', name: 'session/:id', params: { id: sessionId } });
  }, [frame.selectedSessionId, onClose]);

  const confirmDelete = useCallback((session: AgentSession) => {
    void confirmDestructive(
      `Delete “${displaySessionTitle(session)}”?`,
      'This removes the task and its transcript from the daemon for every device.',
      'Delete',
    ).then((confirmed) => {
      if (!confirmed) {return;}
      void runtime.deleteSession(session.id)
        .then(() => Haptics.notificationAsync('success'))
        .catch((cause) => {
          void showAlert(
            'Couldn’t delete task',
            cause instanceof Error ? cause.message : String(cause),
          );
        });
    });
  }, [runtime.deleteSession]);

  const renameSession = useCallback(
    (session: AgentSession) => setRenameTarget(session),
    [],
  );

  const togglePin = useCallback((session: AgentSession) => {
    void Haptics.selectionAsync();
    void runtime.setSessionPinned(session.id, session.pinned_at == null)
      .catch((cause) => {
        void showAlert(
          'Couldn’t update pin',
          cause instanceof Error ? cause.message : String(cause),
        );
      });
  }, [runtime.setSessionPinned]);

  const toggleArchive = useCallback((session: AgentSession) => {
    const archived = session.archived_at != null;
    const run = () => {
      void runtime.setSessionArchived(session.id, !archived)
        .then(() => Haptics.notificationAsync('success'))
        .catch((cause) => {
          void showAlert(
            archived ? 'Couldn’t unarchive task' : 'Couldn’t archive task',
            cause instanceof Error ? cause.message : String(cause),
          );
        });
    };

    void Haptics.selectionAsync();
    // Desktop confirms before archiving a task whose turn is still running:
    // archiving stops it.
    if (!archived && (frame.runtimes[session.id]?.running ?? sessionIsRunning(session))) {
      void confirmDestructive(
        `Archive “${displaySessionTitle(session)}”?`,
        'Archiving stops the running turn.',
        'Archive',
      ).then((confirmed) => {
        if (confirmed) {run();}
      });
    } else {
      run();
    }
  }, [frame.runtimes, runtime.setSessionArchived]);

  const selectSession = useCallback(
    (session: AgentSession) => showSession(session.id),
    [showSession],
  );

  const renderRow = useCallback((row: DrawerListRow) => {
    if (row.kind === 'header') {return <SectionHeader title={row.title} />;}
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
        unseen={sessionHasUnseenReply(session, watermarks)}
        onDelete={confirmDelete}
        onRename={renameSession}
        onSelect={selectSession}
        onToggleArchive={toggleArchive}
        onTogglePin={togglePin}
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
    toggleArchive,
    togglePin,
    watermarks,
  ]);

  async function refreshTasks() {
    setRefreshing(true);
    try {
      if (daemon.phase === 'connected') {await taskState.refetch();}
      else {await daemon.reconnect();}
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <View style={[styles.screen, { backgroundColor: theme.background }]}>
      <Absolute style={[styles.daemonFloat, { top: insets.top + 8 }]}>
        <DaemonPill onPress={() => setDaemonPickerOpen(true)} />
      </Absolute>
      {daemon.phase === 'connected' && (
        <Absolute style={[styles.inboxFloat, { top: insets.top + 8 }]}>
          <InboxButton
            enabled={open}
            onPress={() => {
              onClose();
              pushRoute({ stack: 'root', name: 'notifications', params: {} });
            }}
          />
        </Absolute>
      )}

      <ScrollView
        refreshing={refreshing}
        onRefresh={() => void refreshTasks()}
        style={styles.listHost}>
        <ConnectionBanner />
        <View style={{ paddingTop: insets.top + DaemonPickerHeight + 20 }}>
          {bossState.data ? (
            <BossSection
              now={now}
              selectedSessionId={frame.selectedSessionId}
              state={bossState.data}
              taskState={frame.data}
              watermarks={watermarks}
              onClose={onClose}
              onSelectSession={showSession}
            />
          ) : null}
          {bossState.data ? (
            <DeliverablesSection
              host={daemon.activeProfile?.name}
              now={now}
              state={bossState.data}
              onClose={onClose}
            />
          ) : null}
          {bossState.data ? <Pressable accessibilityRole="button" onPress={() => { onClose(); pushRoute({ stack: 'root', name: 'plans', params: {} }); }} style={{ paddingHorizontal: 24, paddingVertical: 12 }}>
            <Text style={{ color: theme.textSecondary }}>Brain · Plans</Text>
          </Pressable> : null}
          {rows.map(renderRow)}
        </View>
      </ScrollView>

      {!hasRows && (
        <Absolute
          style={[
            styles.emptyOverlay,
            {
              paddingBottom: insets.bottom + 90,
              paddingTop: insets.top + DaemonPickerHeight + 20,
            },
          ]}>
          <TaskListEmpty
            error={taskState.error}
            notice={notice}
            searching={Boolean(search.trim())}
            onNewTask={showNewTask}
          />
        </Absolute>
      )}

      {(daemon.profiles.length > 0 || daemon.phase === 'booting') && (
        <Absolute
          style={[styles.searchDockAvoider, { bottom: insets.bottom + SearchDockGap }]}>
          <View style={styles.searchDock}>
            <GlassSurface interactive style={styles.searchCapsule}>
              <View style={styles.searchCapsuleInner}>
                <AppSymbol
                  name={{ ios: 'magnifyingglass', android: 'search', web: 'search' }}
                  size={17}
                  tintColor={theme.textSecondary}
                />
                <TextInput
                  placeholder="Search"
                  style={[styles.searchInput, { color: theme.text }]}
                  value={search}
                  onChange={setSearch}
                />
                {search.length > 0 && (
                  <Pressable
                    accessibilityLabel="Clear search"
                    accessibilityRole="button"
                    onPress={() => setSearch('')}>
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
                  onPress={showNewTask}
                  style={styles.roundInner}>
                  <AppSymbol
                    name={{ ios: 'square.and.pencil', android: 'edit_square', web: 'edit' }}
                    size={20}
                    tintColor={theme.text}
                  />
                </Pressable>
              </GlassSurface>
            )}
          </View>
        </Absolute>
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
        onPress={onPress}
        style={styles.daemonButtonInner}>
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

/** Inbox shortcut mirroring the daemon pill — polls for unread threads
 * only while the drawer is open, so a closed drawer costs nothing. */
function InboxButton({
  enabled,
  onPress,
}: {
  enabled: boolean;
  onPress: () => void;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const unread = useDaemonQuery(
    ['notifications', profileId, false],
    () => listNotifications(daemon.client!, false),
    { enabled: enabled && Boolean(daemon.client && daemon.phase === 'connected'), staleTime: 60_000 },
  );

  const hasUnread =
    unread.data?.status === 'changed' &&
    unread.data.threads.some((thread: { unread: boolean }) => thread.unread);

  return (
    <GlassSurface interactive style={styles.inboxButton}>
      <Pressable
        accessibilityHint="Opens the GitHub inbox"
        accessibilityLabel={hasUnread ? 'Notifications, unread' : 'Notifications'}
        accessibilityRole="button"
        onPress={onPress}
        style={styles.inboxButtonInner}>
        <AppSymbol
          name={{ ios: 'bell', android: 'notifications', web: 'notifications' }}
          size={16}
          tintColor={theme.text}
        />
        {hasUnread ? (
          <Absolute style={{ right: 8, top: 8 }}>
            <View style={[styles.inboxBadge, { backgroundColor: theme.accent }]} />
          </Absolute>
        ) : null}
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
  if (notice && notice.kind !== 'restored') {return null;}
  if (phase === 'booting' || phase === 'connecting' || phase === 'reconnecting') {
    return (
      <View style={styles.emptyState}>
        <ActivityIndicator busy />
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
          style={[styles.emptyAction, { backgroundColor: theme.inverse }]}>
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
  unseen,
  onDelete,
  onRename,
  onSelect,
  onToggleArchive,
  onTogglePin,
}: {
  drawerWidth: number;
  session: AgentSession;
  projectName: string;
  lastReplyLabel: string | null;
  running: boolean;
  selected: boolean;
  unseen: boolean;
  onDelete: (session: AgentSession) => void;
  onRename: (session: AgentSession) => void;
  onSelect: (session: AgentSession) => void;
  onToggleArchive: (session: AgentSession) => void;
  onTogglePin: (session: AgentSession) => void;
}) {
  const theme = useTheme();
  const rowWidth = Math.max(0, drawerWidth - 24);
  const pinned = session.pinned_at != null;
  const archived = session.archived_at != null;
  return (
    <TaskRowMenu
      accessibilityLabel={`${displaySessionTitle(session)}, ${providerLabel(session.provider)} in ${projectName}${running ? ', Running' : ''}${unseen ? ', New content' : ''}${lastReplyLabel ? `, Last reply: ${lastReplyLabel}` : ''}`}
      archived={archived}
      pinned={pinned}
      onDelete={() => onDelete(session)}
      onRename={() => onRename(session)}
      onSelect={() => onSelect(session)}
      onToggleArchive={() => onToggleArchive(session)}
      onTogglePin={() => onTogglePin(session)}
      renderTrigger={(pressed) => (
        <View
          style={[
            styles.sessionRow,
            {
              backgroundColor: pressed
                ? theme.surfaceMuted
                : selected ? theme.backgroundSelected : 'transparent',
              opacity: archived ? 0.55 : 1,
              width: rowWidth,
            },
          ]}>
          <View style={styles.sessionHeading}>
            {pinned && (
              <AppSymbol
                name={{ ios: 'pin.fill', android: 'push_pin', web: 'push_pin' }}
                size={11}
                tintColor={theme.textTertiary}
              />
            )}
            <Text numberOfLines={1} style={[styles.sessionTitle, { color: theme.text }]}>
              {displaySessionTitle(session)}
            </Text>
            {running && (
              <ActivityIndicator busy />
            )}
            {!running && unseen && (
              <View
                style={[styles.unseenDot, { backgroundColor: theme.info }]}
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

const styles = {
  screen: { flex: '1 1' },
  drawerCardShadow: {
    // Continuous curve matches the display's squircle; borderRadius is set
    // per-device on the element above and shadowOpacity animates with drawer
    // progress.
    flex: '1 1',
  },
  drawerCard: {
    // Continuous curve matches the display's squircle; borderRadius is set
    // per-device on the element above.
    flex: '1 1',
    overflow: 'hidden',
  },
  daemonFloat: {
    left: 12,


  },
  inboxFloat: {

    right: 12,

  },
  inboxButton: { borderRadius: Radius.pill },
  inboxButtonInner: {
    alignItems: 'center',
    height: DaemonPickerHeight,
    justifyContent: 'center',
    width: DaemonPickerHeight,
  },
  inboxBadge: {
    borderRadius: 4,
    height: 8,

    right: 8,
    top: 8,
    width: 8,
  },
  roundInner: { alignItems: 'center', flex: '1 1', justifyContent: 'center' },
  searchDockAvoider: {
    left: 0,

    right: 0,

  },
  searchDock: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 10,
    paddingHorizontal: Spacing.three,
  },
  searchCapsule: { borderRadius: Radius.pill, flex: '1 1' },
  searchCapsuleInner: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 9,
    minHeight: 50,
    paddingHorizontal: 16,
  },
  searchInput: { flex: '1 1', fontSize: 17.5, paddingVertical: 10 },
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
  listHost: { flex: '1 1' },
  emptyOverlay: {
    bottom: 0,
    left: 0,

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
    flex: '1 1',
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
  emptyBody: { fontSize: 15, lineHeight: '20px', marginTop: 7, maxWidth: 320, textAlign: 'center' },
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
  sessionProject: { flex: '1 1', fontSize: 13.5, lineHeight: '17px' },
  sessionTime: { flexShrink: 0, fontSize: 13.5, lineHeight: '17px', marginLeft: 3 },
  sessionSpinner: { height: 14, transform: [{ scale: 0.72 }], width: 14 },
  unseenDot: { borderRadius: 4, height: 8, width: 8 },
  sessionTitle: {
    flex: '1 1',
    fontSize: 17.5,
    fontWeight: '400',
    letterSpacing: -0.2,
    lineHeight: '22px',
  },
};