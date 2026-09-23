import type {
  AgentInvocation,
  AgentSession,
  BranchSnapshot,
  ComposerDraftChange,
  DaemonSettings,
  FileEntry,
  GitPanelSnapshot,
  LandOutcome,
  NotificationPoll,
  Project,
  ProviderKind,
  ProviderProbe,
  ProviderSessionCatalogStatus,
  ProviderSessionSummary,
  ProviderSessionHistory,
  PullOutcome,
  ReviewDiffData,
  ReviewDiffSource,
  ReviewQueue,
  ResponsePayload,
  SessionMessageMatch,
  SessionMessageSearchScope,
  SlashCommand,
  WakuClient,
  WorkingTreeEntry,
  WorkspaceOperation,
  WorkspaceResult,
} from '@waku/client';

export type TaskState = Extract<ResponsePayload, { type: 'taskState' }>;
export type DaemonDirectory = Extract<WorkspaceResult, { type: 'directory' }>;

export const daemonKeys = {
  taskState: (profileId: string) => ['daemon', profileId, 'task-state'] as const,
  session: (profileId: string, sessionId: string) => [
    'daemon',
    profileId,
    'session',
    sessionId,
  ] as const,
  settings: (profileId: string) => ['daemon', profileId, 'settings'] as const,
  provider: (profileId: string, provider: ProviderKind) => [
    'daemon',
    profileId,
    'provider',
    provider,
  ] as const,
  directory: (profileId: string, path: string | null) => [
    'daemon',
    profileId,
    'directory',
    path ?? 'home',
  ] as const,
  branches: (profileId: string, cwd: string) => [
    'daemon',
    profileId,
    'branches',
    cwd,
  ] as const,
  workspaceTree: (profileId: string, root: string, expandedPaths: string[]) => [
    'daemon',
    profileId,
    'workspace-tree',
    root,
    expandedPaths,
  ] as const,
  workspaceFile: (profileId: string, root: string, relativePath: string) => [
    'daemon',
    profileId,
    'workspace-file',
    root,
    relativePath,
  ] as const,
  composerCommands: (
    profileId: string,
    provider: ProviderKind | null,
    root: string | null,
    binaryOverride: string | null,
  ) => ['daemon', profileId, 'composer-commands', provider, root, binaryOverride] as const,
  composerFiles: (profileId: string, root: string | null) => [
    'daemon', profileId, 'composer-files', root,
  ] as const,
  providerSessions: (profileId: string, provider: ProviderKind) => [
    'daemon', profileId, 'provider-sessions', provider,
  ] as const,
  workspaceDiff: (profileId: string, root: string, source: ReviewDiffSource) => [
    'daemon',
    profileId,
    'workspace-diff',
    root,
    source,
  ] as const,
  gitPanel: (profileId: string, root: string) => [
    'daemon',
    profileId,
    'git-panel',
    root,
  ] as const,
  reviewQueue: (profileId: string, cwd: string) => [
    'daemon',
    profileId,
    'review-queue',
    cwd,
  ] as const,
  notifications: (profileId: string, all: boolean) => [
    'daemon',
    profileId,
    'notifications',
    all,
  ] as const,
};

export async function loadTaskState(client: WakuClient): Promise<TaskState> {
  const state = expectResponse(await client.request({ type: 'loadTaskState' }), 'taskState');
  // Catalog entries are list projections: their workspace and transcript
  // fields are placeholders. Flag them so a later save merges only the
  // columns the projection actually carries.
  state.sessions = state.sessions.map((session) => ({ ...session, detail_loaded: false }));
  return state;
}

export async function hydrateSession(
  client: WakuClient,
  sessionId: string,
): Promise<AgentSession | null> {
  const response = expectResponse(
    await client.request({ type: 'hydrateSession', sessionId }),
    'session',
  );
  return response.session && { ...response.session, detail_loaded: true };
}

export async function searchSessionMessages(
  client: WakuClient,
  query: string,
  scope: SessionMessageSearchScope,
): Promise<SessionMessageMatch[]> {
  const response = expectResponse(
    await client.request({ type: 'searchSessionMessages', query, limit: 50, scope }),
    'sessionMessageMatches',
  );
  return response.matches;
}

/** Checkpoint turn counts that have a git ref — desktop's
 * `checkpoint_ref_cache` source. Drives the rewind affordance. */
