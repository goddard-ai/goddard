export type OverlayPortalRoot = "dialog" | "menu"

export type OverlayPortalRootResolver = HTMLElement | null | (() => HTMLElement | null)

const portalRoots: Partial<Record<OverlayPortalRoot, OverlayPortalRootResolver>> = {}

/** Configures the host elements used by overlay primitives without coupling them to an app shell. */
export function setOverlayPortalRoots(
  roots: Partial<Record<OverlayPortalRoot, OverlayPortalRootResolver>>,
) {
  Object.assign(portalRoots, roots)
}

/** Resolves one configured overlay host element for portal rendering. */
export function getOverlayPortalRoot(root: OverlayPortalRoot) {
  const resolver = portalRoots[root]

  return typeof resolver === "function" ? resolver() : (resolver ?? null)
}
