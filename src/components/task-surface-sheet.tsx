import type { CSSProperties } from 'octane/jsx-runtime';
import type {
  AgentInvocation,
  AgentSession,
  GitFileChange,
  Project,
  ReviewDiffSource,
  ReviewQueue,
  WakuClient,
  WorkingTreeEntry,
} from '@waku/client';

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'octane';

import { Spinner, DropdownMenu, Image, Pressable, ScrollableArea, BottomSheet, Text, TextArea, TextInput, useColorScheme, useSafeAreaInsets, View, RichText, RichTextSpan } from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { DiffView } from '@/components/diff-view';
import { TerminalView } from '@/components/terminal-view';
import { MonoFont, NativeTint } from '@/lib/theme-platform';
import type { TerminalViewHandle } from '@/lib/terminal-page';
import { randomUUID } from '@/lib/uuid';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import {
  approveReviewCommit,
  collectWorkspaceDiff,
  commitWorkspace,
  daemonKeys,
  discardWorkspaceFile,
  generateWorkspaceCommitMessage,
  inspectGitPanel,
  listReviewQueue,
  listWorkspaceTree,
  promoteReviewQueue,
  pullWorkspace,
  pushWorkspace,
  readWorkspaceBinaryFile,
  readWorkspaceTextFile,
  rejectReviewCommit,
  stageWorkspaceFile,
  unstageWorkspaceFile,
  writeWorkspaceTextFile,
} from '@/lib/daemon-api';

import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import { invalidateDaemonQueries } from '@/lib/daemon-data';
import { sessionCwd } from '@/lib/mobile-runtime';
import { useProviderModels } from '@/lib/daemon-queries';
import { showAlert, confirmDestructive } from '@/lib/alert';
import {
  gitStatusLabel,
  imageMimeForPath,
  latestReviewTurnSource,
  parseNumstat,
  reviewDiffSourceLabel,
  reviewStatusLabel,
  splitReviewPatch,
  upstreamLabel,
} from '@/lib/task-surfaces';

export type TaskSurface = 'terminal' | 'files' | 'review' | 'git' | 'queue';

const MAX_FILE_CHARACTERS = 250_000;

const SURFACE_DETAILS: Record<
  TaskSurface,
  {
    title: string;
    subtitle: string;
    icon: Parameters<typeof AppSymbol>[0]['name'];
  }
> = {
  terminal: {
    title: 'Terminal',
    subtitle: 'Shell in this task’s workspace',
    icon: { ios: 'terminal', android: 'terminal', web: 'terminal' },
  },
  files: {
    title: 'Files',
    subtitle: 'Workspace files',
    icon: { ios: 'folder', android: 'folder', web: 'folder' },
  },
  review: {
    title: 'Review',
    subtitle: 'Workspace changes',
    icon: {
      ios: 'doc.text.magnifyingglass',
      android: 'difference',
      web: 'difference',
    },
  },
  git: {
    title: 'Git',
    subtitle: 'Branch and working tree',
    icon: {
      ios: 'arrow.triangle.branch',
      android: 'call_split',
      web: 'call_split',
    },
  },
  queue: {
    title: 'Review queue',
    subtitle: 'Approve and promote commits',
    icon: {
      ios: 'checkmark.circle',
      android: 'check_circle',
      web: 'check_circle',
    },
  },
};

export function TaskSurfaceSheet({
  surface,
  visible,
  session,
  project,
  onDismiss,
}: {
  surface: TaskSurface | null;
  visible: boolean;
  session: AgentSession;
  project: Project | undefined;
  onDismiss: () => void;
}) {
  return (
    <BottomSheet label="Task details"
      isOpen={visible && surface != null}
      snapPoints={[0.58, 1]}
      onOpenChange={(isOpen) => {if (!isOpen) {onDismiss();}}}>
      <View style={styles.sheet}>
        {surface ? (
          <SurfaceBody project={project} session={session} surface={surface} />
        ) : null}
      </View>
    </BottomSheet>
  );
}

function SurfaceBody({
  surface,
  session,
  project,
}: {
  surface: TaskSurface;
  session: AgentSession;
  project: Project | undefined;
}) {
  const root = project ? sessionCwd(session, project) : null;
  if (surface === 'terminal') {
    return <TerminalSurface root={root} sessionId={session.id} />;
  }

  if (surface === 'files') {
    return <FilesSurface root={root} />;
  }

  if (surface === 'git') {
    return <GitSurface root={root} session={session} />;
  }

  if (surface === 'queue') {
    return <QueueSurface root={root} />;
  }

  return <ReviewSurface root={root} session={session} />;
}

function SurfaceHeader({
  surface,
  subtitle,
  action,
}: {
  surface: TaskSurface;
  subtitle?: string;
  action?: ReactNode;
}) {
  const theme = useTheme();
  const details = SURFACE_DETAILS[surface];
  return (
    <View style={{ ...styles.header, borderBottomColor: theme.border }}>
      <View style={{ ...styles.headerIcon, backgroundColor: theme.overlay }}>
        <AppSymbol
          name={details.icon}
          size={15}
          tintColor={theme.textSecondary}
        />
      </View>
      <View style={styles.headerCopy}>
        <Text
          numberOfLines={1}
          style={{ ...styles.headerTitle, color: theme.text }}
        >
          {details.title}
        </Text>
        <Text
          numberOfLines={1}
          style={{ ...styles.headerSubtitle, color: theme.textTertiary }}
        >
          {subtitle ?? details.subtitle}
        </Text>
      </View>
      {action}
    </View>
  );
}

