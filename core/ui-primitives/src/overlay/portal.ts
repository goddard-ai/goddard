import { createPortal } from "preact/compat"

import {
  getOverlayPortalRoot,
  setOverlayPortalRoots,
  type OverlayPortalRoot,
  type OverlayPortalRootResolver,
} from "./portal-root.ts"

export { setOverlayPortalRoots, type OverlayPortalRoot, type OverlayPortalRootResolver }

/** Renders private overlay primitives through Preact portals while preserving context. */
export function OverlayPortal(props: {
  root: OverlayPortalRoot
  children?: preact.ComponentChildren
}) {
  const portalRoot = getOverlayPortalRoot(props.root)

  if (!portalRoot) {
    return null
  }

  return createPortal(props.children, portalRoot)
}
