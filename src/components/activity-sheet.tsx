
import type { ActivityFileChange, ActivityItem, AgentSession } from '@waku/client';
import { activitiesForBlock } from '@waku/client/event-reducer';
import {
  activityActionLabel,
  activityDisclosureSections,
  activityDisplayTitle,
  activityFileChangeStats,
  activityPreview,
  activityRowDetail,
  reasoningTitle,
} from '@waku/client/transcript-presentation';

import {
  useCallback,
  useEffect,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from 'octane';

import { createStore, useStore } from '@octane-xplat/ui';
import {
  Image,
  Pressable,
  RichText,
  RichTextSpan,
  ScrollView,
  Text,
  useSafeAreaInsets,
  View,
} from '@octane-xplat/ui';

import { Sheet as XSheet } from '@octane-xplat/ui';

import { ACTIVITY_ICONS } from './activity-icons';
import { AppSymbol } from './app-symbol';
import { DiffView } from './diff-view';

import { MonoFont, NativeTint } from '@/lib/theme-platform';
import { Radius, Spacing } from '@/lib/theme-colors';
import { useReducedMotion } from '@/lib/use-reduced-motion';
import { useTheme } from '@/lib/use-theme';
import { findActivityBlock, type ActivityGroupTarget } from '@/lib/session-presentation';
import { applyAlpha } from '@/md/color';
import { RowVeil, splitRunAtSpans } from '@/md/veil';

type OpenActivityGroup = (target: ActivityGroupTarget) => void;

const activitySheetOpener = createStore<OpenActivityGroup>(noop);

function noop() {}

/** The opener for the activity sheet. Outside a host (tests) it is a no-op,
 * so transcript rows never need to know whether a sheet is mounted. */
export function useActivitySheet(): OpenActivityGroup {
  return useStore(activitySheetOpener);
}

/** Whether the detail page would have anything to show for this activity —
 * the same test the page applies, so a tappable row never opens empty. */
function activityHasDetail(activity: ActivityItem): boolean {
  if (activity.reasoning) {return activity.reasoning.content.trim().length > 0;}
  return Boolean(
    activityDisclosureSections(activity).length
      || activity.file_changes?.length
      || activity.image_urls?.length,
  );
}

/**
 * A tool group opens in a native bottom sheet instead of unfolding in the
 * transcript: the first page lists the group's activities, and choosing one
 * slides across to its detail (command, output, diffs, thinking) with a back
 * row — the same two-page pattern as the model picker. One sheet serves the
 * whole transcript. A summary row hands it a locator, and the body
 * re-resolves that against the freshest session on every commit (each
 * commit deep-clones the session), so a running group keeps growing and a
 * running command's output keeps streaming while the sheet is up.
 */
export function ActivitySheetHost({
  session,
  children,
}: {
  session: AgentSession;
  children: ReactNode;
}) {
  const [target, setTarget] = useState<ActivityGroupTarget | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const open = useCallback<OpenActivityGroup>((next) => {
    setSelectedId(null);
    setTarget(next);
  }, []);

  useEffect(() => {
    activitySheetOpener.set(open);
    return () => activitySheetOpener.set(noop);
  }, [open]);

  const block = target ? findActivityBlock(session, target) : null;
  const activities = block ? activitiesForBlock(block) : [];
  const selected = selectedId
    ? activities.find((activity) => activity.id === selectedId) ?? null
    : null;

  // A rewind can drop the turn the open group belongs to; close rather than
  // leave an empty sheet up.
  useEffect(() => {
    if (target && !block) {setTarget(null);}
  }, [block, target]);

  return (
    <>
      {children}
      <XSheet
        open={Boolean(target && block)}
        detents={[0.5, 1]}
        onDismiss={() => {
          setTarget(null);
          setSelectedId(null);
        }}>
        <View style={styles.fill}>
          {block ? (
            selected ? (
              <>
                <Pressable
                  accessibilityHint="Shows the whole group"
                  accessibilityRole="button"
                  onPress={() => setSelectedId(null)}
                  style={styles.backRow}>
                  <AppSymbol
                    name={{ ios: 'chevron.left', android: 'arrow_back', web: 'arrow_back' }}
                    size={14}
                    tintColor={NativeTint as string}
                  />
                  <Text style={[styles.backLabel, { color: NativeTint as string }]}>Back</Text>
                </Pressable>
                <ActivityDetail activity={selected} />
              </>
            ) : (
              <GroupList
                activities={activities}
                onSelect={(id) => setSelectedId(id)}
              />
            )
          ) : null}
        </View>
      </XSheet>
    </>
  );
}

/** Page one: the group's activities as a picker-style list. The rows say
 * what happened; no headline restates it. */
function GroupList({
  activities,
  onSelect,
}: {
  activities: ActivityItem[];
  onSelect: (id: string) => void;
}) {
  const _insets = useSafeAreaInsets();
  return (
    <ScrollView
           style={styles.fill}>
      {activities.map((activity) => (
        <GroupRow activity={activity} onPress={() => onSelect(activity.id)} />
      ))}
    </ScrollView>
  );
}

/** One activity in the list: the desktop card's header — kind glyph, bold
 * action label, subject, edit totals, trailing state. */
function GroupRow({ activity, onPress }: { activity: ActivityItem; onPress: () => void }) {
  const theme = useTheme();
  const hasDetail = activityHasDetail(activity);
  const preview = activity.reasoning ? '' : activityPreview(activity);
  const detail = activityRowDetail(activity) || preview;
  const stats = activityFileChangeStats(activity);
  return (
    <Pressable
      accessibilityHint={hasDetail ? 'Opens the details' : undefined}
      accessibilityRole={hasDetail ? 'button' : 'text'}
      disabled={!hasDetail}
      onPress={onPress}
      style={styles.row}>
      <View style={[styles.rowIcon, { backgroundColor: theme.overlay }]}>
        <AppSymbol
          name={ACTIVITY_ICONS[activity.kind]}
          size={14}
          tintColor={theme.textSecondary}
        />
      </View>
      <RichText style={[styles.rowLabel, { color: theme.text }]}>
        <RichTextSpan style={[styles.rowAction, { color: activity.failed ? theme.danger : theme.text }]}>
          {activityActionLabel(activity)}
        </RichTextSpan>
        {detail ? <RichTextSpan text={`  ${detail}`} style={{ color: theme.textSecondary }} /> : null}
      </RichText>
      {stats && (
        <RichText style={styles.rowStats}>
          <RichTextSpan style={{ color: theme.success }}>+{stats.additions}</RichTextSpan>
          <RichTextSpan text=" " style={{ color: theme.textGhost }} />
          <RichTextSpan style={{ color: theme.danger }}>−{stats.deletions}</RichTextSpan>
        </RichText>
      )}
      <RowState activity={activity} hasDetail={hasDetail} />
    </Pressable>
  );
}

/** Mirrors the desktop row's trailing state: a disclosure chevron when there
 * is detail to open, nothing for finished reasoning, an alert for failures,
 * a dot while live. */
function RowState({ activity, hasDetail }: { activity: ActivityItem; hasDetail: boolean }) {
  const theme = useTheme();
  if (hasDetail) {
    return (
      <AppSymbol
        name={{ ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
        size={11}
        tintColor={theme.textGhost}
      />
    );
  }

  if (activity.reasoning) {return null;}
  if (activity.failed) {
    return (
      <AppSymbol
        name={{ ios: 'exclamationmark.triangle.fill', android: 'warning', web: 'warning' }}
        size={13}
        tintColor={theme.danger}
      />
    );
  }

  if (activity.complete) {return null;}
  return <View accessibilityLabel="Running" style={[styles.runningDot, { backgroundColor: NativeTint }]} />;
}

/** Page two: everything the desktop shows in an expanded card. */
function ActivityDetail({ activity }: { activity: ActivityItem }) {
  const theme = useTheme();
  const _insets = useSafeAreaInsets();
  const reasoning = activity.reasoning ? activity.reasoning.content.trim() : '';
  const sections = activity.reasoning ? [] : activityDisclosureSections(activity);
  const changes = activity.file_changes ?? [];
  const images = activity.image_urls ?? [];
  const title = activity.reasoning ? reasoningTitle(activity) : activityDisplayTitle(activity);
  return (
    <ScrollView
           style={styles.fill}>
      <View style={styles.header}>
        <View style={[styles.iconWell, { backgroundColor: theme.overlayStrong }]}>
          <AppSymbol
            name={ACTIVITY_ICONS[activity.kind]}
            size={17}
            tintColor={theme.textSecondary}
          />
        </View>
        <View style={styles.headerCopy}>
          <Text
            accessibilityRole="heading"
            numberOfLines={3}
            style={[styles.title, { color: theme.text }]}>
            {title}
          </Text>
          <ActivityStatus activity={activity} />
        </View>
      </View>
      {activity.reasoning ? (
        <ReasoningText content={plainReasoning(reasoning)} live={!activity.complete} />
      ) : (
        <>
          {sections.map((section) => (
            <DetailSection
              content={section.content}
              label={section.label}
              mono={section.kind !== 'detail'}
            />
          ))}
          {changes.map((change) => (
            <FileChange change={change} />
          ))}
          {images.map((url) => (
            <Image
              src={url}
              style={[styles.image, { backgroundColor: theme.inset }]}
              ios={{ stretch: 'aspectFit' }}
              web={{ style: { objectFit: 'contain' } }}
            />
          ))}
        </>
      )}
    </ScrollView>
  );
}

/** Failure, progress, and edit totals under the title — each paired with a
 * word or glyph, never carried by color alone. */
function ActivityStatus({ activity }: { activity: ActivityItem }) {
  const theme = useTheme();
  if (activity.failed) {
    return (
      <View style={styles.status}>
        <AppSymbol
          name={{ ios: 'exclamationmark.triangle.fill', android: 'warning', web: 'warning' }}
          size={12}
          tintColor={theme.danger}
        />
        <Text style={[styles.statusText, { color: theme.danger }]}>Failed</Text>
      </View>
    );
  }

  // A live thought already says "Thinking" in its title.
  if (!activity.complete && !activity.reasoning) {
    return (
      <View accessibilityLiveRegion="polite" style={styles.status}>
        <View style={[styles.runningDot, { backgroundColor: NativeTint }]} />
        <Text style={[styles.statusText, { color: theme.textTertiary }]}>In progress</Text>
      </View>
    );
  }

  const stats = activityFileChangeStats(activity);
  if (stats) {
    return (
      <RichText style={[styles.statusText, styles.stats]}>
        <RichTextSpan style={{ color: theme.success }}>+{stats.additions}</RichTextSpan>
        <RichTextSpan text=" " style={{ color: theme.textGhost }} />
        <RichTextSpan style={{ color: theme.danger }}>−{stats.deletions}</RichTextSpan>
      </RichText>
    );
  }

  return null;
}

function DetailSection({
  label,
  content,
  mono,
}: {
  label: string | null;
  content: string;
  mono: boolean;
}) {
  const theme = useTheme();
  return (
    <View style={styles.section}>
      {label ? (
        <Text style={[styles.sectionLabel, { color: theme.textTertiary }]}>{label}</Text>
      ) : null}
      {content ? (
        mono ? (
          <View style={[styles.well, { backgroundColor: theme.inset, borderColor: theme.border }]}>
            <Text style={[styles.monoText, { color: theme.textSecondary }]}>
              {boundedText(content)}
            </Text>
          </View>
        ) : (
          <Text style={[styles.bodyText, { color: theme.textSecondary }]}>
            {boundedText(content)}
          </Text>
        )
      ) : null}
    </View>
  );
}

function FileChange({ change }: { change: ActivityFileChange }) {
  const theme = useTheme();
  const status = change.status ?? 'modified';
  const statusColor = status === 'added'
    ? theme.success
    : status === 'deleted'
      ? theme.danger
      : theme.warning;

  const statusWord = status === 'added' ? 'Added' : status === 'deleted' ? 'Deleted' : 'Modified';
  const diff = change.diff?.trim() ? change.diff : null;
  return (
    <View style={styles.fileChange}>
      <View accessible accessibilityLabel={`${statusWord} ${change.path}`} style={styles.fileRow}>
        <View style={[styles.statusDot, { backgroundColor: statusColor }]} />
        <Text numberOfLines={1} style={[styles.filePath, { color: theme.text }]}>
          {change.path}
        </Text>
        {(change.additions != null || change.deletions != null) && (
          <RichText style={[styles.statusText, styles.stats]}>
            <RichTextSpan style={{ color: theme.success }}>+{change.additions ?? 0}</RichTextSpan>
            <RichTextSpan text=" " style={{ color: theme.textGhost }} />
            <RichTextSpan style={{ color: theme.danger }}>−{change.deletions ?? 0}</RichTextSpan>
          </RichText>
        )}
      </View>
      {diff ? <DiffView diff={diff} /> : null}
    </View>
  );
}

/** Streaming reasoning dissolves in like the desktop's strided reasoning
 * veil: appended text fades at half the message veil's tick rate, and text
 * present at mount is adopted at full opacity. */
function ReasoningText({ content, live }: { content: string; live: boolean }) {
  const theme = useTheme();
  const reducedMotion = useReducedMotion();
  const veil = useRef<RowVeil | null>(null);
  veil.current ??= new RowVeil(content.length > 0);
  const [, bump] = useReducer((count: number) => count + 1, 0);
  const spans = live && !reducedMotion ? veil.current.advance(0, content, Date.now()) : [];
  useEffect(() => {
    if (!spans.length) {return;}
    const timer = setTimeout(bump, 66);
    return () => clearTimeout(timer);
  });

  return (
    <RichText style={[styles.bodyText, { color: theme.textSecondary }]}>
      {splitRunAtSpans(0, content.length, spans).map(([start, end, opacity]) => (
        <RichTextSpan
          text={content.slice(start, end)}
          style={opacity >= 1 ? undefined : { color: applyAlpha(theme.textSecondary, opacity) }}
        />
      ))}
    </RichText>
  );
}

function boundedText(value: string): string {
  const limit = 12_000;
  return value.length <= limit ? value : `${value.slice(0, limit)}\n\n… Output truncated`;
}

/** Reasoning is throwaway thinking: render it as quiet plain text, never
 * heavier than the answer. Strips the markdown emphasis and heading markers
 * providers put on their summary headlines. */
function plainReasoning(value: string): string {
  return boundedText(value)
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/__([^_]+)__/g, '$1')
    .replace(/^#{1,6}\s+/gm, '');
}

const hairline = 0.5;

const styles = {
  fill: { flex: '1 1' },
  pagerClip: { flex: '1 1', overflow: 'hidden' },
  pagerTrack: { flex: '1 1', flexDirection: 'row', width: '200%' },
  page: { width: '50%' },
  // ── Page one: the list ──────────────────────────────────────────────────
  listContent: { paddingHorizontal: Spacing.two, paddingTop: 6 },
  row: {
    alignItems: 'center',
    borderRadius: Radius.medium,
    flexDirection: 'row',
    gap: 11,
    minHeight: 46,
    paddingHorizontal: 12,
    paddingVertical: 8,
  },
  rowIcon: {
    alignItems: 'center',
    borderRadius: Radius.tiny,
    height: 26,
    justifyContent: 'center',
    width: 26,
  },
  rowLabel: { flex: '1 1', fontSize: 16, minWidth: 0 },
  rowAction: { fontWeight: '600' },
  rowStats: { fontSize: 13, fontVariant: ['tabular-nums'], fontWeight: '600' },
  runningDot: { borderRadius: 3, height: 6, width: 6 },
  // ── Page two: the detail ────────────────────────────────────────────────
  backRow: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 5,
    marginHorizontal: Spacing.two,
    minHeight: 40,
    paddingHorizontal: 10,
  },
  backLabel: { fontSize: 16, fontWeight: '600' },
  content: { gap: 14, paddingHorizontal: Spacing.three, paddingTop: 6 },
  header: { alignItems: 'flex-start', flexDirection: 'row', gap: 12 },
  iconWell: {
    alignItems: 'center',
    borderRadius: Radius.small,
    height: 34,
    justifyContent: 'center',
    width: 34,
  },
  headerCopy: { flex: '1 1', gap: 3, minWidth: 0 },
  title: { fontSize: 18, fontWeight: '600', lineHeight: '22px' },
  status: { alignItems: 'center', flexDirection: 'row', gap: 6 },
  statusText: { fontSize: 14 },
  stats: { fontVariant: ['tabular-nums'], fontWeight: '600' },
  section: { gap: 6 },
  sectionLabel: {
    fontSize: 12.5,
    fontWeight: '600',
    letterSpacing: 0.4,
    textTransform: 'uppercase',
  },
  well: {
    borderRadius: Radius.small,
    borderWidth: hairline,
    paddingHorizontal: 10,
    paddingVertical: 8,
  },
  monoText: { fontFamily: MonoFont, fontSize: 13, lineHeight: '17.5px' },
  bodyText: { fontSize: 15.5, lineHeight: '21px' },
  fileChange: { gap: 8 },
  fileRow: { alignItems: 'center', flexDirection: 'row', gap: 8, minHeight: 24 },
  statusDot: { borderRadius: 3, height: 6, width: 6 },
  filePath: { flex: '1 1', fontFamily: MonoFont, fontSize: 13 },
  image: { borderRadius: Radius.small, height: 240, width: '100%' },
};