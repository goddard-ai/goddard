import { OverlayPortal } from "./overlay/portal.tsrx"

export const menuPortalId = "menu-portal"

/** Renders menus into a shared host that layers above the dialog portal. */
export component MenuPortal(props: { children?: preact.ComponentChildren }) {
  <OverlayPortal root="menu">
    {props.children}
  </OverlayPortal>
}
