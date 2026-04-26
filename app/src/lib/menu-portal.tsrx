import { Portal } from "@ark-ui/react/portal"

export const menuPortalId = "menu-portal"

const portalElementRef = {
  current: document.getElementById(menuPortalId)!,
}

/** Renders menus into a shared host that layers above the dialog portal. */
export component MenuPortal(props: { children?: preact.ComponentChildren }) {
  <Portal container={portalElementRef}>
    {props.children}
  </Portal>
}
