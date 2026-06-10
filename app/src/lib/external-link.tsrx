import { desktopHost } from "~/desktop-host.ts"

function reportOpenFailure(href: string, error?: unknown) {
  if (error) {
    console.error("Failed to open external link.", error)
    return
  }

  console.error(`Failed to open external link: ${href}`)
}

export function ExternalLink(props: {
  "aria-label"?: string;
  children: preact.ComponentChildren;
  class?: string;
  href: string;
  rel?: string;
  target?: string;
  title?: string;
  onClick?: preact.JSX.IntrinsicElements["a"]["onClick"];
}) @{
  <a
    aria-label={props["aria-label"]}
    class={props.class}
    href={props.href}
    rel={props.rel ?? "noreferrer"}
    target={props.target ?? "_blank"}
    title={props.title}
    onClick={(event) => {
      props.onClick?.(event)

      if (event.defaultPrevented) {
        return
      }

      event.preventDefault()
      event.stopPropagation()

      void desktopHost.openExternal(props.href).then((opened) => {
        if (!opened) {
          reportOpenFailure(props.href)
        }
      }).catch((error) => {
        reportOpenFailure(props.href, error)
      })
    }}
  >{props.children}</a>
}
