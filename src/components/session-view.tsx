import { atomVisibleText } from '@waku/client/transcript-presentation';
import { bossEmployeeLifecycle } from '@waku/client';
import { useCallback, useEffect, useMemo, useRef, useState } from 'octane';
import {
  Absolute,
  DropdownMenu,
  Pressable,
  pushRoute,
  Text,
  TextInput,
  View,
} from '@octane-xplat/ui';

import { ActivitySheetHost } from '@/components/activity-sheet';
import { AppSymbol } from '@/components/app-symbol';
import {
  BossAssignmentSheet,
  BossBlockerNotice,
  BossCapacitySheet,
  EmployeeExpiredCard,
  EmployeeStatusCard,
} from '@/components/boss-roster';

import { GlassSurface } from '@/components/glass-surface';
import { ConnectionBanner } from '@/components/connection-banner';
import { MobileComposer, PermissionPanel } from '@/components/mobile-composer';
import { RenameDialog } from '@/components/rename-dialog';
import { ModelSheet, modelDisplayName, type ModelSelection } from '@/components/session-option-sheets';
import {
  navigateBack,
  ScreenHeaderBackdrop,
  useScreenHeaderInset,
} from '@/components/screen-header';

import {
  TaskSurfaceSheet,
  type TaskSurface,
} from '@/components/task-surface-sheet';

import { useTaskDrawer } from '@/components/task-drawer';
import {
  TranscriptList,
  type TranscriptListHandle,
} from '@/components/transcript-list';

import { SessionEmpty } from '@/components/transcript-rows';
import { useBossState, useProviderModels, useSession, useTaskState } from '@/lib/daemon-queries';
import { bossEmployeeForSession, bossQueueRanks } from '@/lib/boss';
import { useKeyboardPadding } from '@/lib/use-keyboard-padding';
import { useTheme } from '@/lib/use-theme';
import { useHasUnseenReplies } from '@/lib/use-unseen-replies';
import { Radius } from '@/lib/theme-colors';
import { useDaemon } from '@/lib/daemon-hooks';
import { listSessionTurnRefs } from '@/lib/daemon-api';
import { sessionBusy, sessionCwd } from '@/lib/mobile-runtime';
import { useRuntime } from '@/lib/use-runtime';
import { showAlert, confirmDestructive } from '@/lib/alert';
import { Haptics } from '@/lib/haptics';
import { setStringAsync as copyToClipboard } from '@/lib/clipboard';
import {
  displaySessionTitle,
  forkableTurnIds,
  rewindableTurnIds,
} from '@/lib/session-presentation';

import { markSessionRepliesSeen } from '@/lib/unseen-replies';

const SURFACE_MENU_COMMANDS = [
  { id: 'terminal', title: 'Terminal', symbol: 'terminal' },
  { id: 'files', title: 'Files', symbol: 'folder' },
  { id: 'review', title: 'Review', symbol: 'doc.text.magnifyingglass' },
  { id: 'git', title: 'Git', symbol: 'arrow.triangle.branch' },
  { id: 'queue', title: 'Review queue', symbol: 'checkmark.circle' },
] as const;

// Employee lifecycle commands — rename, compact, rollback, delete — belong
// to the boss; the human supervises through the boss chat instead.
const EMPLOYEE_HIDDEN_COMMANDS = new Set(['rename', 'compact', 'rollback', 'delete']);

