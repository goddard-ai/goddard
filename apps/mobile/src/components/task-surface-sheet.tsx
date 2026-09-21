import {
  BottomSheetFlatList,
  BottomSheetModal,
  BottomSheetSectionList,
  BottomSheetScrollView,
  BottomSheetView,
  type BottomSheetMethods,
} from "@expo/ui/community/bottom-sheet";
import { MenuView, type MenuAction } from "@expo/ui/community/menu";
import {
  keepPreviousData,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import type {
  AgentInvocation,
  AgentSession,
  GitFileChange,
  Project,
  ReviewDiffSource,
  ReviewEntry,
  ReviewQueue,
  WakuClient,
  WorkingTreeEntry,
} from "@waku/client";
import { TerminalView, type TerminalViewRef } from "expo-libghostty";
import * as Crypto from "expo-crypto";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ActivityIndicator,
  Alert,
  Image,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
  useColorScheme,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { AppSymbol } from "@/components/app-symbol";
import { DiffView } from "@/components/diff-view";
import { liquidGlass } from "@/components/glass-surface";
import { MonoFont, NativeTint, Radius } from "@/constants/theme";
import { useTheme } from "@/hooks/use-theme";
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
} from "@/lib/daemon-api";
import { useDaemon } from "@/lib/daemon-context";
import { sessionCwd } from "@/lib/mobile-runtime";
import { useProviderModels } from "@/hooks/use-daemon-data";
import {
  gitStatusLabel,
  imageMimeForPath,
  latestReviewTurnSource,
  parseNumstat,
  reviewDiffSourceLabel,
  reviewStatusLabel,
  splitReviewPatch,
  upstreamLabel,
  type ReviewPatchFile,
} from "@/lib/task-surfaces";

export type TaskSurface = "terminal" | "files" | "review" | "git" | "queue";

type ReviewPatchSection = ReviewPatchFile & { data: ReviewPatchFile[] };

const SNAP_POINTS = Platform.OS === "ios" ? undefined : ["58%", "100%"];
const MAX_FILE_CHARACTERS = 250_000;

const SURFACE_DETAILS: Record<
  TaskSurface,
  {
    title: string;
    subtitle: string;
    icon: Parameters<typeof AppSymbol>[0]["name"];
  }
