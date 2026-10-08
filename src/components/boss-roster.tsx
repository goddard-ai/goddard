import type {
  AgentSession,
  BossEmployee,
  BossIdentity,
  BossPlan,
  BossState,
  ResourceSet,
  ResourceStatus,
} from '@waku/client';

import { bossEmployeeLifecycle, openBossSession } from '@waku/client/boss';

import { memo, useCallback, useMemo, useState } from 'octane';
import {
  ActivityIndicator,
  Pressable,
  Text,
  View,
} from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { Sheet } from '@/components/sheet';
import { Radius } from '@/lib/theme-colors';
import {
  bossEmployeeIsLive,
  bossEmployeeStatusLabel,
  bossEmployeeSymbol,
  bossModelCapacity,
  bossOpenBlockers,
  bossQueueDetail,
  bossQueueRanks,
  bossRosterSplit,
  employeeAssignmentRows,
  planOutcomeLabel,
} from '@/lib/boss';

import { relativeSessionTime, displaySessionTitle } from '@/lib/session-presentation';
import type { TaskState } from '@/lib/daemon-api';
import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import { daemonKeys } from '@/lib/daemon-api';
import { useProviderCatalog } from '@/lib/daemon-queries';
import { useTheme } from '@/lib/use-theme';
import { displayHost } from '@/lib/daemon-profile';
import { showAlert } from '@/lib/alert';
import { sessionHasUnseenReply, type UnseenReplyWatermarks } from '@/lib/unseen-replies';

/** DiceBear-style boss avatars are daemon-side SVG renders the mobile app
 * doesn't reproduce — the desktop's own placeholder is the name's first
 * letter on a tinted tile. */
export function BossAvatar({
  identity,
  size = 28,
}: {
  identity: BossIdentity;
  size?: number;
}) {
  const theme = useTheme();
  return (
    <View
      style={{
        alignItems: 'center',
        backgroundColor: theme.accentSoft,
        borderRadius: Math.min(7, size / 3.4),
        cornerShape: 'squircle',
        height: size,
        justifyContent: 'center',
        width: size,
      }}
    >
      <Text
        style={{
          color: theme.accent,
          fontSize: Math.round(size * 0.42),
          fontWeight: '700',
        }}
      >
        {identity.name.trim().charAt(0).toUpperCase() || 'B'}
      </Text>
    </View>
  );
}

const STATUS_TONES = {
  neutral: 'textTertiary',
  secondary: 'textSecondary',
  warning: 'warning',
  danger: 'danger',
  working: 'info',
  waiting: 'warning',
} as const;

/** The drawer section a Boss-capable daemon renders ahead of the session
 * groups: the boss chat row, its planning sessions, the live roster, and
 * finished employees behind a Show-more fold. */
