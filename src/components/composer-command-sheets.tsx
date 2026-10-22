import type {
  GoalOperation,
  ProviderKind,
  ProviderModel,
  ProviderSessionSummary,
  RuntimeMode,
  SlashCommand,
  ThreadGoal,
} from '@waku/client';

import {
  isFastModeToggleSubmission,
  isLandSubmission,
  isResumeSubmission,
  parseGoalSubmission,
  parseRenameSubmission,
  toggledFastServiceTier,
} from '@waku/client/composer-autocomplete';

import { fuzzyScore } from '@waku/client/fuzzy-search';
import { useEffect, useMemo, useState } from 'octane';
import { useWindowSize } from '@octane-xplat/platform';
import { Spinner, SearchInput, Text, TextArea, VirtualList } from '@octane-xplat/ui';
import { pushRoute } from '@octane-xplat/ui/routing';

import { Sheet, SheetRow } from './sheet';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import {
  createProject,
  daemonKeys,
  listProviderSessions,
  loadProviderSessionHistory,
  loadTaskState,
  persistProject,
  persistSession,
  providerSessionKey,
} from '@/lib/daemon-api';

import {
  invalidateDaemonQueriesMatching,
  removeDaemonQuery,
} from '@/lib/daemon-data';

import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import { useDaemonSettings } from '@/lib/daemon-queries';
import { createResumedSession } from '@/lib/mobile-runtime';
import { randomUUID } from '@/lib/uuid';
import { providerLabel } from '@/lib/session-presentation';

/** Desktop's local commands must never leak through as provider prompts. */
export function useComposerLocalCommands({
  provider,
  model,
  serviceTier,
  runtimeMode,
  goal,
  contextKey,
  onServiceTier,
  onGoal,
  onLand,
  onRename,
  onClear,
}: {
  provider: ProviderKind | null;
  model: ProviderModel | undefined;
  serviceTier: string | null | undefined;
  runtimeMode: RuntimeMode;
  goal?: ThreadGoal | null;
  contextKey: string;
  onServiceTier: (tier: string) => void | Promise<void>;
  onGoal: (operation: GoalOperation, managed?: boolean) => Promise<void>;
  onLand?: () => Promise<void>;
  onRename?: (title: string | null) => void | Promise<void>;
  onClear: () => void;
}) {
  const daemon = useDaemon();
  const settings = useDaemonSettings();
  const [resumeOpen, setResumeOpen] = useState(false);
  const [goalDialog, setGoalDialog] = useState<{ prefill: string | null; replace: boolean } | null>(null);
  const [goalManaged, setGoalManaged] = useState(false);
  useEffect(() => {
    setResumeOpen(false);
    setGoalDialog(null);
  }, [contextKey, daemon.activeProfile?.id, provider]);

  async function execute(draft: string, commands: SlashCommand[]): Promise<boolean> {
    const rename = parseRenameSubmission(draft);
    if (rename !== undefined) {
      if (!onRename) {throw new Error('Select a task to rename');}
      if (rename === null) {return false;}
      await onRename(rename);
      onClear();
      return true;
    }

    if (!provider) {return false;}
    if (isResumeSubmission(draft)) {
      // Resume lists sessions the provider itself ran, so a disabled
      // provider has nothing to offer; say so instead of opening an
      // empty sheet.
      if (settings.data?.disabled_providers.includes(provider)) {
        throw new Error(`${providerLabel(provider)} is disabled`);
      }

      setResumeOpen(true);
    } else if (isLandSubmission(draft)) {
      // `/land` is always intercepted so it never leaks into a turn; a
      // composer without a session workspace reports instead.
      if (!onLand) {throw new Error('Select a task to land its workspace');}
      await onLand();
    } else if (isFastModeToggleSubmission(provider, draft, commands)) {
      const tier = toggledFastServiceTier(serviceTier, model?.service_tiers ?? []);
      if (!tier) {throw new Error('Fast mode is unavailable for this model');}
      await onServiceTier(tier);
    } else {
      const command = parseGoalSubmission(provider, draft, commands);
      if (!command) {return false;}
      const managed = provider !== 'codex' || Boolean(goal?.managedId)
        || !commands.some((item) => item.name === 'goal' && item.scope === 'Builtin');

      setGoalManaged(managed);
      switch (command.kind) {
        case 'show':
        case 'edit':
          setGoalDialog({ prefill: null, replace: false });
          break;
        case 'pause':
        case 'resume':
          await onGoal({ kind: 'set', objective: null, status: command.kind === 'pause' ? 'paused' : 'active', replace: false }, managed);
          break;
        case 'clear':
          await onGoal({ kind: 'clear' }, managed);
          break;
        case 'set':
          if (goal && goal.status !== 'complete' && goal.status !== 'budgetLimited') {
            setGoalDialog({ prefill: command.objective, replace: true });
          } else {
            await onGoal({ kind: 'set', objective: command.objective, status: 'active', replace: Boolean(goal) }, managed);
          }

          break;
      }
    }

    onClear();
    return true;
  }

  return {
    execute,
    sheets: (
      <>
        {resumeOpen && provider && (
          <ResumeSessionSheet provider={provider} runtimeMode={runtimeMode} onDismiss={() => setResumeOpen(false)} />
        )}
        {goalDialog && (
          <GoalCommandSheet
            goal={goal ?? null}
            prefill={goalDialog.prefill}
            replace={goalDialog.replace}
            onRun={(operation) => onGoal(operation, goalManaged)}
            onDismiss={() => setGoalDialog(null)}
          />
        )}
      </>
    ),
  };
}