export async function listSessionTurnRefs(
  client: WakuClient,
  cwd: string,
  sessionId: string,
): Promise<Set<number>> {
  const response = await client.request({
    type: 'workspace',
    operation: { type: 'sessionTurnRefs', cwd, session_id: sessionId },
  });
  const result = expectResponse(response, 'workspace').result;
  return result.type === 'turnRefs' ? new Set(result.turn_counts) : new Set();
}

export async function listProviderSessions(
  client: WakuClient,
  provider: ProviderKind,
): Promise<{ sessions: ProviderSessionSummary[]; status: ProviderSessionCatalogStatus | undefined }> {
  const response = expectResponse(
    await client.request({ type: 'listProviderSessions', provider, limit: 250 }),
    'providerSessions',
  );
  return { sessions: response.sessions, status: response.status };
}

export async function loadProviderSessionHistory(
  client: WakuClient,
  summary: ProviderSessionSummary,
): Promise<{ history: ProviderSessionHistory; resolvedCwd: string | null | undefined }> {
  const response = expectResponse(await client.request({
    type: 'loadProviderSession', cursor: summary.cursor, cwd: summary.cwd,
  }), 'providerSessionHistory');
  return { history: response.history, resolvedCwd: response.resolvedCwd };
}

export function providerSessionKey(cursor: ProviderSessionSummary['cursor']): string {
  const id = cursor.provider === 'codex' || cursor.provider === 'amp'
    ? cursor.threadId
    : cursor.provider === 'antigravity'
      ? cursor.conversationId
      : cursor.sessionId;
  return `${cursor.provider}:${id}`;
}

export async function attachDaemonSession(
  client: WakuClient,
  sessionId: string,
): Promise<{
  runtimeId: string;
  supportsSteer: boolean;
  supportsUserInputActions: boolean;
} | null> {
  const response = expectResponse(
    await client.request({ type: 'attachSession' }, sessionId),
    'sessionRuntime',
  );
  return response.runtimeId
    ? {
        runtimeId: response.runtimeId,
        supportsSteer: response.supportsSteer,
        supportsUserInputActions: response.supportsUserInputActions,
      }
    : null;
}

/** Fields the daemon omits on the wire when empty get their concrete
 * defaults so readers never re-check for presence. */
export function normalizeDaemonSettings(settings: DaemonSettings): DaemonSettings {
  return {
    ...settings,
    provider_binary_overrides: settings.provider_binary_overrides ?? {},
    custom_commands: settings.custom_commands ?? [],
  };
}

export async function loadDaemonSettings(client: WakuClient): Promise<DaemonSettings> {
  const response = expectResponse(await client.request({ type: 'getSettings' }), 'settings');
  return normalizeDaemonSettings(response.settings);
}

export async function probeProvider(
  client: WakuClient,
  provider: ProviderKind,
  settings: DaemonSettings,
  options: { discoverModels?: boolean; probeVersion?: boolean } = {},
): Promise<ProviderProbe & { version: string | null }> {
  const response = expectResponse(
    await client.request({
      type: 'probeProvider',
      provider,
      binaryOverride: settings.provider_binary_overrides?.[provider] ?? null,
      discoverModels: options.discoverModels ?? true,
      probeVersion: options.probeVersion ?? false,
    }),
    'providerProbe',
  );
  return { ...response.probe, version: response.version ?? null };
}

export async function browseDaemonDirectory(
  client: WakuClient,
  path: string | null,
): Promise<DaemonDirectory> {
  const response = expectResponse(
    await client.request({ type: 'workspace', operation: { type: 'browseDirectory', path } }),
    'workspace',
  );
  if (response.result.type !== 'directory') {
    throw new Error('The daemon returned an unexpected directory response');
  }
  return response.result;
}

export function createProject(
  rawPath: string,
  id: string,
  createdAt = Math.floor(Date.now() / 1_000),
): Project {
  const input = rawPath.trim();
  if (!input.startsWith('/') && !/^[a-z]:[\\/]/i.test(input)) {
    throw new Error('Enter an absolute path on the daemon host');
  }
  const path = input === '/' ? input : input.replace(/[\\/]+$/, '');
  const name = path.split(/[\\/]/).filter(Boolean).at(-1) ?? 'Project';
  return { id, name, path, created_at: createdAt, temporary: false, starred: false };
}

