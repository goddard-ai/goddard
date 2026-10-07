/** @jsxImportSource @nativescript-community/octane */
import type { BossDeliverable } from '@waku/client';

import { useMemo, useState } from 'octane';
import {
  Pressable,
  ScrollView,
  SegmentedControl,
  Text,
  View,
} from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import {
  DeliverableRow,
  useDeliverableActions,
  useOpenDeliverable,
} from '@/components/deliverables';

import {
  navigateBack,
  ScreenHeaderBackdrop,
} from '@/components/screen-header';

import { useDaemon } from '@/lib/daemon-hooks';
import { useBossState } from '@/lib/daemon-queries';
import {
  DELIVERABLE_FILTERS,
  filterDeliverables,
  type DeliverablesFilter,
} from '@/lib/deliverables';

import { useTheme } from '@/lib/use-theme';

const FILTER_LABELS: Record<DeliverablesFilter, string> = {
  all: 'All',
  pinned: 'Pinned',
  dormant: 'Dormant',
  archived: 'Archived',
};

const FILTER_EMPTY: Record<DeliverablesFilter, { title: string; detail: string }> = {
  all: { title: 'No deliverables', detail: 'Files an employee publishes to your sidebar appear here.' },
  pinned: { title: 'Nothing pinned', detail: 'Pin a deliverable to keep it at the top of the sidebar.' },
  dormant: { title: 'Nothing dormant', detail: 'Swept deliverables wait here until you restore them.' },
  archived: { title: 'Nothing archived', detail: 'Archived deliverables leave the sidebar but keep their record.' },
};

/** The deliverables library — every published record, not just the
 * sidebar's 12-hour recents, behind All/Pinned/Dormant/Archived. This is
 * the mobile form of the desktop's Brain → Deliverables tab. */
export default function DeliverablesScreen() {
  const theme = useTheme();
  const daemon = useDaemon();
  const [filter, setFilter] = useState<DeliverablesFilter>('all');
  const [now] = useState(() => Date.now());

  const bossState = useBossState();
  const state = bossState.data ?? null;
  const openDeliverable = useOpenDeliverable(state);

  const deliverables = useMemo(
    () => (state ? filterDeliverables(state.deliverables, filter) : []),
    [state, filter],
  );

  const host = daemon.activeProfile?.name ?? null;

  return (
    <View style={{ flex: '1 1', backgroundColor: theme.background }}>
      <ScreenHeaderBackdrop />
      <View
        style={{
          alignItems: 'center',
          flexDirection: 'row',
          gap: 6,
          height: 44,
          paddingHorizontal: 6,
        }}
      >
        <Pressable
          accessibilityLabel="Back"
          accessibilityRole="button"
          onPress={navigateBack}
          style={{ alignItems: 'center', flexDirection: 'row', minHeight: 36, paddingHorizontal: 8 }}
        >
          <AppSymbol
            name={{ ios: 'chevron.left', android: 'arrow_back', web: 'arrow_back' }}
            size={17}
            tintColor={theme.text}
          />
        </Pressable>
        <View style={{ flex: '1 1', minWidth: 0 }}>
          <Text numberOfLines={1} style={{ color: theme.text, fontSize: 16, fontWeight: '600' }}>
            Deliverables
          </Text>
          <Text numberOfLines={1} style={{ color: theme.textTertiary, fontSize: 11.5 }}>
            {[state?.identity.name, host].filter(Boolean).join(' · ')}
          </Text>
        </View>
      </View>
      {daemon.phase !== 'connected' ? (
        <LibraryMessage
          detail="Connect to a daemon to read its deliverables."
          title="Not connected"
        />
      ) : state == null ? (
        <LibraryMessage
          detail="This daemon's boss experiment is off — employees have no deliverables to publish."
          title="Boss is disabled"
        />
      ) : (
        <View style={{ flex: '1 1' }}>
          <View style={{ paddingHorizontal: 14, paddingVertical: 8 }}>
            <SegmentedControl
              accessibilityLabel="Filter deliverables"
              onValueChange={(value) => setFilter(value as DeliverablesFilter)}
              options={DELIVERABLE_FILTERS.map((value) => ({
                label: FILTER_LABELS[value],
                value,
              }))}
              value={filter}
            />
          </View>
          <ScrollView style={{ flex: '1 1' }}>
            {deliverables.length === 0 ? (
              <LibraryMessage
                detail={FILTER_EMPTY[filter].detail}
                title={FILTER_EMPTY[filter].title}
              />
            ) : null}
            {deliverables.map((deliverable) => (
              <LibraryRow
                deliverable={deliverable}
                host={host}
                now={now}
                onOpen={openDeliverable}
              />
            ))}
          </ScrollView>
        </View>
      )}
    </View>
  );
}

function LibraryRow({
  deliverable,
  host,
  now,
  onOpen,
}: {
  deliverable: BossDeliverable;
  host: string | null;
  now: number;
  onOpen: (deliverable: BossDeliverable) => void;
}) {
  const { menuItems } = useDeliverableActions(deliverable);
  const pinned = deliverable.pinnedAt != null;
  const statusTag = deliverable.archivedAt != null
    ? 'Archived'
    : deliverable.dormantAt != null && !pinned
      ? 'Dormant'
      : null;

  return (
    <DeliverableRow
      deliverable={deliverable}
      host={host}
      menuItems={menuItems}
      now={now}
      statusTag={statusTag}
      onPress={() => onOpen(deliverable)}
    />
  );
}

function LibraryMessage({ title, detail }: { title: string; detail: string }) {
  const theme = useTheme();
  return (
    <View
      style={{
        alignItems: 'center',
        flex: '1 1',
        gap: 6,
        justifyContent: 'center',
        paddingHorizontal: 32,
        paddingVertical: 48,
      }}
    >
      <Text style={{ color: theme.text, fontSize: 15, fontWeight: '600', textAlign: 'center' }}>
        {title}
      </Text>
      <Text style={{ color: theme.textTertiary, fontSize: 13, textAlign: 'center' }}>
        {detail}
      </Text>
    </View>
  );
}
