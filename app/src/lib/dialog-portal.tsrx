import { Portal } from "@ark-ui/react/portal"
import { useRef } from "preact/hooks"

export const dialogPortalId = "dialog-portal"
const dialogPortalElement =
  typeof document === "undefined" ? null : document.getElementById(dialogPortalId)

/** Renders dialogs into a dedicated host so menu overlays can consistently layer above them. */
export component DialogPortal(props: { children?: preact.ComponentChildren }) {
  const containerRef = useRef<HTMLElement | null>(dialogPortalElement)
  containerRef.current = dialogPortalElement

  if (!containerRef.current) {
    return
  }

  <tsx:react>
    <Portal container={containerRef}>
      {props.children}
    </Portal>
  </tsx:react>
}