function ResumeSessionSheet({ provider, runtimeMode, onDismiss }: {
  provider: ProviderKind;
  runtimeMode: RuntimeMode;
  onDismiss: () => void;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const { height } = useWindowSize();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const [search, setSearch] = useState('');
  const [resuming, setResuming] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const sessions = useDaemonQuery(
    daemonKeys.providerSessions(profileId, provider),
    () => listProviderSessions(daemon.client!, provider),
    {
      enabled: daemon.phase === 'connected' && Boolean(daemon.client),
      staleTime: 15_000,
    },
  );

  const catalog = sessions.data;
  const rows = useMemo(() => (catalog?.sessions ?? []).filter((item) =>
    fuzzyScore(search, `${item.title} ${item.cwd}`) !== null), [search, catalog]);

  async function resume(summary: ProviderSessionSummary) {
    const client = daemon.client;
    if (!client || daemon.phase !== 'connected' || resuming) {return;}
    setResuming(providerSessionKey(summary.cursor));
    setError(null);
    try {
      const state = await loadTaskState(client);
      let session = state.sessions.find((item) => item.provider_cursor
        && providerSessionKey(item.provider_cursor) === providerSessionKey(summary.cursor));

      if (!session) {
        const loaded = await loadProviderSessionHistory(client, summary);
        const projectPath = loaded.resolvedCwd ?? summary.cwd;
        let project = state.projects.find((item) => item.path === projectPath);
        if (!project) {project = (await persistProject(client, createProject(projectPath, randomUUID()))).project;}
        session = await persistSession(client, createResumedSession(
          project.id, summary, loaded.history, runtimeMode,
          { nowSeconds: () => Math.floor(Date.now() / 1_000), randomUUID },
        ));

        removeDaemonQuery(daemonKeys.session(profileId, session.id));
      }

      invalidateDaemonQueriesMatching(daemonKeys.taskState(profileId));
      onDismiss();
      pushRoute({ stack: 'root', name: 'session/:id', params: { id: session.id } });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setResuming(null);
    }
  }

  return (
    <Sheet visible onDismiss={onDismiss} scrollable={false} title={`Resume ${providerLabel(provider)} session`}>
      <SearchInput
        placeholder="Search sessions…"
        style={{
          borderRadius: Radius.medium,
          fontSize: 16,
          minHeight: 44,
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 10,
          paddingBottom: 10,
          marginLeft: 6,
          marginRight: 6,
          marginBottom: 8,
          color: theme.text,
          backgroundColor: theme.inset,
        }}
        value={search}
        onChange={setSearch}
      />
      {(error || sessions.error) && (
        <Text style={{
          fontSize: 14,
          lineHeight: '18px',
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 8,
          paddingBottom: 8,
          color: theme.danger,
        }}>
          {error ?? String(sessions.error)}
        </Text>
      )}
      {sessions.isPending ? (
        <Spinner busy style={{ paddingTop: 20, paddingBottom: 20 }} />
      ) : (
        <VirtualList
          items={rows}
          keyExtractor={(item: ProviderSessionSummary) => providerSessionKey(item.cursor)}
          style={{ maxHeight: height * 0.45 }}
          renderItem={(item: ProviderSessionSummary) => (
            <SheetRow
              label={item.title || 'Untitled session'}
              description={item.cwd_missing ? `${item.cwd} · folder missing` : item.cwd}
              disabled={Boolean(resuming)}
              leading={resuming === providerSessionKey(item.cursor) ? <Spinner busy /> : undefined}
              onPress={() => void resume(item)}
            />
          )}
          renderEmpty={() => (
            <Text style={{
              fontSize: 14,
              lineHeight: '18px',
              paddingLeft: 12,
              paddingRight: 12,
              paddingTop: 8,
              paddingBottom: 8,
              color: theme.textTertiary,
            }}>{
              catalog?.status === 'unsupported'
                ? `${providerLabel(provider)} can't list its sessions`
                : catalog?.status === 'binaryMissing'
                  ? `${providerLabel(provider)} isn't installed`
                  : 'No matching sessions'
            }</Text>
          )}
        />
      )}
      {sessions.error && <SheetRow label="Retry" onPress={() => { void sessions.refetch(); }} />}
    </Sheet>
  );
}

function GoalCommandSheet({ goal, prefill, replace, onRun, onDismiss }: {
  goal: ThreadGoal | null;
  prefill: string | null;
  replace: boolean;
  onRun: (operation: GoalOperation) => Promise<void>;
  onDismiss: () => void;
}) {
  const theme = useTheme();
  const [objective, setObjective] = useState(prefill ?? goal?.objective ?? '');
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function run(operation: GoalOperation) {
    if (pending) {return;}
    setPending(true);
    setError(null);
    try {
      await onRun(operation);
      onDismiss();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setPending(false);
    }
  }

  return (
    <Sheet visible onDismiss={onDismiss} title="Goal">
      {goal && (
        <Text style={{
          fontSize: 14,
          lineHeight: '18px',
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 8,
          paddingBottom: 8,
          color: theme.textSecondary,
        }}>
          Status: {{
            active: 'Active', paused: 'Paused', blocked: 'Stalled', usageLimited: 'Usage limit reached',
            budgetLimited: 'Budget reached', complete: 'Complete',
          }[goal.status]}
        </Text>
      )}
      <TextArea
        rows={4}
        isReadOnly={pending}
        placeholder="What should the agent keep working on?"
        style={{
          borderRadius: Radius.medium,
          fontSize: 16,
          minHeight: 88,
          maxHeight: 180,
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 10,
          paddingBottom: 10,
          marginLeft: 6,
          marginRight: 6,
          marginBottom: 8,
          color: theme.text,
          backgroundColor: theme.inset,
        }}
        value={objective}
        onChange={setObjective}
      />
      {replace && goal && (
        <Text style={{
          fontSize: 14,
          lineHeight: '18px',
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 8,
          paddingBottom: 8,
          color: theme.textSecondary,
        }}>
          This replaces the current goal: {goal.objective}
        </Text>
      )}
      {error && (
        <Text style={{
          fontSize: 14,
          lineHeight: '18px',
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 8,
          paddingBottom: 8,
          color: theme.danger,
        }}>
          {error}
        </Text>
      )}
      <SheetRow
        label={goal ? (replace ? 'Replace goal' : 'Save goal') : 'Set goal'}
        disabled={pending || !objective.trim()}
        leading={pending ? <Spinner busy /> : undefined}
        onPress={() => void run({
          kind: 'set', objective: objective.trim(),
          status: goal && !replace && goal.status !== 'complete' && goal.status !== 'budgetLimited'
            ? goal.status : 'active',
          replace: replace && Boolean(goal),
        })}
      />
      {goal && !replace && (
        <>
          {(goal.status === 'active' || goal.status === 'paused' || goal.status === 'blocked' || goal.status === 'usageLimited') && (
            <SheetRow
              label={goal.status === 'active' ? 'Pause goal' : 'Resume goal'}
              disabled={pending}
              onPress={() => void run({ kind: 'set', objective: null, status: goal.status === 'active' ? 'paused' : 'active', replace: false })}
            />
          )}
          <SheetRow label="Clear goal" destructive disabled={pending} onPress={() => void run({ kind: 'clear' })} />
        </>
      )}
    </Sheet>
  );
}
