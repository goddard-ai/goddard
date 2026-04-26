import { Portal } from "@ark-ui/react/portal"

export const dialogPortalId = "dialog-portal"
const dialogPortalElement = document.getElementById(dialogPortalId)

/** Renders dialogs into a dedicated host so menu overlays can consistently layer above them. */
export component DialogPortal(props: { children?: preact.ComponentChildren }) {
  const containerElement = dialogPortalElement

  if (!containerElement) {
    return
  }

  <Portal container={containerElement}>
    {props.children}
  </Portal>
}
