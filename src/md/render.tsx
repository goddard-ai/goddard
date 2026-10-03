/**
 * mdast → xplat elements, with the streaming veil applied as paint-only
 * color fades.
 *
 * Mirrors the desktop renderer's contract (`src/md/render.rs`): every
 * text-bearing flow element (paragraph, heading, code block, table cell, …)
 * gets a stable ordinal in document order and its *visible* text is the veil
 * key. Veil spans split leaf runs and multiply foreground/background alpha —
 * they can never change shaping, wrapping, or heights.
 */

import type {
  BlockContent,
  DefinitionContent,
  ListItem,
  PhrasingContent,
  RootContent,
} from 'mdast';

import type { ReactNode } from 'octane';
import {
  Image,
  Pressable,
  pushRoute,
  RichText,
  RichTextSpan,
  ScrollView,
  View,
} from '@octane-xplat/ui';

import { openUrl } from '@/lib/open-url';
import { applyAlpha } from './color';
import { PENDING_LINK_URL } from './mend';
import { splitRunAtSpans, type RowVeil, type VeilSpan } from './veil';

type Style = Record<string, unknown>;

export interface MarkdownStyles {
  body: Style & { color: string };
  paragraph: Style;
  /** h1–h6. Each carries its own color. */
  heading: readonly (Style & { color: string })[];
  strong: Style;
  em: Style;
  strikethrough: Style & { color: string };
  link: Style & { color: string };
  codespan: Style & { color: string; backgroundColor: string };
  codeBlock: Style;
  codeHeader: Style;
  codeHeaderText: Style & { color: string };
  codeContent: Style;
  codeLine: Style & { color: string };
  blockquote: Style;
  list: Style;
  listItem: Style;
  listMarker: Style & { color: string };
  listContent: Style;
  table: Style;
  tableRow: Style;
  tableHeadRow: Style;
  tableCell: Style;
  tableCellText: Style & { color: string };
  tableHeadText: Style & { color: string };
  hr: Style;
  image: Style;
}

export interface RenderContext {
  styles: MarkdownStyles;
  /** `null` renders at full opacity with no veil bookkeeping. */
  veil: RowVeil | null;
  now: number;
  /** Next text-element ordinal; advances in document order. */
  ordinal: { value: number };
  /** True once any element of the current block produced active spans. */
  hadSpans: boolean;
  onOpenLink: (url: string) => void;
}

interface InlineContext {
  block: RenderContext;
  spans: readonly VeilSpan[];
  cursor: { value: number };
  /** Effective foreground color, for fading pieces that inherit it. */
  ink: string;
}

const TASK_LINK_PREFIX = 'goddard://task/';
const TASK_LINK_ID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function openLinkExternally(url: string) {
  // `[title](goddard://task/<id>)` — the same task-link format the desktop
  // renders — navigates to the task instead of opening a browser.
  const taskId = url.startsWith(TASK_LINK_PREFIX)
    ? url.slice(TASK_LINK_PREFIX.length).replace(/\/$/, '')
    : null;

  if (taskId !== null && TASK_LINK_ID.test(taskId)) {
    pushRoute({ stack: 'root', name: 'session/:id', params: { id: taskId } });
    return;
  }

  if (taskId !== null) {return;}
  try {
    openUrl(url);
  } catch {
    // Unopenable link — ignore.
  }
}

/** Visible text of an inline tree. Must mirror `renderInline`'s traversal
 * exactly — the veil's span offsets are positions in this string. */
export function flattenInline(nodes: readonly PhrasingContent[]): string {
  let flat = '';
  for (const node of nodes) {
    flat += inlineText(node);
  }

  return flat;
}

function inlineText(node: PhrasingContent): string {
  switch (node.type) {
    case 'text':
    case 'html':
    case 'inlineCode':
      return node.value;
    case 'break':
      return '\n';
    case 'image':
    case 'imageReference':
    case 'footnoteReference':
      return '';
    default:
      return 'children' in node ? flattenInline(node.children) : '';
  }
}

/** Begin one veil-keyed text element: assign its ordinal and fetch spans. */
function beginTextElement(ctx: RenderContext, flat: string): readonly VeilSpan[] {
  const ordinal = ctx.ordinal.value;
  ctx.ordinal.value += 1;
  if (!ctx.veil) {return [];}
  const spans = ctx.veil.advance(ordinal, flat, ctx.now);
  if (spans.length) {ctx.hadSpans = true;}
  return spans;
}

function inlineContext(
  ctx: RenderContext,
  flat: string,
  ink: string,
): InlineContext {
  return { block: ctx, spans: beginTextElement(ctx, flat), cursor: { value: 0 }, ink };
}

/**
 * Emit one leaf run, split at veil boundaries. Every piece is a
 * RichTextSpan — the native FormattedString host needs runs, not raw text
 * nodes.
 */