export async function persistProject(
  client: WakuClient,
  candidate: Project,
): Promise<{ project: Project; taskState: TaskState }> {
  const current = await loadTaskState(client);
  const existing = current.projects.find((project) => project.path === candidate.path);
  if (existing) return { project: existing, taskState: current };
  const projects = [...current.projects, candidate];
  expectResponse(
    await client.request({
      type: 'saveTaskState',
      projects,
      liveSessionIds: current.sessions.map((session) => session.id),
      sessions: [],
      sessionTails: [],
    }),
    'taskStateSaved',
  );
  return { project: candidate, taskState: { ...current, projects } };
}

export async function createProjectlessWorkspace(client: WakuClient): Promise<string> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'createProjectlessWorkspace', prompt: null },
    }),
    'workspace',
  );
  if (response.result.type !== 'projectlessWorkspace') {
    throw new Error('The daemon returned an unexpected workspace response');
  }
  return response.result.cwd;
}

/** `/land`: rebase the workspace onto its base branch — or merge it in —
 * then fast-forward the base to the result. `base` is the session's recorded
 * base; `null` lets the daemon resolve the repository's default. */
export async function landWorkspace(
  client: WakuClient,
  cwd: string,
  base: string | null,
): Promise<LandOutcome> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'land', cwd, base, strategy: 'rebase' },
    }),
    'workspace',
  );
  if (response.result.type !== 'land') {
    throw new Error('The daemon returned an unexpected land response');
  }
  return response.result.outcome;
}

export async function materializeWorktree(
  client: WakuClient,
  session: AgentSession,
  projectPath: string,
): Promise<AgentSession> {
  if (session.workspace?.kind !== 'newWorktree') return session;
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: {
        type: 'createWorktree',
        project_path: projectPath,
        name: null,
        base_ref: session.workspace.baseBranch ?? null,
        sync_default_branch: false,
        sync_branches: [],
      },
    }),
    'workspace',
  );
  if (response.result.type !== 'worktreeCreated') {
    throw new Error('The daemon returned an unexpected worktree result');
  }
  return {
    ...session,
    workspace: {
      kind: 'worktree',
      path: response.result.worktree.path,
      name: response.result.worktree.name,
      branch: null,
    },
  };
}

export async function loadComposerDrafts(
  client: WakuClient,
): Promise<Extract<ResponsePayload, { type: 'composerDrafts' }>['drafts']> {
  const response = expectResponse(
    await client.request({ type: 'loadComposerDrafts' }),
    'composerDrafts',
  );
  return response.drafts;
}

export async function applyComposerDraftChanges(
  client: WakuClient,
  changes: ComposerDraftChange[],
): Promise<void> {
  expectResponse(
    await client.request({ type: 'applyComposerDraftChanges', changes }),
    'ack',
  );
}

export async function inspectBranches(
  client: WakuClient,
  cwd: string,
): Promise<BranchSnapshot | null> {
  const response = expectResponse(
    await client.request({ type: 'workspace', operation: { type: 'inspectBranches', cwd } }),
    'workspace',
  );
  if (response.result.type !== 'branches') {
    throw new Error('The daemon returned an unexpected branches response');
  }
  return response.result.snapshot;
}

export async function listWorkspaceTree(
  client: WakuClient,
  root: string,
  expandedPaths: string[],
): Promise<WorkingTreeEntry[]> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'listTree', root, expanded_paths: expandedPaths },
    }),
    'workspace',
  );
  if (response.result.type !== 'workingTree') {
    throw new Error('The daemon returned an unexpected file tree');
  }
  return response.result.entries;
}

export async function discoverComposerCommands(
  client: WakuClient,
  provider: ProviderKind,
  root: string,
  binaryOverride: string | null,
): Promise<SlashCommand[]> {
  const response = expectResponse(await client.request({
    type: 'workspace',
    operation: {
      type: 'discoverSlashCommands',
      provider,
      project_root: root,
      binary_override: binaryOverride,
    },
  }), 'workspace');
  if (response.result.type !== 'slashCommands') {
    throw new Error('The daemon returned an unexpected command catalog');
  }
  return response.result.commands;
}

