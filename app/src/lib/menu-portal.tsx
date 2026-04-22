import { Portal } from "@ark-ui/react/portal";
import { useRef } from "preact/hooks";

export const menuPortalId = "menu-portal";
const menuPortalElement =
  typeof document === "undefined"
    ? null
    : document.getElementById(menuPortalId);

/** Renders menus into a shared host that layers above the dialog portal. */
export function MenuPortal(props: { children?: preact.ComponentChildren }) {
  const containerRef = useRef<HTMLElement | null>(menuPortalElement);
  containerRef.current = menuPortalElement;

  if (!containerRef.current) {
    return null;
  }

  return <Portal container={containerRef}>{props.children}</Portal>;
}
