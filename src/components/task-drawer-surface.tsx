/** @jsxImportSource @nativescript-community/octane */
import { registerElement } from '@nativescript-community/octane';
import type { Attributes } from '@nativescript-community/octane/intrinsics';
import { Color, Device, GridLayout, Screen } from '@nativescript/core';
import { Drawer as NSDrawer, install } from '@nativescript-community/ui-drawer';
import { useEffect, useRef } from 'octane';

import type { TaskDrawerSurfaceProps } from './task-drawer-surface.types';
import { liftCard } from '@/lib/card-lift';
import { useTheme } from '@/lib/use-theme';

/** The open/close/start events ride `notify` — declaring the statics is what
 * turns them into typed `onOpen`/`onClose`/`onStart` props. */
class TaskDrawerView extends NSDrawer {
  static openEvent = 'open';
  static closeEvent = 'close';
  static startEvent = 'start';
}

install();
registerElement('taskdrawer', TaskDrawerView);
registerElement('drawercontent', GridLayout);

declare module '@nativescript-community/octane/intrinsics' {
  interface NativeScriptElements {
    taskdrawer: Attributes<typeof TaskDrawerView>;
    drawercontent: Attributes<typeof GridLayout>;
  }
}

/**
 * The app drawer on iOS/Android — `ui-drawer` in `under` mode, so the screen
 * slides right as a rounded card and reveals the task list underneath. The
 * reference wrapped the whole app in react-native-gesture-handler's Drawer
 * with `drawerType: 'back'` — same contract: full-screen swipe,
 * dismiss-by-tap (a transparent backdrop), and a card-lift edge.
 */
export function TaskDrawerSurface({
  open,
  swipeEnabled,
  drawerWidth,
  drawerContent,
  onGestureStart,
  onOpenChange,
  children,
}: TaskDrawerSurfaceProps) {
  const theme = useTheme();
  const host = useRef<TaskDrawerView | null>(null);
  const swipeWidth = Screen.mainScreen.widthDIPs;
  const radius = displayCornerRadius();

  useEffect(() => {
    const drawer = host.current;
    if (!drawer) {return;}
    if (open && !drawer.isOpened('left')) {
      void drawer.open('left').catch(() => {});
    } else if (!open && drawer.isOpened('left')) {
      void drawer.close('left').catch(() => {});
    }
  }, [open]);

  return (
    <taskdrawer
      ref={host}
      style={{ flex: '1 1' }}
      backDropEnabled
      backdropColor={new Color('transparent')}
      backdropTapGestureEnabled
      gestureEnabled={swipeEnabled}
      leftDrawerMode="under"
      leftSwipeDistance={swipeWidth}
      onClose={() => onOpenChange(false)}
      onOpen={() => onOpenChange(true)}
      onStart={() => onGestureStart()}
    >
      {/* The sheet sits behind the content; `under` mode keeps it fixed
          while the card slides. Its own width is the open offset. */}
      <drawercontent hostSlot="leftDrawer" style={{ width: drawerWidth }}>
        {drawerContent}
      </drawercontent>
      <drawercontent hostSlot="mainContent">
        {/* The lifted card: hardware radius, clipped contents, and a faint
            edge shadow — always on; it paints offscreen until the card
            slides, so no per-progress animation is needed. */}
        <drawercontent
          ref={liftCard}
          style={{
            backgroundColor: new Color(theme.background),
            borderRadius: radius,
            androidElevation: 8,
          }}
        >
          {children}
        </drawercontent>
      </drawercontent>
    </taskdrawer>
  );
}

/** Border radii per iOS display generation — the card rounds into the same
 * corner curve the screen glass has. Mirrored from the reference's table. */
const IOS_DISPLAY_RADIUS: Readonly<Record<string, number>> = {
  iPhone18: 55, iPhone17: 62, iPhone16: 55, iPhone15: 55,
  iPhone14: 47.33, iPhone13: 47.33, iPhone12: 47.33, iPhone11: 41.5,
  iPhone10: 39, iPhone9: 39, iPhone8: 39, iPhone7: 39, iPhone6: 39,
};

function displayCornerRadius(): number {
  if (Device.os === 'iOS') {
    const major = parseInt(Device.model.split(',')[0]?.replace(/\D/g, '') ?? '', 10);
    return IOS_DISPLAY_RADIUS[`iPhone${major}`] ?? 39;
  }

  return Device.os === 'Android' ? 32 : 0;
}
