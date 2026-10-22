import type { CSSProperties } from 'octane/jsx-runtime';
import { useMemo } from 'octane';
import { ScrollableArea, Text, View } from '@octane-xplat/ui';

import { MonoFont } from '@/lib/theme-platform';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import { parseUnifiedDiff, type DiffLine } from '@/lib/diff-presentation';

/** Colored unified-diff body rendered into an inset code well. */
export function DiffView({
  diff,
  mode = 'well',
}: {
  diff: string;
  mode?: 'well' | 'review';
}) {
  const theme = useTheme();
  const { lines, truncated } = useMemo(
    () =>
      parseUnifiedDiff(
        diff,
        mode === 'review'
          ? { contextLines: 3, hidePositionedHunks: true }
          : undefined,
      ),
    [diff, mode],
  );

  if (!lines.length) {return null;}
  if (mode === 'review') {
    return (
      <View style={styles.reviewLines}>
        {lines.map((line) => (
          <ReviewDiffRow line={line} />
        ))}
        {truncated ? (
          <Text
            style={{
              ...styles.reviewTruncated,
              color: theme.textTertiary,
              borderColor: theme.border,
            }}
          >
            … diff truncated
          </Text>
        ) : null}
      </View>
    );
  }

  return (
    <View style={{ ...styles.well, backgroundColor: theme.inset, borderColor: theme.border }}>
      <ScrollableArea axis="inline">
        <View style={styles.lines}>
          {lines.map((line) => (
            <DiffRow line={line} />
          ))}
          {truncated && (
            <Text style={{ ...styles.truncated, color: theme.textTertiary }}>
              … diff truncated
            </Text>
          )}
        </View>
      </ScrollableArea>
    </View>
  );
}

function ReviewDiffRow({ line }: { line: DiffLine }) {
  const theme = useTheme();
  if (line.kind === 'gap' || line.kind === 'hunk') {
    const label =
      line.kind === 'gap'
        ? `${line.hiddenLines ?? 0} unmodified ${line.hiddenLines === 1 ? 'line' : 'lines'}`
        : line.text;

    return (
      <View style={{ ...styles.reviewRow, backgroundColor: theme.overlay }}>
        <View
          style={{
            ...styles.reviewGutter,
            borderColor: theme.border,
            backgroundColor: theme.overlay,
          }}
        />
        <Text
                  style={{ ...styles.reviewMeta, color: theme.textTertiary }}
        >
          {label}
        </Text>
      </View>
    );
  }

  const changed = line.kind === 'add' || line.kind === 'remove';
  const semanticColor = line.kind === 'add' ? theme.success : theme.danger;
  const background =
    line.kind === 'add'
      ? theme.successSoft
      : line.kind === 'remove'
        ? theme.dangerSoft
        : 'transparent';

  const shownLine = line.newLine ?? line.oldLine;
  return (
    <View
      style={{
        ...styles.reviewRow,
        backgroundColor: background,
        borderLeftColor: changed ? semanticColor : 'transparent',
      }}
    >
      <View
        style={{
          ...styles.reviewGutter,
          backgroundColor: changed ? background : 'transparent',
          borderColor: theme.border,
        }}
      >
        <Text
          style={{
            ...styles.reviewLineNumber,
            color: changed ? semanticColor : theme.textTertiary,
          }}
        >
          {shownLine ?? ''}
        </Text>
      </View>
      <Text
              style={{ ...styles.reviewCode, color: theme.textSecondary }}
      >
        {line.text || ' '}
      </Text>
    </View>
  );
}

function DiffRow({ line }: { line: DiffLine }) {
  const theme = useTheme();
  if (line.kind === 'hunk') {
    return (
      <Text style={{ ...styles.line, ...styles.hunk, color: theme.textTertiary }}>{line.text}</Text>
    );
  }

  const background = line.kind === 'add'
    ? theme.successSoft
    : line.kind === 'remove'
      ? theme.dangerSoft
      : 'transparent';

  const color = line.kind === 'add'
    ? theme.success
    : line.kind === 'remove'
      ? theme.danger
      : theme.textSecondary;

  const marker = line.kind === 'add' ? '+' : line.kind === 'remove' ? '−' : ' ';
  return (
    <View style={{ ...styles.row, backgroundColor: background }}>
      <Text style={{ ...styles.marker, color }}>{marker}</Text>
      <Text style={{ ...styles.line, color }}>{line.text || ' '}</Text>
    </View>
  );
}

const hairline = 0.5;

const styles = {
  well: {
    borderRadius: Radius.small,
    borderWidth: hairline,
    overflow: 'hidden',
    paddingTop: 6, paddingBottom: 6,
  },
  lines: { minWidth: '100%', paddingLeft: 8, paddingRight: 8 },
  row: { borderRadius: 3, flexDirection: 'row', paddingRight: 10 },
  marker: { fontFamily: MonoFont, fontSize: 12, lineHeight: '16.5px', width: 12 },
  line: { fontFamily: MonoFont, fontSize: 12, lineHeight: '16.5px' },
  hunk: { fontFamily: MonoFont, fontSize: 11.5, lineHeight: '18px', marginTop: 2, marginBottom: 2 },
  truncated: { fontSize: 11.5, marginTop: 4 },
  reviewLines: { minWidth: '100%', overflow: 'hidden' },
  reviewRow: {
    alignItems: 'stretch',
    borderLeftWidth: 2,
    flexDirection: 'row',
    minHeight: 20,
  },
  reviewGutter: {
    alignItems: 'flex-end',
    borderRightWidth: hairline,
    justifyContent: 'flex-start',
    paddingRight: 8,
    paddingTop: 2,
    width: 42,
  },
  reviewLineNumber: {
    fontFamily: MonoFont,
    fontSize: 11.5,
    lineHeight: '17px',
  },
  reviewCode: {
    flex: '1 1',
    fontFamily: MonoFont,
    fontSize: 12,
    lineHeight: '17px',
    paddingLeft: 10, paddingRight: 10,
    paddingTop: 2, paddingBottom: 2,
  },
  reviewMeta: {
    flex: '1 1',
    fontFamily: MonoFont,
    fontSize: 11.5,
    lineHeight: '17px',
    paddingLeft: 10, paddingRight: 10,
    paddingTop: 2, paddingBottom: 2,
  },
  reviewTruncated: {
    borderTopWidth: hairline,
    fontSize: 11.5,
    paddingLeft: 12, paddingRight: 12,
    paddingTop: 8, paddingBottom: 8,
  },
} satisfies Record<string, CSSProperties>;