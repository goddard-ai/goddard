import type { ReactNode } from 'octane';

export interface TaskDrawerSurfaceProps {
  open: boolean;
  swipeEnabled: boolean;
  drawerWidth: number;
  drawerContent: ReactNode;
  /** A pan is revealing the list — refresh it before it is visible. */
  onGestureStart: () => void;
  onOpenChange: (open: boolean) => void;
  children?: ReactNode;
}
