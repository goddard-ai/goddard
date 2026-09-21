import * as Clipboard from 'expo-clipboard';
import * as Haptics from 'expo-haptics';
import { MenuView, type MenuAction } from '@expo/ui/community/menu';
import {
  router,
  Stack,
  type NativeStackHeaderItem,
  type NativeStackNavigationOptions,
} from 'expo-router';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Alert,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import Animated from 'react-native-reanimated';

import { ActivitySheetHost } from '@/components/activity-sheet';
import { AppSymbol } from '@/components/app-symbol';
import { GlassSurface } from '@/components/glass-surface';
import { ConnectionBanner } from '@/components/connection-banner';
import { MobileComposer } from '@/components/mobile-composer';
import { PROVIDER_MENU_ICONS } from '@/components/provider-menu-icons';
import { RenameDialog } from '@/components/rename-dialog';
import { ModelSheet, modelDisplayName, type ModelSelection } from '@/components/session-option-sheets';
import {
  HeaderAction,
  HeaderActionGroup,
  HeaderMenuTrigger,
  HeaderTitle,
  nativeHeaderButtons,
  navigateBack,
  useScreenHeaderInset,
  type HeaderActionSpec,
} from '@/components/screen-header';
import {
  TaskSurfaceSheet,
  type TaskSurface,
} from '@/components/task-surface-sheet';
import { useTaskDrawer } from '@/components/task-drawer';
import {
  TranscriptList,
  type TranscriptDevSample,
  type TranscriptListHandle,
} from '@/components/transcript-list';
import { SessionEmpty } from '@/components/transcript-rows';
import { useProviderModels, useSession, useTaskState } from '@/hooks/use-daemon-data';
import { useKeyboardPadding } from '@/hooks/use-keyboard-padding';
import { useTheme } from '@/hooks/use-theme';
import { Radius } from '@/constants/theme';
import { useDaemon } from '@/lib/daemon-context';
import { listSessionTurnRefs } from '@/lib/daemon-api';
import { sessionBusy, sessionCwd } from '@/lib/mobile-runtime';
import { useRuntime } from '@/lib/runtime-context';
import {
  displaySessionTitle,
  forkableTurnIds,
  rewindableTurnIds,
} from '@/lib/session-presentation';

const SURFACE_MENU_COMMANDS = [
  { id: 'terminal', title: 'Terminal', symbol: 'terminal' },
  { id: 'files', title: 'Files', symbol: 'folder' },
  { id: 'review', title: 'Review', symbol: 'doc.text.magnifyingglass' },
  { id: 'git', title: 'Git', symbol: 'arrow.triangle.branch' },
] as const;

const TASK_MENU_COMMANDS = [
  { id: 'rename', title: 'Rename task', symbol: 'pencil', destructive: false },
  {
    id: 'copy-last-response',
    title: 'Copy last response',
    symbol: 'doc.on.doc',
    destructive: false,
  },
  {
    id: 'find',
    title: 'Find in transcript',
    symbol: 'magnifyingglass',
    destructive: false,
  },
  {
    id: 'compact',
    title: 'Compact context',
    symbol: 'rectangle.compress.vertical',
    destructive: false,
  },
  {
    id: 'rollback',
    title: 'Roll back last turn',
    symbol: 'arrow.uturn.backward',
    destructive: false,
  },
  {
    id: 'reload',
    title: 'Reload transcript',
    symbol: 'arrow.clockwise',
    destructive: false,
  },
  { id: 'delete', title: 'Delete task', symbol: 'trash', destructive: true },
] as const;

