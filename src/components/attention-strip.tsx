import type { AgentSession, BossState } from '@waku/client';

import { useCallback, useMemo, useState } from 'octane';
import { Pressable, Text, View } from '@octane-xplat/ui';
import { pushRoute, useRoute } from '@octane-xplat/ui/routing';

import { AppSymbol } from '@/components/app-symbol';
import { useOpenDeliverable } from '@/components/deliverables';
import {
  ATTENTION_SWEEP_TIMEOUT_SECS,
  collectAttentionItems,
  nextAttentionItem,
  stripItems,
  type AttentionItem,
  type AttentionItemKind,
} from '@/lib/attention';

import { useBossState, useTaskState } from '@/lib/daemon-queries';
import { Haptics } from '@/lib/haptics';
import { Radius } from '@/lib/theme-colors';
import { useUnseenReplyWatermarks } from '@/lib/use-unseen-replies';
import { useTheme } from '@/lib/use-theme';

const EMPTY_SESSIONS: AgentSession[] = [];

/** The full "needs you" inventory for the active daemon — boss pull-set
 * items plus per-session waits and unseen replies, ordered decision-first. */
export function useAttentionItems(): AttentionItem[] {
  const state = useBossState().data ?? null;
  const sessions = useTaskState().data?.sessions ?? EMPTY_SESSIONS;
  const watermarks = useUnseenReplyWatermarks();
  return useMemo(
    () => collectAttentionItems({ state, sessions, watermarks }),
    [sessions, state, watermarks],
  );
}

/** The ⌘D sweep's mobile form: the visited set keeps taps advancing past
 * items whose marker only clears elsewhere (a blocker survives opening the
 * employee's session); past the timeout the next press sweeps fresh. */
let sweepVisited = new Set<string>();
let sweepAt = 0;

export function useAttentionNavigation(): {
  /** What a press would land on — the desktop bell's preview. */
  next: AttentionItem | null;
  /** Targets left in the current sweep. */
  remaining: number;
  jump: () => void;
} {
  const bossState = useBossState().data ?? null;
  const items = useAttentionItems();
  const route = useRoute('root');
  const currentSessionId = route?.name === 'session/:id'
    ? (route.params as { id?: string }).id ?? null
    : null;

  const openDeliverable = useOpenDeliverable(bossState);
  // A jump mutates the sweep set without touching the memo inputs — the tick
  // is what makes the preview follow it.
  const [sweepTick, setSweepTick] = useState(0);
  const preview = useMemo(
    () => nextAttentionItem(items, sweepVisited, currentSessionId),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- sweepVisited mutates on jump
    [items, currentSessionId, sweepTick],
  );

  const remaining = useMemo(
    () => items.filter(
      (item) =>
        !sweepVisited.has(item.key)
        && !(item.target.type === 'session' && item.target.sessionId === currentSessionId),
    ).length,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [items, currentSessionId, sweepTick],
  );

  const jump = useCallback(() => {
    if (Date.now() - sweepAt > ATTENTION_SWEEP_TIMEOUT_SECS * 1_000) {
      sweepVisited = new Set();
    }

    let target = nextAttentionItem(items, sweepVisited, currentSessionId);
    if (!target) {
      // Chain drained — sweep fresh so the gesture never dead-ends.
      sweepVisited = new Set();
      target = nextAttentionItem(items, sweepVisited, currentSessionId);
    }

    if (!target) {return;}
    sweepVisited.add(target.key);
    sweepAt = Date.now();
    setSweepTick((tick) => tick + 1);
    void Haptics.selectionAsync();

    const destination = target.target;
    if (destination.type === 'deliverable') {
      const record = bossState?.deliverables.find(
        (deliverable) => deliverable.id === destination.deliverableId,
      );

      if (record) {
        openDeliverable(record);
        return;
      }

      pushRoute({ stack: 'root', name: 'deliverables', params: {} });
      return;
    }

    if (destination.sessionId !== currentSessionId) {
      pushRoute({ stack: 'root', name: 'session/:id', params: { id: destination.sessionId } });
    }
  }, [bossState, currentSessionId, items, openDeliverable]);

  return { next: preview, remaining, jump };
}

/** The header's next-attention button — renders only while something
 * needs you, so a quiet app carries no idle chrome. The tap target stays
 * ≥36pt like the sibling header buttons. */
