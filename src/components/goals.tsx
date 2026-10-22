import type { AgentSession, BossState, Project } from '@waku/client';

import { markBossGoalsViewed } from '@waku/client/boss';

import { memo, useCallback, useEffect, useMemo, useState } from 'octane';
import { Spinner, Pressable, Text, View } from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { BossAvatar } from '@/components/boss-roster';
import { Sheet } from '@/components/sheet';
import { useDaemon } from '@/lib/daemon-hooks';
import {
  GOALS_RECENT_LIMIT,
  GOAL_STATUS_LABELS,
  bossGoalPlanGroups,
  bossGoalSections,
  goalStatusMarker,
  goalUpdatedLabel,
  type GoalPlanGroup,
  type GoalRow,
  type GoalTone,
} from '@/lib/goals';

import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';

const TONE_COLORS = {
  accent: 'accent',
  secondary: 'textSecondary',
  tertiary: 'textTertiary',
  warning: 'warning',
  danger: 'danger',
  success: 'success',
} as const;

const ITEM_PROGRESS_SYMBOL = {
  toDo: { ios: 'circle', android: 'radio_button_unchecked', web: 'radio_button_unchecked' },
  inProgress: { ios: 'circle.inset.filled', android: 'timelapse', web: 'timelapse' },
  done: { ios: 'checkmark.circle.fill', android: 'check_circle', web: 'check_circle' },
  dropped: { ios: 'xmark.circle', android: 'cancel', web: 'cancel' },
} as const;

const ITEM_PROGRESS_TONE: Record<keyof typeof ITEM_PROGRESS_SYMBOL, GoalTone> = {
  toDo: 'tertiary',
  inProgress: 'accent',
  done: 'success',
  dropped: 'tertiary',
};

function statusMarkerSymbol(status: GoalRow['status']) {
  const marker = goalStatusMarker(status);
  return { symbol: marker.symbol, tone: marker.tone, spin: marker.spin };
}

/** The Goals sheet — the mobile form of the desktop's Goals panel tab on
 * the boss chat's strip: Finished history first, then the ongoing area
 * (plan groups ahead of loose In-progress and Pending sections). Only
 * goal summons list — errands never appear. Opening stamps the daemon's
 * `goalsViewedAt`, the unread contract the drawer badge reads. */
