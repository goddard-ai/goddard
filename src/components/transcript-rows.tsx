const hairline = 0.5;

import type {
  AgentSession,
  BossIdentity,
  Checkpoint,
  Message,
  ProviderKind,
  ReportTrigger,
  ReportTriggerKind,
  TranscriptNoticeStatus,
} from '@waku/client';

import {
  bossTriggerBurstLabel,
  bossTriggerEntryLabel,
  bossTriggerGroupTone,
  bossTriggerTone,
  type BossSessionMention,
  type BossTriggerTone,
} from '@waku/client/boss-chat';

import { atomSegments, atomVisibleText, formatMessageTime } from '@waku/client/transcript-presentation';
import { memo, useEffect, useState } from 'octane';
import { ActivityIndicator, Pressable, RichText, RichTextSpan, Text, pushRoute, View } from '@octane-xplat/ui';

import { setStringAsync as copyToClipboard } from '@/lib/clipboard';
import { Haptics } from '@/lib/haptics';

import { ActivityGroup } from '@/components/activity-group';
import { AppSymbol } from '@/components/app-symbol';
import { AttachmentTile } from '@/components/attachment-tile';
import { BossAvatar } from '@/components/boss-roster';
import { GoddardThinking } from '@/components/goddard-thinking';
import { ProviderIcon } from '@/components/provider-icon';
import {
  MdBlockLive,
  MdBlockSettled,
  MdRevealOnMount,
  type VeilRegistry,
} from '@/components/md-block-row';

import { useRowAnchor } from '@/components/transcript-anchor';
import { MonoFont } from '@/lib/theme-platform';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import type { TranscriptRow } from '@/lib/session-presentation';
import type { MarkdownStyles } from '@/md/render.tsrx';
import type { TranscriptMarkdownCache } from '@/md/transcript-cache';

/**
 * One transcript row's content. Memoized on its props: the list hands it the
 * same row object whenever nothing about the row changed, so a stream commit
 * re-renders exactly the live tail block.
 */
export const TranscriptRowView = memo(function TranscriptRowView({
  row,
  md,
  veils,
  seeded,
  markdownStyles,
  rewindTurns,
  forkTurns,
  sessionMentions,
  onToggleFold,
  onToggleTrigger,
  onLoadEarlier,
  onRewindTurn,
  onForkTurn,
}: {
  row: TranscriptRow;
  md: TranscriptMarkdownCache;
  veils: VeilRegistry;
  seeded: boolean;
  markdownStyles: MarkdownStyles;
  rewindTurns?: ReadonlySet<string>;
  forkTurns?: ReadonlySet<string>;
  /** Roster-name → session chips prose resolves on boss surfaces. */
  sessionMentions?: readonly BossSessionMention[];
  onToggleFold: (turnId: string) => void;
  onToggleTrigger?: (eventId: string) => void;
  onLoadEarlier?: () => void;
  onRewindTurn?: (turnId: string) => void;
  onForkTurn?: (turnId: string) => void;
}) {
  const theme = useTheme();
  switch (row.kind) {
    case 'user': {
      const rewindTurnId = row.turnId && rewindTurns?.has(row.turnId) ? row.turnId : null;
      return (
        <UserBubble
          message={row.message}
          onRewind={rewindTurnId && onRewindTurn ? () => onRewindTurn(rewindTurnId) : undefined}
        />
      );
    }
    case 'system':
      return (
        <View style={styles.systemFrame}>
          <Text style={[styles.systemMessage, { backgroundColor: theme.overlay, color: theme.textTertiary }]}>
            {atomVisibleText(row.message.display_content ?? row.message.content)}
          </Text>
        </View>
      );
    case 'md': {
      // The footer sits on the last block of a turn's closing response —
      // the same spot desktop hangs its fork button.
      const forkTurnId = row.footerTimestamp != null && row.turnId && forkTurns?.has(row.turnId)
        ? row.turnId
        : null;

      return (
        <>
          {row.live ? (
            <MdBlockLive
              md={md}
              mentions={sessionMentions}
              messageId={row.messageId}
              node={row.node}
              rowKey={row.key}
              seeded={seeded}
              source={row.source}
              styles={markdownStyles}
              veils={veils}
            />
          ) : (
            <MdRevealOnMount enabled={row.streaming && !seeded}>
              <MdBlockSettled mentions={sessionMentions} node={row.node} source={row.source} styles={markdownStyles} />
            </MdRevealOnMount>
          )}
          {row.footerTimestamp != null && (
            <View style={styles.messageFooterRow}>
              <Text style={[styles.messageFooter, { color: theme.textGhost }]}>
                {formatMessageTime(row.footerTimestamp)}
              </Text>
              {forkTurnId && onForkTurn && (
                <Pressable
                  accessibilityHint="Copies the task up to this response"
                  accessibilityLabel="Fork from here"
                  accessibilityRole="button"
                                   onPress={() => onForkTurn(forkTurnId)}
                  style={styles.footerAction}>
                  <AppSymbol
                    name={{ ios: 'arrow.triangle.branch', android: 'call_split', web: 'call_split' }}
                    size={12}
                    tintColor={theme.textGhost}
                  />
                </Pressable>
              )}
            </View>
          )}
        </>
      );
    }
    case 'activities':
      return <ActivityGroup block={row.block} blockIndex={row.blockIndex} live={row.live} />;
    case 'bossTrigger':
      return (
        <BossTriggerRow
          expanded={row.expanded}
          group={row.group}
          onToggle={onToggleTrigger}
        />
      );
    case 'notice':
      return <NoticeRow message={row.message} provider={row.provider} />;
    case 'historyBoundary':
      return (
        <HistoryBoundaryRow
          error={row.error}
          loading={row.loading}
          onLoad={onLoadEarlier}
        />
      );
    case 'fold':
      return (
        <FoldRow
          expanded={row.expanded}
          label={row.label}
          turnId={row.turn.id}
          onToggle={onToggleFold}
        />
      );
    case 'changed':
      return <ChangedFilesCard checkpoint={row.checkpoint} />;
  }
});

