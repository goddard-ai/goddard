/** @jsxImportSource @nativescript-community/octane */
/**
 * Native transcript scroller — the reference mobile engine
 * (reference/mobile/src/components/transcript-list.tsx) ported to raw
 * NativeScript intrinsics.
 *
 *   RN ScrollView                     NativeScript <scrollview>
 *   onScroll                          scroll event ({ scrollY })
 *   scrollTo({ y, animated })         scrollToVerticalOffset
 *   onContentSizeChange               content column layoutChanged → getActualSize
 *   layoutMeasurement                 the scroll view's own getActualSize
 *   onScrollBeginDrag                 first scroll event while a touch is down
 *   onMomentumScrollBegin/End         post-release scroll events + a quiet timer
 *   onTouchStart/End                  touch gesture (action down/up/cancel)
 *   maintainVisibleContentPosition    no NS equivalent — approximated by
 *      measuring content height before/after a change and issuing one
 *      scrollToVerticalOffset adjustment while the reader is off the tail,
 *      touching, or a row pin is armed (see keepRowTop).
 *
 * The inversion is the same trick: the scrollview renders scaleY -1 and each
 * direct child flips back, so native offset 0 is the visual bottom and a
 * streaming tail stays glued by layout. All offsets are native (unflipped):
 * `offset` grows as the reader scrolls up into history.
 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'octane';

import type {
  EventData,
  FlexboxLayout,
  ScrollEventData,
  ScrollView as NativeScrollView,
  StackLayout,
  TouchGestureEventData,
  View as NativeView,
} from '@nativescript/core';

import { Absolute, Pressable, useAnimation, View } from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import { GlassSurface } from './glass-surface';
import { EarlierIndicator } from './transcript-rows';
import type {
  TranscriptScrollHandle,
  TranscriptScrollViewProps,
} from './transcript-scroll-view.types';

import { dismissKeyboard } from '@/lib/keyboard';
import { Radius } from '@/lib/theme-colors';
import { useReducedMotion } from '@/lib/use-reduced-motion';
import { useTheme } from '@/lib/use-theme';
import {
  JUMP_HIDE_DISTANCE,
  JUMP_SHOW_DISTANCE,
  jumpVisible,
  offTail,
  peekedWithinBand,
  shouldExtendTail,
  type ScrollMetrics,
} from '@/lib/transcript-scroll';

/** Longer than an animated scroll (~0.3 s) plus a frame. */
const SEAT_GLIDE_MS = 700;
/** Post-release window in which a continuing stream of scroll events means
 * the gesture became a fling. */
const FLING_GRACE_MS = 60;
/** A fling is over once this long passes with no scroll event. */
const MOMENTUM_QUIET_MS = 120;
/** Give up waiting for a revealed row's layout after this long. */
const REVEAL_TIMEOUT_MS = 2_000;

