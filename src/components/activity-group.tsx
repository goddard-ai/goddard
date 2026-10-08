import type { TranscriptBlock } from '@waku/client';
import {
  bossSummonCard,
  bossSummonSessionId,
  type BossSummonCard,
} from '@waku/client/boss-chat';

import { activitiesForBlock } from '@waku/client/event-reducer';
import { activityHeaderTitle, atomVisibleText } from '@waku/client/transcript-presentation';
import { memo, useEffect } from 'octane';
import { Pressable, Text, pushRoute, View } from '@octane-xplat/ui';

import { useActivitySheet } from './activity-sheet';
import { AppSymbol } from './app-symbol';
import { BossAvatar } from './boss-roster';
import {
  bossEmployeeForSession,
  bossEmployeeStatusLabel,
  bossEmployeeSymbol,
  bossQueueDetail,
  bossQueueRanks,
  commandIconSymbol,
  jobTitleSymbol,
} from '@/lib/boss';

import { useDaemon } from '@/lib/daemon-hooks';
import { useBossState, useSession } from '@/lib/daemon-queries';
import { useRuntime } from '@/lib/use-runtime';
import { sessionBusy } from '@/lib/mobile-runtime';
import { useTheme } from '@/lib/use-theme';
import { Radius } from '@/lib/theme-colors';
import { displaySessionTitle } from '@/lib/session-presentation';

/**
 * The transcript's view of a tool group is the desktop's collapsed summary
 * line — the live activity's title while streaming, "Ran N commands · …"
 * once settled — and nothing else. Tapping it opens the group in the native
 * activity sheet (`ActivitySheetHost`) rather than unfolding cards in
 * place: a phone transcript stays a readable feed, and the inverted,
 * anchored list never has to absorb an in-place layout change.
 *
 * `boss_summon` markers are the exception that proves the rule: the daemon
 * writes one into the boss transcript when a summon completes, and the
 * desktop lifts it out of the fold entirely — a block of only summons is
 * just the cards, and a mixed block keeps its summary line with the cards
 * stacked below. Successful dispatch is silent; the card is the record.
 */
