import { Portal } from "@ark-ui/react/portal"

export const dialogPortalId = "dialog-portal"

const dialogPortalElementRef = {
  current: document.getElementById(dialogPortalId),
}

/** Renders dialogs into a dedicated host so menu overlays can consistently layer above them. */
export component DialogPortal(props: { children?: preact.ComponentChildren }) {
  if (!dialogPortalElementRef.current) {
    return
  }

  <Portal container={dialogPortalElementRef}>
    {props.children}
  </Portal>
}