export async function listComposerFiles(client: WakuClient, root: string): Promise<FileEntry[]> {
  const response = expectResponse(await client.request({
    type: 'workspace',
    operation: { type: 'listProjectFiles', root, cap: 50_000 },
  }), 'workspace');
  if (response.result.type !== 'projectFiles') {
    throw new Error('The daemon returned an unexpected project file index');
  }
  return response.result.entries;
}

export async function readWorkspaceTextFile(
  client: WakuClient,
  root: string,
  relativePath: string,
): Promise<string> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'readTextFile', root, relative_path: relativePath },
    }),
    'workspace',
  );
  if (response.result.type !== 'textFile') {
    throw new Error('The daemon returned an unexpected file response');
  }
  return response.result.content;
}

/** Base64 bytes for files a text read cannot carry (image previews).
 * The daemon caps the size and errors past it. */
export async function readWorkspaceBinaryFile(
  client: WakuClient,
  root: string,
  relativePath: string,
): Promise<string> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'readBinaryFile', root, relative_path: relativePath },
    }),
    'workspace',
  );
  if (response.result.type !== 'file') {
    throw new Error('The daemon returned an unexpected file response');
  }
  return response.result.data;
}

export async function writeWorkspaceTextFile(
  client: WakuClient,
  root: string,
  relativePath: string,
  content: string,
): Promise<void> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'writeTextFile', root, relative_path: relativePath, content },
    }),
    'workspace',
  );
  if (response.result.type !== 'ack') {
    throw new Error('The daemon returned an unexpected file response');
  }
}

export async function collectWorkspaceDiff(
  client: WakuClient,
  cwd: string,
  source: ReviewDiffSource = 'uncommitted',
): Promise<ReviewDiffData> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'collectReviewDiff', cwd, source },
    }),
    'workspace',
  );
  if (response.result.type !== 'reviewDiff') {
    throw new Error('The daemon returned an unexpected diff response');
  }
  return response.result.data;
}

async function runGitOperation(
  client: WakuClient,
  operation: Extract<WorkspaceOperation, { cwd: string }>,
): Promise<WorkspaceResult> {
  const response = expectResponse(
    await client.request({ type: 'workspace', operation }),
    'workspace',
  );
  return response.result;
}

/** The Git panel's branch/upstream/change-list snapshot — `null` when the
 * workspace is not inside a repository. */
export async function inspectGitPanel(
  client: WakuClient,
  cwd: string,
  base: string | null = null,
): Promise<GitPanelSnapshot | null> {
  const result = await runGitOperation(client, { type: 'inspectGitPanel', cwd, base });
  if (result.type !== 'gitPanel') {
    throw new Error('The daemon returned an unexpected git panel response');
  }
  return result.snapshot;
}

export async function stageWorkspaceFile(client: WakuClient, cwd: string, path: string) {
  await runGitOperation(client, { type: 'stageFile', cwd, path });
}

export async function unstageWorkspaceFile(client: WakuClient, cwd: string, path: string) {
  await runGitOperation(client, { type: 'unstageFile', cwd, path });
}

export async function discardWorkspaceFile(client: WakuClient, cwd: string, path: string) {
  await runGitOperation(client, { type: 'discardFile', cwd, path });
}

/** Commits the staged changes — or the whole worktree when
 * `includeUnstaged` — and optionally pushes the result, matching the
 * desktop git panel's one-tap flow. */
export async function commitWorkspace(
  client: WakuClient,
  cwd: string,
  message: string,
  includeUnstaged: boolean,
  push: boolean,
) {
  await runGitOperation(client, {
    type: 'commit',
    cwd,
    message,
    include_unstaged: includeUnstaged,
    push,
  });
}

export async function pushWorkspace(client: WakuClient, cwd: string) {
  await runGitOperation(client, { type: 'push', cwd });
}

/** Integrates upstream commits; the outcome reports clean vs conflicted. */
export async function pullWorkspace(client: WakuClient, cwd: string): Promise<PullOutcome> {
  const result = await runGitOperation(client, { type: 'pullUpstream', cwd, strategy: 'rebase' });
  if (result.type !== 'pull') {
    throw new Error('The daemon returned an unexpected pull response');
  }
  return result.outcome;
}

/** Blank-message commits run through the provider, the way the desktop
 * panel's generate-then-commit flow works. */
