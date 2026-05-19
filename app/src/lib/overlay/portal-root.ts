import { menuPortalId } from "../menu-portal.tsrx"

export type OverlayPortalRoot = "dialog" | "menu"

const dialogPortalId = "dialog-portal"

const portalRootIds: Record<OverlayPortalRoot, string> = {
  dialog: dialogPortalId,
  menu: menuPortalId,
}

/** Resolves the existing app portal hosts without making portal selection a public UI API. */
export function getOverlayPortalRoot(root: OverlayPortalRoot) {
  return document.getElementById(portalRootIds[root])
}
