import { createPortal } from "preact/compat"

import { getOverlayPortalRoot, type OverlayPortalRoot } from "./portal-root.ts"

/** Renders private overlay primitives through Preact portals while preserving context. */
export component OverlayPortal(props: {
  root: OverlayPortalRoot;
  children?: preact.ComponentChildren;
}) {
  const portalRoot = getOverlayPortalRoot(props.root)

  if (!portalRoot) {
    return
  }

  {createPortal(props.children, portalRoot)}
}