export function SessionView({
  sessionId,
  devPrompt,
}: {
  sessionId: string | undefined;
  /** Dev-only: auto-submit this prompt through the composer path once the
   * session loads — lets headless rigs exercise the exact user flow. */
  devPrompt?: string;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const runtime = useRuntime();
  const { openTaskDrawer } = useTaskDrawer();
  const query = useSession(sessionId);
  const session = query.data;
  const modelProbe = useProviderModels(session?.provider ?? null);
  const models = modelProbe.data?.models;
  const modelLabel = modelDisplayName(
    models,
    session?.model ?? models?.find((item) => item.is_default)?.id ?? models?.[0]?.id ?? null,
  );
  const modelIcon = session ? PROVIDER_MENU_ICONS[session.provider] : undefined;
  const [taskSurface, setTaskSurface] = useState<TaskSurface | null>(null);
  const [taskSurfaceOpen, setTaskSurfaceOpen] = useState(false);
  const [modelSheetOpen, setModelSheetOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [turnRefs, setTurnRefs] = useState<ReadonlySet<number> | null>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const [findIndex, setFindIndex] = useState(0);
  const [mountedTranscriptSessionId, setMountedTranscriptSessionId] = useState<string | null>(null);
  const running = Boolean(session && sessionBusy(session));
  const listRef = useRef<TranscriptListHandle>(null);
  const lastFindQuery = useRef('');
  const headerInset = useScreenHeaderInset();
  const sessionRef = useRef(session);
  const queryRef = useRef(query);
  const runtimeRef = useRef(runtime);
  sessionRef.current = session;
  queryRef.current = query;
  runtimeRef.current = runtime;

  useEffect(() => {
    if (!session || daemon.phase !== 'connected') return;
    void runtime.attachSession(session).catch(() => {});
    // Re-runs when the session starts working (another client may have
    // started the runtime after this screen mounted).
  }, [daemon.phase, runtime.attachSession, session?.id, running]);

  // Transient task chrome belongs to one session. A route reuse must not show
  // the previous task's file, review, or terminal surface.
  useEffect(() => {
    setTaskSurfaceOpen(false);
    setTaskSurface(null);
    setModelSheetOpen(false);
    setFindOpen(false);
    setFindQuery('');
    setFindIndex(0);
    lastFindQuery.current = '';
  }, [session?.id]);

  // Route/header/composer get the first commit by themselves. Transcript row
  // construction includes pipeline building and Markdown expansion, so doing
  // it in the route's first render delays the entire native screen appearing.
  // Two frames guarantee the lightweight task shell has painted before that
  // synchronous presentation work begins.
  useEffect(() => {
    const sessionId = session?.id;
    if (!sessionId) {
      setMountedTranscriptSessionId(null);
      return;
    }
    let mountFrame = 0;
    const shellFrame = requestAnimationFrame(() => {
      mountFrame = requestAnimationFrame(() => setMountedTranscriptSessionId(sessionId));
    });
    return () => {
      cancelAnimationFrame(shellFrame);
      if (mountFrame) cancelAnimationFrame(mountFrame);
    };
  }, [session?.id]);

  const probe = useDevProbe(Boolean(devPrompt));

  // Dev-only auto-submit: same path as the composer's send button.
  const devPromptSent = useRef(false);
  useEffect(() => {
    if (!devPrompt || devPromptSent.current) return;
    if (!session || query.isPlaceholderData) {
      probe.setStatus('waiting for session');
      return;
    }
    if (daemon.phase !== 'connected') {
      probe.setStatus(`daemon ${daemon.phase}`);
      return;
    }
    if (sessionBusy(session)) {
      probe.setStatus('session busy');
      return;
    }
    devPromptSent.current = true;
    probe.setStatus('submitting');
    listRef.current?.followNextGrowth();
    runtime.sendPrompt(session, devPrompt)
      .then(() => probe.setStatus('submitted'))
      .catch((cause) => probe.setStatus(`failed ${String(cause).slice(0, 120)}`));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [daemon.phase, devPrompt, query.isPlaceholderData, session]);

  const openTaskSurface = useCallback((surface: TaskSurface) => {
    setTaskSurface(surface);
    setTaskSurfaceOpen(true);
  }, []);

  const applyModelSelection = useCallback((selection: ModelSelection) => {
    const current = sessionRef.current;
    if (!current) return;
    void runtimeRef.current.updateSessionOptions(current.id, selection).catch((cause) => {
      Alert.alert('Couldn’t change model', cause instanceof Error ? cause.message : String(cause));
    });
  }, []);

  const copyLastResponse = useCallback(async () => {
    const lastAssistant = [...(sessionRef.current?.messages ?? [])]
      .reverse()
      .find((message) => message.role === 'assistant' && message.content.trim());
    if (!lastAssistant) return;
    await Clipboard.setStringAsync(lastAssistant.content);
    await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
  }, []);

  const confirmDelete = useCallback(() => {
    const current = sessionRef.current;
    if (!current) return;
    Alert.alert(
      `Delete “${displaySessionTitle(current)}”?`,
      'This removes the task and its transcript from the daemon for every device.',
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Delete',
          style: 'destructive',
          onPress: () => {
            void runtimeRef.current.deleteSession(current.id)
              .then(() => {
                void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
                navigateBack();
              })
              .catch((cause) => {
                Alert.alert('Couldn’t delete task', cause instanceof Error ? cause.message : String(cause));
              });
          },
        },
      ],
    );
  }, []);

  const compactSession = useCallback(() => {
    const current = sessionRef.current;
    if (!current) return;
    void runtimeRef.current.compactSession(current.id).catch((cause) => {
      Alert.alert('Couldn’t compact context', cause instanceof Error ? cause.message : String(cause));
    });
  }, []);

  const confirmRollback = useCallback(() => {
    const current = sessionRef.current;
    if (!current) return;
    Alert.alert(
      `Roll back the last turn in “${displaySessionTitle(current)}”?`,
      'This rewinds the conversation and workspace to before the previous turn.',
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Roll back',
          style: 'destructive',
          onPress: () => {
            void runtimeRef.current.rollbackSession(current.id, 1)
              .then(() => Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success))
              .catch((cause) => {
                Alert.alert('Couldn’t roll back', cause instanceof Error ? cause.message : String(cause));
              });
          },
        },
      ],
    );
  }, []);

  const handleTaskMenuCommand = useCallback(
    (command: string) => {
      if (command === 'terminal' || command === 'files' || command === 'review' || command === 'git') {
        openTaskSurface(command);
      } else if (command === 'model') {
        setModelSheetOpen(true);
      } else if (command === 'rename') {
        setRenaming(true);
      } else if (command === 'find') {
        setFindOpen(true);
      } else if (command === 'copy-last-response') {
        void copyLastResponse();
      } else if (command === 'compact') {
        compactSession();
      } else if (command === 'rollback') {
        confirmRollback();
      } else if (command === 'reload') {
        void queryRef.current.refetch();
      } else if (command === 'delete') {
        confirmDelete();
      }
    },
    [compactSession, confirmDelete, confirmRollback, copyLastResponse, openTaskSurface],
  );

  const taskState = useTaskState().data;
  const project = taskState?.projects.find((item) => item.id === session?.project_id);

  // Checkpoint turn counts drive the rewind affordance — one workspace op
  // per settle, mirroring desktop's checkpoint ref cache.
  const settledTurns = session?.turns.filter((turn) => turn.status !== 'running').length ?? 0;
  const workspaceCwd = session && project ? sessionCwd(session, project) : null;
  useEffect(() => {
    const client = daemon.client;
    const current = sessionRef.current;
    if (!client || !current || !workspaceCwd || daemon.phase !== 'connected') {
      setTurnRefs(null);
      return;
    }
    let cancelled = false;
    void listSessionTurnRefs(client, workspaceCwd, current.id)
      .then((refs) => { if (!cancelled) setTurnRefs(refs); })
      .catch(() => { if (!cancelled) setTurnRefs(null); });
    return () => { cancelled = true; };
  }, [daemon.client, daemon.phase, session?.id, settledTurns, workspaceCwd]);

  // Set contents change only when eligibility flips; a fresh identity per
  // stream commit would re-render every memoized transcript row.
  const rewindTurns = useStableSet(useMemo(
    () => (session ? rewindableTurnIds(session, turnRefs) : new Set<string>()),
    [session, turnRefs],
  ));
  const forkTurns = useStableSet(useMemo(
    () => (session ? forkableTurnIds(session) : new Set<string>()),
    [session],
  ));

  const confirmRewind = useCallback((turnId: string) => {
    const current = sessionRef.current;
    const turn = current?.turns.find((item) => item.id === turnId);
    if (!current || !turn) return;
    const dropped = current.turns.length - turn.turn_count + 1;
    Alert.alert(
      `Rewind “${displaySessionTitle(current)}” to here?`,
      `This removes ${dropped === 1 ? 'this turn' : `${dropped} turns`} and restores the files and conversation to before this message.`,
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Rewind',
          style: 'destructive',
          onPress: () => {
            void runtimeRef.current.rewindSessionToMessage(current.id, turn.turn_count)
              .then(({ warning }) => {
                void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
                if (warning) Alert.alert('Rewind cleanup warning', warning);
              })
              .catch((cause) => {
                Alert.alert('Couldn’t rewind', cause instanceof Error ? cause.message : String(cause));
              });
          },
        },
      ],
    );
  }, []);

  const forkTurn = useCallback((turnId: string) => {
    const current = sessionRef.current;
    const turn = current?.turns.find((item) => item.id === turnId);
    if (!current || !turn) return;
    void runtimeRef.current.forkSessionFromResponse(current.id, turn.turn_count)
      .then(({ session: forked, warning }) => {
        void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
        router.push({ pathname: '/session/[id]', params: { id: forked.id } });
        if (warning) Alert.alert('Fork checkpoint warning', warning);
      })
      .catch((cause) => {
        Alert.alert('Couldn’t fork task', cause instanceof Error ? cause.message : String(cause));
      });
  }, []);

  // Transcript find bar: matches resolve to expanded row keys; the list's
  // revealRow mounts windowed history and opens folds as needed.
  const findMatches = useMemo(() => {
    const needle = findQuery.trim().toLowerCase();
    if (!findOpen || needle.length < 2 || !session) return [] as Array<{ key: string; turnId: string | null }>;
    const matches: Array<{ key: string; turnId: string | null }> = [];
    for (const message of session.messages) {
      const text = (message.display_content ?? message.content).toLowerCase();
      if (message.hidden || !text.includes(needle)) continue;
      if (message.role === 'user') matches.push({ key: `user:${message.id}`, turnId: message.turn_id });
      else if (message.role === 'system') matches.push({ key: `system:${message.id}`, turnId: message.turn_id });
      else if (message.role === 'assistant') matches.push({ key: `md:${message.id}.0`, turnId: message.turn_id });
    }
    return matches;
  }, [findOpen, findQuery, session]);

  const revealMatch = useCallback((index: number, matches: Array<{ key: string; turnId: string | null }>) => {
    const match = matches[index];
    if (!match) return;
    listRef.current?.revealRow(match.key, match.turnId);
  }, []);

  // A new query jumps to the newest match — the convention for find-in-page.
  // Streaming updates rebuild the match list without re-yanking the reader.
  useEffect(() => {
    if (findQuery === lastFindQuery.current) return;
    lastFindQuery.current = findQuery;
    if (!findMatches.length) return;
    const index = findMatches.length - 1;
    setFindIndex(index);
    revealMatch(index, findMatches);
  }, [findMatches, findQuery, revealMatch]);

  const stepFind = useCallback((delta: number) => {
    if (!findMatches.length) return;
    const next = (findIndex + delta + findMatches.length) % findMatches.length;
    setFindIndex(next);
    revealMatch(next, findMatches);
  }, [findIndex, findMatches, revealMatch]);

  const closeFind = useCallback(() => {
    setFindOpen(false);
    setFindQuery('');
    setFindIndex(0);
    lastFindQuery.current = '';
  }, []);
  const subtitleParts = [project?.name, daemon.activeProfile?.name].filter(Boolean);
  const title = session ? displaySessionTitle(session) : 'Task';
  // The bar's subtitle doubles as the quiet link indicator, the way messaging
  // apps do it; the banner below only steps in when the outage persists.
  const linkSubtitle = daemon.phase === 'reconnecting'
    ? daemon.outage?.interrupted ? 'Reconnecting…' : 'Connecting…'
    : daemon.phase === 'connecting' || daemon.phase === 'booting'
      ? 'Connecting…'
      : daemon.phase === 'error' ? 'Not connected' : null;
  const subtitle = linkSubtitle ?? (subtitleParts.length ? subtitleParts.join(' · ') : null);
  const hasSession = Boolean(session);
  const transcriptMounted = Boolean(
    session && mountedTranscriptSessionId === session.id,
  );
  const taskMenuActions = useMemo<MenuAction[]>(
    () => [
      ...SURFACE_MENU_COMMANDS.map((item) => ({
        id: item.id,
        title: item.title,
        image: item.symbol,
      })),
      {
        id: 'task-actions',
        title: '',
        displayInline: true,
        subactions: [
          { id: 'model', title: modelLabel, image: modelIcon, imageColor: theme.text },
          ...TASK_MENU_COMMANDS.map((item) => ({
            id: item.id,
            title: item.title,
            image: item.symbol,
            attributes: item.destructive ? { destructive: true } : undefined,
          })),
        ],
      },
    ],
    [modelIcon, modelLabel, theme.text],
  );

  // The chrome lives in the native navigation bar, so it stays put while the
  // page slides under a swipe-back. Keyed on the strings, not the session, so
  // streaming updates never touch the bar.
  const headerOptions = useMemo<NativeStackNavigationOptions>(() => {
    const drawer: HeaderActionSpec = {
      icon: { ios: 'sidebar.left', android: 'menu', web: 'menu' },
      label: 'Task history',
      onPress: openTaskDrawer,
    };
    const newTask: HeaderActionSpec = {
      icon: { ios: 'square.and.pencil', android: 'edit_square', web: 'edit' },
      label: 'New task',
      onPress: () => router.dismissTo('/'),
    };
    const nativeItems: NativeStackHeaderItem[] = hasSession
      ? [
          ...nativeHeaderButtons([newTask]),
          {
            type: 'menu',
            label: 'Task options',
            accessibilityLabel: 'Task options',
            icon: { type: 'sfSymbol', name: 'ellipsis' },
            menu: {
              title,
              // These are commands, not a single-selection picker.
              multiselectable: true,
              items: [
                ...SURFACE_MENU_COMMANDS.map((item) => ({
                  type: 'action' as const,
                  label: item.title,
                  icon: { type: 'sfSymbol' as const, name: item.symbol },
                  onPress: () => handleTaskMenuCommand(item.id),
                })),
                {
                  type: 'submenu',
                  label: '',
                  inline: true,
                  multiselectable: true,
                  items: [
                    {
                      type: 'action',
                      label: modelLabel,
                      icon: modelIcon ? { type: 'image', source: modelIcon } : undefined,
                      onPress: () => handleTaskMenuCommand('model'),
                    },
                    ...TASK_MENU_COMMANDS.map((item) => ({
                      type: 'action' as const,
                      label: item.title,
                      icon: { type: 'sfSymbol' as const, name: item.symbol },
                      destructive: item.destructive,
                      onPress: () => handleTaskMenuCommand(item.id),
                    })),
                  ],
                },
              ],
            },
          },
        ]
      : [];
    return {
      // iOS 26 scroll edge effects read as the transcript fading out once it
      // becomes scrollable — suppress them at the Screen level too, so the
      // treatment holds even where the ScrollViewMarker class is absent
      // (older bundled screens in Expo Go).
      scrollEdgeEffects: {
        bottom: 'hidden',
        left: 'hidden',
        right: 'hidden',
        top: 'hidden',
      },
      headerTitle: Platform.OS === 'ios'
        ? ''
        : () => <HeaderTitle subtitle={subtitle} title={title} />,
      headerTitleAlign: 'left',
      headerRight: hasSession
        ? () => (
            <HeaderActionGroup>
              <HeaderAction {...newTask} />
              <MenuView
                actions={taskMenuActions}
                title={title}
                onPressAction={({ nativeEvent }) =>
                  handleTaskMenuCommand(nativeEvent.event)
                }
              >
                <HeaderMenuTrigger
                  icon={{
                    ios: 'ellipsis',
                    android: 'more_horiz',
                    web: 'more_horiz',
                  }}
                  label="Task options"
                />
              </MenuView>
            </HeaderActionGroup>
          )
        : undefined,
      unstable_headerRightItems: nativeItems.length
        ? () => nativeItems
        : undefined,
      unstable_headerLeftItems: Platform.OS === 'ios'
        ? () => [
            ...nativeHeaderButtons([drawer]),
            {
              type: 'custom' as const,
              element: <HeaderTitle subtitle={subtitle} title={title} />,
              hidesSharedBackground: true,
            },
          ]
        : undefined,
    };
  }, [handleTaskMenuCommand, hasSession, modelIcon, modelLabel, openTaskDrawer, subtitle, taskMenuActions, title]);

  const keyboardPadding = useKeyboardPadding();

  return (
    <Animated.View
      style={[styles.screen, { backgroundColor: theme.background }, keyboardPadding]}>
      <Stack.Screen options={headerOptions} />
      <View style={styles.body}>
        {session && transcriptMounted ? (
          <ActivitySheetHost key={`activity-sheet:${session.id}`} session={session}>
            <TranscriptList
              forkTurns={forkTurns}
              headerInset={headerInset}
              hydrated={!query.isPlaceholderData}
              ref={listRef}
              rewindTurns={rewindTurns}
              running={running}
              session={session}
              onDevSample={devPrompt ? probe.sample : undefined}
              onForkTurn={forkTurn}
              onRewindTurn={confirmRewind}
            />
          </ActivitySheetHost>
        ) : (
          <View style={styles.placeholder}>
            <SessionEmpty
              error={query.error}
              loading={Boolean(session) || query.isPending}
              missing={query.data === null}
            />
          </View>
        )}
        {findOpen && session && (
          <GlassSurface
            fallbackColor={theme.surface}
            interactive
            style={[styles.findBar, { top: headerInset + 8 }]}>
            <View style={styles.findBarInner}>
              <AppSymbol
                name={{ ios: 'magnifyingglass', android: 'search', web: 'search' }}
                size={14}
                tintColor={theme.textTertiary}
              />
              <TextInput
                autoCapitalize="none"
                autoCorrect={false}
                autoFocus
                onChangeText={setFindQuery}
                placeholder="Find in transcript"
                placeholderTextColor={theme.textGhost}
                returnKeyType="search"
                style={[styles.findInput, { color: theme.text }]}
                value={findQuery}
              />
              <Text style={[styles.findCount, { color: theme.textTertiary }]}>
                {findMatches.length
                  ? `${Math.min(findIndex + 1, findMatches.length)} of ${findMatches.length}`
                  : 'No matches'}
              </Text>
              <Pressable
                accessibilityLabel="Previous match"
                accessibilityRole="button"
                hitSlop={8}
                onPress={() => stepFind(-1)}
                style={({ pressed }) => ({ opacity: pressed ? 0.5 : 1 })}>
                <AppSymbol
                  name={{ ios: 'chevron.up', android: 'keyboard_arrow_up', web: 'keyboard_arrow_up' }}
                  size={16}
                  tintColor={theme.textSecondary}
                />
              </Pressable>
              <Pressable
                accessibilityLabel="Next match"
                accessibilityRole="button"
                hitSlop={8}
                onPress={() => stepFind(1)}
                style={({ pressed }) => ({ opacity: pressed ? 0.5 : 1 })}>
                <AppSymbol
                  name={{ ios: 'chevron.down', android: 'keyboard_arrow_down', web: 'keyboard_arrow_down' }}
                  size={16}
                  tintColor={theme.textSecondary}
                />
              </Pressable>
              <Pressable
                accessibilityLabel="Close find"
                accessibilityRole="button"
                hitSlop={8}
                onPress={closeFind}
                style={({ pressed }) => ({ opacity: pressed ? 0.5 : 1 })}>
                <AppSymbol
                  name={{ ios: 'xmark', android: 'close', web: 'close' }}
                  size={15}
                  tintColor={theme.textSecondary}
                />
              </Pressable>
            </View>
          </GlassSurface>
        )}
        <View pointerEvents="box-none" style={[styles.linkBanner, { top: headerInset + 8 }]}>
          <ConnectionBanner floating />
        </View>
        {Boolean(devPrompt && probe.text) && (
          <View pointerEvents="none" style={[styles.devBadge, { top: headerInset + 8 }]}>
            <Text style={styles.devBadgeText}>{probe.text}</Text>
          </View>
        )}
      </View>
      {session && (
        <MobileComposer
          key={`composer:${session.id}`}
          session={session}
          onSubmitted={() => listRef.current?.followNextGrowth()}
        />
      )}

      {session && (
        <TaskSurfaceSheet
          key={`task-surface:${session.id}`}
          onDismiss={() => {
            setTaskSurfaceOpen(false);
          }}
          project={project}
          session={session}
          surface={taskSurface}
          visible={taskSurfaceOpen}
        />
      )}
      {session && (
        <ModelSheet
          key={`model-sheet:${session.id}`}
          model={session.model ?? null}
          onApply={applyModelSelection}
          onDismiss={() => setModelSheetOpen(false)}
          provider={session.provider}
          visible={modelSheetOpen}
        />
      )}
      {session && (
        <RenameDialog
          initialValue={displaySessionTitle(session)}
          onDismiss={() => setRenaming(false)}
          onSubmit={(title) => runtime.renameSession(session.id, title)}
          visible={renaming}
        />
      )}
    </Animated.View>
  );
}