/** Desktop's turn fold: a hairline divider carrying "Worked for X ›". */
function FoldRow({
  label,
  expanded,
  turnId,
  onToggle,
}: {
  label: string;
  expanded: boolean;
  turnId: string;
  onToggle: (turnId: string) => void;
}) {
  const theme = useTheme();
  const keepTop = useRowAnchor();
  return (
    <Pressable
      accessibilityHint={expanded ? 'Collapses the agent’s work' : 'Shows the agent’s work'}
      accessibilityLabel={label}
      accessibilityRole="button"
      
            onPress={() => keepTop(() => onToggle(turnId))}
      style={styles.foldRow}>
      <View style={[styles.foldLine, { backgroundColor: theme.border }]} />
      <Text numberOfLines={1} style={[styles.foldLabel, { color: theme.textTertiary }]}>
        {label}
      </Text>
      <AppSymbol
        name={expanded
          ? { ios: 'chevron.down', android: 'keyboard_arrow_down', web: 'keyboard_arrow_down' }
          : { ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
        size={10}
        tintColor={theme.textGhost}
      />
      <View style={[styles.foldLine, { backgroundColor: theme.border }]} />
    </Pressable>
  );
}

const TRIGGER_TONE_COLOR = {
  tertiary: 'textTertiary',
  warning: 'warning',
  danger: 'danger',
} as const satisfies Record<BossTriggerTone, string>;

function triggerSymbol(kind: ReportTriggerKind): Parameters<typeof AppSymbol>[0]['name'] {
  switch (kind) {
    case 'finished':
      return { ios: 'checkmark.circle', android: 'check_circle', web: 'check_circle' };
    case 'failed':
      return { ios: 'xmark.octagon', android: 'cancel', web: 'cancel' };
    case 'blocker':
    case 'finishedWithBlocker':
    case 'interrupted':
      return { ios: 'exclamationmark.circle', android: 'error', web: 'error' };
    case 'planFinalized':
      return { ios: 'doc.text', android: 'description', web: 'description' };
  }
}

function openEmployee(sessionId: string) {
  pushRoute({ stack: 'root', name: 'session/:id', params: { id: sessionId } });
}

/** One report entry: a status glyph, the event-time label, and a tap that
 * opens the reporting employee's transcript — desktop's
 * `render_boss_trigger_entry`. */
function BossTriggerEntry({
  trigger,
  duringTurn,
  indent = 0,
}: {
  trigger: ReportTrigger;
  duringTurn: boolean;
  indent?: number;
}) {
  const theme = useTheme();
  const label = bossTriggerEntryLabel(trigger, duringTurn);
  const accessibilityLabel = trigger.jobTitle ? `${label} · ${trigger.jobTitle}` : label;
  return (
    <Pressable
      accessibilityHint="Opens the employee's transcript"
      accessibilityLabel={accessibilityLabel}
      accessibilityRole="button"
      onPress={() => openEmployee(trigger.employee)}
      style={[styles.triggerEntry, { paddingLeft: indent }]}>
      <AppSymbol
        name={triggerSymbol(trigger.kind)}
        size={12}
        tintColor={theme[TRIGGER_TONE_COLOR[bossTriggerTone(trigger.kind)] as keyof typeof theme] as string}
      />
      <Text numberOfLines={1} style={[styles.triggerLabel, { color: theme.textSecondary }]}>
        {label}
      </Text>
      <AppSymbol
        name={{ ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
        size={9}
        tintColor={theme.textGhost}
      />
    </Pressable>
  );
}

/** The transcript's report-trigger marker: a lone report is one quiet row;
 * a burst collapses into an expandable "N employee updates" header —
 * desktop's `render_boss_trigger_row`. */
function BossTriggerRow({
  group,
  expanded,
  onToggle,
}: {
  group: { boundary: 'opening' | 'steer'; triggers: ReportTrigger[] };
  expanded: boolean;
  onToggle?: (eventId: string) => void;
}) {
  const theme = useTheme();
  const duringTurn = group.boundary === 'steer';
  if (group.triggers.length === 1) {
    return <BossTriggerEntry duringTurn={duringTurn} trigger={group.triggers[0]!} />;
  }

  const anchor = group.triggers[0];
  if (!anchor) {return null;}
  return (
    <View>
      <Pressable
        accessibilityHint={expanded ? 'Collapses the employee updates' : 'Shows each employee update'}
        accessibilityLabel={bossTriggerBurstLabel(group.triggers)}
        accessibilityRole="button"
        onPress={() => onToggle?.(anchor.eventId)}
        style={styles.triggerEntry}>
        <AppSymbol
          name={expanded
            ? { ios: 'chevron.down', android: 'keyboard_arrow_down', web: 'keyboard_arrow_down' }
            : { ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
          size={11}
          tintColor={theme.textGhost}
        />
        <AppSymbol
          name={triggerSymbol(
            group.triggers.some((trigger) => trigger.kind === 'failed')
              ? 'failed'
              : group.triggers.some((trigger) => bossTriggerTone(trigger.kind) === 'warning')
                ? 'blocker'
                : 'finished',
          )}
          size={12}
          tintColor={theme[TRIGGER_TONE_COLOR[bossTriggerGroupTone(group.triggers)] as keyof typeof theme] as string}
        />
        <Text numberOfLines={1} style={[styles.triggerLabel, { color: theme.textSecondary }]}>
          {bossTriggerBurstLabel(group.triggers)}
        </Text>
      </Pressable>
      {expanded ? (
        <View style={styles.triggerEntries}>
          {group.triggers.map((trigger) => (
            <BossTriggerEntry
              duringTurn={duringTurn}
              indent={31}
              trigger={trigger}
            />
          ))}
        </View>
      ) : null}
    </View>
  );
}

const STATUS_NOTICE_SYMBOLS: Record<TranscriptNoticeStatus, Parameters<typeof AppSymbol>[0]['name']> = {
  stopped: { ios: 'hand.raised', android: 'pan_tool', web: 'pan_tool' },
  completed: { ios: 'checkmark.circle', android: 'check_circle', web: 'check_circle' },
  stoppedBeforeResponse: { ios: 'xmark.octagon', android: 'cancel', web: 'cancel' },
  outOfContext: { ios: 'battery.25', android: 'battery_2_bar', web: 'battery_2_bar' },
  declined: { ios: 'nosign', android: 'block', web: 'block' },
  stoppedWithReason: { ios: 'exclamationmark.octagon', android: 'report', web: 'report' },
  exited: { ios: 'powerplug', android: 'power_off', web: 'power_off' },
  interrupted: { ios: 'exclamationmark.octagon', android: 'report', web: 'report' },
  startFailed: { ios: 'exclamationmark.circle', android: 'error', web: 'error' },
  error: { ios: 'exclamationmark.triangle', android: 'warning', web: 'warning' },
  goal: { ios: 'flag', android: 'flag', web: 'flag' },
  projectSwitched: { ios: 'folder', android: 'folder', web: 'folder' },
  // The model glyph is the provider's own mark — handled separately.
  modelSwitched: { ios: 'cpu', android: 'memory', web: 'memory' },
};

function StatusNoticeGlyph({
  kind,
  provider,
  size,
}: {
  kind: TranscriptNoticeStatus;
  provider: ProviderKind;
  size: number;
}) {
  const theme = useTheme();
  if (kind === 'modelSwitched') {
    return <ProviderIcon provider={provider} size={size} />;
  }

  return <AppSymbol name={STATUS_NOTICE_SYMBOLS[kind]} size={size} tintColor={theme.textTertiary} />;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {return `${bytes} B`;}
  if (bytes < 1024 * 1024) {return `${(bytes / 1024).toFixed(1)} KB`;}
  if (bytes < 1024 * 1024 * 1024) {return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;}
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

const LANDED_SHOWN_COMMITS = 5;

/** The "Landed on {base}" card — header disclosure over the commit list,
 * wearing the changed-files card's chrome. The desktop's push/archive
 * controls stay desktop-side; the card is read-only here. */
function LandedNoticeCard({
  base,
  commits,
  ahead,
}: {
  base: string;
  commits: NonNullable<Extract<Message['notice'], { type: 'landed' }>['commits']>;
  ahead: number;
}) {
  const theme = useTheme();
  const keepTop = useRowAnchor();
  const [open, setOpen] = useState(false);
  const [showAll, setShowAll] = useState(false);
  const shown = showAll ? commits : commits.slice(0, LANDED_SHOWN_COMMITS);
  const unloaded = Math.max(0, ahead - commits.length);
  return (
    <View style={[styles.noticeCard, { backgroundColor: theme.overlay, borderColor: theme.border }]}>
      <Pressable
        accessibilityHint={open ? 'Collapses the landed commits' : 'Shows the landed commits'}
        accessibilityLabel={`Landed on ${base}`}
        accessibilityRole="button"
        onPress={() => keepTop(() => setOpen((value) => !value))}
        style={styles.noticeHeader}>
        <AppSymbol
          name={{ ios: 'arrow.triangle.merge', android: 'call_merge', web: 'call_merge' }}
          size={12}
          tintColor={theme.textTertiary}
        />
        <Text numberOfLines={1} style={[styles.noticeTitle, { color: theme.textSecondary }]}>
          Landed on {base}
        </Text>
        <AppSymbol
          name={open
            ? { ios: 'chevron.down', android: 'keyboard_arrow_down', web: 'keyboard_arrow_down' }
            : { ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
          size={11}
          tintColor={theme.textGhost}
        />
      </Pressable>
      {open ? (
        <View style={[styles.noticeBody, { borderTopColor: theme.border }]}>
          {shown.map((commit) => (
            <View style={styles.noticeCommit}>
              <Text style={[styles.noticeSha, { color: theme.textTertiary, fontFamily: MonoFont }]}>
                {commit.short_sha}
              </Text>
              <Text numberOfLines={1} style={[styles.noticeCommitSubject, { color: theme.textSecondary }]}>
                {commit.subject}
              </Text>
            </View>
          ))}
          {commits.length > LANDED_SHOWN_COMMITS && !showAll ? (
            <Pressable
              accessibilityRole="button"
              onPress={() => setShowAll(true)}
              style={styles.noticeMore}>
              <Text style={{ fontSize: 12.5, color: theme.textTertiary }}>
                Show {commits.length - LANDED_SHOWN_COMMITS} more commits
              </Text>
            </Pressable>
          ) : null}
          {unloaded > 0 ? (
            <Text style={[styles.noticeMore, { fontSize: 12.5, color: theme.textGhost }]}>
              {unloaded} earlier commit{unloaded === 1 ? '' : 's'} not loaded
            </Text>
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

/** A `transferReceived` receipt: sender, title, and a folder's children or
 * the file's kind and size. Trust/file actions are desktop-local; the card
 * renders read-only here. */
function TransferNoticeCard({ message }: { message: Message }) {
  const theme = useTheme();
  const notice = message.notice;
  if (notice?.type !== 'transferReceived') {return null;}
  const detail = notice.isDir
    ? notice.entryCount === 0
      ? 'Empty folder'
      : `${notice.entryCount} item${notice.entryCount === 1 ? '' : 's'}`
    : notice.title.split('.').at(-1)?.toUpperCase() ?? 'File';

  return (
    <View style={[styles.noticeCard, { backgroundColor: theme.overlay, borderColor: theme.border }]}>
      <View style={styles.noticeHeader}>
        <AppSymbol
          name={notice.isDir
            ? { ios: 'folder', android: 'folder', web: 'folder' }
            : { ios: 'doc', android: 'description', web: 'description' }}
          size={14}
          tintColor={theme.textTertiary}
        />
        <View style={{ flex: '1 1', minWidth: 0 }}>
          <Text numberOfLines={1} style={[styles.noticeTitle, { color: theme.text }]}>
            {notice.title}
          </Text>
          <Text numberOfLines={1} style={{ fontSize: 12, color: theme.textTertiary }}>
            From {notice.peerName} · {detail} · {formatBytes(notice.sizeBytes)}
          </Text>
        </View>
      </View>
      {notice.entries?.length ? (
        <View style={[styles.noticeBody, { borderTopColor: theme.border }]}>
          {notice.entries.map((entry) => (
            <View style={styles.noticeCommit}>
              <AppSymbol
                name={entry.isDir
                  ? { ios: 'folder', android: 'folder', web: 'folder' }
                  : { ios: 'doc', android: 'description', web: 'description' }}
                size={11}
                tintColor={theme.textTertiary}
              />
              <Text numberOfLines={1} style={[styles.noticeCommitSubject, { color: theme.textSecondary }]}>
                {entry.name}
              </Text>
              <Text style={{ fontSize: 11.5, color: theme.textGhost }}>
                {entry.isDir ? '' : formatBytes(entry.sizeBytes)}
              </Text>
            </View>
          ))}
          {notice.entryCount > notice.entries.length ? (
            <Text style={[styles.noticeMore, { fontSize: 12.5, color: theme.textGhost }]}>
              +{notice.entryCount - notice.entries.length} more
            </Text>
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

/** A structured `notice` message: status pills and rows keep the plain
 * `content` text and add the kind's glyph; landed and transfer receipts get
 * their cards. */
function NoticeRow({ message, provider }: { message: Message; provider: ProviderKind }) {
  const theme = useTheme();
  const notice = message.notice;
  const content = atomVisibleText(message.display_content ?? message.content);
  if (!notice) {return null;}

  if (notice.type === 'landed') {
    return <LandedNoticeCard ahead={notice.ahead} base={notice.base} commits={notice.commits} />;
  }

  if (notice.type === 'transferReceived') {
    return <TransferNoticeCard message={message} />;
  }

  const status = notice.type === 'status' ? notice.kind : null;
  if (message.role === 'assistant' && status) {
    return (
      <View style={styles.noticeStatusRow}>
        <StatusNoticeGlyph kind={status} provider={provider} size={12} />
        <Text style={[styles.noticeStatusText, { color: theme.textTertiary }]}>{content}</Text>
      </View>
    );
  }

  return (
    <View style={styles.systemFrame}>
      <View style={[styles.systemNotice, { backgroundColor: theme.overlay }]}>
        {status ? <StatusNoticeGlyph kind={status} provider={provider} size={11} /> : null}
        <Text style={[styles.systemMessageText, { color: theme.textTertiary }]}>{content}</Text>
      </View>
    </View>
  );
}

/** The "Load earlier conversation" affordance where the boss chat's
 * rotation chain ran out — desktop's `render_boss_history_boundary`. */
function HistoryBoundaryRow({
  loading,
  error,
  onLoad,
}: {
  loading: boolean;
  error: string | null;
  onLoad?: () => void;
}) {
  const theme = useTheme();
  const label = loading
    ? 'Loading earlier conversation…'
    : error
      ? 'Retry loading earlier conversation'
      : 'Load earlier conversation';

  return (
    <View style={styles.historyBoundary}>
      <Pressable
        accessibilityRole="button"
        disabled={loading}
        onPress={onLoad}
        style={styles.historyButton}>
        {loading ? <ActivityIndicator busy /> : null}
        <Text style={[styles.historyLabel, { color: theme.textSecondary }]}>{label}</Text>
      </Pressable>
      {error ? (
        <Text style={[styles.historyError, { color: theme.textSecondary }]}>{error}</Text>
      ) : null}
    </View>
  );
}

function UserBubbleInner({ message, onRewind }: { message: Message; onRewind?: () => void }) {
  const theme = useTheme();
  const segments = atomSegments(message.display_content ?? message.content, message.atoms);
  const hasAtoms = segments.some((segment) => segment.kind === 'atom');
  const content = hasAtoms
    ? ''
    : atomVisibleText(message.display_content ?? message.content);

  async function copy() {
    await copyToClipboard(message.content);
    await Haptics.notificationAsync('success');
  }

  return (
    <View style={styles.userFrame}>
      {message.sent_by_task ? (
        <View style={[styles.agentBadge, { backgroundColor: theme.overlay }]}>
          <AppSymbol
            name={{ ios: 'cpu', android: 'smart_toy', web: 'smart_toy' }}
            size={9}
            tintColor={theme.textTertiary}
          />
          <Text style={[styles.agentBadgeText, { color: theme.textTertiary }]}>Sent by agent</Text>
        </View>
      ) : null}
      {message.attachments?.length ? (
        <View style={styles.attachments}>
          {message.attachments.map((attachment) => (
            <AttachmentTile
              attachment={attachment}
            />
          ))}
        </View>
      ) : null}
      {segments.length ? (
        <Pressable
          accessibilityHint="Long press to copy"
                   onLongPress={() => void copy()}
          style={[styles.userBubble, { backgroundColor: theme.raised }]}>
          {hasAtoms ? (
            <RichText style={[styles.userText, { color: theme.text }]}>
              {segments.map((segment) => segment.kind === 'text'
                ? <RichTextSpan text={segment.text} />
                : <AtomChip label={segment.label} sessionId={segment.sessionId} />)}
            </RichText>
          ) : (
            <Text style={[styles.userText, { color: theme.text }]}>{content}</Text>
          )}
        </Pressable>
      ) : null}
      {onRewind && (
        <Pressable
          accessibilityHint="Restores the task to before this message"
          accessibilityLabel="Rewind here"
          accessibilityRole="button"
                   onPress={onRewind}
          style={styles.userFooterAction}>
          <AppSymbol
            name={{ ios: 'arrow.uturn.backward', android: 'undo', web: 'undo' }}
            size={12}
            tintColor={theme.textGhost}
          />
        </Pressable>
      )}
    </View>
  );
}

const UserBubble = memo(
  UserBubbleInner,
  (previous, next) =>
    previous.message.id === next.message.id &&
    previous.message.content === next.message.content &&
    previous.message.display_content === next.message.display_content &&
    previous.message.attachments === next.message.attachments &&
    previous.message.atoms === next.message.atoms &&
    previous.message.sent_by_task === next.message.sent_by_task &&
    (previous.onRewind != null) === (next.onRewind != null),
);

/** A submitted inline atom — the desktop paints an accent-washed chip with an
 * icon slot and links session atoms to their task. Spans can't hold a view,
 * so the chip is the tinted label alone; a task reference still navigates. */
function AtomChip({ label, sessionId }: { label: string; sessionId: string | null }) {
  const theme = useTheme();
  return (
    <RichTextSpan
      text={` ${label} `}
      onPress={sessionId
        ? () => pushRoute({ stack: 'root', name: 'session/:id', params: { id: sessionId } })
        : undefined}
      style={{
        backgroundColor: theme.accentSoft,
        color: theme.accent,
      }}
    />
  );
}

function ChangedFilesCard({ checkpoint }: { checkpoint: Checkpoint }) {
  const theme = useTheme();
  const keepTop = useRowAnchor();
  const [open, setOpen] = useState(false);
  return (
    <View style={[styles.changedCard, { backgroundColor: theme.surface, borderColor: theme.border }]}>
      <Pressable
        accessibilityRole="button"
        
        onPress={() => keepTop(() => setOpen((value) => !value))}
        style={styles.changedHeader}>
        <AppSymbol
          name={{ ios: 'plusminus', android: 'difference', web: 'difference' }}
          size={13}
          tintColor={theme.textSecondary}
        />
        <Text style={[styles.changedTitle, { color: theme.textSecondary }]}>
          {checkpoint.files.length} file{checkpoint.files.length === 1 ? '' : 's'} changed
        </Text>
        <Text style={styles.changedStats}>
          <Text style={{ color: theme.success }}>+{checkpoint.additions}</Text>
          <Text style={{ color: theme.textGhost }}> </Text>
          <Text style={{ color: theme.danger }}>−{checkpoint.deletions}</Text>
        </Text>
        <AppSymbol
          name={open
            ? { ios: 'chevron.up', android: 'keyboard_arrow_up', web: 'keyboard_arrow_up' }
            : { ios: 'chevron.down', android: 'keyboard_arrow_down', web: 'keyboard_arrow_down' }}
          size={11}
          tintColor={theme.textGhost}
        />
      </Pressable>
      {open && (
        <View style={[styles.changedFiles, { borderTopColor: theme.border }]}>
          {checkpoint.files.map((file) => (
            <View style={styles.changedFileRow}>
              <Text numberOfLines={1} style={[styles.changedFilePath, { color: theme.text }]}>
                {file.path}
              </Text>
              <Text style={styles.changedStats}>
                <Text style={{ color: theme.success }}>+{file.additions}</Text>
                <Text style={{ color: theme.textGhost }}> </Text>
                <Text style={{ color: theme.danger }}>−{file.deletions}</Text>
              </Text>
            </View>
          ))}
        </View>
      )}
    </View>
  );
}

/** Working-indicator flavour words, rotated every 7s, seeded per chat. */
const FLAVOUR_WORDS = [
  'Thinking', 'Pondering', 'Scheming', 'Brewing', 'Weaving', 'Tinkering',
  'Musing', 'Composing', 'Sifting', 'Untangling', 'Distilling', 'Sketching',
  'Plotting', 'Riffing', 'Combobulating', 'Percolating', 'Marinating',
  'Noodling', 'Puzzling', 'Conjuring',
];

function flavourSeed(chatId: string): number {
  let hash = 0x811c9dc5;
  for (let ix = 0; ix < chatId.length; ix += 1) {
    hash ^= chatId.charCodeAt(ix);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }

  return hash;
}

function formatElapsed(seconds: number): string {
  const clamped = Math.max(0, seconds);
  if (clamped < 60) {return `${clamped}s`;}
  return `${Math.floor(clamped / 60)}m ${clamped % 60}s`;
}

/** The live footer under the streaming tail: the thinking mark, a flavour
 * word, and the elapsed count. */
export function WorkingStrip({ session }: { session: AgentSession }) {
  const theme = useTheme();
  const startedAt = session.turns.at(-1)?.started_at ?? null;
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1_000));
  useEffect(() => {
    const timer = setInterval(() => setNow(Math.floor(Date.now() / 1_000)), 1_000);
    return () => clearInterval(timer);
  }, []);

  const elapsed = startedAt ? Math.max(0, now - startedAt) : 0;
  const word = session.status === 'waiting'
    ? 'Waiting for you'
    : `${FLAVOUR_WORDS[(flavourSeed(session.id) + Math.floor(elapsed / 7)) % FLAVOUR_WORDS.length]}…`;

  return (
    <View accessibilityLiveRegion="polite" style={styles.workingStrip}>
      <GoddardThinking color={theme.textTertiary} />
      <Text style={[styles.workingText, { color: theme.textTertiary }]}>{word}</Text>
      {startedAt != null && session.status !== 'waiting' && (
        <Text style={[styles.workingElapsed, { color: theme.textGhost }]}>
          {formatElapsed(elapsed)}
        </Text>
      )}
    </View>
  );
}

/** Sits above the oldest mounted row while more history exists; the window
 * extends ahead of the reader so this is rarely on screen for long. */
export function EarlierIndicator() {
  return (
    <View accessibilityLabel="Loading earlier messages" style={styles.earlier}>
      <ActivityIndicator busy />
    </View>
  );
}

export function SessionEmpty({
  loading,
  error,
  missing,
  boss,
}: {
  loading: boolean;
  error: unknown;
  missing: boolean;
  /** Set on the boss chat — its empty state is the per-daemon identity plus
   * the desktop greeting, not the generic "No messages yet". */
  boss?: BossIdentity | null;
}) {
  const theme = useTheme();
  if (loading) {return <ActivityIndicator busy />;}
  if (boss && !error && !missing) {
    return (
      <View style={styles.empty}>
        <BossAvatar identity={boss} size={54} />
        <Text style={[styles.emptyTitle, { color: theme.text, marginTop: 14 }]}>
          What should we do today?
        </Text>
      </View>
    );
  }

  return (
    <View style={styles.empty}>
      <Text style={[styles.emptyTitle, { color: theme.text }]}>
        {missing ? 'Task not found' : error ? 'Couldn’t load this task' : 'No messages yet'}
      </Text>
      {Boolean(error) && (
        <Text style={[styles.emptyBody, { color: theme.textSecondary }]}>
          {error instanceof Error ? error.message : String(error)}
        </Text>
      )}
    </View>
  );
}

const styles = {
  earlier: { alignItems: 'center', paddingVertical: 10 },
  userFrame: { alignItems: 'flex-end', gap: 6 },
  userBubble: {
    borderRadius: Radius.large,
    borderBottomRightRadius: 6,
    maxWidth: '88%',
    paddingHorizontal: 16,
    paddingVertical: 10,
  },
  userText: { fontSize: 15, lineHeight: '21px' },
  agentBadge: {
    alignItems: 'center',
    borderRadius: Radius.pill,
    flexDirection: 'row',
    gap: 4,
    paddingHorizontal: 8,
    paddingVertical: 2,
  },
  agentBadgeText: { fontSize: 11.5, lineHeight: '14px' },
  messageFooter: { fontSize: 11.5 },
  messageFooterRow: { alignItems: 'center', flexDirection: 'row', gap: 10, marginTop: 10 },
  footerAction: { padding: 4 },
  userFooterAction: { alignSelf: 'flex-end', marginTop: 4, padding: 4 },
  systemFrame: { alignItems: 'center' },
  systemNotice: {
    alignItems: 'center',
    borderRadius: Radius.pill,
    flexDirection: 'row',
    gap: 5,
    paddingHorizontal: 10,
    paddingVertical: 4,
  },
  systemMessageText: { fontSize: 12.5, lineHeight: '16px' },
  triggerEntry: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 8,
    minHeight: 28,
    paddingHorizontal: 4,
  },
  triggerEntries: { marginTop: 4 },
  triggerLabel: { flex: '1 1', fontSize: 12.5, lineHeight: '18px' },
  noticeCard: {
    borderRadius: Radius.medium,
    borderWidth: hairline,
    overflow: 'hidden',
  },
  noticeHeader: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 8,
    minHeight: 34,
    paddingHorizontal: 12,
  },
  noticeTitle: { flex: '1 1', fontSize: 12.5, fontWeight: '600' },
  noticeBody: {
    borderTopWidth: hairline,
    gap: 4,
    paddingHorizontal: 12,
    paddingVertical: 8,
  },
  noticeCommit: { alignItems: 'center', flexDirection: 'row', gap: 8, minHeight: 22 },
  noticeSha: { fontSize: 12 },
  noticeCommitSubject: { flex: '1 1', fontSize: 12.5 },
  noticeMore: { paddingVertical: 4 },
  noticeStatusRow: { alignItems: 'center', flexDirection: 'row', gap: 6, paddingVertical: 4 },
  noticeStatusText: { flex: '1 1', fontSize: 12.5, lineHeight: '16px' },
  historyBoundary: { alignItems: 'center', gap: 4, paddingVertical: 12 },
  historyButton: {
    alignItems: 'center',
    borderRadius: Radius.small,
    flexDirection: 'row',
    gap: 6,
    paddingHorizontal: 12,
    paddingVertical: 6,
  },
  historyLabel: { fontSize: 12.5 },
  historyError: { fontSize: 12 },
  systemMessage: {
    borderRadius: Radius.pill,
    fontSize: 12.5,
    lineHeight: '16px',
    overflow: 'hidden',
    paddingHorizontal: 11,
    paddingVertical: 5,
    textAlign: 'center',
  },
  attachments: { flexDirection: 'row', flexWrap: 'wrap', justifyContent: 'flex-end', gap: 8, maxWidth: '88%' },
  foldRow: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 9,
    minHeight: 28,
  },
  foldLine: { flex: '1 1', height: hairline },
  foldLabel: { flexShrink: 1, fontSize: 13, fontWeight: '500' },
  changedCard: {
    borderRadius: Radius.medium,
    borderWidth: hairline,
    overflow: 'hidden',
  },
  changedHeader: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 8,
    minHeight: 40,
    paddingHorizontal: 11,
  },
  changedTitle: { flex: '1 1', fontSize: 13.5, fontWeight: '600' },
  changedStats: { fontSize: 12.5, fontVariant: ['tabular-nums'], fontWeight: '600' },
  changedFiles: {
    borderTopWidth: hairline,
    gap: 6,
    paddingHorizontal: 11,
    paddingVertical: 9,
  },
  changedFileRow: { alignItems: 'center', flexDirection: 'row', gap: 8 },
  changedFilePath: { flex: '1 1', fontFamily: MonoFont, fontSize: 12 },
  workingStrip: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 8,
    paddingBottom: 8,
    paddingTop: 14,
  },
  workingText: { fontSize: 13.5, fontWeight: '500' },
  workingElapsed: { fontSize: 13, fontVariant: ['tabular-nums'] },
  empty: { alignItems: 'center', paddingHorizontal: 32 },
  emptyTitle: { fontSize: 18, fontWeight: '700', textAlign: 'center' },
  emptyBody: { fontSize: 14.5, lineHeight: '19px', marginTop: 8, textAlign: 'center' },
};
