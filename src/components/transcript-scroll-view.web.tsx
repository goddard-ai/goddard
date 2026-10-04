import { Pressable, ScrollView, Text, View } from '@octane-xplat/ui';

import { useTheme } from '@/lib/use-theme';
import type { TranscriptScrollViewProps } from './transcript-scroll-view.types';

/**
 * Web transcript scroller — the pre-leaf behavior: rows in natural order
 * inside the shared ScrollView, a manual "Earlier messages" control in
 * place of scroll-triggered window extension, and no scroll engine (no
 * follow/jump/reveal — the handle is never handed out).
 */
export function TranscriptScrollView({
  rows,
  renderRow,
  strip,
  hasEarlier,
  onExtendTail,
}: TranscriptScrollViewProps) {
  const theme = useTheme();
  return (
    <ScrollView style={{ flex: '1 1' }}>
      <View style={{ paddingHorizontal: 16, paddingBottom: 12 }}>
        {hasEarlier && (
          <Pressable
            accessibilityLabel="Load earlier messages"
            accessibilityRole="button"
            onPress={onExtendTail}
            style={{ alignItems: 'center', paddingVertical: 10 }}>
            <Text style={{ fontSize: 13, fontWeight: '600', color: theme.textTertiary }}>
              Earlier messages…
            </Text>
          </Pressable>
        )}
        {rows.map((row) => renderRow(row, { keepRowTop: applyNow }))}
        {strip}
      </View>
    </ScrollView>
  );
}

function applyNow(apply: () => void) {
  apply();
}
