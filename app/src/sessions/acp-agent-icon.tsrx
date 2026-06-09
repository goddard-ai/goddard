import { cx } from "@goddard-ai/styled-system/css"
import { Bot } from "lucide-react"

import styles from "./acp-agent-icon.style.ts"

const blockedSvgElements = new Set([
  "script",
  "foreignObject",
  "iframe",
  "object",
  "embed",
  "link",
  "style",
])

function decodeSvgDataUrl(value: string) {
  const [metadata, data] = value.split(",", 2)

  if (
    !metadata || data === undefined ||
      !metadata.toLocaleLowerCase().startsWith("data:image/svg+xml")
  ) {
    return null
  }

  try {
    return metadata.toLocaleLowerCase().includes(";base64") ? atob(data) : decodeURIComponent(data)
  } catch {
    return null
  }
}

function getSvgMarkup(icon: string | null | undefined) {
  const source = icon?.trim()

  if (!source) {
    return null
  }

  const svgMarkup =
    source.startsWith("data:image/svg+xml") ? decodeSvgDataUrl(source) : source

  if (!svgMarkup?.trimStart().startsWith("<svg")) {
    return null
  }

  const document = new DOMParser().parseFromString(svgMarkup, "image/svg+xml")
  const root = document.documentElement

  if (
    root.nodeName.toLocaleLowerCase() !== "svg" ||
      document.getElementsByTagName("parsererror").length > 0
  ) {
    return null
  }

  for (const element of Array.from(root.querySelectorAll("*"))) {
    if (blockedSvgElements.has(element.tagName)) {
      element.remove()
      continue
    }

    for (const attribute of Array.from(element.attributes)) {
      const attributeName = attribute.name.toLocaleLowerCase()
      const attributeValue = attribute.value.trim().toLocaleLowerCase()

      if (
        attributeName.startsWith("on") ||
          (attributeName === "href" || attributeName === "xlink:href") &&
            attributeValue.startsWith("javascript:")
      ) {
        element.removeAttribute(attribute.name)
      }
    }
  }

  root.removeAttribute("id")
  return new XMLSerializer().serializeToString(root)
}

function getTextIcon(icon: string | null | undefined) {
  const source = icon?.trim()

  if (!source || source.includes("/") || source.includes("<") || source.includes(">")) {
    return null
  }

  return Array.from(source).length <= 3 ? source : null
}

export function AcpAgentIcon(props: {
  class?: string;
  icon?: string | null;
  size?: number;
  strokeWidth?: number;
}) @{
  const svgMarkup = getSvgMarkup(props.icon)
  const textIcon =
    svgMarkup ? null : getTextIcon(props.icon)

  <span class={cx(styles.root, props.class)} aria-hidden={true}>
    @if (svgMarkup) {
      <span
        dangerouslySetInnerHTML={{
          __html: svgMarkup,
        }}
      />
    } @else if (textIcon) {
      <span class={styles.text}>{textIcon}</span>
    } @else {
      <Bot aria-hidden={true} size={props.size ?? 16} strokeWidth={props.strokeWidth ?? 2.2} />
    }
  </span>
}