export function BossSection({
  state,
  taskState,
  watermarks,
  now,
  selectedSessionId,
  onSelectSession,
  onClose,
}: {
  state: BossState;
  taskState: TaskState | undefined;
  watermarks: UnseenReplyWatermarks;
  now: number;
  selectedSessionId: string | null;
  onSelectSession: (sessionId: string) => void;
  onClose: () => void;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const catalog = useProviderCatalog();
  const [showFinished, setShowFinished] = useState(false);
  const [capacityOpen, setCapacityOpen] = useState(false);
  const [openingChat, setOpeningChat] = useState(false);

  const sessions = useMemo(
    () => new Map((taskState?.sessions ?? []).map((session) => [session.id, session])),
    [taskState?.sessions],
  );

  const sessionCreatedAt = useMemo(
    () => new Map((taskState?.sessions ?? []).map((session) => [session.id, session.created_at])),
    [taskState?.sessions],
  );

  const projects = useMemo(
    () => new Map((taskState?.projects ?? []).map((project) => [project.id, project.name])),
    [taskState?.projects],
  );

  const ranks = useMemo(() => bossQueueRanks(state), [state]);
  const roster = useMemo(() => bossRosterSplit(state, sessionCreatedAt), [state, sessionCreatedAt]);
  const blockers = useMemo(() => bossOpenBlockers(state), [state]);
  const bossSession = state.sessionId ? sessions.get(state.sessionId) : undefined;
  const bossUnseen = bossSession ? sessionHasUnseenReply(bossSession, watermarks) : false;

  const openChat = useCallback(async () => {
    const client = daemon.client;
    if (state.sessionId) {
      onSelectSession(state.sessionId);
      return;
    }

    if (!client || daemon.phase !== 'connected' || openingChat) {return;}
    setOpeningChat(true);
    try {
      // Provider/mode only matter when this call creates the chat — reuse
      // the composer's own preference order (codex first, else whatever
      // the daemon has installed).
      const installed = catalog.providers.filter((item) => item.installed).map((item) => item.id);
      const provider = installed.includes('codex') ? 'codex' : installed[0] ?? 'codex';
      const { session } = await openBossSession(client, {
        provider,
        model: null,
        mode: 'autoAcceptEdits',
      });

      onClose();
      onSelectSession(session.id);
    } catch (cause) {
      void showAlert('Couldn’t open boss chat', cause instanceof Error ? cause.message : String(cause));
    } finally {
      setOpeningChat(false);
    }
  }, [catalog.providers, daemon.client, daemon.phase, state.sessionId, onClose, onSelectSession, openingChat]);

  const openEmployee = useCallback(
    (sessionId: string) => {
      onClose();
      onSelectSession(sessionId);
    },
    [onClose, onSelectSession],
  );

  return (
    <View>
      <Text
        style={{
          color: theme.textTertiary,
          fontSize: 13,
          fontWeight: '600',
          marginBottom: 4,
          marginTop: 10,
          paddingHorizontal: 14,
          textTransform: 'uppercase',
        }}
      >
        {`Boss · ${daemon.activeProfile?.name ?? 'Host'}${daemon.phase === 'connected' ? '' : ' · Stale'}`}
      </Text>

      {/* The boss chat row — destination of every supervision message. */}
      <View style={{ alignItems: 'stretch', flexDirection: 'row' }}>
        <Pressable
          accessibilityHint="Opens the boss chat"
          accessibilityLabel={`${state.identity.name}, boss${blockers.length ? `, ${blockers.length} needs attention` : ''}`}
          accessibilityRole="button"
          className="vx-pressable"
          onPress={() => void openChat()}
          style={{
            alignItems: 'center',
            borderRadius: Radius.medium,
            flex: '1 1',
            flexDirection: 'row',
            gap: 9,
            marginHorizontal: 6,
            minHeight: 46,
            paddingHorizontal: 8,
            backgroundColor: selectedSessionId === state.sessionId ? theme.overlay : 'transparent',
          }}
        >
          <BossAvatar identity={state.identity} size={26} />
          <View style={{ flex: '1 1', minWidth: 0 }}>
            <Text numberOfLines={1} style={{ color: theme.text, fontSize: 15, fontWeight: '600' }}>
              {state.identity.name}
            </Text>
            <Text numberOfLines={1} style={{ color: blockers.length ? theme.warning : theme.textTertiary, fontSize: 12.5, marginTop: 1 }}>
              {blockers.length
                ? `${blockers.length} need${blockers.length === 1 ? 's' : ''} attention`
                : daemon.activeProfile ? displayHost(daemon.activeProfile.address) : 'Boss'}
            </Text>
          </View>
          {openingChat ? <ActivityIndicator busy /> : null}
          {!openingChat && bossUnseen ? (
            <View style={{ backgroundColor: theme.info, borderRadius: 3.5, height: 7, width: 7 }} />
          ) : null}
        </Pressable>
        <Pressable
          accessibilityHint="Shows model and host capacity"
          accessibilityLabel="Capacity"
          accessibilityRole="button"
          hitSlop={6}
          onPress={() => setCapacityOpen(true)}
          style={{ alignItems: 'center', justifyContent: 'center', paddingHorizontal: 8 }}
        >
          <AppSymbol
            name={{ ios: 'gauge', android: 'speed', web: 'gauge' }}
            size={15}
            tintColor={theme.textTertiary}
          />
        </Pressable>
      </View>

      {state.planning.map((plan) => (
        <PlanRow
          now={now}
          plan={plan}
          selected={plan.sessionId === selectedSessionId}
          session={sessions.get(plan.sessionId)}
          onPress={openEmployee}
        />
      ))}

      {roster.live.map((employee) => (
        <EmployeeRow
          employee={employee}
          now={now}
          projectName={
            employee.ticket?.project
            ?? projects.get(sessions.get(employee.sessionId)?.project_id ?? '')
            ?? undefined
          }
          queueDetail={bossQueueDetail(employee, ranks.get(employee.sessionId)) ?? undefined}
          selected={employee.sessionId === selectedSessionId}
          session={sessions.get(employee.sessionId)}
          state={state}
          unseen={
            bossEmployeeIsLive(employee)
            && sessions.get(employee.sessionId) != null
            && sessionHasUnseenReply(sessions.get(employee.sessionId)!, watermarks)
          }
          onPress={openEmployee}
        />
      ))}

      {roster.finished.length > 0 && (
        <Pressable
          accessibilityRole="button"
          className="vx-pressable"
          onPress={() => setShowFinished((current) => !current)}
          style={{ paddingHorizontal: 14, paddingVertical: 8 }}
        >
          <Text style={{ color: theme.textSecondary, fontSize: 13.5, fontWeight: '500' }}>
            {showFinished ? 'Show less' : `Show more (${roster.finished.length} older)`}
          </Text>
        </Pressable>
      )}
      {showFinished
        ? roster.finished.map((employee) => (
            <EmployeeRow
              employee={employee}
              now={now}
              projectName={
                employee.ticket?.project
                ?? projects.get(sessions.get(employee.sessionId)?.project_id ?? '')
                ?? undefined
              }
              selected={employee.sessionId === selectedSessionId}
              session={sessions.get(employee.sessionId)}
              state={state}
              unseen={false}
              onPress={openEmployee}
            />
          ))
        : null}

      <BossCapacitySheet
        onDismiss={() => setCapacityOpen(false)}
        state={state}
        visible={capacityOpen}
      />
    </View>
  );
}

function PlanRow({
  plan,
  session,
  selected,
  now,
  onPress,
}: {
  plan: BossPlan;
  session: AgentSession | undefined;
  selected: boolean;
  now: number;
  onPress: (sessionId: string) => void;
}) {
  const theme = useTheme();
  const outcome = planOutcomeLabel(plan.outcome);
  const status = plan.finalizedAt == null ? 'Planning' : outcome ?? 'Finalized';
  return (
    <Pressable
      accessibilityHint="Opens the planning session"
      accessibilityLabel={`${plan.idea || 'Plan'}, ${status}`}
      accessibilityRole="button"
      className="vx-pressable"
      onPress={() => onPress(plan.sessionId)}
      style={{
        alignItems: 'center',
        borderRadius: Radius.medium,
        flexDirection: 'row',
        gap: 9,
        marginHorizontal: 6,
        minHeight: 40,
        paddingHorizontal: 8,
        paddingLeft: 34,
        backgroundColor: selected ? theme.overlay : 'transparent',
      }}
    >
      <AppSymbol
        name={{ ios: 'doc.text.magnifyingglass', android: 'plagiarism', web: 'plagiarism' }}
        size={15}
        tintColor={theme.textTertiary}
      />
      <View style={{ flex: '1 1', minWidth: 0 }}>
        <Text numberOfLines={1} style={{ color: theme.text, fontSize: 14.5 }}>
          {plan.idea.trim() || (session ? displaySessionTitle(session) : 'Plan')}
        </Text>
        <Text numberOfLines={1} style={{ color: theme.textTertiary, fontSize: 12, marginTop: 1 }}>
          {status}
        </Text>
      </View>
      {session?.last_reply_at != null && (
        <Text style={{ color: theme.textGhost, fontSize: 11.5 }}>
          {relativeSessionTime(session.last_reply_at, now)}
        </Text>
      )}
    </Pressable>
  );
}

const EmployeeRow = memo(function EmployeeRow({
  employee,
  session,
  projectName,
  queueDetail,
  selected,
  state,
  unseen,
  now,
  onPress,
}: {
  employee: BossEmployee;
  session: AgentSession | undefined;
  projectName?: string;
  queueDetail?: string;
  selected: boolean;
  state: BossState;
  unseen: boolean;
  now: number;
  onPress: (sessionId: string) => void;
}) {
  const theme = useTheme();
  const status = bossEmployeeStatusLabel(employee, session, queueDetail ?? null);
  const lifecycle = bossEmployeeLifecycle(employee);
  const expired = lifecycle === 'expired';
  const stamp =
    lifecycle === 'queued'
      ? employee.queuedAt ?? employee.createdAt
      : expired
        ? employee.expiredAt ?? employee.createdAt
        : employee.createdAt;

  const detail = [employee.jobTitle, projectName].filter(Boolean).join(' · ');
  return (
    <Pressable
      accessibilityHint={expired ? 'Opens the finished employee’s task' : 'Opens the employee’s task'}
      accessibilityLabel={`${employee.identity.name}, ${employee.jobTitle}, ${status.label}${employee.blocker ? `, blocker: ${employee.blocker}` : ''}`}
      accessibilityRole="button"
      className="vx-pressable"
      onPress={() => onPress(employee.sessionId)}
      style={{
        alignItems: 'center',
        borderRadius: Radius.medium,
        flexDirection: 'row',
        gap: 9,
        marginHorizontal: 6,
        minHeight: 46,
        paddingHorizontal: 8,
        paddingLeft: 34,
        backgroundColor: selected ? theme.overlay : 'transparent',
        opacity: expired ? 0.6 : 1,
      }}
    >
      <BossAvatar identity={employee.identity} size={24} />
      <View style={{ flex: '1 1', minWidth: 0 }}>
        <Text numberOfLines={1} style={{ color: theme.text, fontSize: 14.5, fontWeight: '500' }}>
          {employee.identity.name}
        </Text>
        <View style={{ alignItems: 'center', flexDirection: 'row', gap: 4, minWidth: 0 }}>
          <AppSymbol
            name={{ ios: bossEmployeeSymbol(state, employee), android: 'work', web: 'work' }}
            size={11}
            tintColor={theme.textTertiary}
          />
          <Text numberOfLines={1} style={{ color: theme.textTertiary, flexShrink: 1, fontSize: 12 }}>
            {detail}
          </Text>
          {employee.blocker ? (
            <Text numberOfLines={1} style={{ color: theme.warning, flexShrink: 1, fontSize: 12 }}>
              {`— ${employee.blocker}`}
            </Text>
          ) : null}
        </View>
      </View>
      <View style={{ alignItems: 'flex-end', gap: 2 }}>
        <Text
          numberOfLines={1}
          style={{ color: theme[STATUS_TONES[status.tone]], fontSize: 11.5 }}
        >
          {status.label}
        </Text>
        {unseen ? (
          <View style={{ backgroundColor: theme.info, borderRadius: 3, height: 6, width: 6 }} />
        ) : stamp != null ? (
          <Text style={{ color: theme.textGhost, fontSize: 11 }}>
            {relativeSessionTime(stamp, now)}
          </Text>
        ) : null}
      </View>
    </Pressable>
  );
});

/** Per-model used/limit plus the host broker's live status — read-only; the
 * boss owns policy changes on the desktop. */
export function BossCapacitySheet({
  state,
  visible,
  onDismiss,
}: {
  state: BossState;
  visible: boolean;
  onDismiss: () => void;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const capacity = useMemo(() => bossModelCapacity(state), [state]);
  const resources = useDaemonQuery(
    daemonKeys.bossResources(daemon.activeProfile?.id ?? 'disconnected'),
    async () => {
      const client = daemon.client;
      if (!client) {throw new Error('Goddard daemon is disconnected');}
      const response = await client.request({ type: 'agentResources', operation: { action: 'status', id: null } });
      if (response.type !== 'agentResources') {
        throw new Error(`Expected daemon response agentResources, received ${response.type}`);
      }

      return response.status;
    },
    { enabled: visible && daemon.phase === 'connected' && Boolean(daemon.client), staleTime: 2_000 },
  );

  const status: ResourceStatus | undefined = resources.data;

  return (
    <Sheet onDismiss={onDismiss} title="Capacity" visible={visible}>
      <Text style={{ color: theme.textSecondary, fontSize: 13, fontWeight: '600', marginBottom: 4, marginTop: 6, textTransform: 'uppercase' }}>
        Model slots
      </Text>
      {capacity.length === 0 ? (
        <Text style={{ color: theme.textTertiary, fontSize: 14, paddingVertical: 8 }}>
          No model limits set — every live employee runs uncapped.
        </Text>
      ) : (
        capacity.map((entry) => (
          <View
            style={{
              alignItems: 'center',
              borderBottomColor: theme.separator,
              borderBottomWidth: 1,
              flexDirection: 'row',
              justifyContent: 'space-between',
              paddingVertical: 9,
            }}
          >
            <View style={{ flex: '1 1', minWidth: 0 }}>
              <Text numberOfLines={1} style={{ color: theme.text, fontSize: 15 }}>
                {entry.model}
              </Text>
              <Text numberOfLines={1} style={{ color: theme.textTertiary, fontSize: 12 }}>
                {entry.provider}
              </Text>
            </View>
            <Text style={{ color: theme.textSecondary, fontSize: 14, fontVariant: ['tabular-nums'] }}>
              {entry.liveLimit < 0 ? `${entry.used} in use` : `${entry.used}/${entry.liveLimit}`}
            </Text>
          </View>
        ))
      )}

      <Text style={{ color: theme.textSecondary, fontSize: 13, fontWeight: '600', marginBottom: 4, marginTop: 16, textTransform: 'uppercase' }}>
        Host resources
      </Text>
      {resources.isPending ? (
        <ActivityIndicator busy />
      ) : status ? (
        <View>
          <CapacityLine label="Devices" used={status.reservations.reduce((total, item) => total + item.resources.resident_devices, 0)} limit={status.policy.resident_devices} />
          <CapacityLine label="Native builds" used={status.reservations.reduce((total, item) => total + item.resources.native_builds, 0)} limit={status.policy.native_builds} />
          <CapacityLine label="Desktop input" used={status.reservations.reduce((total, item) => total + item.resources.desktop_input, 0)} limit={status.policy.desktop_input} />
          {status.reservations.length ? (
            <Text style={{ color: theme.textTertiary, fontSize: 12.5, marginTop: 8 }}>
              {status.reservations.length} reservation{status.reservations.length === 1 ? '' : 's'} held
            </Text>
          ) : null}
          {status.external_devices.length ? (
            <Text style={{ color: theme.textTertiary, fontSize: 12.5, marginTop: 4 }}>
              {status.external_devices.length} external device{status.external_devices.length === 1 ? '' : 's'} running (unmanaged)
            </Text>
          ) : null}
        </View>
      ) : (
        <Text style={{ color: theme.textTertiary, fontSize: 14, paddingVertical: 8 }}>
          {resources.error instanceof Error ? resources.error.message : 'Host resource status unavailable.'}
        </Text>
      )}
    </Sheet>
  );
}

function CapacityLine({ label, used, limit }: { label: string; used: number; limit: number }) {
  const theme = useTheme();
  return (
    <View
      style={{
        alignItems: 'center',
        borderBottomColor: theme.separator,
        borderBottomWidth: 1,
        flexDirection: 'row',
        justifyContent: 'space-between',
        paddingVertical: 9,
      }}
    >
      <Text style={{ color: theme.text, fontSize: 15 }}>{label}</Text>
      <Text style={{ color: theme.textSecondary, fontSize: 14, fontVariant: ['tabular-nums'] }}>
        {used}/{limit}
      </Text>
    </View>
  );
}

/** The summon record's own inventory — Brief, Persona, Model, Workspace,
 * Access, Resources — the mobile form of the desktop's assignment
 * popover. */
export function BossAssignmentSheet({
  state,
  session,
  employee,
  visible,
  onDismiss,
}: {
  state: BossState;
  session: AgentSession | undefined;
  employee: BossEmployee;
  visible: boolean;
  onDismiss: () => void;
}) {
  const theme = useTheme();
  const rows = employeeAssignmentRows(state, session, employee);
  return (
    <Sheet onDismiss={onDismiss} title="Assignment" visible={visible}>
      {rows.map((row) => (
        <View style={{ paddingBottom: 10, paddingHorizontal: 4 }}>
          <Text style={{ color: theme.textTertiary, fontSize: 12 }}>{row.label}</Text>
          <Text style={{ color: theme.textSecondary, fontSize: 14, marginTop: 2 }}>{row.value}</Text>
        </View>
      ))}
    </Sheet>
  );
}

/** The floating card a queued or dispatching employee's view carries: the
 * wait reason (or launch state), the resolved model · effort, the ticket's
 * resource claims, and the full prompt the summon parked. */
export function EmployeeStatusCard({
  employee,
  queueRank,
}: {
  employee: BossEmployee;
  queueRank?: number;
}) {
  const theme = useTheme();
  const lifecycle = bossEmployeeLifecycle(employee);
  const waiting = lifecycle === 'queued';
  const ticket = employee.ticket;
  const detail = waiting ? bossQueueDetail(employee, queueRank) : null;
  const model = ticket
    ? [ticket.model, ticket.reasoningEffort].filter(Boolean).join(' · ')
    : null;

  const resources = ticket ? ticketResourceSummary(ticket.resources, Boolean(ticket.reservation)) : null;

  return (
    <View
      style={{
        backgroundColor: theme.surface,
        borderColor: theme.separator,
        borderRadius: Radius.large,
        borderWidth: 1,
        gap: 8,
        padding: 14,
      }}
    >
      <View style={{ alignItems: 'center', flexDirection: 'row', gap: 8 }}>
        <ActivityIndicator busy={lifecycle === 'dispatching' || lifecycle === 'finishing'} />
        <Text style={{ color: theme.text, fontSize: 15, fontWeight: '600' }}>
          {waiting ? 'Queued' : lifecycle === 'dispatching' ? 'Starting' : 'Finishing'}
        </Text>
        {detail ? (
          <Text numberOfLines={2} style={{ color: theme.textSecondary, flex: '1 1', fontSize: 13 }}>
            {detail}
          </Text>
        ) : null}
      </View>
      {model ? (
        <CardLine label="Model" value={model} />
      ) : null}
      {resources ? (
        <CardLine label="Resources" value={resources} />
      ) : null}
      {ticket?.prompt ? (
        <View>
          <Text style={{ color: theme.textTertiary, fontSize: 11.5, marginBottom: 2, textTransform: 'uppercase' }}>
            Prompt
          </Text>
          <Text style={{ color: theme.textSecondary, fontSize: 13.5 }}>{ticket.prompt.trim()}</Text>
        </View>
      ) : null}
    </View>
  );
}

function CardLine({ label, value }: { label: string; value: string }) {
  const theme = useTheme();
  return (
    <View style={{ flexDirection: 'row', gap: 8 }}>
      <Text style={{ color: theme.textTertiary, fontSize: 12, width: 72 }}>{label}</Text>
      <Text style={{ color: theme.textSecondary, flex: '1 1', fontSize: 13 }}>{value}</Text>
    </View>
  );
}

function ticketResourceSummary(resources: ResourceSet, reserved: boolean): string | null {
  const parts: string[] = [];
  if (resources.exclusive.length) {parts.push(`exclusive: ${resources.exclusive.join(', ')}`);}
  if (resources.resident_devices) {parts.push(`devices: ${resources.resident_devices}`);}
  if (resources.native_builds) {parts.push(`builds: ${resources.native_builds}`);}
  if (resources.desktop_input) {parts.push('desktop input');}
  if (!parts.length) {return null;}
  const detail = parts.join(' · ');
  return reserved ? `Reserved: ${detail}` : `Requested: ${detail}`;
}

/** The boss chat's actionable blocker notice — one row per employee that
 * raised a blocker, tapping opens that employee (where the composer still
 * addresses the boss). */
export function BossBlockerNotice({
  state,
  onOpen,
}: {
  state: BossState;
  onOpen: (sessionId: string) => void;
}) {
  const theme = useTheme();
  const blockers = bossOpenBlockers(state);
  if (!blockers.length) {return null;}
  return (
    <View
      style={{
        backgroundColor: theme.warningSoft,
        borderColor: theme.warning,
        borderRadius: Radius.large,
        borderWidth: 1,
        gap: 2,
        paddingVertical: 8,
      }}
    >
      {blockers.map((employee) => (
        <Pressable
          accessibilityHint="Opens the blocked employee"
          accessibilityLabel={`${employee.identity.name} reported a blocker: ${employee.blocker}`}
          accessibilityRole="button"
          className="vx-pressable"
          onPress={() => onOpen(employee.sessionId)}
          style={{
            alignItems: 'center',
            flexDirection: 'row',
            gap: 8,
            minHeight: 34,
            paddingHorizontal: 12,
          }}
        >
          <AppSymbol
            name={{ ios: 'exclamationmark.triangle', android: 'warning', web: 'warning' }}
            size={14}
            tintColor={theme.warning}
          />
          <View style={{ flex: '1 1', minWidth: 0 }}>
            <Text numberOfLines={1} style={{ color: theme.text, fontSize: 13.5, fontWeight: '600' }}>
              {employee.identity.name} reported a blocker
            </Text>
            <Text numberOfLines={1} style={{ color: theme.textSecondary, fontSize: 12.5 }}>
              {employee.blocker}
            </Text>
          </View>
          <AppSymbol
            name={{ ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
            size={12}
            tintColor={theme.textTertiary}
          />
        </Pressable>
      ))}
    </View>
  );
}

/** The "this job is done" card an expired employee's view renders —
 * distinct from live statuses: it describes the roster record, not the
 * transcript's outcome. */
export function EmployeeExpiredCard({ employee }: { employee: BossEmployee }) {
  const theme = useTheme();
  const label = employee.cancelled
    ? 'Cancelled'
    : employee.blocker != null
      ? 'Needs attention'
      : 'Finished';

  return (
    <View
      style={{
        backgroundColor: theme.surface,
        borderColor: theme.separator,
        borderRadius: Radius.large,
        borderWidth: 1,
        gap: 6,
        padding: 14,
      }}
    >
      <View style={{ alignItems: 'center', flexDirection: 'row', gap: 8 }}>
        <AppSymbol
          name={{ ios: 'checkmark.circle', android: 'check_circle', web: 'check_circle' }}
          size={15}
          tintColor={employee.blocker != null ? theme.warning : theme.textTertiary}
        />
        <Text style={{ color: theme.text, fontSize: 15, fontWeight: '600' }}>{label}</Text>
      </View>
      {employee.blocker ? (
        <Text style={{ color: theme.warning, fontSize: 13 }}>{employee.blocker}</Text>
      ) : null}
      <Text style={{ color: theme.textTertiary, fontSize: 13 }}>
        This employee has finished. Send another request to the boss.
      </Text>
    </View>
  );
}