export const ActivityGroup = memo(function ActivityGroup({
  block,
  blockIndex,
  live,
}: {
  block: TranscriptBlock;
  blockIndex: number;
  live: boolean;
}) {
  const theme = useTheme();
  const openSheet = useActivitySheet();
  const activities = activitiesForBlock(block);
  const summons = activities
    .map((activity) => bossSummonCard(activity))
    .filter((card): card is BossSummonCard => card != null);

  const plainCount = activities.length - summons.length;
  if (!activities.length) {return null;}
  return (
    <View style={{ gap: 6 }}>
      {plainCount ? (
        <Pressable
          accessibilityHint="Opens the activity list"
          accessibilityRole="button"
          onPress={() => openSheet({
            blockIndex,
            turnId: block.turn_id,
            afterMessage: block.after_message,
          })}
          style={styles.header}>
          <Text numberOfLines={1} style={[styles.title, { color: theme.textSecondary }]}>
            {activityHeaderTitle(
              activities.filter((activity) => bossSummonSessionId(activity) == null),
              live,
            )}
          </Text>
          <AppSymbol
            name={{ ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
            size={10}
            tintColor={theme.textGhost}
          />
        </Pressable>
      ) : null}
      {summons.map((card) => (
        <SummonCard card={card} />
      ))}
    </View>
  );
});

/** The live card a `boss_summon` marker becomes: avatar, name, job title,
 * current status, and the employee's freshest assistant line clamped to two
 * lines. The whole card opens the employee's chat. Identity reads the roster
 * while the employee is on it — tracking renames — and falls back to the
 * marker's frozen summon-time payload once the record retires. */
function SummonCard({ card }: { card: BossSummonCard }) {
  const theme = useTheme();
  const daemon = useDaemon();
  const runtime = useRuntime();
  const boss = useBossState().data;
  const session = useSession(card.sessionId).data;

  // The commentary line is only as live as the session behind it — attach
  // the same way the session screen does so a working employee's card keeps
  // repainting while it streams.
  useEffect(() => {
    if (!session || daemon.phase !== 'connected') {return;}
    void runtime.attachSession(session).catch(() => {});
  }, [daemon.phase, runtime.attachSession, session?.id]);

  const employee = bossEmployeeForSession(boss ?? undefined, card.sessionId);

  const name = employee?.identity.name
    || card.name
    || (session ? displaySessionTitle(session) : '')
    || 'Employee';

  const identity = employee?.identity ?? {
    id: card.sessionId,
    name,
    avatarSeed: card.avatarSeed || card.sessionId,
  };

  const jobTitle = employee?.jobTitle ?? card.jobTitle;

  const queueDetail = employee && boss
    ? bossQueueDetail(employee, bossQueueRanks(boss).get(employee.sessionId))
    : null;

  const status = employee
    ? bossEmployeeStatusLabel(employee, session ?? undefined, queueDetail)
    : {
        label: session
          ? sessionBusy(session)
            ? 'Working'
            : session.status === 'waiting'
              ? 'Waiting'
              : session.status === 'failed'
                ? 'Failed'
                : 'Finished'
          : 'Finished',
        tone: 'secondary' as const,
      };

  const symbol = boss && employee
    ? bossEmployeeSymbol(boss, employee)
    : card.icon
      ? commandIconSymbol(card.icon)
      : jobTitleSymbol(jobTitle);

  // Freshest assistant message, tail-first: while the employee streams its
  // newest message lands here chunk by chunk. Flattened to one paragraph.
  const commentary = (() => {
    for (let index = (session?.messages.length ?? 0) - 1; index >= 0; index -= 1) {
      const message = session!.messages[index]!;
      if (message.role !== 'assistant' || message.hidden) {continue;}
      const text = atomVisibleText(message.display_content ?? message.content);
      const paragraph = text.split(/\n\s*\n/u).find((part) => part.trim());
      if (paragraph) {return paragraph.trim().replace(/\s+/gu, ' ');}
    }

    return '';
  })();

  return (
    <Pressable
      accessibilityHint="Opens the employee's chat"
      accessibilityLabel={`${name}${jobTitle ? `, ${jobTitle}` : ''}, ${status.label}`}
      accessibilityRole="button"
      onPress={() => pushRoute({ stack: 'root', name: 'session/:id', params: { id: card.sessionId } })}
      style={[styles.card, { backgroundColor: theme.surface, borderColor: theme.border }]}>
      <View style={styles.cardHeader}>
        <BossAvatar identity={identity} size={28} />
        <View style={styles.cardNames}>
          <Text numberOfLines={1} style={[styles.cardName, { color: theme.text }]}>
            {name}
          </Text>
          {jobTitle ? (
            <View style={styles.cardJob}>
              <AppSymbol name={{ ios: symbol, android: 'work', web: 'work' }} size={10} tintColor={theme.textTertiary} />
              <Text numberOfLines={1} style={[styles.cardJobText, { color: theme.textTertiary }]}>
                {jobTitle}
              </Text>
            </View>
          ) : null}
        </View>
        <Text style={[styles.cardStatus, { color: theme.textSecondary }]}>{status.label}</Text>
      </View>
      <Text numberOfLines={2} style={[styles.cardCommentary, { color: theme.textTertiary }]}>
        {commentary || '…'}
      </Text>
    </Pressable>
  );
}

const styles = {
  header: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 6,
    minHeight: 28,
  },
  title: { flexShrink: 1, fontSize: 14, fontWeight: '500' },
  card: {
    borderRadius: Radius.medium,
    borderWidth: 0.5,
    gap: 6,
    paddingHorizontal: 10,
    paddingVertical: 8,
  },
  cardHeader: { alignItems: 'center', flexDirection: 'row', gap: 8 },
  cardNames: { flex: '1 1', minWidth: 0 },
  cardName: { fontSize: 13, fontWeight: '600', lineHeight: '16px' },
  cardJob: { alignItems: 'center', flexDirection: 'row', gap: 4, marginTop: 1 },
  cardJobText: { flexShrink: 1, fontSize: 12, lineHeight: '15px' },
  cardStatus: { fontSize: 12, lineHeight: '15px' },
  cardCommentary: { fontSize: 12.5, lineHeight: '15px', paddingLeft: 36 },
};