export function GoalsSheet({
  state,
  sessions,
  projects,
  visible,
  onDismiss,
  onOpenSession,
}: {
  state: BossState;
  sessions: AgentSession[];
  projects: Project[];
  visible: boolean;
  onDismiss: () => void;
  onOpenSession: (sessionId: string) => void;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [historyExpanded, setHistoryExpanded] = useState(false);
  const [now, setNow] = useState(Date.now);

  // The viewed stamp mirrors the desktop's "Goals page stamps on open"
  // contract — the row badge counts finishes past it.
  const client = daemon.client;
  const connected = daemon.phase === 'connected';
  useEffect(() => {
    if (!visible || !client || !connected) {return;}
    setNow(Date.now());
    void markBossGoalsViewed(client).catch(() => {});
  }, [client, connected, visible]);

  const sessionMap = useMemo(
    () => new Map(sessions.map((session) => [session.id, session])),
    [sessions],
  );

  const projectMap = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );

  const groups = useMemo(
    () => bossGoalPlanGroups(state, sessionMap, projectMap),
    [state, sessionMap, projectMap],
  );

  const grouped = useMemo(() => {
    const ids = new Set<string>();
    for (const group of groups) {
      for (const id of group.memberIds) {ids.add(id);}
    }

    return ids;
  }, [groups]);

  const sections = useMemo(
    () => bossGoalSections(state, sessionMap, projectMap, grouped),
    [state, sessionMap, projectMap, grouped],
  );

  const toggle = useCallback((key: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(key)) {next.delete(key);}
      else {next.add(key);}

      return next;
    });
  }, []);

  const openRow = useCallback(
    (row: GoalRow) => {
      if (!row.navigable) {return;}
      onOpenSession(row.employee.sessionId);
    },
    [onOpenSession],
  );

  const empty =
    sections.finished.length === 0
    && sections.inProgress.length === 0
    && sections.pending.length === 0
    && groups.every((group) => group.items.every((item) => item.rows.length === 0) && group.unallocated.length === 0);

  const visibleFinished = historyExpanded
    ? sections.finished
    : sections.finished.slice(0, GOALS_RECENT_LIMIT);

  const olderCount = sections.finished.length - visibleFinished.length;

  return (
    <Sheet onDismiss={onDismiss} title="Goals" visible={visible}>
      {!connected ? (
        <Text
          style={{
            color: theme.textTertiary,
            fontSize: 12,
            marginBottom: 6,
            paddingLeft: 8,
            paddingRight: 8,
          }}
        >
          Offline · last known state
        </Text>
      ) : null}

      {empty ? (
        <View style={{
          alignItems: 'center',
          paddingLeft: 24,
          paddingRight: 24,
          paddingTop: 28,
          paddingBottom: 28,
        }}>
          <Text style={{ color: theme.text, fontSize: 14, fontWeight: '600' }}>
            No goals yet
          </Text>
          <Text
            style={{ color: theme.textTertiary, fontSize: 13, marginTop: 4, textAlign: 'center' }}
          >
            Ask your Boss to assign a goal.{'\n'}Goals stay here after they finish.
          </Text>
        </View>
      ) : null}

      {sections.finished.length > 0 ? (
        <GoalSectionHeader
          collapsed={collapsed.has('finished')}
          label="Finished"
          onToggle={() => toggle('finished')}
        />
      ) : null}
      {sections.finished.length > 0 && !collapsed.has('finished') ? (
        <View>
          {visibleFinished.map((row) => (
            <GoalRowView now={now} row={row} onOpen={openRow} />
          ))}
          {sections.finished.length > GOALS_RECENT_LIMIT ? (
            <Pressable
              accessibilityRole="button"
              className="vx-pressable"
              onPress={() => setHistoryExpanded((current) => !current)}
              style={{ paddingLeft: 26, paddingRight: 26, paddingTop: 8, paddingBottom: 8 }}
            >
              <Text style={{ color: theme.textSecondary, fontSize: 13.5, fontWeight: '500' }}>
                {historyExpanded ? 'Show less' : `Show more (${olderCount} older)`}
              </Text>
            </Pressable>
          ) : null}
        </View>
      ) : null}

      {groups.map((group) => (
        <GoalPlanGroupView
          collapsed={collapsed.has(`plan:${group.planId}`)}
          group={group}
          now={now}
          onOpen={openRow}
          onToggle={() => toggle(`plan:${group.planId}`)}
        />
      ))}

      <GoalOngoingSection
        collapsed={collapsed.has('inProgress')}
        label="In progress"
        rows={sections.inProgress}
        now={now}
        showHeader={sections.inProgress.length > 0}
        onOpen={openRow}
        onToggle={() => toggle('inProgress')}
      />
      <GoalOngoingSection
        collapsed={collapsed.has('pending')}
        label="Pending"
        rows={sections.pending}
        now={now}
        showHeader={sections.pending.length > 0}
        onOpen={openRow}
        onToggle={() => toggle('pending')}
      />
    </Sheet>
  );
}

/** One collapsible section header — quiet label, trailing chevron, and the
 * "needs attention" tally a folded section keeps in place of its rows. */
function GoalSectionHeader({
  label,
  attention = 0,
  collapsed,
  topGap = false,
  onToggle,
}: {
  label: string;
  attention?: number;
  collapsed: boolean;
  topGap?: boolean;
  onToggle: () => void;
}) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityHint={collapsed ? 'Expands the section' : 'Collapses the section'}
      accessibilityLabel={label}
      accessibilityRole="button"
      className="vx-pressable"
      onPress={onToggle}
      style={{
        alignItems: 'center',
        borderRadius: Radius.small,
        flexDirection: 'row',
        gap: 6,
        marginTop: topGap ? 12 : 4,
        minHeight: 28,
        paddingLeft: 8,
        paddingRight: 8,
      }}
    >
      <Text
        style={{ color: theme.textSecondary, flexShrink: 1, fontSize: 12.5, fontWeight: '600' }}
        numberOfLines={1}
      >
        {label}
      </Text>
      <AppSymbol
        name={{
          ios: collapsed ? 'chevron.right' : 'chevron.down',
          android: collapsed ? 'chevron_right' : 'keyboard_arrow_down',
          web: collapsed ? 'chevron_right' : 'keyboard_arrow_down',
        }}
        size={11}
        tintColor={theme.textTertiary}
      />
      <View style={{ flex: '1 1' }} />
      {attention > 0 && collapsed ? (
        <Text style={{ color: theme.warning, fontSize: 11.5 }}>
          {`${attention} need${attention === 1 ? 's' : ''} attention`}
        </Text>
      ) : null}
    </Pressable>
  );
}

