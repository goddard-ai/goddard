import { Portal } from "@ark-ui/react/portal"

export const menuPortalId = "menu-portal"
const menuPortalElement = document.getElementById(menuPortalId)

/** Renders menus into a shared host that layers above the dialog portal. */
export component MenuPortal(props: { children?: preact.ComponentChildren }) {
  const containerElement = menuPortalElement

  if (!containerElement) {
    return
  }

  <Portal container={containerElement}>
    {props.children}
  </Portal>
}
