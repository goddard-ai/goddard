import { type JSX } from "preact"
import { omit } from "radashi"
/* SVG_ICON_IMPORTS */

const SVG_NAMESPACE = "http://www.w3.org/2000/svg"
const SPRITE_ROOT_ID = "goddard-svg-icon-sprite-root"
const SYMBOL_ID_PREFIX = "goddard-svg-icon"

const svgSources = {
  /* SVG_ICON_ENTRIES */
} as const

/** Identifiers for SVG assets generated from SVG files under the public directory. */
export type SvgIconName = keyof typeof svgSources

/** SVG data type for inline SVG strings. */
export type SvgIconData = string & { _brand: "svg" }

/** A SvgIcon source can be a generated icon name or an inline SVG string. */
type SvgIconSource = { name: SvgIconName; data?: never } | { data: SvgIconData; name?: never }

const mountedSymbols = new Set<string>()
let parser: DOMParser | null = null
let spriteRoot: SVGSVGElement | null = null

export type SvgIconProps = SvgIconSource & JSX.IntrinsicElements["svg"]

/** Renders a named or inline SVG icon through the shared sprite root. */
export function SvgIcon(props: SvgIconProps) {
  const symbolId = ensureSymbolMounted(props)
  const svgProps = omit(props, ["name", "data"])

  return (
    <svg {...svgProps}>
      <use href={`#${symbolId}`} />
    </svg>
  )
}

/** Mounts the requested SVG source into the shared sprite and returns its symbol id. */
function ensureSymbolMounted(source: SvgIconSource) {
  const symbolId = getSymbolId(source)

  if (mountedSymbols.has(symbolId)) {
    return symbolId
  }

  const sprite = getSpriteRoot()

  if (!sprite) {
    return symbolId
  }

  if (document.getElementById(symbolId)) {
    mountedSymbols.add(symbolId)
    return symbolId
  }

  parser ??= new DOMParser()

  const parsedDocument = parser.parseFromString(
    source.data !== undefined ? source.data : svgSources[source.name],
    "image/svg+xml",
  )
  const sourceSvg = parsedDocument.documentElement as unknown as SVGSVGElement

  if (sourceSvg.tagName.toLowerCase() !== "svg") {
    throw new Error(
      `Expected ${JSON.stringify(source.data !== undefined ? symbolId : source.name)} to parse into an <svg> root element.`,
    )
  }

  rewriteLocalIds(sourceSvg, symbolId)

  const symbol = document.createElementNS(SVG_NAMESPACE, "symbol")
  symbol.setAttribute("id", symbolId)

  const viewBox = sourceSvg.getAttribute("viewBox")

  if (viewBox) {
    symbol.setAttribute("viewBox", viewBox)
  }

  const preserveAspectRatio = sourceSvg.getAttribute("preserveAspectRatio")

  if (preserveAspectRatio) {
    symbol.setAttribute("preserveAspectRatio", preserveAspectRatio)
  }

  symbol.innerHTML = sourceSvg.innerHTML
  sprite.append(symbol)
  mountedSymbols.add(symbolId)

  return symbolId
}

/** Creates or reuses the hidden sprite root used by every mounted icon symbol. */
function getSpriteRoot(): SVGSVGElement | null {
  if (typeof document === "undefined") {
    return null
  }

  if (spriteRoot?.isConnected) {
    return spriteRoot
  }

  const existingRoot = document.getElementById(SPRITE_ROOT_ID)

  if (existingRoot instanceof SVGSVGElement) {
    spriteRoot = existingRoot
    return spriteRoot
  }

  const root = document.createElementNS(SVG_NAMESPACE, "svg") as SVGSVGElement
  root.setAttribute("aria-hidden", "true")
  root.setAttribute("height", "0")
  root.setAttribute("id", SPRITE_ROOT_ID)
  root.setAttribute("width", "0")
  root.style.overflow = "hidden"
  root.style.pointerEvents = "none"
  root.style.position = "absolute"

  const mountTarget = document.body ?? document.documentElement

  mountTarget.prepend(root)
  spriteRoot = root

  return spriteRoot
}

/** Rewrites local SVG ids so multiple mounted symbols cannot collide in the DOM. */
function rewriteLocalIds(sourceSvg: SVGSVGElement, symbolId: string) {
  const idMap = new Map<string, string>()

  for (const element of sourceSvg.querySelectorAll("[id]")) {
    const currentId = element.getAttribute("id")

    if (!currentId) {
      continue
    }

    const nextId = `${symbolId}-${currentId}`
    idMap.set(currentId, nextId)
    element.setAttribute("id", nextId)
  }

  if (idMap.size === 0) {
    return
  }

  for (const element of sourceSvg.querySelectorAll("*")) {
    for (const attribute of Array.from(element.attributes)) {
      let nextValue = attribute.value

      for (const [currentId, nextId] of idMap) {
        nextValue = nextValue.replaceAll(`url(#${currentId})`, `url(#${nextId})`)

        if (nextValue === `#${currentId}`) {
          nextValue = `#${nextId}`
        }
      }

      if (attribute.name === "aria-describedby" || attribute.name === "aria-labelledby") {
        nextValue = nextValue
          .split(/\s+/)
          .map((value) => {
            return idMap.get(value) ?? value
          })
          .join(" ")
      }

      if (nextValue !== attribute.value) {
        element.setAttribute(attribute.name, nextValue)
      }
    }
  }
}

/** Returns the sprite symbol id for either a named icon or inline SVG data. */
function getSymbolId(source: SvgIconSource) {
  if (source.data !== undefined) {
    return `${SYMBOL_ID_PREFIX}-data-${hashSvgData(source.data)}`
  }

  const name = source.name as string

  return `${SYMBOL_ID_PREFIX}-${name
    .replaceAll(/[^a-zA-Z0-9]+/g, "-")
    .replaceAll(/^-+|-+$/g, "")
    .toLowerCase()}`
}

/** Generates a compact stable hash for inline SVG strings. */
function hashSvgData(data: string) {
  let hash1 = 0xdeadbeef ^ data.length
  let hash2 = 0x41c6ce57 ^ data.length

  for (let index = 0; index < data.length; index += 1) {
    const codePoint = data.charCodeAt(index)

    hash1 = Math.imul(hash1 ^ codePoint, 2654435761)
    hash2 = Math.imul(hash2 ^ codePoint, 1597334677)
  }

  hash1 =
    Math.imul(hash1 ^ (hash1 >>> 16), 2246822507) ^ Math.imul(hash2 ^ (hash2 >>> 13), 3266489909)
  hash2 =
    Math.imul(hash2 ^ (hash2 >>> 16), 2246822507) ^ Math.imul(hash1 ^ (hash1 >>> 13), 3266489909)

  return (4294967296 * (2097151 & hash2) + (hash1 >>> 0)).toString(36)
}