export function AttentionJumpButton() {
  const theme = useTheme();
  const { next, remaining, jump } = useAttentionNavigation();
  if (!next) {return null;}
  return (
    <Pressable
      accessibilityHint="Jumps to the next thing needing attention"
      accessibilityLabel={`Next: ${next.title}`}
      accessibilityRole="button"
      onPress={jump}
      style={{
        alignItems: 'center',
        flexDirection: 'row',
        gap: 3,
        justifyContent: 'center',
        minHeight: 36,
        minWidth: 36,
      }}
    >
      <AppSymbol
        name={{ ios: 'bell.fill', android: 'notifications', web: 'bell' }}
        size={17}
        tintColor={theme.accent}
      />
      {remaining > 1 ? (
        <Text style={{ color: theme.accent, fontSize: 12, fontVariant: 'tabular-nums', fontWeight: '700' }}>
          {remaining}
        </Text>
      ) : null}
    </Pressable>
  );
}

const KIND_ICON: Record<AttentionItemKind, { ios: string; android: string; web: string }> = {
  blocker: { ios: 'exclamationmark.triangle', android: 'warning', web: 'warning' },
  bossQuestion: { ios: 'questionmark.circle', android: 'help', web: 'help' },
  planApproval: { ios: 'checkmark.seal', android: 'verified', web: 'verified' },
  employeeWaiting: { ios: 'questionmark.circle', android: 'help', web: 'help' },
  waiting: { ios: 'questionmark.circle', android: 'help', web: 'help' },
  deliverable: { ios: 'doc', android: 'draft', web: 'doc.text' },
  goalFinished: { ios: 'flag.checkered', android: 'flag', web: 'flag' },
  planFinished: { ios: 'checkmark.seal', android: 'verified', web: 'verified' },
  unseenReply: { ios: 'bubble.left', android: 'chat_bubble', web: 'chat_bubble' },
};

/** The boss chat's attention strip — the aggregate "needs you" view: cards
 * for blockers, parked asks, unread deliverables, and goal/plan finishes.
 * Decision rows read warning; arrivals read info. Everything is a tap
 * target into its session or record — these are cards with an action, not
 * a second transcript. The boss's own pending ask already pins above the
 * composer, so it never doubles here. */
export function BossAttentionStrip({
  state,
  sessions,
  onOpen,
}: {
  state: BossState;
  sessions: AgentSession[];
  onOpen: (sessionId: string) => void;
}) {
  const theme = useTheme();
  const watermarks = useUnseenReplyWatermarks();
  const openDeliverable = useOpenDeliverable(state);
  const items = useMemo(
    () => stripItems(collectAttentionItems({ state, sessions, watermarks })),
    [sessions, state, watermarks],
  );

  if (!items.length) {return null;}

  const press = (item: AttentionItem) => {
    const destination = item.target;
    if (destination.type === 'deliverable') {
      const record = state.deliverables.find(
        (deliverable) => deliverable.id === destination.deliverableId,
      );

      if (record) {openDeliverable(record);}
      return;
    }

    onOpen(destination.sessionId);
  };

  return (
    <View
      style={{
        backgroundColor: theme.surface,
        borderColor: theme.separator,
        borderRadius: Radius.large,
        borderWidth: 1,
        gap: 2,
        paddingTop: 6, paddingBottom: 6,
      }}
    >
      {items.map((item) => (
        <AttentionItemRow item={item} onPress={() => press(item)} />
      ))}
    </View>
  );
}

function AttentionItemRow({ item, onPress }: { item: AttentionItem; onPress: () => void }) {
  const theme = useTheme();
  const decision =
    item.kind === 'blocker'
    || item.kind === 'bossQuestion'
    || item.kind === 'planApproval'
    || item.kind === 'employeeWaiting'
    || item.kind === 'waiting';

  const tint = decision
    ? theme.warning
    : item.kind === 'deliverable' ? theme.info : theme.textSecondary;

  return (
    <Pressable
      accessibilityHint="Opens where this needs you"
      accessibilityLabel={`${item.title}${item.detail ? `, ${item.detail}` : ''}`}
      accessibilityRole="button"
      className="vx-pressable"
      onPress={onPress}
      style={{
        alignItems: 'center',
        flexDirection: 'row',
        gap: 8,
        minHeight: 34,
        paddingLeft: 12,
        paddingRight: 12,
      }}
    >
      <AppSymbol name={KIND_ICON[item.kind]} size={14} tintColor={tint} />
      <View style={{ flex: '1 1', minWidth: 0 }}>
        <Text numberOfLines={1} style={{ color: theme.text, fontSize: 13.5, fontWeight: '600' }}>
          {item.title}
        </Text>
        {item.detail ? (
          <Text numberOfLines={1} style={{ color: theme.textSecondary, fontSize: 12.5 }}>
            {item.detail}
          </Text>
        ) : null}
      </View>
      <AppSymbol
        name={{ ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
        size={12}
        tintColor={theme.textTertiary}
      />
    </Pressable>
  );
}