function TerminalSurface({
  root,
  sessionId,
}: {
  root: string | null;
  sessionId: string;
}) {
  const theme = useTheme();
  return (
    <View style={{ ...styles.fill, backgroundColor: theme.surface }}>
      <SurfaceHeader surface="terminal" />
      {root ? (
        <TerminalSession root={root} sessionId={sessionId} />
      ) : (
        <PanelMessage
          detail="This task does not have an available workspace."
          title="No workspace"
        />
      )}
    </View>
  );
}

/** xterm.js inside a framework WebView (see components/terminal-view) wired
 * to the daemon's openTerminal/writeTerminal/resizeTerminal protocol, the
 * same event flow the reference drives expo-libghostty with. */
function TerminalSession({ root, sessionId }: { root: string; sessionId: string }) {
  const daemon = useDaemon();
  const theme = useTheme();
  const colorScheme = useColorScheme();
  const terminal = useRef<TerminalViewHandle | null>(null);
  const terminalId = useRef(randomUUID()).current;
  const [error, setError] = useState<string | null>(null);
  const [exited, setExited] = useState(false);

  useEffect(() => {
    const client = daemon.client;
    if (!client || daemon.phase !== 'connected') {return;}
    let disposed = false;
    setError(null);
    setExited(false);
    const unsubscribe = client.subscribe(terminalId, terminalId, (event) => {
      if (event.event.kind === 'terminalOutput') {
        const payload = event.event.payload as { data?: unknown };
        if (typeof payload.data !== 'string') {return;}
        terminal.current?.write(payload.data);
      } else if (event.event.kind === 'terminalExited') {
        setExited(true);
        terminal.current?.finish(0);
      } else if (event.event.kind === 'terminalError') {
        setError(
          typeof event.event.payload === 'string'
            ? event.event.payload
            : 'The terminal connection failed.',
        );
      }
    });

    void client
      .request(
        { type: 'openTerminal', cwd: root, cols: 80, rows: 24, owner: sessionId },
        terminalId,
        terminalId,
      )
      .catch((cause) => {
        if (!disposed) {setError(errorMessage(cause));}
      });

    return () => {
      disposed = true;
      unsubscribe();
      void client
        .notify({ type: 'closeTerminal' }, terminalId, terminalId)
        .catch(() => {});
    };
  }, [daemon.client, daemon.phase, root, sessionId, terminalId]);

  const status =
    error ?? (exited ? 'Shell exited' : daemon.phase !== 'connected' ? 'Reconnecting…' : null);

  const terminalTheme = useMemo(
    () => ({
      background: theme.inset,
      foreground: theme.text,
      cursor: theme.text,
      selectionBackground: colorScheme === 'dark' ? '#40546f' : '#c8d8ee',
    }),
    [theme.inset, theme.text, colorScheme],
  );

  return (
    <View style={styles.terminalBody}>
      <TerminalView
        bind={(handle) => {
          terminal.current = handle;
        }}
        bridgeId={terminalId}
        fontFamily={`${MonoFont}, monospace`}
        fontSize={13}
        onInput={(data) => {
          const client = daemon.client;
          if (!client || daemon.phase !== 'connected' || exited) {return;}
          void client
            .notify({ type: 'writeTerminal', data }, terminalId, terminalId)
            .catch((cause) => setError(errorMessage(cause)));
        }}
        onResize={(cols, rows) => {
          const client = daemon.client;
          if (!client || daemon.phase !== 'connected') {return;}
          void client
            .notify(
              { type: 'resizeTerminal', cols: clampU16(cols), rows: clampU16(rows) },
              terminalId,
              terminalId,
            )
            .catch(() => {});
        }}
        style={styles.fill}
        theme={terminalTheme}
      />
      {status ? (
        <Text
          numberOfLines={2}
          style={{
            ...styles.terminalStatus,
            backgroundColor: theme.surface,
            borderTopColor: theme.border,
            color: error ? theme.danger : theme.textSecondary,
          }}
        >
          {status}
        </Text>
      ) : null}
    </View>
  );
}