function GoalOngoingSection({
  label,
  rows,
  collapsed,
  showHeader,
  now,
  onOpen,
  onToggle,
}: {
  label: string;
  rows: GoalRow[];
  collapsed: boolean;
  showHeader: boolean;
  now: number;
  onOpen: (row: GoalRow) => void;
  onToggle: () => void;
}) {
  if (!showHeader) {return null;}
  const attention = rows.filter((row) => row.attention).length;
  return (
    <View style={{ marginTop: 6 }}>
      <GoalSectionHeader
        attention={attention}
        collapsed={collapsed}
        label={label}
        onToggle={onToggle}
      />
      {!collapsed
        ? rows.map((row) => (
            <GoalRowView now={now} row={row} onOpen={onOpen} />
          ))
        : null}
    </View>
  );
}

/** One plan group in the ongoing area: the plan's work breakdown in
 * declared order with its linked goal rows nested beneath each item, then
 * the plan-tagged rows with no live item. */
function GoalPlanGroupView({
  group,
  collapsed,
  now,
  onOpen,
  onToggle,
}: {
  group: GoalPlanGroup;
  collapsed: boolean;
  now: number;
  onOpen: (row: GoalRow) => void;
  onToggle: () => void;
}) {
  const theme = useTheme();
  return (
    <View style={{ marginTop: 6 }}>
      <Pressable
        accessibilityHint={collapsed ? 'Expands the plan' : 'Collapses the plan'}
        accessibilityLabel={`${group.title}, plan`}
        accessibilityRole="button"
        className="vx-pressable"
        onPress={onToggle}
        style={{
          alignItems: 'center',
          borderRadius: Radius.small,
          flexDirection: 'row',
          gap: 6,
          minHeight: 28,
          paddingLeft: 8,
          paddingRight: 8,
        }}
      >
        <AppSymbol
          name={{ ios: 'list.bullet.clipboard', android: 'assignment', web: 'assignment' }}
          size={12}
          tintColor={theme.textTertiary}
        />
        <Text
          numberOfLines={1}
          style={{ color: theme.text, flexShrink: 1, fontSize: 13, fontWeight: '600', minWidth: 0 }}
        >
          {group.title}
        </Text>
        <AppSymbol
          name={{
            ios: collapsed ? 'chevron.right' : 'chevron.down',
            android: collapsed ? 'chevron_right' : 'keyboard_arrow_down',
            web: collapsed ? 'chevron_right' : 'keyboard_arrow_down',
          }}
          size={11}
          tintColor={theme.textTertiary}
        />
        <View style={{ flex: '1 1' }} />
        {!group.finalized ? (
          <Text style={{ color: theme.textGhost, fontSize: 11.5 }}>Draft</Text>
        ) : null}
        {group.attention > 0 && collapsed ? (
          <Text style={{ color: theme.warning, fontSize: 11.5 }}>
            {`${group.attention} need${group.attention === 1 ? 's' : ''} attention`}
          </Text>
        ) : null}
      </Pressable>
      {!collapsed ? (
        <View style={{ paddingLeft: 10 }}>
          {group.items.map((item) => (
            <View>
              <View
                style={{
                  alignItems: 'center',
                  flexDirection: 'row',
                  gap: 6,
                  minHeight: 26,
                  paddingLeft: 8,
                  paddingRight: 8,
                }}
              >
                <AppSymbol
                  name={ITEM_PROGRESS_SYMBOL[item.progress]}
                  size={12}
                  tintColor={theme[TONE_COLORS[ITEM_PROGRESS_TONE[item.progress]]]}
                />
                <Text
                  numberOfLines={1}
                  style={{
                    color: item.progress === 'done' || item.progress === 'dropped'
                      ? theme.textTertiary
                      : theme.textSecondary,
                    flexShrink: 1,
                    fontSize: 12.5,
                    minWidth: 0,
                  }}
                >
                  {item.title}
                </Text>
              </View>
              <View style={{ paddingLeft: 10 }}>
                {item.rows.map((row) => (
                  <GoalRowView now={now} row={row} onOpen={onOpen} />
                ))}
              </View>
            </View>
          ))}
          {group.unallocated.map((row) => (
            <GoalRowView now={now} row={row} onOpen={onOpen} />
          ))}
        </View>
      ) : null}
    </View>
  );
}

