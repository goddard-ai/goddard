import { Drawer } from '@octane-xplat/ui';

import type { TaskDrawerSurfaceProps } from './task-drawer-surface.types';

/**
 * Web keeps the shared in-window overlay drawer — pointer input has no
 * edge-swipe, so the mobile card-lift reveal does not apply here.
 */
export function TaskDrawerSurface({
  open,
  drawerContent,
  onOpenChange,
  children,
}: TaskDrawerSurfaceProps) {
  return (
    <>
      {children}
      <Drawer
        open={open}
        onDismiss={() => onOpenChange(false)}
        drawer={drawerContent}
      />
    </>
  );
}