const TASK_MENU_COMMANDS = [
  { id: 'rename', title: 'Rename task', symbol: 'pencil' },
  { id: 'copy-last-response', title: 'Copy last response', symbol: 'doc.on.doc' },
  { id: 'find', title: 'Find in transcript', symbol: 'magnifyingglass' },
  { id: 'compact', title: 'Compact context', symbol: 'rectangle.compress.vertical' },
  { id: 'rollback', title: 'Roll back last turn', symbol: 'arrow.uturn.backward' },
  { id: 'reload', title: 'Reload transcript', symbol: 'arrow.clockwise' },
  { id: 'delete', title: 'Delete task', symbol: 'trash' },
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

  const [taskSurface, setTaskSurface] = useState<TaskSurface | null>(null);
  const [taskSurfaceOpen, setTaskSurfaceOpen] = useState(false);
  const [modelSheetOpen, setModelSheetOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [turnRefs, setTurnRefs] = useState<ReadonlySet<number> | null>(null);
  const [assignmentOpen, setAssignmentOpen] = useState(false);
  const [capacityOpen, setCapacityOpen] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const [findIndex, setFindIndex] = useState(0);
  const [mountedTranscriptSessionId, setMountedTranscriptSessionId] = useState<string | null>(null);
  const running = Boolean(session && sessionBusy(session));
  const listRef = useRef<TranscriptListHandle | null>(null);
  const lastFindQuery = useRef('');
  const headerInset = useScreenHeaderInset();
  const sessionRef = useRef(session);
  const queryRef = useRef(query);
  const runtimeRef = useRef(runtime);
  sessionRef.current = session;
  queryRef.current = query;
  runtimeRef.current = runtime;

  useEffect(() => {
    if (!session || daemon.phase !== 'connected') {return;}
    void runtime.attachSession(session).catch(() => {});
    // Re-runs when the session starts working (another client may have
    // started the runtime after this screen mounted).
  }, [daemon.phase, runtime.attachSession, session?.id, running]);

  // The drawer's new-content dot tracks `last_reply_at` against the newest
  // stamp the user has had on screen; keep that watermark current for as
  // long as this transcript is showing.
  const daemonAddress = daemon.activeProfile?.address;
  useEffect(() => {
    if (session && daemonAddress) {
      void markSessionRepliesSeen(daemonAddress, session);
    }
  }, [daemonAddress, session?.id, session?.last_reply_at]);

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
  useEffect(() => {
    const id = session?.id;
    if (!id) {
      setMountedTranscriptSessionId(null);
      return;
    }

    const timer = setTimeout(() => setMountedTranscriptSessionId(id), 32);
    return () => clearTimeout(timer);
  }, [session?.id]);

  // Dev-only auto-submit: same path as the composer's send button.
  const devPromptSent = useRef(false);
  useEffect(() => {
    if (!devPrompt || devPromptSent.current) {return;}
    if (!session || query.isPending) {return;}
    if (daemon.phase !== 'connected' || sessionBusy(session)) {return;}
    devPromptSent.current = true;
    listRef.current?.followNextGrowth();
    void runtime.sendPrompt(session, devPrompt).catch(() => {});
  }, [daemon.phase, devPrompt, query.isPending, session]);

  const openTaskSurface = useCallback((surface: TaskSurface) => {
    setTaskSurface(surface);
    setTaskSurfaceOpen(true);
  }, []);

  const applyModelSelection = useCallback((selection: ModelSelection) => {
    const current = sessionRef.current;
    if (!current) {return;}
    void runtimeRef.current.updateSessionOptions(current.id, selection).catch((cause) => {
      void showAlert('Couldn’t change model', cause instanceof Error ? cause.message : String(cause));
    });
  }, []);

  const copyLastResponse = useCallback(async () => {
    const lastAssistant = [...(sessionRef.current?.messages ?? [])]
      .reverse()
      .find((message) => message.role === 'assistant' && message.content.trim());

    if (!lastAssistant) {return;}
    await copyToClipboard(lastAssistant.content);
    await Haptics.notificationAsync('success');
  }, []);

  const confirmDelete = useCallback(() => {
    const current = sessionRef.current;
    if (!current) {return;}
    void confirmDestructive(
      `Delete “${displaySessionTitle(current)}”?`,
      'This removes the task and its transcript from the daemon for every device.',
      'Delete',
    ).then((confirmed) => {
      if (!confirmed) {return;}
      void runtimeRef.current.deleteSession(current.id)
        .then(() => {
          void Haptics.notificationAsync('success');
          navigateBack();
        })
        .catch((cause) => {
          void showAlert('Couldn’t delete task', cause instanceof Error ? cause.message : String(cause));
        });
    });
  }, []);

  const compactSession = useCallback(() => {
    const current = sessionRef.current;
    if (!current) {return;}
    void runtimeRef.current.compactSession(current.id).catch((cause) => {
      void showAlert('Couldn’t compact context', cause instanceof Error ? cause.message : String(cause));
    });
  }, []);

  const confirmRollback = useCallback(() => {
    const current = sessionRef.current;
    if (!current) {return;}
    void confirmDestructive(
      `Roll back the last turn in “${displaySessionTitle(current)}”?`,
      'This rewinds the conversation and workspace to before the previous turn.',
      'Roll back',
    ).then((confirmed) => {
      if (!confirmed) {return;}
      void runtimeRef.current.rollbackSession(current.id, 1)
        .then(() => Haptics.notificationAsync('success'))
        .catch((cause) => {
          void showAlert('Couldn’t roll back', cause instanceof Error ? cause.message : String(cause));
        });
    });
  }, []);

  const handleTaskMenuCommand = useCallback(
    (command: string) => {
      if (command === 'terminal' || command === 'files' || command === 'review' || command === 'git' || command === 'queue') {
        openTaskSurface(command);
      } else if (command === 'assignment') {
        setAssignmentOpen(true);
      } else if (command === 'capacity') {
        setCapacityOpen(true);
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

  // Boss context: a viewed employee gets the supervision composer plus its
  // lifecycle chrome; the boss's own chat gets the blocker notice and the
  // capacity readout.
  const bossState = useBossState();
  const bossData = bossState.data;
  const employee = bossEmployeeForSession(bossData, session?.id);
  const isBossChat = Boolean(bossData?.sessionId && bossData.sessionId === session?.id);
  const employeeLifecycle = employee ? bossEmployeeLifecycle(employee) : null;
  const queueRank = useMemo(
    () => (bossData ? bossQueueRanks(bossData) : new Map<string, number>()),
    [bossData],
  );

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
      .then((refs) => { if (!cancelled) {setTurnRefs(refs);} })
      .catch(() => { if (!cancelled) {setTurnRefs(null);} });

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
    if (!current || !turn) {return;}
    const dropped = current.turns.length - turn.turn_count + 1;
    void confirmDestructive(
      `Rewind “${displaySessionTitle(current)}” to here?`,
      `This removes ${dropped === 1 ? 'this turn' : `${dropped} turns`} and restores the files and conversation to before this message.`,
      'Rewind',
    ).then((confirmed) => {
      if (!confirmed) {return;}
      void runtimeRef.current.rewindSessionToMessage(current.id, turn.turn_count)
        .then(({ warning }) => {
          void Haptics.notificationAsync('success');
          if (warning) {void showAlert('Rewind cleanup warning', warning);}
        })
        .catch((cause) => {
          void showAlert('Couldn’t rewind', cause instanceof Error ? cause.message : String(cause));
        });
    });
  }, []);

  const forkTurn = useCallback((turnId: string) => {
    const current = sessionRef.current;
    const turn = current?.turns.find((item) => item.id === turnId);
    if (!current || !turn) {return;}
    void runtimeRef.current.forkSessionFromResponse(current.id, turn.turn_count)
      .then(({ session: forked, warning }) => {
        void Haptics.notificationAsync('success');
        pushRoute({ stack: 'root', name: 'session/:id', params: { id: forked.id } });
        if (warning) {void showAlert('Fork checkpoint warning', warning);}
      })
      .catch((cause) => {
        void showAlert('Couldn’t fork task', cause instanceof Error ? cause.message : String(cause));
      });
  }, []);

  // Transcript find bar: matches resolve to expanded row keys; the list's
  // revealRow mounts windowed history and opens folds as needed.
  const findMatches = useMemo(() => {
    const needle = findQuery.trim().toLowerCase();
    if (!findOpen || needle.length < 2 || !session) {return [] as Array<{ key: string; turnId: string | null }>;}
    const matches: Array<{ key: string; turnId: string | null }> = [];
    for (const message of session.messages) {
      const text = atomVisibleText(message.display_content ?? message.content).toLowerCase();
      if (message.hidden || !text.includes(needle)) {continue;}
      if (message.role === 'user') {matches.push({ key: `user:${message.id}`, turnId: message.turn_id });}
      else if (message.role === 'system') {matches.push({ key: `system:${message.id}`, turnId: message.turn_id });}
      else if (message.role === 'assistant') {matches.push({ key: `md:${message.id}.0`, turnId: message.turn_id });}
    }

    return matches;
  }, [findOpen, findQuery, session]);

  const revealMatch = useCallback((index: number, matches: Array<{ key: string; turnId: string | null }>) => {
    const match = matches[index];
    if (!match) {return;}
    listRef.current?.revealRow(match.key, match.turnId);
  }, []);

  // A new query jumps to the newest match — the convention for find-in-page.
  useEffect(() => {
    if (findQuery === lastFindQuery.current) {return;}
    lastFindQuery.current = findQuery;
    if (!findMatches.length) {return;}
    const index = findMatches.length - 1;
    setFindIndex(index);
    revealMatch(index, findMatches);
  }, [findMatches, findQuery, revealMatch]);

  const stepFind = useCallback((delta: number) => {
    if (!findMatches.length) {return;}
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
    ? 'Reconnecting…'
    : daemon.phase === 'connecting' || daemon.phase === 'booting'
      ? 'Connecting…'
      : daemon.phase === 'error' ? 'Not connected' : null;

  const subtitle = linkSubtitle ?? (subtitleParts.length ? subtitleParts.join(' · ') : null);
  const hasSession = Boolean(session);
  const unseenReplies = useHasUnseenReplies(sessionId);
  const renameRequest = session ? runtime.renameRequests[session.id] : undefined;
  const transcriptMounted = Boolean(
    session && mountedTranscriptSessionId === session.id,
  );

  const taskMenuItems = useMemo(
    () => [
      ...SURFACE_MENU_COMMANDS.map((item) => ({
        key: item.id,
        label: item.title,
        icon: item.symbol,
        onSelect: () => handleTaskMenuCommand(item.id),
      })),
      ...(employee
        ? [{
            key: 'assignment',
            label: 'Assignment',
            icon: 'info.circle',
            onSelect: () => handleTaskMenuCommand('assignment'),
          }]
        : []),
      ...(isBossChat && bossData
        ? [{
            key: 'capacity',
            label: 'Capacity',
            icon: 'gauge',
            onSelect: () => handleTaskMenuCommand('capacity'),
          }]
        : []),
      {
        key: 'model',
        label: modelLabel,
        onSelect: () => handleTaskMenuCommand('model'),
      },
      ...TASK_MENU_COMMANDS
        .filter((item) => !employee || !EMPLOYEE_HIDDEN_COMMANDS.has(item.id))
        .map((item) => ({
          key: item.id,
          label: item.title,
          icon: item.symbol,
          onSelect: () => handleTaskMenuCommand(item.id),
        })),
    ],
    [bossData, employee, handleTaskMenuCommand, isBossChat, modelLabel],
  );

  const keyboardPadding = useKeyboardPadding();

  return (
    <View style={[{ flex: '1 1', backgroundColor: theme.background }, keyboardPadding]}>
      {/* Self-drawn header strip — the reference's native nav bar becomes
       * composed views (task-menu trigger, title+subtitle, new-task). */}
      <ScreenHeaderBackdrop />
      <View style={{
        alignItems: 'center',
        flexDirection: 'row',
        gap: 6,
        height: 44,
        paddingHorizontal: 6,
      }}>
        <Pressable
          accessibilityLabel="Task history"
          accessibilityRole="button"
          onPress={openTaskDrawer}
          style={{ alignItems: 'center', flexDirection: 'row', gap: 4, minHeight: 36, paddingHorizontal: 8 }}>
          <AppSymbol
            name={{ ios: 'sidebar.left', android: 'menu', web: 'menu' }}
            size={19}
            tintColor={theme.text}
          />
          {unseenReplies ? (
            <View style={{ borderRadius: 3, height: 6, width: 6, backgroundColor: theme.info }} />
          ) : null}
        </Pressable>
        <View style={{ flex: '1 1', minWidth: 0 }}>
          <Text numberOfLines={1} style={{ fontSize: 16, fontWeight: '600', color: theme.text }}>
            {title}
          </Text>
          {subtitle ? (
            <Text numberOfLines={1} style={{ fontSize: 11.5, color: theme.textTertiary }}>
              {subtitle}
            </Text>
          ) : null}
        </View>
        {hasSession ? (
          <>
            <Pressable
              accessibilityLabel="New task"
              accessibilityRole="button"
              onPress={() => pushRoute({ stack: 'root', name: 'index', params: {} })}
              style={{ alignItems: 'center', justifyContent: 'center', minHeight: 36, minWidth: 36 }}>
              <AppSymbol
                name={{ ios: 'square.and.pencil', android: 'edit_square', web: 'edit' }}
                size={19}
                tintColor={theme.text}
              />
            </Pressable>
            <DropdownMenu
              items={taskMenuItems}
              trigger={(
                <Pressable
                  accessibilityLabel="Task options"
                  accessibilityRole="button"
                  style={{ alignItems: 'center', justifyContent: 'center', minHeight: 36, minWidth: 36 }}>
                  <AppSymbol
                    name={{ ios: 'ellipsis', android: 'more_horiz', web: 'more_horiz' }}
                    size={19}
                    tintColor={theme.text}
                  />
                </Pressable>
              )}
            />
          </>
        ) : null}
      </View>
      <View style={{ flex: '1 1' }}>
        {session && transcriptMounted ? (
          <ActivitySheetHost session={session}>
            <TranscriptList
              onHandle={(h) => { listRef.current = h; }}
              forkTurns={forkTurns}
              headerInset={headerInset}
              hydrated={!query.isPending}
              rewindTurns={rewindTurns}
              running={running}
              session={session}
              onForkTurn={forkTurn}
              onRewindTurn={confirmRewind}
            />
          </ActivitySheetHost>
        ) : (
          <View style={{ alignItems: 'center', flex: '1 1', justifyContent: 'center', paddingHorizontal: 32 }}>
            <SessionEmpty
              error={query.error}
              loading={Boolean(session) || query.isPending}
              missing={query.data === null}
            />
          </View>
        )}
        {/* A daemon-owned rename request pins above the transcript — it's
            not a row, so scrolling and turn folds can never hide it — until
            the user answers or the daemon settles it. */}
        {session && renameRequest ? (
          <Absolute style={{ left: 12, right: 12, top: headerInset + 8 }}>
            <PermissionPanel
              permission={renameRequest}
              onRespond={(optionId) =>
                runtime.respond(session.id, renameRequest.requestId, optionId)
              }
            />
          </Absolute>
        ) : null}
        {employee && bossData && (employeeLifecycle === 'queued' || employeeLifecycle === 'dispatching') ? (
          <Absolute style={{ left: 12, right: 12, top: headerInset + 8 }}>
            <EmployeeStatusCard
              employee={employee}
              queueRank={queueRank.get(employee.sessionId)}
            />
          </Absolute>
        ) : null}
        {employee && bossData && employeeLifecycle === 'expired' ? (
          <Absolute style={{ left: 12, right: 12, top: headerInset + 8 }}>
            <EmployeeExpiredCard employee={employee} />
          </Absolute>
        ) : null}
        {isBossChat && bossData ? (
          <Absolute style={{ left: 12, right: 12, top: headerInset + 8 }}>
            <BossBlockerNotice
              state={bossData}
              onOpen={(id) => pushRoute({ stack: 'root', name: 'session/:id', params: { id } })}
            />
          </Absolute>
        ) : null}
        {findOpen && session && (
          <GlassSurface
            fallbackColor={theme.surface}
            style={{ borderRadius: Radius.pill, marginHorizontal: 12, marginTop: headerInset + 8 }}>
            <View style={{ alignItems: 'center', flexDirection: 'row', gap: 8, minHeight: 40, paddingHorizontal: 12 }}>
              <AppSymbol
                name={{ ios: 'magnifyingglass', android: 'search', web: 'search' }}
                size={14}
                tintColor={theme.textTertiary}
              />
              <TextInput
                returnKeyType="search"
                onChange={setFindQuery}
                placeholder="Find in transcript"
                placeholderTextColor={theme.textGhost}
                style={{ flex: '1 1', fontSize: 15, paddingVertical: 6, color: theme.text }}
                value={findQuery}
              />
              <Text style={{ fontSize: 12, fontVariant: ['tabular-nums'], color: theme.textTertiary }}>
                {findMatches.length
                  ? `${Math.min(findIndex + 1, findMatches.length)} of ${findMatches.length}`
                  : 'No matches'}
              </Text>
              <Pressable
                accessibilityLabel="Previous match"
                accessibilityRole="button"
                onPress={() => stepFind(-1)}
                style={{ padding: 6 }}>
                <AppSymbol
                  name={{ ios: 'chevron.up', android: 'keyboard_arrow_up', web: 'keyboard_arrow_up' }}
                  size={16}
                  tintColor={theme.textSecondary}
                />
              </Pressable>
              <Pressable
                accessibilityLabel="Next match"
                accessibilityRole="button"
                onPress={() => stepFind(1)}
                style={{ padding: 6 }}>
                <AppSymbol
                  name={{ ios: 'chevron.down', android: 'keyboard_arrow_down', web: 'keyboard_arrow_down' }}
                  size={16}
                  tintColor={theme.textSecondary}
                />
              </Pressable>
              <Pressable
                accessibilityLabel="Close find"
                accessibilityRole="button"
                onPress={closeFind}
                style={{ padding: 6 }}>
                <AppSymbol
                  name={{ ios: 'xmark', android: 'close', web: 'close' }}
                  size={15}
                  tintColor={theme.textSecondary}
                />
              </Pressable>
            </View>
          </GlassSurface>
        )}
        <Absolute style={{ left: 12, right: 12, top: headerInset + 8 }}>
          <ConnectionBanner floating />
        </Absolute>
      </View>
      {session && (
        <MobileComposer
          boss={employee && bossData
            ? { employee, identity: bossData.identity }
            : null}
          session={session}
          onSubmitted={() => listRef.current?.followNextGrowth()}
        />
      )}

      {session && (
        <TaskSurfaceSheet
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
          onSubmit={(nextTitle) => runtime.renameSession(session.id, nextTitle)}
          visible={renaming}
        />
      )}
      {bossData && employee && (
        <BossAssignmentSheet
          employee={employee}
          onDismiss={() => setAssignmentOpen(false)}
          session={session ?? undefined}
          state={bossData}
          visible={assignmentOpen}
        />
      )}
      {bossData && (
        <BossCapacitySheet
          onDismiss={() => setCapacityOpen(false)}
          state={bossData}
          visible={capacityOpen}
        />
      )}
    </View>
  );
}

/** Same-contents-in, same-identity-out, so memoized children see a stable
 * prop until membership actually changes. */
function useStableSet(current: ReadonlySet<string>): ReadonlySet<string> {
  const ref = useRef(current);
  const previous = ref.current;
  const same = previous.size === current.size
    && [...current].every((value) => previous.has(value));

  if (!same) {ref.current = current;}
  return ref.current;
}