/** The compact two-line goal row: status icon and title, then an indented
 * detail line — project, employee avatar + name, worktree mark, and the
 * relative update time. A blocker appends its reason in the attention
 * tone (the roster row's convention — there is no hover to reveal it).
 * Rows without a live session stay visible but don't navigate. */
const GoalRowView = memo(function GoalRowView({
  row,
  now,
  onOpen,
}: {
  row: GoalRow;
  now: number;
  onOpen: (row: GoalRow) => void;
}) {
  const theme = useTheme();
  const marker = statusMarkerSymbol(row.status);
  const updatedLabel = goalUpdatedLabel(row.updatedAt, now);
  const employee = row.employee;

  return (
    <Pressable
      accessibilityHint={row.navigable ? 'Opens the employee’s task' : undefined}
      accessibilityLabel={`${row.navigable ? 'Open task: ' : ''}${row.title}, ${employee.identity.name}, ${GOAL_STATUS_LABELS[row.status]}${employee.blocker ? `, ${employee.blocker}` : ''}`}
      accessibilityRole={row.navigable ? 'button' : undefined}
      className={row.navigable ? 'vx-pressable' : undefined}
      onPress={() => onOpen(row)}
      style={{
        borderRadius: Radius.medium,
        gap: 3,
        marginLeft: 6,
        marginRight: 6,
        minHeight: 46,
        paddingLeft: 8,
        paddingRight: 8,
        paddingTop: 7,
        paddingBottom: 7,
      }}
    >
      <View style={{ alignItems: 'center', flexDirection: 'row', gap: 6 }}>
        <View style={{ alignItems: 'center', height: 14, justifyContent: 'center', width: 14 }}>
          {marker.spin ? (
            <Spinner busy style={{ height: 14, width: 14 }} />
          ) : (
            <AppSymbol
              name={marker.symbol}
              size={12}
              tintColor={theme[TONE_COLORS[marker.tone]]}
            />
          )}
        </View>
        <Text
          numberOfLines={1}
          style={{ color: theme.text, flex: '1 1', fontSize: 13.5, fontWeight: '500', minWidth: 0 }}
        >
          {row.title}
        </Text>
      </View>
      <View style={{ alignItems: 'center', flexDirection: 'row', gap: 5, paddingLeft: 20 }}>
        <AppSymbol
          name={{ ios: 'folder', android: 'folder', web: 'folder' }}
          size={11}
          tintColor={theme.textTertiary}
        />
        <Text
          numberOfLines={1}
          style={{ color: theme.textTertiary, flexShrink: 3, fontSize: 11.5, minWidth: 0 }}
        >
          {row.projectLabel}
        </Text>
        <Text style={{ color: theme.textGhost, fontSize: 11.5 }}>·</Text>
        <BossAvatar identity={employee.identity} size={15} />
        <Text
          numberOfLines={1}
          style={{ color: theme.textSecondary, flexShrink: 2, fontSize: 11.5, minWidth: 0 }}
        >
          {employee.identity.name}
        </Text>
        {employee.blocker ? (
          <Text
            numberOfLines={1}
            style={{ color: theme.warning, flexShrink: 1, fontSize: 11.5, minWidth: 0 }}
          >
            {`— ${employee.blocker}`}
          </Text>
        ) : null}
        <View style={{ flex: '1 1' }} />
        {row.worktree ? (
          <AppSymbol
            name={{ ios: 'arrow.triangle.branch', android: 'call_split', web: 'call_split' }}
            size={11}
            tintColor={theme.textTertiary}
          />
        ) : null}
        {updatedLabel ? (
          <Text style={{ color: theme.textTertiary, fontSize: 11.5 }}>{updatedLabel}</Text>
        ) : null}
      </View>
    </Pressable>
  );
});