function FilesSurface({ root }: { root: string | null }) {
  const daemon = useDaemon();
  const theme = useTheme();
  const _insets = useSafeAreaInsets();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const [expanded, setExpanded] = useState<string[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const imageMime = selectedPath ? imageMimeForPath(selectedPath) : null;

  useEffect(() => {
    setExpanded([]);
    setSelectedPath(null);
  }, [root]);

  useEffect(() => {
    setEditing(false);
    setDraft('');
    setSaving(false);
  }, [selectedPath]);

  const tree = useDaemonQuery(
    daemonKeys.workspaceTree(profileId, root ?? 'none', expanded),
    () => listWorkspaceTree(daemon.client!, root!, expanded),
    { enabled: Boolean(daemon.client && daemon.phase === 'connected' && root) },
  );

  const file = useDaemonQuery(
    daemonKeys.workspaceFile(profileId, root ?? 'none', selectedPath ?? 'none'),
    () => readWorkspaceTextFile(daemon.client!, root!, selectedPath!),
    {
      enabled: Boolean(
        daemon.client
          && daemon.phase === 'connected'
          && root
          && selectedPath
          && !imageMime,
      ),
    },
  );

  const image = useDaemonQuery(
    [
      ...daemonKeys.workspaceFile(profileId, root ?? 'none', selectedPath ?? 'none'),
      'binary',
    ],
    () => readWorkspaceBinaryFile(daemon.client!, root!, selectedPath!),
    {
      enabled: Boolean(
        daemon.client
          && daemon.phase === 'connected'
          && root
          && selectedPath
          && imageMime,
      ),
    },
  );

  const saveFile = useCallback(() => {
    const client = daemon.client;
    if (!client || !root || !selectedPath || saving) {return;}
    setSaving(true);
    void writeWorkspaceTextFile(client, root, selectedPath, draft)
      .then(() => {
        setEditing(false);
        invalidateDaemonQueries(profileId, () => true);
      })
      .catch((cause) =>
        void showAlert('Couldn’t save file', errorMessage(cause)),
      )
      .finally(() => setSaving(false));
  }, [daemon.client, draft, profileId, root, saving, selectedPath]);

  if (!root) {
    return (
      <View style={styles.fill}>
        <SurfaceHeader surface="files" />
        <PanelMessage
          detail="This task does not have an available workspace."
          title="No workspace"
        />
      </View>
    );
  }

  if (selectedPath) {
    const truncated = (file.data?.length ?? 0) > MAX_FILE_CHARACTERS;
    return (
      <View style={styles.fill}>
        <SurfaceHeader
          action={
            imageMime ? (
              <HeaderTextButton
                label="Refresh"
                onPress={() => void image.refetch()}
              />
            ) : editing ? (
              <View style={styles.headerActions}>
                <HeaderTextButton
                  label="Cancel"
                  onPress={() => {
                    setEditing(false);
                    setDraft('');
                  }}
                />
                <HeaderTextButton
                  label={saving ? 'Saving…' : 'Save'}
                  onPress={saveFile}
                />
              </View>
            ) : (
              <View style={styles.headerActions}>
                <HeaderTextButton
                  label="Edit"
                  onPress={() => {
                    setDraft(file.data ?? '');
                    setEditing(true);
                  }}
                />
                <HeaderTextButton
                  label="Refresh"
                  onPress={() => void file.refetch()}
                />
              </View>
            )
          }
          subtitle={selectedPath}
          surface="files"
        />
        <Pressable
          accessibilityHint="Returns to the workspace file list"
          accessibilityRole="button"
          onPress={() => setSelectedPath(null)}
          style={styles.backRow}
        >
          <AppSymbol
            name={{ ios: 'chevron.left', android: 'arrow_back', web: 'arrow_back' }}
            size={13}
            tintColor={NativeTint as string}
          />
          <Text style={{ ...styles.backLabel, color: NativeTint as string }}>Files</Text>
        </Pressable>
        {imageMime ? (
          image.isPending ? (
            <LoadingMessage />
          ) : image.error ? (
            <PanelMessage
              detail={errorMessage(image.error)}
              title="Couldn’t read image"
            />
          ) : (
            <ScrollableArea style={{ flexGrow: 1 }}>
              <Image
                src={`data:${imageMime};base64,${image.data ?? ''}`}
                style={{ aspectRatio: 1, width: '100%' }}
                ios={{ stretch: 'aspectFit' }}
                web={{ style: { objectFit: 'contain' } }}
              />
            </ScrollableArea>
          )
        ) : file.isPending ? (
          <LoadingMessage />
        ) : file.error ? (
          <PanelMessage
            detail={errorMessage(file.error)}
            title="Couldn’t read file"
          />
        ) : editing ? (
          <ScrollableArea style={styles.fileContent}>
            <TextArea
              onChange={setDraft}
              style={{ ...styles.fileText, ...styles.fileEditor, color: theme.text }}
              value={draft}
            />
          </ScrollableArea>
        ) : (
          <ScrollableArea style={styles.fileContent}>
            <Text style={{ ...styles.fileText, color: theme.text }}>
              {(file.data ?? '').slice(0, MAX_FILE_CHARACTERS)}
            </Text>
            {truncated ? (
              <Text style={{ ...styles.truncated, color: theme.textTertiary }}>
                File truncated in this viewer.
              </Text>
            ) : null}
          </ScrollableArea>
        )}
      </View>
    );
  }

  return (
    <View style={styles.fill}>
      <SurfaceHeader
        action={
          <HeaderTextButton
            label="Refresh"
            onPress={() => void tree.refetch()}
          />
        }
        surface="files"
      />
      {tree.isPending ? (
        <LoadingMessage />
      ) : tree.error ? (
        <PanelMessage
          detail={errorMessage(tree.error)}
          title="Couldn’t load files"
        />
      ) : (
        <ScrollableArea style={{ flex: '1 1' }}>
          {(tree.data ?? []).length === 0 ? (
            <PanelMessage
              detail="The workspace has no visible files."
              title="No files"
            />
          ) : null}
          {(tree.data ?? []).map((item: WorkingTreeEntry) => (
            <Pressable
              accessibilityRole="button"
              onPress={() => {
                if (!item.isDir) {
                  setSelectedPath(item.relativePath);
                  return;
                }

                setExpanded((current) =>
                  current.includes(item.absolutePath)
                    ? current.filter((path) => path !== item.absolutePath)
                    : [...current, item.absolutePath].sort(),
                );
              }}
              style={{ ...styles.fileRow, paddingLeft: 14 + Math.min(item.depth, 12) * 16 }}
            >
              <AppSymbol
                name={
                  item.isDir
                    ? {
                        ios: item.expanded ? 'chevron.down' : 'chevron.right',
                        android: item.expanded
                          ? 'expand_more'
                          : 'chevron_right',
                        web: item.expanded ? 'expand_more' : 'chevron_right',
                      }
                    : { ios: 'doc', android: 'draft', web: 'draft' }
                }
                size={item.isDir ? 12 : 14}
                tintColor={theme.textTertiary}
              />
              <Text
                numberOfLines={1}
                style={{ ...styles.fileRowLabel, color: theme.text }}
              >
                {item.name}
              </Text>
            </Pressable>
          ))}
        </ScrollableArea>
      )}
    </View>
  );
}

const STANDARD_REVIEW_SOURCES: Array<{
  id: Exclude<ReviewDiffSource, object>;
  source: Exclude<ReviewDiffSource, object>;
}> = [
  { id: 'uncommitted', source: 'uncommitted' },
  { id: 'unstaged', source: 'unstaged' },
  { id: 'staged', source: 'staged' },
  { id: 'committed', source: 'committed' },
  { id: 'branch', source: 'branch' },
];

function ReviewSurface({
  root,
  session,
}: {
  root: string | null;
  session: AgentSession;
}) {
  const daemon = useDaemon();
  const theme = useTheme();
  const _insets = useSafeAreaInsets();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const lastTurn = useMemo(() => latestReviewTurnSource(session), [session]);
  const [source, setSource] = useState<ReviewDiffSource>('uncommitted');

  useEffect(() => setSource('uncommitted'), [session.id]);

  const diff = useDaemonQuery(
    daemonKeys.workspaceDiff(profileId, root ?? 'none', source),
    () => collectWorkspaceDiff(daemon.client!, root!, source),
    { enabled: Boolean(daemon.client && daemon.phase === 'connected' && root) },
  );

  const files = useMemo(
    () => splitReviewPatch(diff.data?.patch ?? ''),
    [diff.data?.patch],
  );

  const stats = useMemo(
    () => parseNumstat(diff.data?.numstat ?? ''),
    [diff.data?.numstat],
  );

  if (!root) {
    return (
      <View style={styles.fill}>
        <SurfaceHeader surface="review" />
        <PanelMessage
          detail="This task does not have an available workspace."
          title="No workspace"
        />
      </View>
    );
  }

  return (
    <View style={styles.fill}>
      <SurfaceHeader
        action={
          <HeaderTextButton
            label="Refresh"
            onPress={() => void diff.refetch()}
          />
        }
        subtitle={reviewDiffSourceLabel(source)}
        surface="review"
      />
      <View
        style={{ ...styles.reviewControls, borderBottomColor: theme.border }}
      >
        <DropdownMenu
          items={[
            ...(lastTurn
              ? [{
                  type: 'action' as const,
                  key: 'last-turn',
                  label: reviewDiffSourceLabel(lastTurn),
                  onSelect: () => setSource(lastTurn),
                }]
              : []),
            ...STANDARD_REVIEW_SOURCES.map((item) => ({
              type: 'action' as const,
              key: item.id,
              label: reviewDiffSourceLabel(item.source),
              onSelect: () => setSource(item.source),
            })),
          ]}
          trigger={(
            <View
              accessibilityLabel="Choose review source"
              accessibilityRole="button"
              style={{ ...styles.sourceButton, backgroundColor: theme.overlay }}
            >
              <Text style={{ ...styles.sourceLabel, color: theme.text }}>
                {reviewDiffSourceLabel(source)}
              </Text>
              <AppSymbol
                name={{
                  ios: 'chevron.up.chevron.down',
                  android: 'unfold_more',
                  web: 'unfold_more',
                }}
                size={12}
                tintColor={theme.textTertiary}
              />
            </View>
          )}
        />
        <RichText style={styles.reviewStats}>
          <RichTextSpan style={{ color: theme.textTertiary }}>
            {stats.files} {stats.files === 1 ? 'file' : 'files'}{' '}
          </RichTextSpan>
          <RichTextSpan style={{ color: theme.success }}>+{stats.additions}</RichTextSpan>
          <RichTextSpan text=" " style={{ color: theme.textGhost }} />
          <RichTextSpan style={{ color: theme.danger }}>−{stats.deletions}</RichTextSpan>
        </RichText>
      </View>
      {diff.isPending ? (
        <LoadingMessage />
      ) : diff.error ? (
        <PanelMessage
          detail={errorMessage(diff.error)}
          title="Couldn’t load review"
        />
      ) : (
        <ScrollableArea style={styles.reviewList}>
          {!diff.data?.completeContext ? (
            <Text
              style={{
                ...styles.contextNote,
                color: theme.warning,
                backgroundColor: theme.warningSoft,
              }}
            >
              This comparison has partial context.
            </Text>
          ) : null}
          {files.length === 0 ? (
            <PanelMessage
              detail="There are no changes in this comparison."
              title="No changes"
            />
          ) : null}
          {files.map((file) => (
            <View>
              <View
                style={{
                  ...styles.diffFileHeader,
                  backgroundColor: theme.surface,
                  borderBottomColor: theme.border,
                }}
              >
                <AppSymbol
                  name={{ ios: 'doc', android: 'draft', web: 'draft' }}
                  size={13}
                  tintColor={theme.textTertiary}
                />
                <Text
                  numberOfLines={1}
                  style={{ ...styles.diffPath, color: theme.textSecondary }}
                >
                  {file.path}
                </Text>
              </View>
              <View
                style={{ ...styles.diffSection, borderBottomColor: theme.border }}
              >
                <DiffView diff={file.patch} mode="review" />
              </View>
            </View>
          ))}
        </ScrollableArea>
      )}
    </View>
  );
}

type GitSection = {
  key: 'staged' | 'unstaged';
  title: string;
  data: GitFileChange[];
};

/**
 * The working-tree half of the desktop Git panel: branch + upstream, the
 * staged/unstaged file lists with their per-file actions, and the
 * commit-or-generate bar. Branch switching, worktrees, and the commit log
 * stay desktop-side for now.
 */
function GitSurface({ root, session }: { root: string | null; session: AgentSession }) {
  const daemon = useDaemon();
  const theme = useTheme();
  const _insets = useSafeAreaInsets();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const probe = useProviderModels(session.provider);
  const [message, setMessage] = useState('');
  const [pending, setPending] = useState<string | null>(null);
  const [opError, setOpError] = useState<string | null>(null);

  useEffect(() => {
    setMessage('');
    setOpError(null);
  }, [root, session.id]);

  const panel = useDaemonQuery(
    daemonKeys.gitPanel(profileId, root ?? 'none'),
    () => inspectGitPanel(daemon.client!, root!),
    { enabled: Boolean(daemon.client && daemon.phase === 'connected' && root) },
  );

  const snapshot = panel.data ?? null;
  const stagedCount = snapshot?.staged.length ?? 0;
  const unstagedCount = snapshot?.unstaged.length ?? 0;

  const refresh = useCallback(() => {
    invalidateDaemonQueries(profileId, () => true);
  }, [profileId]);

  const runOp = useCallback(
    <T,>(label: string, op: (client: WakuClient) => Promise<T>, onSuccess?: (value: T) => void) => {
      const client = daemon.client;
      if (!client || !root || pending) {return;}
      setPending(label);
      setOpError(null);
      void Promise.resolve()
        .then(() => op(client))
        .then((value) => onSuccess?.(value))
        .catch((cause) => setOpError(errorMessage(cause)))
        .finally(() => {
          setPending(null);
          refresh();
        });
    },
    [daemon.client, pending, refresh, root],
  );

  // The same invocation the desktop panel builds for generated messages:
  // the session's provider binary, model, and effort.
  const invocation = useMemo<AgentInvocation | null>(() => {
    const binary = probe.data?.path;
    if (!binary) {return null;}
    return {
      provider: session.provider,
      binary,
      model: session.model ?? null,
      reasoning_effort: session.reasoning_effort ?? null,
    };
  }, [probe.data?.path, session.model, session.provider, session.reasoning_effort]);

  /** Commits staged — or sweeps the worktree — generating the message from
   * the provider when the field is blank, like the desktop panel. */
  const commit = useCallback(
    (includeUnstaged: boolean) => {
      if (!root) {return;}
      const typed = message.trim();
      runOp(
        'commit',
        async (client) => {
          const finalMessage =
            typed ||
            (await generateWorkspaceCommitMessage(client, root, includeUnstaged, invocation!));

          await commitWorkspace(client, root, finalMessage, includeUnstaged, false);
        },
        () => setMessage(''),
      );
    },
    [invocation, message, root, runOp],
  );

  const confirmCommit = useCallback(() => {
    if (stagedCount === 0 && unstagedCount > 0) {
      void confirmDestructive(
        'Nothing staged',
        `Commit all ${unstagedCount} ${unstagedCount === 1 ? 'change' : 'changes'}?`,
        'Commit all',
      ).then((confirmed) => {
        if (confirmed) {commit(true);}
      });

      return;
    }

    commit(false);
  }, [commit, stagedCount, unstagedCount]);

  const discard = useCallback(
    (file: GitFileChange) => {
      void confirmDestructive(
        `Discard changes to ${file.path}?`,
        file.untracked
          ? 'This deletes the untracked file and cannot be undone.'
          : 'This restores the file to HEAD and cannot be undone.',
        'Discard',
      ).then((confirmed) => {
        if (confirmed) {
          runOp('discard', (client) => discardWorkspaceFile(client, root!, file.path));
        }
      });
    },
    [root, runOp],
  );

  const sections = useMemo<GitSection[]>(() => {
    if (!snapshot) {return [];}
    const all: GitSection[] = [
      { key: 'staged', title: 'Staged', data: snapshot.staged },
      { key: 'unstaged', title: 'Changes', data: snapshot.unstaged },
    ];

    return all.filter((section) => section.data.length > 0);
  }, [snapshot]);

  const canCommit = Boolean(
    snapshot && (stagedCount > 0 || unstagedCount > 0) && (message.trim() || invocation),
  );

  const behind = snapshot?.upstream?.behind ?? 0;
  const subtitle = snapshot
    ? [snapshot.branch, upstreamLabel(snapshot.upstream)].filter(Boolean).join(' · ')
    : undefined;

  if (!root) {
    return (
      <View style={styles.fill}>
        <SurfaceHeader surface="git" />
        <PanelMessage
          detail="This task does not have an available workspace."
          title="No workspace"
        />
      </View>
    );
  }

  return (
    <View style={styles.fill}>
      <SurfaceHeader
        action={
          <HeaderTextButton label="Refresh" onPress={() => void panel.refetch()} />
        }
        subtitle={subtitle}
        surface="git"
      />
      {panel.isPending ? (
        <LoadingMessage />
      ) : panel.error ? (
        <PanelMessage detail={errorMessage(panel.error)} title="Couldn’t load git status" />
      ) : !snapshot ? (
        <PanelMessage
          detail="This workspace is not inside a git repository."
          title="No repository"
        />
      ) : (
        <>
          {behind > 0 || snapshot.can_push ? (
            <View style={{ ...styles.gitSyncRow, borderBottomColor: theme.border }}>
              {behind > 0 ? (
                <HeaderTextButton
                  label={pending === 'pull' ? 'Pulling…' : `Pull ↓${behind}`}
                  onPress={() =>
                    runOp('pull', async (client) => {
                      const outcome = await pullWorkspace(client, root);
                      if (typeof outcome === 'object' && 'conflict' in outcome) {
                        throw new Error(
                          `Pull stopped on a conflict in ${outcome.conflict.files.length} ${
                            outcome.conflict.files.length === 1 ? 'file' : 'files'
                          } — resolve it on the host.`,
                        );
                      }
                    })
                  }
                />
              ) : null}
              {snapshot.can_push ? (
                <HeaderTextButton
                  label={pending === 'push' ? 'Pushing…' : 'Push'}
                  onPress={() => runOp('push', (client) => pushWorkspace(client, root))}
                />
              ) : null}
            </View>
          ) : null}
          <ScrollableArea style={{ flex: '1 1' }}>
            {sections.length === 0 ? (
              <PanelMessage detail="The working tree is clean." title="No changes" />
            ) : null}
            {sections.map((section) => (
              <View>
                <View
                  style={{
                    ...styles.diffFileHeader,
                    backgroundColor: theme.surface,
                    borderBottomColor: theme.border,
                  }}
                >
                  <Text style={{ ...styles.diffPath, color: theme.textSecondary }}>
                    {section.title} ({section.data.length})
                  </Text>
                </View>
                {section.data.map((item) => (
                  <View style={{ ...styles.gitFileRow, borderBottomColor: theme.border }}>
                    <View style={{ ...styles.gitStatusBadge, backgroundColor: theme.overlay }}>
                      <Text style={{ ...styles.gitStatusText, color: theme.textTertiary }}>
                        {item.untracked ? '??' : item.status}
                      </Text>
                    </View>
                    <View style={styles.gitFileCopy}>
                      <Text numberOfLines={1} style={{ ...styles.gitFilePath, color: theme.text }}>
                        {item.path}
                      </Text>
                      <Text style={{ ...styles.gitFileMeta, color: theme.textGhost }}>
                        {gitStatusLabel(item.status, item.untracked)}
                        {item.additions || item.deletions
                          ? ` · +${item.additions} −${item.deletions}`
                          : ''}
                      </Text>
                    </View>
                    {section.key === 'staged' ? (
                      <HeaderTextButton
                        label="Unstage"
                        onPress={() =>
                          runOp('unstage', (client) => unstageWorkspaceFile(client, root, item.path))
                        }
                      />
                    ) : (
                      <View style={styles.gitFileActions}>
                        <HeaderTextButton
                          label="Stage"
                          onPress={() =>
                            runOp('stage', (client) => stageWorkspaceFile(client, root, item.path))
                          }
                        />
                        <Pressable
                          accessibilityLabel={`Discard ${item.path}`}
                          accessibilityRole="button"
                          onPress={() => discard(item)}
                          style={styles.textButton}
                        >
                          <Text style={{ ...styles.textButtonLabel, color: theme.danger }}>
                            Discard
                          </Text>
                        </Pressable>
                      </View>
                    )}
                  </View>
                ))}
              </View>
            ))}
          </ScrollableArea>
          {opError ? (
            <Text style={{ ...styles.gitError, color: theme.danger }}>{opError}</Text>
          ) : null}
          <View
            style={{
              ...styles.commitBar,
              backgroundColor: theme.surface,
              borderTopColor: theme.border,
              paddingBottom: Math.max(_insets.bottom, 10),
            }}
          >
            <TextInput
              isReadOnly={pending != null}
              onChange={setMessage}
              placeholder={
                invocation ? 'Message (blank generates one)' : 'Commit message'
              }
              placeholderTextColor={theme.textGhost}
              style={{
                ...styles.commitInput,
                backgroundColor: theme.overlay,
                color: theme.text,
              }}
              value={message}
            />
            <Pressable
              accessibilityHint={
                message.trim()
                  ? 'Commits the staged changes'
                  : 'Generates a message and commits'
              }
              accessibilityLabel="Commit"
              accessibilityRole="button"
              accessibilityState={{ disabled: !canCommit || pending != null }}
              disabled={!canCommit || pending != null}
              onPress={confirmCommit}
              style={{
                ...styles.commitButton,
                backgroundColor: theme.accent,
                ...((!canCommit || pending != null) && { opacity: 0.5 }),
              }}
            >
              {pending === 'commit' ? (
                <Spinner busy />
              ) : (
                <Text style={styles.commitButtonLabel}>
                  {stagedCount > 0 ? 'Commit' : 'Commit all'}
                </Text>
              )}
            </Pressable>
          </View>
        </>
      )}
    </View>
  );
}

/** The repo's `qa`-branch review queue — approve or reject proposed
 * commits and promote the approved prefix onto the base branch, mirroring
 * desktop's Projects → Review tab. */
function QueueSurface({ root }: { root: string | null }) {
  const daemon = useDaemon();
  const theme = useTheme();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const [pending, setPending] = useState<string | null>(null);
  const [opError, setOpError] = useState<string | null>(null);

  const queue = useDaemonQuery(
    daemonKeys.reviewQueue(profileId, root ?? 'none'),
    () => listReviewQueue(daemon.client!, root!),
    { enabled: Boolean(daemon.client && daemon.phase === 'connected' && root) },
  );

  const snapshot = queue.data;

  const refresh = useCallback(() => {
    invalidateDaemonQueries(profileId, () => true);
  }, [profileId]);

  const runOp = useCallback(
    (
      label: string,
      op: (client: WakuClient) => Promise<ReviewQueue | null>,
    ) => {
      const client = daemon.client;
      if (!client || !root || pending) {return;}
      setPending(label);
      setOpError(null);
      void op(client)
        .catch((cause) => setOpError(errorMessage(cause)))
        .finally(() => {
          setPending(null);
          refresh();
        });
    },
    [daemon.client, pending, refresh, root],
  );

  const confirmPromote = useCallback(() => {
    if (!snapshot?.frontier) {return;}
    const target = snapshot.baseBranch ?? 'main';
    void confirmDestructive(
      `Promote to ${target}?`,
      `Fast-forwards ${target} through the approved commits.`,
      'Promote',
    ).then((confirmed) => {
      if (confirmed) {
        runOp('promote', (client) => promoteReviewQueue(client, root!));
      }
    });
  }, [root, runOp, snapshot]);

  if (!root) {
    return (
      <View style={styles.fill}>
        <SurfaceHeader surface="queue" />
        <PanelMessage
          detail="This task does not have an available workspace."
          title="No workspace"
        />
      </View>
    );
  }

  return (
    <View style={styles.fill}>
      <SurfaceHeader
        action={
          <HeaderTextButton
            label="Refresh"
            onPress={() => void queue.refetch()}
          />
        }
        subtitle={
          snapshot
            ? snapshot.baseBranch
              ? `${snapshot.reviewBranch || 'dev'} → ${snapshot.baseBranch}`
              : snapshot.reviewBranch || 'dev'
            : undefined
        }
        surface="queue"
      />
      {queue.isPending ? (
        <LoadingMessage />
      ) : queue.error ? (
        <PanelMessage
          detail={errorMessage(queue.error)}
          title="Couldn’t load review queue"
        />
      ) : !snapshot ? (
        <PanelMessage
          detail="This repository has no review branch on origin."
          title="No review queue"
        />
      ) : (
        <>
          {snapshot.frontier ? (
            <View
              style={{ ...styles.gitSyncRow, borderBottomColor: theme.border }}
            >
              <Text
                style={{ ...styles.queueFrontier, color: theme.textSecondary }}
              >
                {`Promotable to ${snapshot.baseBranch ?? 'main'}`}
              </Text>
              <HeaderTextButton
                label={pending === 'promote' ? 'Promoting…' : 'Promote'}
                onPress={confirmPromote}
              />
            </View>
          ) : null}
          <ScrollableArea style={{ flex: '1 1' }}>
            {snapshot.entries.length === 0 ? (
              <PanelMessage
                detail="Nothing is waiting for review."
                title="Queue is empty"
              />
            ) : null}
            {snapshot.entries.map((item) => (
              <View
                style={{ ...styles.gitFileRow, borderBottomColor: theme.border }}
              >
                <View style={styles.gitFileCopy}>
                  <Text
                    numberOfLines={2}
                    style={{ ...styles.gitFilePath, color: theme.text }}
                  >
                    {item.commit.subject}
                  </Text>
                  <Text
                    style={{ ...styles.gitFileMeta, color: theme.textGhost }}
                  >
                    {[
                      item.commit.short_sha,
                      item.commit.author,
                      reviewStatusLabel(item),
                      item.commit.additions || item.commit.deletions
                        ? `+${item.commit.additions} −${item.commit.deletions}`
                        : null,
                      item.testPlans.length > 0
                        ? `${item.testPlans.length} test-plan ${
                            item.testPlans.length === 1 ? 'item' : 'items'
                          }`
                        : null,
                    ]
                      .filter(Boolean)
                      .join(' · ')}
                  </Text>
                  {item.reviews.length > 0 ? (
                    <Text
                      style={{ ...styles.gitFileMeta, color: theme.textGhost }}
                    >
                      {item.reviews
                        .map(
                          (record) =>
                            `${record.reviewer}: ${record.decision}`,
                        )
                        .join(' · ')}
                    </Text>
                  ) : null}
                </View>
                {!item.reverted ? (
                  <View style={styles.queueActions}>
                    <HeaderTextButton
                      label={
                        pending === `approve:${item.commit.sha}`
                          ? '…'
                          : 'Approve'
                      }
                      onPress={() =>
                        runOp(`approve:${item.commit.sha}`, (client) =>
                          approveReviewCommit(client, root, item.commit.sha),
                        )
                      }
                    />
                    <Pressable
                      accessibilityLabel={`Reject ${item.commit.short_sha}`}
                      accessibilityRole="button"
                      onPress={() =>
                        runOp(`reject:${item.commit.sha}`, (client) =>
                          rejectReviewCommit(client, root, item.commit.sha),
                        )
                      }
                      style={styles.textButton}
                    >
                      <Text
                        style={{
                          ...styles.textButtonLabel,
                          color:
                            pending === `reject:${item.commit.sha}`
                              ? theme.textGhost
                              : theme.danger,
                        }}
                      >
                        Reject
                      </Text>
                    </Pressable>
                  </View>
                ) : null}
              </View>
            ))}
          </ScrollableArea>
          {opError ? (
            <Text style={{ ...styles.gitError, color: theme.danger }}>
              {opError}
            </Text>
          ) : null}
        </>
      )}
    </View>
  );
}

function HeaderTextButton({
  label,
  onPress,
}: {
  label: string;
  onPress: () => void;
}) {
  return (
    <Pressable
      accessibilityRole="button"
      onPress={onPress}
      style={styles.textButton}
    >
      <Text style={{ ...styles.textButtonLabel, color: NativeTint as string }}>
        {label}
      </Text>
    </Pressable>
  );
}

function LoadingMessage() {
  const theme = useTheme();
  return (
    <View style={styles.loading}>
      <Spinner busy />
      <Text style={{ ...styles.loadingText, color: theme.textTertiary }}>
        Loading…
      </Text>
    </View>
  );
}

function PanelMessage({ title, detail }: { title: string; detail: string }) {
  const theme = useTheme();
  return (
    <View style={styles.message}>
      <Text style={{ ...styles.messageTitle, color: theme.text }}>{title}</Text>
      <Text style={{ ...styles.messageDetail, color: theme.textTertiary }}>
        {detail}
      </Text>
    </View>
  );
}

function clampU16(value: number): number {
  return Math.min(65_535, Math.max(1, Math.round(value)));
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

const hairline = 0.5;

const styles = {
  sheet: { flex: '1 1', minHeight: 0 },
  fill: { flex: '1 1', minHeight: 0 },
  header: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    gap: 10,
    minHeight: 54,
    paddingLeft: 14, paddingRight: 14,
    paddingTop: 8, paddingBottom: 8,
  },
  headerIcon: {
    alignItems: 'center',
    borderRadius: Radius.small,
    height: 30,
    justifyContent: 'center',
    width: 30,
  },
  headerCopy: { flex: '1 1', minWidth: 0 },
  headerTitle: { fontSize: 16, fontWeight: '600' },
  headerSubtitle: { fontSize: 12.5, marginTop: 1 },
  textButton: { justifyContent: 'center', minHeight: 36, paddingLeft: 4, paddingRight: 4 },
  textButtonLabel: { fontSize: 15, fontWeight: '500' },
  backRow: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 5,
    minHeight: 38,
    paddingLeft: 15, paddingRight: 15,
  },
  backLabel: { fontSize: 15.5, fontWeight: '500' },
  terminalBody: { flex: '1 1', minHeight: 0 },
  terminalStatus: {
    borderTopWidth: hairline,
    fontSize: 12,
    paddingLeft: 14, paddingRight: 14,
    paddingTop: 8, paddingBottom: 8,
  },
  headerActions: { alignItems: 'center', flexDirection: 'row', gap: 14 },
  fileContent: { flex: '1 1', paddingLeft: 14, paddingRight: 14, paddingTop: 4 },
  fileText: { fontFamily: MonoFont, fontSize: 12.5, lineHeight: '17px' },
  fileEditor: { minHeight: 200 },
  truncated: { fontSize: 12, marginTop: 14 },
  fileRow: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 8,
    minHeight: 39,
    paddingRight: 14,
  },
  fileRowLabel: { flex: '1 1', fontSize: 14.5 },
  reviewControls: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    justifyContent: 'space-between',
    minHeight: 48,
    paddingLeft: 14, paddingRight: 14,
  },
  sourceButton: {
    alignItems: 'center',
    borderRadius: Radius.small,
    flexDirection: 'row',
    gap: 8,
    minHeight: 32,
    paddingLeft: 10, paddingRight: 10,
  },
  sourceLabel: { fontSize: 14, fontWeight: '500' },
  reviewStats: { fontSize: 12.5 },
  reviewList: { flex: '1 1' },
  contextNote: {
    borderRadius: Radius.small,
    fontSize: 12.5,
    margin: 12,
    padding: 9,
  },
  diffSection: {
    borderBottomWidth: hairline,
    overflow: 'hidden',
  },
  diffFileHeader: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    gap: 8,
    minHeight: 36,
    paddingLeft: 12, paddingRight: 12,
  },
  diffPath: { flex: '1 1', fontSize: 13.5, fontWeight: '500' },
  gitSyncRow: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    gap: 18,
    justifyContent: 'flex-end',
    minHeight: 40,
    paddingLeft: 14, paddingRight: 14,
  },
  gitFileRow: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    gap: 10,
    minHeight: 46,
    paddingLeft: 14, paddingRight: 14,
  },
  gitStatusBadge: {
    alignItems: 'center',
    borderRadius: Radius.small,
    minWidth: 28,
    paddingLeft: 5, paddingRight: 5,
    paddingTop: 2, paddingBottom: 2,
  },
  gitStatusText: { fontFamily: MonoFont, fontSize: 11, fontWeight: '600' },
  gitFileCopy: { flex: '1 1', minWidth: 0 },
  gitFilePath: { fontFamily: MonoFont, fontSize: 13 },
  gitFileMeta: { fontSize: 11.5, marginTop: 1 },
  gitFileActions: { alignItems: 'center', flexDirection: 'row', gap: 12 },
  gitError: {
    borderTopWidth: hairline,
    fontSize: 12.5,
    paddingLeft: 14, paddingRight: 14,
    paddingTop: 8, paddingBottom: 8,
  },
  commitBar: {
    alignItems: 'center',
    borderTopWidth: hairline,
    flexDirection: 'row',
    gap: 10,
    paddingLeft: 14, paddingRight: 14,
    paddingTop: 10,
  },
  commitInput: {
    borderRadius: Radius.small,
    flex: '1 1',
    fontSize: 14.5,
    minHeight: 38,
    paddingLeft: 11, paddingRight: 11,
    paddingTop: 8, paddingBottom: 8,
  },
  commitButton: {
    alignItems: 'center',
    borderRadius: Radius.small,
    justifyContent: 'center',
    minHeight: 38,
    minWidth: 88,
    paddingLeft: 14, paddingRight: 14,
  },
  commitButtonLabel: { color: '#fff', fontSize: 14.5, fontWeight: '600' },
  queueFrontier: { flex: '1 1', fontSize: 13.5 },
  queueActions: { alignItems: 'flex-end', gap: 2 },
  loading: {
    alignItems: 'center',
    flex: '1 1',
    gap: 10,
    justifyContent: 'center',
    padding: 24,
  },
  loadingText: { fontSize: 13.5 },
  message: {
    alignItems: 'center',
    flex: '1 1',
    justifyContent: 'center',
    padding: 28,
  },
  messageTitle: { fontSize: 16, fontWeight: '600', textAlign: 'center' },
  messageDetail: {
    fontSize: 13.5,
    lineHeight: '18px',
    marginTop: 5,
    textAlign: 'center',
  },
} satisfies Record<string, CSSProperties>;