function renderLeaf(
  value: string,
  ictx: InlineContext,
  style?: Style & { color?: string; backgroundColor?: string },
): ReactNode[] {
  const start = ictx.cursor.value;
  ictx.cursor.value += value.length;
  if (!value) {return [];}
  const pieces = splitRunAtSpans(start, value.length, ictx.spans);
  return pieces.map(([pieceStart, pieceEnd, opacity]) => {
    const slice = value.slice(pieceStart - start, pieceEnd - start);
    const faded: Style = {};
    if (opacity < 1) {
      faded.color = applyAlpha(style?.color ?? ictx.ink, opacity);
      if (style?.backgroundColor) {
        faded.backgroundColor = applyAlpha(style.backgroundColor, opacity);
      }
    }

    return (
      <RichTextSpan text={slice} style={[style, faded]} />
    );
  });
}

function renderInline(
  nodes: readonly PhrasingContent[],
  ictx: InlineContext,
): ReactNode[] {
  const { styles } = ictx.block;
  return nodes.flatMap((node) => {
    switch (node.type) {
      case 'text':
      case 'html':
        return renderLeaf(node.value, ictx);
      case 'inlineCode':
        return renderLeaf(node.value, { ...ictx, ink: styles.codespan.color }, styles.codespan);
      case 'break':
        return renderLeaf('\n', ictx);
      case 'strong':
        return (
          <RichTextSpan style={styles.strong}>
            {renderInline(node.children, ictx)}
          </RichTextSpan>
        );
      case 'emphasis':
        return (
          <RichTextSpan style={styles.em}>
            {renderInline(node.children, ictx)}
          </RichTextSpan>
        );
      case 'delete':
        return (
          <RichTextSpan style={styles.strikethrough}>
            {renderInline(node.children, { ...ictx, ink: styles.strikethrough.color })}
          </RichTextSpan>
        );
      case 'link': {
        // A link whose URL is still streaming is styled but inert (see mend).
        const pending = node.url === PENDING_LINK_URL;
        return (
          <RichTextSpan
            onPress={pending ? undefined : () => ictx.block.onOpenLink(node.url)}
            style={styles.link}
          >
            {renderInline(node.children, { ...ictx, ink: styles.link.color })}
          </RichTextSpan>
        );
      }
      case 'linkReference':
        // Unresolved references render as link-styled text without a target.
        return (
          <RichTextSpan style={styles.link}>
            {renderInline(node.children, { ...ictx, ink: styles.link.color })}
          </RichTextSpan>
        );
      case 'image':
      case 'imageReference':
      case 'footnoteReference':
        return [];
      default:
        return 'children' in node
          ? renderInline((node as { children: PhrasingContent[] }).children, ictx)
          : [];
    }
  });
}

/** Split paragraph children at images so images render as blocks between
 * text runs — a formatted string cannot lay out images inline. */
function paragraphSegments(children: readonly PhrasingContent[]) {
  const segments: Array<
    | { kind: 'text'; nodes: PhrasingContent[] }
    | { kind: 'image'; url: string; alt: string; href?: string }
  > = [];

  let run: PhrasingContent[] = [];
  const flushRun = () => {
    if (run.length) {segments.push({ kind: 'text', nodes: run });}
    run = [];
  };

  for (const child of children) {
    if (child.type === 'image') {
      flushRun();
      segments.push({ kind: 'image', url: child.url, alt: child.alt ?? '' });
    } else if (
      child.type === 'link' &&
      child.children.length === 1 &&
      child.children[0]!.type === 'image'
    ) {
      flushRun();
      const image = child.children[0]!;
      segments.push({
        kind: 'image',
        url: image.url,
        alt: image.alt ?? '',
        href: child.url,
      });
    } else {
      run.push(child);
    }
  }

  flushRun();
  return segments;
}

function renderParagraph(
  children: readonly PhrasingContent[],
  ctx: RenderContext,
  _key: string | number,
): ReactNode {
  const { styles } = ctx;
  const ictx = inlineContext(ctx, flattenInline(children), styles.body.color);
  const segments = paragraphSegments(children);
  return (
    <View style={styles.paragraph}>
      {segments.map((segment) =>
        segment.kind === 'text' ? (
          <RichText style={styles.body}>
            {renderInline(segment.nodes, ictx)}
          </RichText>
        ) : (
          <MarkdownImage
            href={segment.href}
            label={segment.alt}
            styles={styles}
            url={segment.url}
            onOpenLink={ctx.onOpenLink}
          />
        ),
      )}
    </View>
  );
}

function MarkdownImage({
  url,
  label,
  href,
  styles,
  onOpenLink,
}: {
  url: string;
  label: string;
  href?: string;
  styles: MarkdownStyles;
  onOpenLink: (url: string) => void;
}) {
  const image = (
    <Image
      alt={label || undefined}
      src={url}
      style={styles.image}
      ios={{ stretch: 'aspectFit' }}
      web={{ style: { objectFit: 'contain' } }}
    />
  );

  if (!href || href === PENDING_LINK_URL) {return image;}
  return (
    <Pressable accessibilityRole="link" onPress={() => onOpenLink(href)}>
      {image}
    </Pressable>
  );
}