/** Same-contents-in, same-identity-out, so memoized children see a stable
 * prop until membership actually changes. */
function useStableSet(current: ReadonlySet<string>): ReadonlySet<string> {
  const ref = useRef(current);
  const previous = ref.current;
  const same = previous.size === current.size
    && [...current].every((value) => previous.has(value));
  if (!same) ref.current = current;
  return ref.current;
}

/**
 * Dev-only motion probe. Screen recorders capture ~8 real fps and cannot
 * tell a seated stream from a bouncing one; the scroll events can. While a
 * stream is followed the native offset must read 0 — `drift` is the largest
 * untouched offset seen in the last second, and `grow` counts content-size
 * commits, so "drift 0" across a fast stream is the pass condition.
 */
function useDevProbe(enabled: boolean) {
  const [status, setStatus] = useState('');
  const [text, setText] = useState('');
  const samples = useRef<TranscriptDevSample[]>([]);
  const rates = useRef<Array<{ at: number; count: number; flips: number }>>([]);
  const growths = useRef(0);
  const lastHeight = useRef(0);

  const sample = useCallback((next: TranscriptDevSample) => {
    if (next.contentHeight !== lastHeight.current) {
      if (lastHeight.current > 0) growths.current += 1;
      lastHeight.current = next.contentHeight;
    }
    samples.current.push(next);
  }, []);

  useEffect(() => {
    if (!enabled) return;
    const timer = setInterval(() => {
      const now = Date.now();
      const recent = samples.current.filter((item) => now - item.at < 1_000);
      samples.current = recent;
      let drift = 0;
      for (const item of recent) {
        if (!item.touching) drift = Math.max(drift, item.offset);
      }
      const latest = recent.at(-1);
      const scrolls = recent.filter((item) => item.source === 'scroll');
      // Direction reversals between consecutive scroll samples: an animation
      // is monotonic, a compensation fight alternates.
      let flips = 0;
      for (let ix = 2; ix < scrolls.length; ix += 1) {
        const a = scrolls[ix - 1]!.offset - scrolls[ix - 2]!.offset;
        const b = scrolls[ix]!.offset - scrolls[ix - 1]!.offset;
        if (a * b < 0) flips += 1;
      }
      // Peaks over the last five seconds survive the snapshot latency of an
      // external reader.
      rates.current = [
        ...rates.current.filter((item) => now - item.at < 5_000),
        { at: now, count: scrolls.length, flips },
      ];
      const peak = Math.max(...rates.current.map((item) => item.count));
      const peakFlips = Math.max(...rates.current.map((item) => item.flips));
      setText(
        `dev: ${status} · scr ${scrolls.length}/s (peak ${peak}, flips ${peakFlips}) · size ${recent.length - scrolls.length}/s · off ${latest ? latest.offset.toFixed(0) : '–'} · drift ${drift.toFixed(0)} · grow ${growths.current} · touch ${latest ? (latest.touching ? 1 : 0) : '–'}`,
      );
    }, 500);
    return () => clearInterval(timer);
  }, [enabled, status]);

  return { sample, setStatus, text: enabled ? text : '' };
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  body: { flex: 1 },
  placeholder: { alignItems: 'center', flex: 1, justifyContent: 'center', paddingHorizontal: 32 },
  linkBanner: { left: 12, position: 'absolute', right: 12, zIndex: 10 },
  findBar: {
    borderRadius: Radius.pill,
    left: 12,
    position: 'absolute',
    right: 12,
    zIndex: 10,
  },
  findBarInner: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 8,
    minHeight: 40,
    paddingHorizontal: 12,
  },
  findCount: { fontSize: 12, fontVariant: ['tabular-nums'] },
  findInput: { flex: 1, fontSize: 15, paddingVertical: 6 },
  devBadge: {
    backgroundColor: 'rgba(0,0,0,0.75)',
    borderRadius: 6,
    left: 12,
    padding: 6,
    position: 'absolute',
  },
  devBadgeText: { color: '#fff', fontSize: 12 },
});