export function TranscriptScrollView({
  rows,
  renderRow,
  strip,
  hasEarlier,
  onExtendTail,
  onScrollHandle,
  headerInset,
}: TranscriptScrollViewProps) {
  const theme = useTheme();
  const reducedMotion = useReducedMotion();
  const scrollRef = useRef<NativeScrollView | null>(null);
  const rowViews = useRef(new Map<string, FlexboxLayout>());
  const rowHeights = useRef(new Map<string, number>());
  const stripHeight = useRef(0);

  // ── Scroll bookkeeping ──────────────────────────────────────────────────
  const metrics = useRef<ScrollMetrics>({ offset: 0, contentHeight: 0, viewportHeight: 0 });
  const touchingRef = useRef(false);
  const touchMoved = useRef(false);
  /** Set while a tail extension is pending; the window's leading row key at
   * request time tells extension growth (which prepends rows) apart from a
   * stream commit that happens to land first. */
  const extending = useRef<string | null>(null);
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  const decelerating = useRef(false);
  const touchOrigin = useRef<{ offset: number; contentHeight: number } | null>(null);
  const followRequest = useRef<ReturnType<typeof setTimeout> | null>(null);
  const momentumTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** Armed by keepRowTop for the commit that applies a row-local change. */
  const pinning = useRef(false);
  const jumpRef = useRef(false);
  const unseenRef = useRef(false);
  const offTailRef = useRef(false);
  /** Where a release happened, so a fling can be told from a still drag. */
  const releasedAt = useRef(0);
  /** While a glide to the tail is in flight the reader is treated as seated:
   * growth during the glide lands at the target instead of compensating
   * against it. */
  const seatingUntil = useRef(0);
  const pendingReveal = useRef<string | null>(null);
  const revealDeadline = useRef(0);
  const [jump, setJump] = useState(false);
  const [unseen, setUnseen] = useState(false);
  const [jumpMounted, setJumpMounted] = useState(false);
  const jumpOpacity = useAnimation(0, 'opacity');

  const scrollToLatest = useCallback((animated = true) => {
    seatingUntil.current = Date.now() + SEAT_GLIDE_MS;
    if (offTailRef.current) {offTailRef.current = false;}
    if (jumpRef.current) {
      jumpRef.current = false;
      setJump(false);
    }

    scrollRef.current?.scrollToVerticalOffset(0, animated && !reducedMotion);
  }, [reducedMotion]);

  const evaluate = useCallback(() => {
    const current = metrics.current;
    // Scroll events arrive at display rate; touch state on transitions only,
    // and not at all mid-glide — the offset is on its way to 0.
    const gliding = Date.now() < seatingUntil.current && !touchingRef.current;
    offTailRef.current = !gliding && offTail(current.offset);
    const nextJump = !gliding && jumpVisible(current.offset, jumpRef.current);
    if (nextJump !== jumpRef.current) {
      jumpRef.current = nextJump;
      setJump(nextJump);
    }

    if (current.offset <= JUMP_HIDE_DISTANCE && unseenRef.current) {
      unseenRef.current = false;
      setUnseen(false);
    }

    if (extending.current === null && shouldExtendTail(current, hasEarlier)) {
      extending.current = rowsRef.current[0]?.key ?? null;
      onExtendTail();
    }
  }, [hasEarlier, onExtendTail]);

  // A window extension that changes nothing on screen (all-hidden rows) must
  // not wedge the extender — layoutChanged clears it when size does change.
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      extending.current = null;
      evaluate();
    });

    return () => cancelAnimationFrame(frame);
  }, [evaluate, rows]);

  /** A reader resting just off the bottom is brought back to it — never
   * under a finger, never through a fling. The band is judged by what the
   * reader did, not by what the stream did underneath them. */
  const reseatIfInBand = useCallback(() => {
    if (touchingRef.current || decelerating.current) {return;}
    const { offset, contentHeight } = metrics.current;
    const origin = touchOrigin.current;
    const grown = origin ? Math.max(0, contentHeight - origin.contentHeight) : 0;
    if (peekedWithinBand(offset, offset - grown)) {scrollToLatest(true);}
  }, [scrollToLatest]);

  /** The gesture is over (tap, drag without fling, or momentum end): decide
   * once, then forget its origin so later growth is judged on its own. */
  const settleGesture = useCallback(() => {
    reseatIfInBand();
    touchOrigin.current = null;
  }, [reseatIfInBand]);

  function armMomentumTimer(ms: number) {
    if (momentumTimer.current) {clearTimeout(momentumTimer.current);}
    momentumTimer.current = setTimeout(() => {
      momentumTimer.current = null;
      decelerating.current = false;
      releasedAt.current = 0;
      settleGesture();
    }, ms);
  }

  function onScroll(event: EventData) {
    metrics.current.offset = (event as ScrollEventData).scrollY;
    const scroll = scrollRef.current;
    if (scroll) {
      metrics.current.contentHeight = scroll.scrollableHeight + metrics.current.viewportHeight;
    }

    if (touchingRef.current) {
      touchMoved.current = true;
    } else if (releasedAt.current) {
      // Scroll events without a finger down = momentum. Re-arm the quiet
      // timer; its expiry is the momentum end.
      decelerating.current = true;
      armMomentumTimer(MOMENTUM_QUIET_MS);
    }

    evaluate();
  }

  function onViewportLayout(event: EventData) {
    metrics.current = {
      ...metrics.current,
      viewportHeight: (event.object as NativeScrollView).getActualSize().height,
    };

    evaluate();
  }

  function onContentLayout(event: EventData) {
    const scroll = scrollRef.current;
    const height = (event.object as StackLayout).getActualSize().height;
    const delta = height - metrics.current.contentHeight;
    const grew = delta > 0.5;
    metrics.current.contentHeight = height;
    // Extension growth lands at the native end of the content — no existing
    // row moves, so it is never compensated. A stream commit may land while
    // an extension is still pending: it prepends nothing, so the window's
    // leading row key is unchanged and the flag stays armed.
    const wasExtending = extending.current !== null
      && (rowsRef.current[0]?.key ?? null) !== extending.current;

    if (wasExtending) {extending.current = null;}
    if (scroll && delta !== 0) {
      metrics.current.offset = scroll.verticalOffset;
    }

    if (followRequest.current) {
      clearTimeout(followRequest.current);
      followRequest.current = null;
      scrollToLatest(true);
    } else {
      // The maintainVisibleContentPosition approximation: growth above the
      // reader's line pushes it down, so the offset rides along by the same
      // delta. Seated readers are pinned by the layout itself (offset 0 is
      // the tail), extension growth sits below every mounted row, and a
      // glide to the tail is heading for offset 0 anyway. A pinned row
      // compensates shrink as well as growth (a collapsing fold holds its
      // header), the stream anchor only growth.
      const gliding = Date.now() < seatingUntil.current && !touchingRef.current;
      const shift = pinning.current ? delta : delta > 0.5 ? delta : 0;
      const pinned = pinning.current
        || (!gliding && (touchingRef.current || decelerating.current || offTailRef.current));

      if (scroll && pinned && !wasExtending && Math.abs(shift) > 0.5) {
        scroll.scrollToVerticalOffset(Math.max(0, metrics.current.offset + shift), false);
        metrics.current.offset += shift;
      }

      if (grew && metrics.current.offset > JUMP_SHOW_DISTANCE && !unseenRef.current) {
        unseenRef.current = true;
        setUnseen(true);
      } else if (grew) {
        reseatIfInBand();
      }
    }

    pinning.current = false;
    tryReveal();
    evaluate();
  }

  /** Row-local change (fold, disclosure): hold the row's visual top for the
   * commit that applies it. Growth inside the row then extends downward —
   * the anchor compensation above pins everything older. */
  const keepRowTop = useCallback((apply: () => void) => {
    if (pinning.current) {
      apply();
      return;
    }

    pinning.current = true;
    apply();
    // If the apply changed no layout the pin would linger — disarm it.
    setTimeout(() => { pinning.current = false; }, 400);
  }, []);

  // ── Row reveal (search navigation) ──────────────────────────────────────
  // Row native y = the strip plus every newer row's height (children mount
  // newest-first below the strip). `tryReveal` retried on each content
  // layout lands the scroll as soon as the row's window reaches it.
  const tryReveal = useCallback(() => {
    const key = pendingReveal.current;
    const scroll = scrollRef.current;
    if (!key || !scroll) {return;}
    if (Date.now() > revealDeadline.current) {
      pendingReveal.current = null;
      return;
    }

    const view = rowViews.current.get(key);
    if (!view || view.getActualSize().height <= 0) {return;}
    let top = stripHeight.current;
    for (let ix = rows.length - 1; ix >= 0; ix -= 1) {
      const row = rows[ix]!;
      if (row.key === key) {
        // Off the tail on purpose: the same "seating" glide the jump button
        // uses so streaming growth does not fight the animation.
        seatingUntil.current = Date.now() + SEAT_GLIDE_MS;
        const target = Math.max(
          0,
          top + view.getActualSize().height - metrics.current.viewportHeight + headerInset + 8,
        );

        scroll.scrollToVerticalOffset(target, !reducedMotion);
        pendingReveal.current = null;
        return;
      }

      const height = rowHeights.current.get(row.key);
      if (height == null) {return;}
      top += height;
    }
  }, [headerInset, reducedMotion, rows]);

  const revealRow = useCallback((rowKey: string) => {
    dismissKeyboard();
    pendingReveal.current = rowKey;
    revealDeadline.current = Date.now() + REVEAL_TIMEOUT_MS;
    tryReveal();
  }, [tryReveal]);

  const followNextGrowth = useCallback(() => {
    if (followRequest.current) {clearTimeout(followRequest.current);}
    followRequest.current = setTimeout(() => {
      followRequest.current = null;
    }, 1_500);

    scrollToLatest(true);
  }, [scrollToLatest]);

  useEffect(() => {
    const handle: TranscriptScrollHandle = { scrollToLatest, followNextGrowth, revealRow };
    onScrollHandle?.(handle);
    return () => onScrollHandle?.(null);
  }, [followNextGrowth, onScrollHandle, revealRow, scrollToLatest]);

  useEffect(() => () => {
    if (followRequest.current) {clearTimeout(followRequest.current);}
    if (momentumTimer.current) {clearTimeout(momentumTimer.current);}
  }, []);

  // ── Touch / gesture ─────────────────────────────────────────────────────
  // Any touch owns the viewport: content must not move under a finger, so
  // the history anchor applies for the whole gesture. A plain tap also puts
  // the keyboard away, the way every iOS chat does.
  function onTouch(event: TouchGestureEventData) {
    switch (event.action) {
      case 'down': {
        touchingRef.current = true;
        touchMoved.current = false;
        decelerating.current = false;
        seatingUntil.current = 0;
        releasedAt.current = 0;
        if (momentumTimer.current) {
          clearTimeout(momentumTimer.current);
          momentumTimer.current = null;
        }

        touchOrigin.current = {
          offset: metrics.current.offset,
          contentHeight: metrics.current.contentHeight,
        };

        break;
      }
      case 'up':
      case 'cancel': {
        if (!touchingRef.current) {return;}
        touchingRef.current = false;
        if (!touchMoved.current) {
          dismissKeyboard();
          settleGesture();
          return;
        }

        // A fling shows up as scroll events after release; give it a beat
        // before settling the drag in place.
        releasedAt.current = Date.now();
        armMomentumTimer(FLING_GRACE_MS);
        break;
      }
      default:
        break;
    }
  }

  // ── Jump button ─────────────────────────────────────────────────────────
  useEffect(() => {
    if (jump) {
      setJumpMounted(true);
      const frame = requestAnimationFrame(() => jumpOpacity.to(1, { duration: reducedMotion ? 0 : 160 }));
      return () => cancelAnimationFrame(frame);
    }

    jumpOpacity.to(0, { duration: reducedMotion ? 0 : 160 });
    const timer = setTimeout(() => setJumpMounted(false), reducedMotion ? 0 : 160);
    return () => clearTimeout(timer);
  }, [jump, jumpOpacity, reducedMotion]);

  return (
    <View style={{ flex: '1 1' }}>
      <scrollview
        ref={scrollRef}
        flexGrow={1}
        iosContentInsetAdjustmentBehavior="never"
        scaleY={-1}
        scrollBarIndicatorVisible={false}
        onLayoutChanged={onViewportLayout}
        onScroll={onScroll}
        onTouch={onTouch}>
        {/* Content column: native top is the visual bottom. */}
        <stacklayout
          style={{ paddingLeft: 16, paddingRight: 16, paddingTop: 12 }}
          onLayoutChanged={onContentLayout}>
          {/* Child 0, always mounted: the native anchor while pinned. */}
          <flexboxlayout
            flexDirection="column"
            scaleY={-1}
            onLayoutChanged={(event: EventData) => {
              stripHeight.current = (event.object as NativeView).getActualSize().height;
            }}>
            {strip}
          </flexboxlayout>
          {rows.map((_, reversed) => {
            const row = rows[rows.length - 1 - reversed]!;
            return (
              <flexboxlayout
                key={row.key}
                ref={(view: FlexboxLayout | null) => {
                  if (view) {rowViews.current.set(row.key, view);}
                  else {
                    rowViews.current.delete(row.key);
                    rowHeights.current.delete(row.key);
                  }
                }}
                flexDirection="column"
                scaleY={-1}
                onLayoutChanged={(event: EventData) => {
                  rowHeights.current.set(row.key, (event.object as NativeView).getActualSize().height);
                  tryReveal();
                }}>
                {renderRow(row, { keepRowTop })}
              </flexboxlayout>
            );
          })}
          {/* Last child: the visual top, just under the header strip the
           * frame inset leaves clear. */}
          <flexboxlayout flexDirection="column" scaleY={-1} style={{ paddingTop: 2 }}>
            {hasEarlier && <EarlierIndicator />}
          </flexboxlayout>
        </stacklayout>
      </scrollview>
      {jumpMounted && (
        <Absolute style={{ bottom: 12, right: 14 }}>
          <View ref={jumpOpacity.ref}>
            <GlassSurface
              fallbackColor={theme.surface}
              interactive
              style={{ borderRadius: Radius.pill, height: 40, width: 40 }}>
              <Pressable
                accessibilityHint={unseen ? 'New content arrived below' : undefined}
                accessibilityLabel="Scroll to latest"
                accessibilityRole="button"
                onPress={() => scrollToLatest(true)}
                style={{ alignItems: 'center', flex: '1 1', justifyContent: 'center' }}>
                <AppSymbol
                  name={{ ios: 'arrow.down', android: 'arrow_downward', web: 'arrow_downward' }}
                  size={15}
                  tintColor={theme.textSecondary}
                />
                {unseen && (
                  <Absolute style={{ right: 7, top: 7 }}>
                    <View
                      style={{
                        backgroundColor: theme.accent,
                        borderColor: theme.surface,
                        borderRadius: 5,
                        borderWidth: 1.5,
                        height: 10,
                        width: 10,
                      }}
                    />
                  </Absolute>
                )}
              </Pressable>
            </GlassSurface>
          </View>
        </Absolute>
      )}
    </View>
  );
}