function renderCode(
  value: string,
  language: string | null,
  ctx: RenderContext,
  _key: string | number,
): ReactNode {
  const { styles } = ctx;
  const spans = beginTextElement(ctx, value);
  const lines = value.split('\n');
  const ictx: InlineContext = {
    block: ctx,
    spans,
    cursor: { value: 0 },
    ink: styles.codeLine.color,
  };

  return (
    <View style={styles.codeBlock}>
      {language ? (
        <View style={styles.codeHeader}>
          <RichText style={styles.codeHeaderText}>
            <RichTextSpan text={language} />
          </RichText>
        </View>
      ) : null}
      <ScrollView horizontal>
        <View style={styles.codeContent}>
          {lines.map((line, index) => {
            const pieces = renderLeaf(line, ictx);
            // The '\n' separating lines is part of the flattened text.
            if (index < lines.length - 1) {ictx.cursor.value += 1;}
            return (
              <RichText style={styles.codeLine}>
                {pieces.length ? pieces : <RichTextSpan text=" " />}
              </RichText>
            );
          })}
        </View>
      </ScrollView>
    </View>
  );
}

function renderListItem(
  item: ListItem,
  marker: string,
  ctx: RenderContext,
  _key: string | number,
): ReactNode {
  const { styles } = ctx;
  // The marker is a veil element of its own: a freshly streamed item must
  // dissolve in whole — a solid bullet floating over still-fading text reads
  // as a hole in the list.
  const spans = beginTextElement(ctx, marker);
  const opacity = spans.length ? spans[0]![2] : 1;
  const markerStyle = opacity < 1
    ? [styles.listMarker, { color: applyAlpha(styles.listMarker.color, opacity) }]
    : styles.listMarker;

  return (
    <View style={styles.listItem}>
      <RichText style={markerStyle}>
        <RichTextSpan text={marker} />
      </RichText>
      <View style={styles.listContent}>
        {item.children.map((child, index) => renderBlock(child, ctx, index))}
      </View>
    </View>
  );
}

function renderTable(
  node: Extract<RootContent, { type: 'table' }>,
  ctx: RenderContext,
  _key: string | number,
): ReactNode {
  const { styles } = ctx;
  const [head, ...rows] = node.children;
  const renderRow = (
    row: (typeof node.children)[number],
    rowKey: string | number,
    isHead: boolean,
  ) => (
    <View style={[styles.tableRow, isHead && styles.tableHeadRow]}>
      {row.children.map((cell, cellIndex) => {
        const textStyle = isHead ? styles.tableHeadText : styles.tableCellText;
        const align = node.align?.[cellIndex];
        const ictx = inlineContext(ctx, flattenInline(cell.children), textStyle.color);
        return (
          <View style={styles.tableCell}>
            <RichText style={[textStyle, align ? { textAlign: align } : null]}>
              {renderInline(cell.children, ictx)}
            </RichText>
          </View>
        );
      })}
    </View>
  );

  return (
    <View style={styles.table}>
      <ScrollView horizontal>
        <View>
          {head && renderRow(head, 'head', true)}
          {rows.map((row, index) => renderRow(row, index, false))}
        </View>
      </ScrollView>
    </View>
  );
}

export function renderBlock(
  node: RootContent | BlockContent | DefinitionContent,
  ctx: RenderContext,
  key: string | number,
): ReactNode {
  const { styles } = ctx;
  switch (node.type) {
    case 'paragraph':
      return renderParagraph(node.children, ctx, key);
    case 'heading': {
      const style = styles.heading[node.depth - 1] ?? styles.heading[5]!;
      const ictx = inlineContext(ctx, flattenInline(node.children), style.color);
      return (
        <RichText style={style}>
          {renderInline(node.children, ictx)}
        </RichText>
      );
    }
    case 'code':
      return renderCode(node.value, node.lang ?? null, ctx, key);
    case 'blockquote':
      return (
        <View style={styles.blockquote}>
          {node.children.map((child, index) => renderBlock(child, ctx, index))}
        </View>
      );
    case 'list': {
      const start = node.start ?? 1;
      return (
        <View style={styles.list}>
          {node.children.map((item, index) => {
            const marker = item.checked != null
              ? item.checked ? '☑' : '☐'
              : node.ordered
                ? `${start + index}.`
                : '•';

            return renderListItem(item, marker, ctx, index);
          })}
        </View>
      );
    }
    case 'table':
      return renderTable(node, ctx, key);
    case 'thematicBreak':
      return <View style={styles.hr} />;
    case 'html': {
      // Raw HTML stays literal text, matching the desktop transcript.
      const ictx = inlineContext(ctx, node.value, styles.body.color);
      return (
        <View style={styles.paragraph}>
          <RichText style={styles.body}>
            {renderLeaf(node.value, ictx)}
          </RichText>
        </View>
      );
    }
    case 'definition':
    case 'footnoteDefinition':
      return null;
    default:
      return null;
  }
}