> = {
  terminal: {
    title: "Terminal",
    subtitle: "Shell in this task’s workspace",
    icon: { ios: "terminal", android: "terminal", web: "terminal" },
  },
  files: {
    title: "Files",
    subtitle: "Workspace files",
    icon: { ios: "folder", android: "folder", web: "folder" },
  },
  review: {
    title: "Review",
    subtitle: "Workspace changes",
    icon: {
      ios: "doc.text.magnifyingglass",
      android: "difference",
      web: "difference",
    },
  },
  git: {
    title: "Git",
    subtitle: "Branch and working tree",
    icon: {
      ios: "arrow.triangle.branch",
      android: "call_split",
      web: "call_split",
    },
  },
  queue: {
    title: "Review queue",
    subtitle: "Approve and promote commits",
    icon: {
      ios: "checkmark.circle",
      android: "check_circle",
      web: "check_circle",
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
  const theme = useTheme();
  const sheet = useRef<BottomSheetMethods>(null);

  useEffect(() => {
    if (visible && surface) sheet.current?.present();
    else sheet.current?.dismiss();
  }, [surface, visible]);

  return (
    <BottomSheetModal
      ref={sheet}
      backgroundStyle={
        surface === "terminal" || !liquidGlass
          ? { backgroundColor: theme.surface }
          : undefined
      }
      enableDynamicSizing={false}
      enablePanDownToClose
      snapPoints={SNAP_POINTS}
      onDismiss={onDismiss}
    >
      <BottomSheetView style={styles.sheet}>
        {surface ? (
          <SurfaceBody project={project} session={session} surface={surface} />
        ) : null}
      </BottomSheetView>
    </BottomSheetModal>
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
  if (surface === "terminal") {
    return <TerminalSurface root={root} sessionId={session.id} />;
  }
  if (surface === "files") {
    return <FilesSurface root={root} />;
  }
  if (surface === "git") {
    return <GitSurface root={root} session={session} />;
  }
  if (surface === "queue") {
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
    <View style={[styles.header, { borderBottomColor: theme.border }]}>
      <View style={[styles.headerIcon, { backgroundColor: theme.overlay }]}>
        <AppSymbol
          name={details.icon}
          size={15}
          tintColor={theme.textSecondary}
        />
      </View>
      <View style={styles.headerCopy}>
        <Text
          numberOfLines={1}
          style={[styles.headerTitle, { color: theme.text }]}
        >
          {details.title}
        </Text>
        <Text
          numberOfLines={1}
          style={[styles.headerSubtitle, { color: theme.textTertiary }]}
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

  if (!root) {
    return (
      <View style={[styles.fill, { backgroundColor: theme.surface }]}>
        <PanelMessage
          detail="This task does not have an available workspace."
          title="No workspace"
        />
      </View>
    );
  }
  if (Platform.OS === "web") {
    return (
      <View style={[styles.fill, { backgroundColor: theme.surface }]}>
        <PanelMessage
          detail="The native terminal is available on iOS and Android."
          title="Unavailable on web"
        />
      </View>
    );
  }

  return (
    <View style={[styles.fill, { backgroundColor: theme.surface }]}>
      <TerminalSession root={root} sessionId={sessionId} />
    </View>
  );
}

function TerminalSession({
  root,
  sessionId,
}: {
  root: string;
  sessionId: string;
}) {
  const daemon = useDaemon();
  const theme = useTheme();
  const colorScheme = useColorScheme();
  const terminal = useRef<TerminalViewRef>(null);
  const terminalId = useRef(Crypto.randomUUID()).current;
  const [error, setError] = useState<string | null>(null);
  const [exited, setExited] = useState(false);

  useEffect(() => {
    const client = daemon.client;
    if (!client || daemon.phase !== "connected") return;
    let disposed = false;
    setError(null);
    setExited(false);
    const unsubscribe = client.subscribe(terminalId, terminalId, (event) => {
      if (event.event.kind === "terminalOutput") {
        const payload = event.event.payload as { data?: unknown };
        if (typeof payload.data !== "string") return;
        void terminal.current?.write(payload.data).catch((cause) => {
          if (!disposed) setError(errorMessage(cause));
        });
      } else if (event.event.kind === "terminalExited") {
        setExited(true);
        void terminal.current?.finish(0).catch(() => {});
      } else if (event.event.kind === "terminalError") {
        setError(
          typeof event.event.payload === "string"
            ? event.event.payload
            : "The terminal connection failed.",
        );
      }
    });

    void client
      .request(
        { type: "openTerminal", cwd: root, cols: 80, rows: 24, owner: sessionId },
        terminalId,
        terminalId,
      )
      .catch((cause) => {
        if (!disposed) setError(errorMessage(cause));
      });

    return () => {
      disposed = true;
      unsubscribe();
      void client
        .notify({ type: "closeTerminal" }, terminalId, terminalId)
        .catch(() => {});
    };
  }, [daemon.client, daemon.phase, root, sessionId, terminalId]);

  const reportTransportError = useCallback((cause: unknown) => {
    setError(errorMessage(cause));
  }, []);

  return (
    <View style={styles.terminalBody}>
      <TerminalView
        ref={terminal}
        fontSize={13}
        style={styles.fill}
        theme={{
          background: theme.surface,
          foreground: theme.text,
          cursorColor: theme.text,
          selectionBackground: colorScheme === "dark" ? "#40546f" : "#c8d8ee",
        }}
        onInput={({ nativeEvent }) => {
          const client = daemon.client;
          if (!client || daemon.phase !== "connected" || exited) return;
          void client
            .notify(
              {
                type: "writeTerminal",
                data: nativeEvent.data,
              },
              terminalId,
              terminalId,
            )
            .catch(reportTransportError);
        }}
        onResize={({ nativeEvent }) => {
          const client = daemon.client;
          if (!client || daemon.phase !== "connected") return;
          void client
            .notify(
              {
                type: "resizeTerminal",
                cols: clampU16(nativeEvent.cols),
                rows: clampU16(nativeEvent.rows),
              },
              terminalId,
              terminalId,
            )
            .catch(() => {});
        }}
      />
      {daemon.phase !== "connected" || error || exited ? (
        <View
          pointerEvents="none"
          style={[styles.terminalStatus, { backgroundColor: theme.surface }]}
        >
          <Text
            numberOfLines={2}
            style={[
              styles.terminalStatusText,
              { color: error ? theme.danger : theme.textSecondary },
            ]}
          >
            {error ?? (exited ? "Shell exited" : "Reconnecting…")}
          </Text>
        </View>
      ) : null}
    </View>
  );
}

function FilesSurface({ root }: { root: string | null }) {
  const daemon = useDaemon();
  const theme = useTheme();
  const insets = useSafeAreaInsets();
  const queryClient = useQueryClient();
  const profileId = daemon.activeProfile?.id ?? "disconnected";
  const [expanded, setExpanded] = useState<string[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const imageMime = selectedPath ? imageMimeForPath(selectedPath) : null;

  useEffect(() => {
    setExpanded([]);
    setSelectedPath(null);
  }, [root]);

  useEffect(() => {
    setEditing(false);
    setDraft("");
    setSaving(false);
  }, [selectedPath]);

  const tree = useQuery({
    queryKey: daemonKeys.workspaceTree(profileId, root ?? "none", expanded),
    queryFn: () => listWorkspaceTree(daemon.client!, root!, expanded),
    enabled: Boolean(daemon.client && daemon.phase === "connected" && root),
    placeholderData: keepPreviousData,
  });
  const file = useQuery({
    queryKey: daemonKeys.workspaceFile(
      profileId,
      root ?? "none",
      selectedPath ?? "none",
    ),
    queryFn: () => readWorkspaceTextFile(daemon.client!, root!, selectedPath!),
    enabled: Boolean(
      daemon.client &&
        daemon.phase === "connected" &&
        root &&
        selectedPath &&
        !imageMime,
    ),
  });
  const image = useQuery({
    queryKey: [
      ...daemonKeys.workspaceFile(
        profileId,
        root ?? "none",
        selectedPath ?? "none",
      ),
      "binary",
    ],
    queryFn: () =>
      readWorkspaceBinaryFile(daemon.client!, root!, selectedPath!),
    enabled: Boolean(
      daemon.client &&
        daemon.phase === "connected" &&
        root &&
        selectedPath &&
        imageMime,
    ),
  });

  const saveFile = useCallback(() => {
    const client = daemon.client;
    if (!client || !root || !selectedPath || saving) return;
    setSaving(true);
    void writeWorkspaceTextFile(client, root, selectedPath, draft)
      .then(async () => {
        setEditing(false);
        await queryClient.invalidateQueries({
          queryKey: daemonKeys.workspaceFile(profileId, root, selectedPath),
        });
        await queryClient.invalidateQueries({
          queryKey: ["daemon", profileId, "workspace-diff"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["daemon", profileId, "git-panel"],
        });
      })
      .catch((cause) =>
        Alert.alert("Couldn’t save file", errorMessage(cause)),
      )
      .finally(() => setSaving(false));
  }, [daemon.client, draft, profileId, queryClient, root, saving, selectedPath]);

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
                    setDraft("");
                  }}
                />
                <HeaderTextButton
                  label={saving ? "Saving…" : "Save"}
                  onPress={saveFile}
                />
              </View>
            ) : (
              <View style={styles.headerActions}>
                <HeaderTextButton
                  label="Edit"
                  onPress={() => {
                    setDraft(file.data ?? "");
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
          style={({ pressed }) => [
            styles.backRow,
            { opacity: pressed ? 0.55 : 1 },
          ]}
        >
          <AppSymbol
            name={{
              ios: "chevron.left",
              android: "arrow_back",
              web: "arrow_back",
            }}
            size={13}
            tintColor={NativeTint}
          />
          <Text style={[styles.backLabel, { color: NativeTint }]}>Files</Text>
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
            <BottomSheetScrollView
              contentContainerStyle={[
                styles.fileImageContent,
                { paddingBottom: Math.max(insets.bottom, 18) + 12 },
              ]}
              horizontal={false}
              showsVerticalScrollIndicator
            >
              <Image
                accessibilityLabel={selectedPath}
                resizeMode="contain"
                source={{
                  uri: `data:${imageMime};base64,${image.data ?? ""}`,
                }}
                style={styles.fileImage}
              />
            </BottomSheetScrollView>
          )
        ) : file.isPending ? (
          <LoadingMessage />
        ) : file.error ? (
          <PanelMessage
            detail={errorMessage(file.error)}
            title="Couldn’t read file"
          />
        ) : editing ? (
          <BottomSheetScrollView
            contentContainerStyle={[
              styles.fileContent,
              { paddingBottom: Math.max(insets.bottom, 18) + 12 },
            ]}
            horizontal={false}
            keyboardShouldPersistTaps="handled"
            showsVerticalScrollIndicator
          >
            <TextInput
              accessibilityLabel={`Edit ${selectedPath}`}
              autoCapitalize="none"
              autoCorrect={false}
              multiline
              onChangeText={setDraft}
              scrollEnabled={false}
              spellCheck={false}
              style={[styles.fileText, styles.fileEditor, { color: theme.text }]}
              value={draft}
            />
          </BottomSheetScrollView>
        ) : (
          <BottomSheetScrollView
            contentContainerStyle={[
              styles.fileContent,
              { paddingBottom: Math.max(insets.bottom, 18) + 12 },
            ]}
            horizontal={false}
            showsVerticalScrollIndicator
          >
            <Text selectable style={[styles.fileText, { color: theme.text }]}>
              {(file.data ?? "").slice(0, MAX_FILE_CHARACTERS)}
            </Text>
            {truncated ? (
              <Text style={[styles.truncated, { color: theme.textTertiary }]}>
                File truncated in this viewer.
              </Text>
            ) : null}
          </BottomSheetScrollView>
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
        <BottomSheetFlatList<WorkingTreeEntry>
          contentContainerStyle={{
            paddingBottom: Math.max(insets.bottom, 18) + 12,
          }}
          data={tree.data ?? []}
          keyExtractor={(entry) => entry.relativePath}
          keyboardShouldPersistTaps="handled"
          ListEmptyComponent={
            <PanelMessage
              detail="The workspace has no visible files."
              title="No files"
            />
          }
          renderItem={({ item }) => (
            <Pressable
              accessibilityHint={
                item.isDir
                  ? item.expanded
                    ? "Collapses folder"
                    : "Expands folder"
                  : "Opens file"
              }
              accessibilityRole="button"
              accessibilityState={{
                expanded: item.isDir ? item.expanded : undefined,
              }}
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
              style={({ pressed }) => [
                styles.fileRow,
                {
                  backgroundColor: pressed
                    ? theme.overlayStrong
                    : "transparent",
                  paddingLeft: 14 + Math.min(item.depth, 12) * 16,
                },
              ]}
            >
              <AppSymbol
                name={
                  item.isDir
                    ? {
                        ios: item.expanded ? "chevron.down" : "chevron.right",
                        android: item.expanded
                          ? "expand_more"
                          : "chevron_right",
                        web: item.expanded ? "expand_more" : "chevron_right",
                      }
                    : { ios: "doc", android: "draft", web: "draft" }
                }
                size={item.isDir ? 12 : 14}
                tintColor={theme.textTertiary}
              />
              <Text
                numberOfLines={1}
                style={[styles.fileRowLabel, { color: theme.text }]}
              >
                {item.name}
              </Text>
            </Pressable>
          )}
        />
      )}
    </View>
  );
}

const STANDARD_REVIEW_SOURCES: Array<{
  id: Exclude<ReviewDiffSource, object>;
  source: Exclude<ReviewDiffSource, object>;
}> = [
  { id: "uncommitted", source: "uncommitted" },
  { id: "unstaged", source: "unstaged" },
  { id: "staged", source: "staged" },
  { id: "committed", source: "committed" },
  { id: "branch", source: "branch" },
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
  const insets = useSafeAreaInsets();
  const profileId = daemon.activeProfile?.id ?? "disconnected";
  const lastTurn = useMemo(() => latestReviewTurnSource(session), [session]);
  const [source, setSource] = useState<ReviewDiffSource>("uncommitted");

  useEffect(() => setSource("uncommitted"), [session.id]);

  const diff = useQuery({
    queryKey: daemonKeys.workspaceDiff(profileId, root ?? "none", source),
    queryFn: () => collectWorkspaceDiff(daemon.client!, root!, source),
    enabled: Boolean(daemon.client && daemon.phase === "connected" && root),
    placeholderData: keepPreviousData,
  });
  const files = useMemo(
    () => splitReviewPatch(diff.data?.patch ?? ""),
    [diff.data?.patch],
  );
  const sections = useMemo<ReviewPatchSection[]>(
    () => files.map((file) => ({ ...file, data: [file] })),
    [files],
  );
  const stats = useMemo(
    () => parseNumstat(diff.data?.numstat ?? ""),
    [diff.data?.numstat],
  );
  const actions = useMemo<MenuAction[]>(
    () => [
      ...(lastTurn
        ? [
            {
              id: "last-turn",
              title: reviewDiffSourceLabel(lastTurn),
              image: "clock.arrow.circlepath" as const,
              state:
                typeof source === "object" ? ("on" as const) : ("off" as const),
            },
          ]
        : []),
      ...STANDARD_REVIEW_SOURCES.map((item) => ({
        id: item.id,
        title: reviewDiffSourceLabel(item.source),
        state: source === item.source ? ("on" as const) : ("off" as const),
      })),
    ],
    [lastTurn, source],
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
        style={[styles.reviewControls, { borderBottomColor: theme.border }]}
      >
        <MenuView
          actions={actions}
          onPressAction={({ nativeEvent }) => {
            if (nativeEvent.event === "last-turn" && lastTurn) {
              setSource(lastTurn);
              return;
            }
            const item = STANDARD_REVIEW_SOURCES.find(
              (choice) => choice.id === nativeEvent.event,
            );
            if (item) setSource(item.source);
          }}
        >
          <View
            accessible
            accessibilityLabel="Choose review source"
            accessibilityRole="button"
            style={[styles.sourceButton, { backgroundColor: theme.overlay }]}
          >
            <Text style={[styles.sourceLabel, { color: theme.text }]}>
              {reviewDiffSourceLabel(source)}
            </Text>
            <AppSymbol
              name={{
                ios: "chevron.up.chevron.down",
                android: "unfold_more",
                web: "unfold_more",
              }}
              size={12}
              tintColor={theme.textTertiary}
            />
          </View>
        </MenuView>
        <Text style={styles.reviewStats}>
          <Text style={{ color: theme.textTertiary }}>
            {stats.files} {stats.files === 1 ? "file" : "files"}{" "}
          </Text>
          <Text style={{ color: theme.success }}>+{stats.additions}</Text>
          <Text style={{ color: theme.textGhost }}> </Text>
          <Text style={{ color: theme.danger }}>−{stats.deletions}</Text>
        </Text>
      </View>
      {diff.isPending ? (
        <LoadingMessage />
      ) : diff.error ? (
        <PanelMessage
          detail={errorMessage(diff.error)}
          title="Couldn’t load review"
        />
      ) : (
        <BottomSheetSectionList<ReviewPatchFile, ReviewPatchSection>
          contentContainerStyle={[
            styles.reviewList,
            { paddingBottom: Math.max(insets.bottom, 18) + 12 },
          ]}
          sections={sections}
          keyExtractor={(file) => file.key}
          stickySectionHeadersEnabled
          ListHeaderComponent={
            !diff.data?.completeContext ? (
              <Text
                style={[
                  styles.contextNote,
                  { color: theme.warning, backgroundColor: theme.warningSoft },
                ]}
              >
                This comparison has partial context.
              </Text>
            ) : null
          }
          ListEmptyComponent={
            <PanelMessage
              detail="There are no changes in this comparison."
              title="No changes"
            />
          }
          renderSectionHeader={({ section }) => (
            <View
              style={[
                styles.diffFileHeader,
                {
                  backgroundColor: theme.surface,
                  borderBottomColor: theme.border,
                },
              ]}
            >
              <AppSymbol
                name={{ ios: "doc", android: "draft", web: "draft" }}
                size={13}
                tintColor={theme.textTertiary}
              />
              <Text
                numberOfLines={1}
                style={[styles.diffPath, { color: theme.textSecondary }]}
              >
                {section.path}
              </Text>
            </View>
          )}
          renderItem={({ item }) => (
            <View
              style={[
                styles.diffSection,
                { borderBottomColor: theme.border },
              ]}
            >
              <DiffView diff={item.patch} mode="review" />
            </View>
          )}
        />
      )}
    </View>
  );
}

type GitSection = {
  key: "staged" | "unstaged";
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
  const insets = useSafeAreaInsets();
  const queryClient = useQueryClient();
  const profileId = daemon.activeProfile?.id ?? "disconnected";
  const probe = useProviderModels(session.provider);
  const [message, setMessage] = useState("");
  const [pending, setPending] = useState<string | null>(null);
  const [opError, setOpError] = useState<string | null>(null);

  useEffect(() => {
    setMessage("");
    setOpError(null);
  }, [root, session.id]);

  const panel = useQuery({
    queryKey: daemonKeys.gitPanel(profileId, root ?? "none"),
    queryFn: () => inspectGitPanel(daemon.client!, root!),
    enabled: Boolean(daemon.client && daemon.phase === "connected" && root),
    placeholderData: keepPreviousData,
  });
  const snapshot = panel.data ?? null;
  const stagedCount = snapshot?.staged.length ?? 0;
  const unstagedCount = snapshot?.unstaged.length ?? 0;

  // Everything that lists this working tree shares the invalidation.
  const refresh = useCallback(() => {
    void queryClient.invalidateQueries({
      queryKey: daemonKeys.gitPanel(profileId, root ?? "none"),
    });
    void queryClient.invalidateQueries({
      queryKey: ["daemon", profileId, "workspace-diff", root ?? "none"],
    });
    void queryClient.invalidateQueries({
      queryKey: ["daemon", profileId, "workspace-tree", root ?? "none"],
    });
  }, [profileId, queryClient, root]);

  const runOp = useCallback(
    <T,>(label: string, op: (client: WakuClient) => Promise<T>, onSuccess?: (value: T) => void) => {
      const client = daemon.client;
      if (!client || !root || pending) return;
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
    if (!binary) return null;
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
      if (!root) return;
      const typed = message.trim();
      runOp(
        "commit",
        async (client) => {
          const finalMessage =
            typed ||
            (await generateWorkspaceCommitMessage(client, root, includeUnstaged, invocation!));
          await commitWorkspace(client, root, finalMessage, includeUnstaged, false);
        },
        () => setMessage(""),
      );
    },
    [invocation, message, root, runOp],
  );

  const confirmCommit = useCallback(() => {
    if (stagedCount === 0 && unstagedCount > 0) {
      Alert.alert(
        "Nothing staged",
        `Commit all ${unstagedCount} ${unstagedCount === 1 ? "change" : "changes"}?`,
        [
          { text: "Cancel", style: "cancel" },
          { text: "Commit all", onPress: () => commit(true) },
        ],
      );
      return;
    }
    commit(false);
  }, [commit, stagedCount, unstagedCount]);

  const discard = useCallback(
    (file: GitFileChange) => {
      Alert.alert(
        `Discard changes to ${file.path}?`,
        file.untracked
          ? "This deletes the untracked file and cannot be undone."
          : "This restores the file to HEAD and cannot be undone.",
        [
          { text: "Cancel", style: "cancel" },
          {
            text: "Discard",
            style: "destructive",
            onPress: () =>
              runOp("discard", (client) => discardWorkspaceFile(client, root!, file.path)),
          },
        ],
      );
    },
    [root, runOp],
  );

  const sections = useMemo<GitSection[]>(() => {
    if (!snapshot) return [];
    const all: GitSection[] = [
      { key: "staged", title: "Staged", data: snapshot.staged },
      { key: "unstaged", title: "Changes", data: snapshot.unstaged },
    ];
    return all.filter((section) => section.data.length > 0);
  }, [snapshot]);

  const canCommit = Boolean(
    snapshot && (stagedCount > 0 || unstagedCount > 0) && (message.trim() || invocation),
  );
  const behind = snapshot?.upstream?.behind ?? 0;
  const subtitle = snapshot
    ? [snapshot.branch, upstreamLabel(snapshot.upstream)].filter(Boolean).join(" · ")
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
            <View style={[styles.gitSyncRow, { borderBottomColor: theme.border }]}>
              {behind > 0 ? (
                <HeaderTextButton
                  label={pending === "pull" ? "Pulling…" : `Pull ↓${behind}`}
                  onPress={() =>
                    runOp("pull", async (client) => {
                      const outcome = await pullWorkspace(client, root);
                      if (outcome !== "clean") {
                        throw new Error(
                          `Pull stopped on a conflict in ${outcome.conflict.files.length} ${
                            outcome.conflict.files.length === 1 ? "file" : "files"
                          } — resolve it on the host.`,
                        );
                      }
                    })
                  }
                />
              ) : null}
              {snapshot.can_push ? (
                <HeaderTextButton
                  label={pending === "push" ? "Pushing…" : "Push"}
                  onPress={() => runOp("push", (client) => pushWorkspace(client, root))}
                />
              ) : null}
            </View>
          ) : null}
          <BottomSheetSectionList<GitFileChange, GitSection>
            contentContainerStyle={{
              paddingBottom: 12,
            }}
            sections={sections}
            keyExtractor={(file) => file.path}
            stickySectionHeadersEnabled
            ListEmptyComponent={
              <PanelMessage detail="The working tree is clean." title="No changes" />
            }
            renderSectionHeader={({ section }) => (
              <View
                style={[
                  styles.diffFileHeader,
                  { backgroundColor: theme.surface, borderBottomColor: theme.border },
                ]}
              >
                <Text style={[styles.diffPath, { color: theme.textSecondary }]}>
                  {section.title} ({section.data.length})
                </Text>
              </View>
            )}
            renderItem={({ item, section }) => (
              <View style={[styles.gitFileRow, { borderBottomColor: theme.border }]}>
                <View style={[styles.gitStatusBadge, { backgroundColor: theme.overlay }]}>
                  <Text style={[styles.gitStatusText, { color: theme.textTertiary }]}>
                    {item.untracked ? "??" : item.status}
                  </Text>
                </View>
                <View style={styles.gitFileCopy}>
                  <Text numberOfLines={1} style={[styles.gitFilePath, { color: theme.text }]}>
                    {item.path}
                  </Text>
                  <Text style={[styles.gitFileMeta, { color: theme.textGhost }]}>
                    {gitStatusLabel(item.status, item.untracked)}
                    {item.additions || item.deletions
                      ? ` · +${item.additions} −${item.deletions}`
                      : ""}
                  </Text>
                </View>
                {section.key === "staged" ? (
                  <HeaderTextButton
                    label="Unstage"
                    onPress={() =>
                      runOp("unstage", (client) => unstageWorkspaceFile(client, root, item.path))
                    }
                  />
                ) : (
                  <View style={styles.gitFileActions}>
                    <HeaderTextButton
                      label="Stage"
                      onPress={() =>
                        runOp("stage", (client) => stageWorkspaceFile(client, root, item.path))
                      }
                    />
                    <Pressable
                      accessibilityLabel={`Discard ${item.path}`}
                      accessibilityRole="button"
                      hitSlop={6}
                      onPress={() => discard(item)}
                      style={({ pressed }) => [
                        styles.textButton,
                        { opacity: pressed ? 0.5 : 1 },
                      ]}
                    >
                      <Text style={[styles.textButtonLabel, { color: theme.danger }]}>
                        Discard
                      </Text>
                    </Pressable>
                  </View>
                )}
              </View>
            )}
          />
          {opError ? (
            <Text style={[styles.gitError, { color: theme.danger }]}>{opError}</Text>
          ) : null}
          <View
            style={[
              styles.commitBar,
              {
                backgroundColor: theme.surface,
                borderTopColor: theme.border,
                paddingBottom: Math.max(insets.bottom, 10),
              },
            ]}
          >
            <TextInput
              editable={pending == null}
              onChangeText={setMessage}
              placeholder={
                invocation ? "Message (blank generates one)" : "Commit message"
              }
              placeholderTextColor={theme.textGhost}
              style={[
                styles.commitInput,
                { backgroundColor: theme.overlay, color: theme.text },
              ]}
              value={message}
            />
            <Pressable
              accessibilityHint={
                message.trim()
                  ? "Commits the staged changes"
                  : "Generates a message and commits"
              }
              accessibilityLabel="Commit"
              accessibilityRole="button"
              accessibilityState={{ disabled: !canCommit || pending != null }}
              disabled={!canCommit || pending != null}
              onPress={confirmCommit}
              style={({ pressed }) => [
                styles.commitButton,
                { backgroundColor: theme.accent },
                (!canCommit || pending != null || pressed) && { opacity: 0.5 },
              ]}
            >
              {pending === "commit" ? (
                <ActivityIndicator color="#fff" size="small" />
              ) : (
                <Text style={styles.commitButtonLabel}>
                  {stagedCount > 0 ? "Commit" : "Commit all"}
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
  const queryClient = useQueryClient();
  const profileId = daemon.activeProfile?.id ?? "disconnected";
  const [pending, setPending] = useState<string | null>(null);
  const [opError, setOpError] = useState<string | null>(null);

  const queue = useQuery({
    queryKey: daemonKeys.reviewQueue(profileId, root ?? "none"),
    queryFn: () => listReviewQueue(daemon.client!, root!),
    enabled: Boolean(daemon.client && daemon.phase === "connected" && root),
  });
  const snapshot = queue.data;

  const refresh = useCallback(async () => {
    await queryClient.invalidateQueries({
      queryKey: daemonKeys.reviewQueue(profileId, root ?? "none"),
    });
    await queryClient.invalidateQueries({
      queryKey: ["daemon", profileId, "git-panel"],
    });
  }, [profileId, queryClient, root]);

  const runOp = useCallback(
    (
      label: string,
      op: (client: WakuClient) => Promise<ReviewQueue | null>,
    ) => {
      const client = daemon.client;
      if (!client || !root || pending) return;
      setPending(label);
      setOpError(null);
      void op(client)
        .catch((cause) => setOpError(errorMessage(cause)))
        .finally(() => {
          setPending(null);
          void refresh();
        });
    },
    [daemon.client, pending, refresh, root],
  );

  const confirmPromote = useCallback(() => {
    if (!snapshot?.frontier) return;
    const target = snapshot.baseBranch ?? "main";
    Alert.alert(
      `Promote to ${target}?`,
      `Fast-forwards ${target} through the approved commits.`,
      [
        { text: "Cancel", style: "cancel" },
        {
          text: "Promote",
          onPress: () =>
            runOp("promote", (client) => promoteReviewQueue(client, root!)),
        },
      ],
    );
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
              ? `qa → ${snapshot.baseBranch}`
              : "qa"
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
          detail="This repository has no origin/qa branch."
          title="No review queue"
        />
      ) : (
        <>
          {snapshot.frontier ? (
            <View
              style={[styles.gitSyncRow, { borderBottomColor: theme.border }]}
            >
              <Text
                style={[styles.queueFrontier, { color: theme.textSecondary }]}
              >
                {`Promotable to ${snapshot.baseBranch ?? "main"}`}
              </Text>
              <HeaderTextButton
                label={pending === "promote" ? "Promoting…" : "Promote"}
                onPress={confirmPromote}
              />
            </View>
          ) : null}
          <BottomSheetFlatList<ReviewEntry>
            contentContainerStyle={{ paddingBottom: 12 }}
            data={snapshot.entries}
            keyExtractor={(entry) => entry.commit.sha}
            ListEmptyComponent={
              <PanelMessage
                detail="Nothing is waiting for review."
                title="Queue is empty"
              />
            }
            renderItem={({ item }) => (
              <View
                style={[styles.gitFileRow, { borderBottomColor: theme.border }]}
              >
                <View style={styles.gitFileCopy}>
                  <Text
                    numberOfLines={2}
                    style={[styles.gitFilePath, { color: theme.text }]}
                  >
                    {item.commit.subject}
                  </Text>
                  <Text
                    style={[styles.gitFileMeta, { color: theme.textGhost }]}
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
                            item.testPlans.length === 1 ? "item" : "items"
                          }`
                        : null,
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </Text>
                  {item.reviews.length > 0 ? (
                    <Text
                      style={[styles.gitFileMeta, { color: theme.textGhost }]}
                    >
                      {item.reviews
                        .map(
                          (record) =>
                            `${record.reviewer}: ${record.decision}`,
                        )
                        .join(" · ")}
                    </Text>
                  ) : null}
                </View>
                {!item.reverted ? (
                  <View style={styles.queueActions}>
                    <HeaderTextButton
                      label={
                        pending === `approve:${item.commit.sha}`
                          ? "…"
                          : "Approve"
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
                      hitSlop={6}
                      onPress={() =>
                        runOp(`reject:${item.commit.sha}`, (client) =>
                          rejectReviewCommit(client, root, item.commit.sha),
                        )
                      }
                      style={({ pressed }) => [
                        styles.textButton,
                        { opacity: pressed ? 0.5 : 1 },
                      ]}
                    >
                      <Text
                        style={[
                          styles.textButtonLabel,
                          {
                            color:
                              pending === `reject:${item.commit.sha}`
                                ? theme.textGhost
                                : theme.danger,
                          },
                        ]}
                      >
                        Reject
                      </Text>
                    </Pressable>
                  </View>
                ) : null}
              </View>
            )}
          />
          {opError ? (
            <Text style={[styles.gitError, { color: theme.danger }]}>
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
      hitSlop={6}
      onPress={onPress}
      style={({ pressed }) => [
        styles.textButton,
        { opacity: pressed ? 0.5 : 1 },
      ]}
    >
      <Text style={[styles.textButtonLabel, { color: NativeTint }]}>
        {label}
      </Text>
    </Pressable>
  );
}

function LoadingMessage() {
  const theme = useTheme();
  return (
    <View accessibilityLabel="Loading" style={styles.loading}>
      <ActivityIndicator color={NativeTint} />
      <Text style={[styles.loadingText, { color: theme.textTertiary }]}>
        Loading…
      </Text>
    </View>
  );
}

function PanelMessage({ title, detail }: { title: string; detail: string }) {
  const theme = useTheme();
  return (
    <View style={styles.message}>
      <Text style={[styles.messageTitle, { color: theme.text }]}>{title}</Text>
      <Text style={[styles.messageDetail, { color: theme.textTertiary }]}>
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

const styles = StyleSheet.create({
  sheet: { flex: 1, minHeight: 0 },
  fill: { flex: 1, minHeight: 0 },
  header: {
    alignItems: "center",
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: 10,
    minHeight: 54,
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  headerIcon: {
    alignItems: "center",
    borderRadius: Radius.small,
    height: 30,
    justifyContent: "center",
    width: 30,
  },
  headerCopy: { flex: 1, minWidth: 0 },
  headerTitle: { fontSize: 16, fontWeight: "600" },
  headerSubtitle: { fontSize: 12.5, marginTop: 1 },
  textButton: { justifyContent: "center", minHeight: 36, paddingHorizontal: 4 },
  textButtonLabel: { fontSize: 15, fontWeight: "500" },
  terminalBody: { flex: 1, minHeight: 0, position: "relative" },
  terminalStatus: {
    borderRadius: Radius.small,
    bottom: 10,
    maxWidth: "80%",
    paddingHorizontal: 9,
    paddingVertical: 6,
    position: "absolute",
    right: 10,
  },
  terminalStatusText: { fontSize: 12 },
  backRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 5,
    minHeight: 38,
    paddingHorizontal: 15,
  },
  backLabel: { fontSize: 15.5, fontWeight: "500" },
  headerActions: { alignItems: "center", flexDirection: "row", gap: 14 },
  fileContent: { paddingHorizontal: 14, paddingTop: 4 },
  fileText: { fontFamily: MonoFont, fontSize: 12.5, lineHeight: 17 },
  fileEditor: { minHeight: 200, textAlignVertical: "top" },
  fileImage: { aspectRatio: 1, width: "100%" },
  fileImageContent: { flexGrow: 1, justifyContent: "center", padding: 12 },
  truncated: { fontSize: 12, marginTop: 14 },
  fileRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 8,
    minHeight: 39,
    paddingRight: 14,
  },
  fileRowLabel: { flex: 1, fontSize: 14.5 },
  reviewControls: {
    alignItems: "center",
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    justifyContent: "space-between",
    minHeight: 48,
    paddingHorizontal: 14,
  },
  sourceButton: {
    alignItems: "center",
    borderRadius: Radius.small,
    flexDirection: "row",
    gap: 8,
    minHeight: 32,
    paddingHorizontal: 10,
  },
  sourceLabel: { fontSize: 14, fontWeight: "500" },
  reviewStats: { fontSize: 12.5 },
  reviewList: { paddingTop: 0 },
  contextNote: {
    borderRadius: Radius.small,
    fontSize: 12.5,
    margin: 12,
    padding: 9,
  },
  diffSection: {
    borderBottomWidth: StyleSheet.hairlineWidth,
    overflow: "hidden",
  },
  diffFileHeader: {
    alignItems: "center",
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: 8,
    minHeight: 36,
    paddingHorizontal: 12,
  },
  diffPath: { flex: 1, fontSize: 13.5, fontWeight: "500" },
  gitSyncRow: {
    alignItems: "center",
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: 18,
    justifyContent: "flex-end",
    minHeight: 40,
    paddingHorizontal: 14,
  },
  gitFileRow: {
    alignItems: "center",
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: 10,
    minHeight: 46,
    paddingHorizontal: 14,
  },
  gitStatusBadge: {
    alignItems: "center",
    borderRadius: Radius.small,
    minWidth: 28,
    paddingHorizontal: 5,
    paddingVertical: 2,
  },
  gitStatusText: { fontFamily: MonoFont, fontSize: 11, fontWeight: "600" },
  gitFileCopy: { flex: 1, minWidth: 0 },
  gitFilePath: { fontFamily: MonoFont, fontSize: 13 },
  gitFileMeta: { fontSize: 11.5, marginTop: 1 },
  gitFileActions: { alignItems: "center", flexDirection: "row", gap: 12 },
  gitError: {
    borderTopWidth: StyleSheet.hairlineWidth,
    fontSize: 12.5,
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  commitBar: {
    alignItems: "center",
    borderTopWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: 10,
    paddingHorizontal: 14,
    paddingTop: 10,
  },
  commitInput: {
    borderRadius: Radius.small,
    flex: 1,
    fontSize: 14.5,
    minHeight: 38,
    paddingHorizontal: 11,
    paddingVertical: 8,
  },
  commitButton: {
    alignItems: "center",
    borderRadius: Radius.small,
    justifyContent: "center",
    minHeight: 38,
    minWidth: 88,
    paddingHorizontal: 14,
  },
  commitButtonLabel: { color: "#fff", fontSize: 14.5, fontWeight: "600" },
  queueFrontier: { flex: 1, fontSize: 13.5 },
  queueActions: { alignItems: "flex-end", gap: 2 },
  loading: {
    alignItems: "center",
    flex: 1,
    gap: 10,
    justifyContent: "center",
    padding: 24,
  },
  loadingText: { fontSize: 13.5 },
  message: {
    alignItems: "center",
    flex: 1,
    justifyContent: "center",
    padding: 28,
  },
  messageTitle: { fontSize: 16, fontWeight: "600", textAlign: "center" },
  messageDetail: {
    fontSize: 13.5,
    lineHeight: 18,
    marginTop: 5,
    textAlign: "center",
  },
});