export async function generateWorkspaceCommitMessage(
  client: WakuClient,
  cwd: string,
  includeUnstaged: boolean,
  invocation: AgentInvocation,
): Promise<string> {
  const result = await runGitOperation(client, {
    type: 'generateCommitMessage',
    cwd,
    include_unstaged: includeUnstaged,
    invocation,
  });
  if (result.type !== 'commitMessage') {
    throw new Error('The daemon returned an unexpected commit message response');
  }
  return result.message;
}

/** The QA branch's proposed commits — `null` when the repo has no
 * `origin/<qa branch>`. The daemon's `qa_branch` setting names the
 * branch. Approve/reject/promote all return the refreshed queue. */
export async function listReviewQueue(
  client: WakuClient,
  cwd: string,
): Promise<ReviewQueue | null> {
  const result = await runGitOperation(client, { type: 'reviewQueue', cwd });
  if (result.type !== 'reviewQueue') {
    throw new Error('The daemon returned an unexpected review queue response');
  }
  return result.queue;
}

export async function approveReviewCommit(
  client: WakuClient,
  cwd: string,
  sha: string,
): Promise<ReviewQueue | null> {
  const result = await runGitOperation(client, { type: 'reviewApprove', cwd, sha });
  if (result.type !== 'reviewQueue') {
    throw new Error('The daemon returned an unexpected review queue response');
  }
  return result.queue;
}

export async function rejectReviewCommit(
  client: WakuClient,
  cwd: string,
  sha: string,
): Promise<ReviewQueue | null> {
  const result = await runGitOperation(client, { type: 'reviewReject', cwd, sha });
  if (result.type !== 'reviewQueue') {
    throw new Error('The daemon returned an unexpected review queue response');
  }
  return result.queue;
}

/** Fast-forwards the base branch through the QA branch's approved
 * prefix. */
export async function promoteReviewQueue(
  client: WakuClient,
  cwd: string,
): Promise<ReviewQueue | null> {
  const result = await runGitOperation(client, { type: 'reviewPromote', cwd });
  if (result.type !== 'reviewQueue') {
    throw new Error('The daemon returned an unexpected review queue response');
  }
  return result.queue;
}

/** Daemon-wide GitHub inbox — these ops carry no `cwd`. `all: false`
 * returns unread threads only. */
export async function listNotifications(
  client: WakuClient,
  all: boolean,
): Promise<NotificationPoll> {
  const response = expectResponse(
    await client.request({
      type: 'workspace',
      operation: { type: 'listNotifications', all },
    }),
    'workspace',
  );
  if (response.result.type !== 'notifications') {
    throw new Error('The daemon returned an unexpected notifications response');
  }
  return response.result.poll;
}

export async function markNotificationRead(client: WakuClient, threadId: string) {
  await client.request({
    type: 'workspace',
    operation: { type: 'markNotificationRead', thread_id: threadId },
  });
}

export async function markNotificationDone(client: WakuClient, threadId: string) {
  await client.request({
    type: 'workspace',
    operation: { type: 'markNotificationDone', thread_id: threadId },
  });
}

export async function markRepoNotificationsRead(client: WakuClient, repo: string) {
  await client.request({
    type: 'workspace',
    operation: { type: 'markRepoNotificationsRead', repo },
  });
}

export async function markAllNotificationsRead(client: WakuClient) {
  await client.request({
    type: 'workspace',
    operation: { type: 'markAllNotificationsRead' },
  });
}

export async function removeDaemonSession(
  client: WakuClient,
  sessionId: string,
): Promise<void> {
  expectResponse(await client.request({ type: 'removeSession' }, sessionId), 'ack');
}

export async function persistSession(
  client: WakuClient,
  session: AgentSession,
): Promise<AgentSession> {
  const response = expectResponse(
    await client.request({
      type: 'saveTaskState',
      projects: [],
      liveSessionIds: [session.id],
      sessions: [session],
      sessionTails: [],
    }),
    'taskStateSaved',
  );
  // A skeleton save is echoed as a skeleton — never let it replace the
  // loaded session this client is holding.
  return (
    response.sessions.find((item) => item.id === session.id && item.detail_loaded !== false)
    ?? session
  );
}

function expectResponse<T extends ResponsePayload['type']>(
  response: ResponsePayload,
  expected: T,
): Extract<ResponsePayload, { type: T }> {
  if (response.type !== expected) {
    throw new Error(`Expected daemon response ${expected}, received ${response.type}`);
  }
  return response as Extract<ResponsePayload, { type: T }>;
}
